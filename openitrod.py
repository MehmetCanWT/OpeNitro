#!/usr/bin/env python3
"""
openitrod.py - Background daemon for OpeNitro
Runs as root, listens on a UNIX socket, monitors the OpeNitro hotkey,
and enforces EC settings (battery charge limit, fan profiles, etc.).
"""

import grp
import json
import os
import platform
import pwd
import re
import select
import signal
import socket
import struct
import subprocess
import sys
import threading
import time

# Ensure we can import nitro_ec from the same directory
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from openitro_ec import OpeNitroEC

# ─── Constants ───
CONFIG_PATH = "/etc/openitro.json"
SOCKET_PATH = "/run/openitro.sock"

EV_KEY = 1
KEY_PRESS = 1
OPENITRO_KEYCODE = 425

IS_64BIT = platform.machine().endswith("64")
EVENT_SIZE = 24 if IS_64BIT else 16
EVENT_FMT = "QQHHi" if IS_64BIT else "IIHHi"

# List of compositor / session processes whose environment we probe
_SESSION_PROCS = frozenset(
    [
        "gnome-shell",
        "kwin_wayland",
        "kwin_x11",
        "xfce4-session",
        "hyprland",
        "sway",
        "wayfire",
        "plasmashell",
    ]
)


class OpeNitroDaemon:
    def __init__(self):
        self.ec = OpeNitroEC()
        self.config = {
            "battery_limit_active": False,
            "cpu_fan_mode": "auto",
            "gpu_fan_mode": "auto",
            "cpu_manual_speed": 100,
            "gpu_manual_speed": 100,
            "nitro_mode": "default",
        }
        self.running = True
        self._gui_proc = None        # Track spawned GUI process
        self._gui_lock = threading.Lock()
        self._load_config()

    # ─── Config persistence ───

    def _load_config(self):
        if not os.path.exists(CONFIG_PATH):
            return
        try:
            with open(CONFIG_PATH, "r") as f:
                loaded = json.load(f)
            self.config.update(loaded)
            print(f"[config] Loaded from {CONFIG_PATH}")
        except (json.JSONDecodeError, OSError) as e:
            print(f"[config] Failed to load: {e}", file=sys.stderr)

    def _save_config(self):
        try:
            with open(CONFIG_PATH, "w") as f:
                json.dump(self.config, f, indent=4)
        except OSError as e:
            print(f"[config] Failed to save: {e}", file=sys.stderr)

    # ─── EC write helpers ───

    def _apply_stored_config(self):
        """Push all stored config values into the EC on daemon start."""
        print("[daemon] Applying saved configuration…")
        self._set_power_mode_ec(self.config.get("nitro_mode", "default"))
        for unit in ("cpu", "gpu"):
            mode = self.config.get(f"{unit}_fan_mode", "auto")
            speed = self.config.get(f"{unit}_manual_speed", 100)
            self._set_fan_mode_ec(unit, mode, speed)
        self._set_battery_limit_ec(self.config.get("battery_limit_active", False))

    def _set_power_mode_ec(self, mode: str) -> bool:
        val_map = {
            "quiet": self.ec.NITRO_MODE_QUIET,
            "extreme": self.ec.NITRO_MODE_EXTREME,
        }
        val = val_map.get(mode, self.ec.NITRO_MODE_DEFAULT)
        return self.ec.ec_write(self.ec.REG_NITRO_MODE, val)

    def _set_fan_mode_ec(self, unit: str, mode: str, speed: int = 100) -> bool:
        if unit == "cpu":
            reg_mode = self.ec.REG_CPU_FAN_MODE_CONTROL
            reg_speed = self.ec.REG_CPU_MANUAL_SPEED_CONTROL
            mode_map = {
                "auto": self.ec.CPU_AUTO_MODE,
                "turbo": self.ec.CPU_TURBO_MODE,
                "manual": self.ec.CPU_MANUAL_MODE,
            }
        else:
            reg_mode = self.ec.REG_GPU_FAN_MODE_CONTROL
            reg_speed = self.ec.REG_GPU_MANUAL_SPEED_CONTROL
            mode_map = {
                "auto": self.ec.GPU_AUTO_MODE,
                "turbo": self.ec.GPU_TURBO_MODE,
                "manual": self.ec.GPU_MANUAL_MODE,
            }

        mode_val = mode_map.get(mode, mode_map["auto"])
        if mode == "manual":
            self.ec.ec_write(reg_speed, max(0, min(200, speed)))
        return self.ec.ec_write(reg_mode, mode_val)

    def _set_battery_limit_ec(self, enable: bool) -> bool:
        val = self.ec.BATTERY_LIMIT_ON if enable else self.ec.BATTERY_LIMIT_OFF
        return self.ec.ec_write(self.ec.REG_BATTERY_CHARGE_LIMIT, val)

    # ─── Background monitor (battery limit enforcement) ───

    def _monitor_loop(self):
        """Periodically verify the battery-limit register hasn't been reset by the BIOS."""
        print("[monitor] Battery-limit enforcement thread started")
        while self.running:
            try:
                if self.ec.ec_refresh():
                    want_on = self.config.get("battery_limit_active", False)
                    expected = (
                        self.ec.BATTERY_LIMIT_ON if want_on else self.ec.BATTERY_LIMIT_OFF
                    )
                    actual = self.ec.ec_read(self.ec.REG_BATTERY_CHARGE_LIMIT)
                    if actual != expected:
                        print(
                            f"[monitor] Battery limit mismatch "
                            f"(got {hex(actual)}, want {hex(expected)}). Reinforcing…"
                        )
                        self._set_battery_limit_ec(want_on)
            except Exception as e:
                print(f"[monitor] Error: {e}", file=sys.stderr)
            time.sleep(5)

    # ─── OpeNitro hotkey listener ───

    @staticmethod
    def _find_keyboard_device() -> str | None:
        """Locate the evdev node for the laptop keyboard."""
        try:
            with open("/proc/bus/input/devices", "r") as f:
                content = f.read()
        except OSError:
            return None

        for block in content.split("\n\n"):
            lines = block.strip().split("\n")
            is_keyboard = False
            event_num = None
            for line in lines:
                stripped = line.strip()
                if "keyboard" in stripped.lower():
                    is_keyboard = True
                if stripped.startswith("H:") and "event" in stripped:
                    m = re.search(r"event(\d+)", stripped)
                    if m:
                        event_num = m.group(1)
            if is_keyboard and event_num:
                path = f"/dev/input/event{event_num}"
                if os.path.exists(path):
                    return path
        return None

    @staticmethod
    def _get_console_user() -> str | None:
        """Best-effort detection of the active GUI user."""
        try:
            result = subprocess.run(
                ["who"], capture_output=True, text=True, check=False
            )
            for line in result.stdout.splitlines():
                parts = line.split()
                if len(parts) >= 2 and (":0" in line or "tty" in parts[1]):
                    return parts[0]
            # Fallback: first line
            if result.stdout.strip():
                return result.stdout.splitlines()[0].split()[0]
        except OSError:
            pass
        return None

    @staticmethod
    def _get_user_gui_env(user: str) -> dict:
        """Probe /proc to discover the user's display environment."""
        env = {
            "HOME": f"/home/{user}",
            "PATH": "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        }
        try:
            uid = pwd.getpwnam(user).pw_uid
            env["XDG_RUNTIME_DIR"] = f"/run/user/{uid}"
        except KeyError:
            pass

        try:
            uid = pwd.getpwnam(user).pw_uid
            for pid_str in os.listdir("/proc"):
                if not pid_str.isdigit():
                    continue
                try:
                    stat = os.stat(f"/proc/{pid_str}")
                    if stat.st_uid != uid:
                        continue
                    with open(f"/proc/{pid_str}/comm", "r") as f:
                        comm = f.read().strip()
                    if comm not in _SESSION_PROCS:
                        continue

                    with open(f"/proc/{pid_str}/environ", "rb") as f:
                        env_data = f.read()

                    for item in env_data.split(b"\x00"):
                        if b"=" not in item:
                            continue
                        k, v = item.split(b"=", 1)
                        key = k.decode("utf-8", errors="ignore")
                        if key in (
                            "DISPLAY",
                            "WAYLAND_DISPLAY",
                            "XAUTHORITY",
                            "XDG_RUNTIME_DIR",
                            "DBUS_SESSION_BUS_ADDRESS",
                        ):
                            env[key] = v.decode("utf-8", errors="ignore")

                    if "WAYLAND_DISPLAY" in env or "DISPLAY" in env:
                        break
                except (OSError, ValueError):
                    continue
        except (KeyError, OSError):
            pass

        # Provide sane fallbacks
        env.setdefault("DISPLAY", ":0")
        env.setdefault("XAUTHORITY", f"/home/{user}/.Xauthority")
        return env

    def _launch_or_focus_gui(self):
        """Launch the GUI (or re-focus if already open via single-instance mechanism)."""
        user = self._get_console_user()
        if not user:
            print("[hotkey] No active console user found", file=sys.stderr)
            return

        with self._gui_lock:
            # If we previously spawned the GUI, check if it's still alive
            if self._gui_proc is not None:
                rc = self._gui_proc.poll()
                if rc is None:
                    # GUI process still running — the single-instance logic inside
                    # openitro-gui will bring it to the front if we spawn again.
                    pass
                else:
                    self._gui_proc = None

        env = self._get_user_gui_env(user)

        cmd = ["sudo", "-u", user, "env"]
        for k, v in env.items():
            cmd.append(f"{k}={v}")
        cmd.append("openitro-gui")

        print(f"[hotkey] Launching GUI as '{user}'")
        try:
            proc = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                start_new_session=True,
            )
            with self._gui_lock:
                self._gui_proc = proc
        except OSError as e:
            print(f"[hotkey] Failed to spawn GUI: {e}", file=sys.stderr)

    def _keyboard_listener_loop(self):
        """Listen for the physical OpeNitro key press via evdev."""
        print("[hotkey] Keyboard listener started")

        while self.running:
            dev_path = self._find_keyboard_device()
            if not dev_path:
                print(
                    "[hotkey] No keyboard device found. Retry in 10 s…",
                    file=sys.stderr,
                )
                time.sleep(10)
                continue

            print(f"[hotkey] Monitoring {dev_path}")
            try:
                with open(dev_path, "rb") as dev:
                    while self.running:
                        r, _, _ = select.select([dev], [], [], 1.0)
                        if not r:
                            continue
                        data = dev.read(EVENT_SIZE)
                        if len(data) != EVENT_SIZE:
                            continue
                        _, _, ev_type, code, value = struct.unpack(EVENT_FMT, data)
                        if (
                            ev_type == EV_KEY
                            and code == OPENITRO_KEYCODE
                            and value == KEY_PRESS
                        ):
                            print("[hotkey] OpeNitro key pressed!")
                            self._launch_or_focus_gui()
            except PermissionError:
                print(
                    f"[hotkey] Permission denied for {dev_path}",
                    file=sys.stderr,
                )
                time.sleep(5)
            except OSError as e:
                print(
                    f"[hotkey] Device error: {e}. Retrying in 3 s…",
                    file=sys.stderr,
                )
                time.sleep(3)

    # ─── Socket command handler ───

    def _handle_client(self, conn: socket.socket):
        try:
            raw = conn.recv(1024)
            if not raw:
                return
            data = raw.decode("utf-8").strip()
            parts = data.split()
            cmd = parts[0].upper()

            response: dict

            if cmd == "GET_STATUS":
                status = self.ec.get_status()
                if status:
                    # Sync config from live EC values
                    for key in (
                        "battery_limit_active",
                        "cpu_fan_mode",
                        "gpu_fan_mode",
                        "cpu_manual_speed",
                        "gpu_manual_speed",
                        "nitro_mode",
                    ):
                        self.config[key] = status[key]
                    response = {"status": "success", "data": status}
                else:
                    response = {"status": "error", "message": "Failed to read EC"}

            elif cmd == "SET_POWER_MODE":
                if len(parts) < 2:
                    response = {"status": "error", "message": "Missing mode"}
                else:
                    mode = parts[1].lower()
                    if mode not in ("quiet", "default", "extreme"):
                        response = {"status": "error", "message": "Invalid mode"}
                    elif self._set_power_mode_ec(mode):
                        self.config["nitro_mode"] = mode
                        self._save_config()
                        response = {
                            "status": "success",
                            "message": f"Power mode → {mode}",
                        }
                    else:
                        response = {"status": "error", "message": "EC write failed"}

            elif cmd == "SET_FAN_MODE":
                if len(parts) < 3:
                    response = {"status": "error", "message": "Missing arguments"}
                else:
                    unit = parts[1].lower()
                    mode = parts[2].lower()
                    try:
                        speed = int(parts[3]) if len(parts) > 3 else 100
                    except ValueError:
                        speed = 100

                    if unit not in ("cpu", "gpu") or mode not in (
                        "auto",
                        "turbo",
                        "manual",
                    ):
                        response = {"status": "error", "message": "Invalid arguments"}
                    elif self._set_fan_mode_ec(unit, mode, speed):
                        self.config[f"{unit}_fan_mode"] = mode
                        if mode == "manual":
                            self.config[f"{unit}_manual_speed"] = speed
                        self._save_config()
                        response = {
                            "status": "success",
                            "message": f"{unit.upper()} fan → {mode}",
                        }
                    else:
                        response = {"status": "error", "message": "EC write failed"}

            elif cmd == "SET_BATTERY_LIMIT":
                if len(parts) < 2:
                    response = {"status": "error", "message": "Missing state"}
                else:
                    state = parts[1].lower()
                    if state not in ("on", "off", "1", "0", "true", "false"):
                        response = {"status": "error", "message": "Invalid state"}
                    else:
                        enable = state in ("on", "1", "true")
                        if self._set_battery_limit_ec(enable):
                            self.config["battery_limit_active"] = enable
                            self._save_config()
                            response = {
                                "status": "success",
                                "message": f"Battery limit → {'on' if enable else 'off'}",
                            }
                        else:
                            response = {
                                "status": "error",
                                "message": "EC write failed",
                            }
            else:
                response = {"status": "error", "message": f"Unknown command: {cmd}"}

            conn.sendall(json.dumps(response).encode("utf-8"))
        except Exception as e:
            print(f"[socket] Client error: {e}", file=sys.stderr)
            try:
                conn.sendall(
                    json.dumps({"status": "error", "message": str(e)}).encode("utf-8")
                )
            except OSError:
                pass
        finally:
            conn.close()

    # ─── Main run loop ───

    def run(self):
        if not self.ec.init_ec():
            print("[daemon] Fatal: Could not initialize EC access.", file=sys.stderr)
            sys.exit(1)

        self._apply_stored_config()

        # Battery-limit enforcement thread
        threading.Thread(target=self._monitor_loop, daemon=True).start()

        # Keyboard hotkey listener thread
        threading.Thread(target=self._keyboard_listener_loop, daemon=True).start()

        # Prepare UNIX socket
        if os.path.exists(SOCKET_PATH):
            try:
                os.remove(SOCKET_PATH)
            except OSError:
                pass

        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            server.bind(SOCKET_PATH)
            os.chmod(SOCKET_PATH, 0o660)
            try:
                wheel_gid = grp.getgrnam("wheel").gr_gid
                os.chown(SOCKET_PATH, 0, wheel_gid)
            except KeyError:
                print(
                    "[daemon] Warning: group 'wheel' not found; "
                    "socket may not be accessible to unprivileged users.",
                    file=sys.stderr,
                )
            server.listen(5)
            print(f"[daemon] Listening on {SOCKET_PATH}")
        except OSError as e:
            print(f"[daemon] Fatal: Cannot bind socket: {e}", file=sys.stderr)
            self.ec.close_ec()
            sys.exit(1)

        server.setblocking(False)
        while self.running:
            try:
                readable, _, _ = select.select([server], [], [], 1.0)
                if readable:
                    conn, _ = server.accept()
                    self._handle_client(conn)
            except KeyboardInterrupt:
                break
            except Exception as e:
                if self.running:
                    print(f"[daemon] Accept error: {e}", file=sys.stderr)

        # Shutdown
        print("[daemon] Shutting down…")
        server.close()
        try:
            os.remove(SOCKET_PATH)
        except OSError:
            pass
        self.ec.close_ec()
        print("[daemon] Stopped.")

    def stop(self):
        self.running = False


if __name__ == "__main__":
    daemon = OpeNitroDaemon()

    def _sig_handler(signum, _frame):
        print(f"[daemon] Signal {signum} received, stopping…")
        daemon.stop()

    signal.signal(signal.SIGTERM, _sig_handler)
    signal.signal(signal.SIGINT, _sig_handler)
    daemon.run()
