# OpeNitro

A lightweight, GPU-accelerated Linux hardware and thermal controller for **Acer Nitro** and **Predator** laptops, written purely in Rust with **GPUI** and **[GPUI Kit](https://gpui-kit.com/)**.

---

## Key Highlights

- **100% Pure Rust, Single Binary**: Built on `gpui-kit`. Zero Python, zero runtime dependencies, zero out-of-tree kernel modules, and zero installation scripts required.
- **Zero `systemd` Daemons**: No background service or persistent socket created by OpeNitro. Telemetry runs unprivileged; hardware writes use an ephemeral `pkexec openitro --ec-helper` pipe with a sanitized environment that terminates immediately on window close.
- **Anti-Cheat & Kernel Integrity Ready (Epic EAC Linux v2)**:
  - `#![forbid(unsafe_code)]` across the entire crate.
  - Never loads or unloads kernel modules (`no modprobe`), never recompiles kernel drivers, and never touches `/dev/port`.
  - Reads `/sys/kernel/security/lockdown`, UEFI Secure Boot, and TPM 2.0 posture at startup.
  - Under **Kernel Lockdown** (`integrity` / `confidentiality`), operates strictly over signed kernel `sysfs` (`platform_profile`, `charge_control_end_threshold`, `hwmon`).
  - When Kernel Lockdown is `none` and `ec_sys` debugfs (`/sys/kernel/debug/ec/ec0/io`) is exposed, provides full EC fan RPM readout, fan modes, custom curves, and hardware toggles.
- **Optional NBFC Coexistence**: Standalone by default (`ec_sys` / `sysfs`). If `nbfc_service` is already running on your system, OpeNitro automatically routes fan modes (`Auto`, `Max`, `Manual`, `Curve`) through `/usr/bin/nbfc` instead of fighting it, while still letting you switch to direct EC with one click.
- **Responsive Hyprland / Wayland UI**: Adaptive layout breakpoints for tiled and floating windows, `Dark`/`Light` native themes, `QuitMode::LastWindowClosed`, and 50 ms rotor animations active only when the Dashboard is focused.

---

## Features

- **Unprivileged Live Telemetry (No Password Required)**:
  - CPU, System/ACPI, and NVMe temperatures from `/sys/class/hwmon`.
  - AC adapter state, battery percentage, and charge/discharge wattage from `/sys/class/power_supply`.
  - NVIDIA dGPU temperature via `/usr/bin/nvidia-smi`, gated on PCI `power/runtime_status == active` so a suspended dGPU is never woken up.
- **Fan Control & Custom Fan Curves**:
  - Per-unit **Auto**, **Max** (Turbo), **Manual** (`0–100%`), and draggable **Curve** (`30°C–100°C` × `0%–100%`) modes with presets (*Silent*, *Balanced*, *Gaming*, *Aggressive*) and configurable hysteresis.
- **Performance & Power Care**:
  - **Quiet**, **Balanced**, and **Extreme** thermal profiles.
  - **80% Battery Charge Limit** (read-modify-write on EC `0x03` bit `0x40` or `charge_control_end_threshold`).
  - **Keyboard Backlight 30s Timeout** & **USB Power-Off Charging**.
- **Persistent Settings**:
  - EC hardware states (and `nbfc_service` speeds when active) remain active after closing the app.
  - Preferences and curves are saved to `~/.config/openitro/config.json` and automatically re-applied upon unlocking.
- **Built-In CLI**: Same binary exposes `--status`, `--json`, `--power`, `--cpu-fan`, `--gpu-fan`, `--battery-limit`, `--kb-timeout`, and `--usb-charge`.

---

## Building & Running

```bash
cargo build --release
./target/release/openitro
```

For quick development builds:

```bash
cargo run
```

### CLI Usage

```bash
# Unprivileged telemetry summary or machine-readable JSON
./target/release/openitro --status
./target/release/openitro --status --json

# Hardware controls (run elevated for direct EC / sysfs writes)
sudo ./target/release/openitro --power extreme
sudo ./target/release/openitro --cpu-fan manual:75 --gpu-fan turbo
sudo ./target/release/openitro --battery-limit on
```

---

## Supported Devices

See [supported_devices.md](supported_devices.md) for compatible Acer Nitro / Predator models and EC register notes (`0x03` manual-enable + 80% charge limit bitmask, `0x21`/`0x22`, `0x37`/`0x3A`).

## License

Distributed under the MIT License. See [LICENSE](LICENSE).
