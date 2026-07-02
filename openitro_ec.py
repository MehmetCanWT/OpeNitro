#!/usr/bin/env python3
"""
openitro_ec.py - OpeNitro Embedded Controller Interface
Low-level abstraction for reading/writing EC registers via ec_sys debugfs.
"""

import os
import sys
import time
import threading


class OpeNitroEC:
    """Thread-safe interface to the Acer Nitro Embedded Controller."""

    # ─── EC Register Map (ECS_AN515_46 compatible) ───

    # GPU Fan
    REG_GPU_FAN_MODE_CONTROL = 0x21
    GPU_AUTO_MODE = 0x10
    GPU_TURBO_MODE = 0x20
    GPU_MANUAL_MODE = 0x30
    REG_GPU_MANUAL_SPEED_CONTROL = 0x3A

    # CPU Fan
    REG_CPU_FAN_MODE_CONTROL = 0x22
    CPU_AUTO_MODE = 0x04
    CPU_TURBO_MODE = 0x08
    CPU_MANUAL_MODE = 0x0C
    REG_CPU_MANUAL_SPEED_CONTROL = 0x37

    # Fan RPM readout registers
    REG_CPU_FAN_SPEED_HIGH = 0x13
    REG_CPU_FAN_SPEED_LOW = 0x14
    REG_GPU_FAN_SPEED_HIGH = 0x15
    REG_GPU_FAN_SPEED_LOW = 0x16

    # Temperature sensors
    REG_CPU_TEMP = 0xB0
    REG_GPU_TEMP = 0xB6
    REG_SYS_TEMP = 0xB3

    # Power / AC status
    REG_POWER_STATUS = 0x00
    POWER_PLUGGED_IN = 0x01
    POWER_UNPLUGGED = 0x00

    # Battery charge limit (80% protection)
    REG_BATTERY_CHARGE_LIMIT = 0x03
    BATTERY_LIMIT_ON = 0x51
    BATTERY_LIMIT_OFF = 0x11

    # Battery status
    REG_BATTERY_STATUS = 0xC1
    BATTERY_PLUGGED_IN_CHARGING = 0x02
    BATTERY_DRAINING = 0x01
    BATTERY_OFF = 0x00

    # Performance mode profiles
    REG_NITRO_MODE = 0x2C
    NITRO_MODE_QUIET = 0x00
    NITRO_MODE_DEFAULT = 0x01
    NITRO_MODE_EXTREME = 0x04

    # ─── Mode lookup maps ───
    _CPU_MODE_MAP = {
        CPU_AUTO_MODE: "auto",
        CPU_TURBO_MODE: "turbo",
        CPU_MANUAL_MODE: "manual",
    }
    _GPU_MODE_MAP = {
        GPU_AUTO_MODE: "auto",
        GPU_TURBO_MODE: "turbo",
        GPU_MANUAL_MODE: "manual",
    }
    _NITRO_MODE_MAP = {
        NITRO_MODE_QUIET: "quiet",
        NITRO_MODE_DEFAULT: "default",
        NITRO_MODE_EXTREME: "extreme",
    }

    def __init__(self):
        self.ec_path = "/sys/kernel/debug/ec/ec0/io"
        self.ec_file = None
        self.buffer = b""
        self._lock = threading.Lock()
        self.model = self._detect_model()

    # ─── Initialization ───

    @staticmethod
    def _detect_model() -> str:
        try:
            with open("/sys/class/dmi/id/product_name", "r") as f:
                return f.read().strip()
        except (OSError, IOError):
            return "Unknown"

    def init_ec(self) -> bool:
        """Load ec_sys kernel module and open debugfs EC file for read/write."""
        if not os.path.exists(self.ec_path):
            print("ec_sys not loaded, attempting modprobe...", file=sys.stderr)
            os.system("modprobe -r ec_sys 2>/dev/null")
            os.system("modprobe ec_sys write_support=y 2>/dev/null")
            time.sleep(0.5)

        if not os.path.exists(self.ec_path):
            print(
                f"Error: {self.ec_path} does not exist after modprobe. "
                "Make sure you are running as root.",
                file=sys.stderr,
            )
            return False

        try:
            self.ec_file = open(self.ec_path, "r+b", buffering=0)
            return True
        except PermissionError:
            print(
                f"Error: Permission denied opening {self.ec_path}. Run as root.",
                file=sys.stderr,
            )
            return False
        except OSError as e:
            print(f"Error opening EC file: {e}", file=sys.stderr)
            return False

    def close_ec(self):
        with self._lock:
            if self.ec_file:
                try:
                    self.ec_file.close()
                except OSError:
                    pass
                self.ec_file = None

    # ─── Low-level read / write (must hold _lock) ───

    def ec_write(self, address: int, value: int) -> bool:
        """Write a single byte to an EC register. Thread-safe."""
        with self._lock:
            if not self.ec_file:
                print("EC file not initialized.", file=sys.stderr)
                return False
            try:
                self.ec_file.seek(address)
                self.ec_file.write(bytes([value]))
                self.ec_file.flush()
                return True
            except OSError as e:
                print(
                    f"EC write failed @ {hex(address)} = {hex(value)}: {e}",
                    file=sys.stderr,
                )
                return False

    def ec_refresh(self) -> bool:
        """Read the full 256-byte EC register space into an internal buffer. Thread-safe."""
        with self._lock:
            if not self.ec_file:
                return False
            try:
                self.ec_file.seek(0)
                self.buffer = self.ec_file.read(256)
                return len(self.buffer) >= 256
            except OSError as e:
                print(f"EC refresh failed: {e}", file=sys.stderr)
                return False

    def ec_read(self, address: int) -> int:
        """Read a byte from the cached buffer (call ec_refresh first)."""
        if not self.buffer or address >= len(self.buffer):
            return 0
        return self.buffer[address]

    # ─── High-level helpers ───

    def get_status(self) -> dict:
        """Read all registers once and return a status dict."""
        if not self.ec_refresh():
            return {}

        # Fan RPM: low_byte << 8 | high_byte  (naming follows EC register layout)
        cpu_rpm = (self.ec_read(self.REG_CPU_FAN_SPEED_LOW) << 8) | self.ec_read(
            self.REG_CPU_FAN_SPEED_HIGH
        )
        gpu_rpm = (self.ec_read(self.REG_GPU_FAN_SPEED_LOW) << 8) | self.ec_read(
            self.REG_GPU_FAN_SPEED_HIGH
        )

        cpu_mode_raw = self.ec_read(self.REG_CPU_FAN_MODE_CONTROL)
        gpu_mode_raw = self.ec_read(self.REG_GPU_FAN_MODE_CONTROL)
        nitro_mode_raw = self.ec_read(self.REG_NITRO_MODE)

        return {
            "model": self.model,
            "cpu_temp": self.ec_read(self.REG_CPU_TEMP),
            "gpu_temp": self.ec_read(self.REG_GPU_TEMP),
            "sys_temp": self.ec_read(self.REG_SYS_TEMP),
            "cpu_rpm": cpu_rpm,
            "gpu_rpm": gpu_rpm,
            "cpu_fan_mode": self._CPU_MODE_MAP.get(cpu_mode_raw, "unknown"),
            "gpu_fan_mode": self._GPU_MODE_MAP.get(gpu_mode_raw, "unknown"),
            "cpu_manual_speed": self.ec_read(self.REG_CPU_MANUAL_SPEED_CONTROL),
            "gpu_manual_speed": self.ec_read(self.REG_GPU_MANUAL_SPEED_CONTROL),
            "power_plugged": self.ec_read(self.REG_POWER_STATUS) == self.POWER_PLUGGED_IN,
            "battery_charging": self.ec_read(self.REG_BATTERY_STATUS)
            == self.BATTERY_PLUGGED_IN_CHARGING,
            "battery_limit_active": self.ec_read(self.REG_BATTERY_CHARGE_LIMIT)
            == self.BATTERY_LIMIT_ON,
            "nitro_mode": self._NITRO_MODE_MAP.get(nitro_mode_raw, "unknown"),
        }
