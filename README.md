# OpeNitro

A lightweight, resource-efficient, and fast Linux alternative to the Acer NitroSense utility. Written in Python and PyQt6, this application provides system performance mode adjustments, fan speed controls (Auto/Max/Manual), and an 80% battery charge limit toggle to preserve battery health. 

It is designed to run silently in the background on startup, instantly launching the custom graphical interface when you press the physical **NitroSense key** on your keyboard.

---

## Features
- **Instant Hotkey Launch**: Listens to the physical NitroSense key (`XF86Launch2` / keycode 425) via a background evdev daemon and opens the GUI instantly.
- **Fan Control**: Set modes to **Auto**, **Max (Turbo)**, or adjust custom sliders in **Manual** mode for both CPU and GPU.
- **Battery Protection**: Keep your battery healthy by limiting the maximum charge level to 80% (persists across reboots via daemon monitoring).
- **Performance Profiles**: Change performance profiles (**Quiet**, **Default**, **Extreme**).
- **Lightweight GUI**: Premium dark theme designed with custom animated cooling fans and temperature gauges.
- **CLI Utility**: Control everything directly from the command line using `openitro-cli`.

---

## Supported Devices
For a detailed list of compatible and tested laptops, please refer to the:
👉 **[Supported Devices List](supported_devices.md)**

---

## Credits & Respects
This project wouldn't be possible without the incredible work done by the Linux community. Big respects and credits to the authors of the following reference projects:
- **[Linuwu-Sense](https://github.com/musicanan/Linuwu-Sense)**: Provided the hardware mapping and kernel/WMI module inspirations.
- **[Div-Acer-Manager-Max](https://github.com/musicanan/Div-Acer-Manager-Max)**: Showcased robust keycode monitoring and user environment routing techniques.
- **[Linux-NitroSense](https://github.com/musicanan/Linux-NitroSense)**: Documented register configurations and the core EC read/write mechanisms.

---

## Installation

### Prerequisites
Make sure you have `python3` and `PyQt6` installed. On Arch/CachyOS:
```bash
sudo pacman -S python pyqt6
```

### Installation Steps
1. Clone the repository and navigate into it:
   ```bash
   git clone https://github.com/YOUR_USERNAME/OpeNitro.git
   cd OpeNitro
   ```
2. Run the installer script as root:
   ```bash
   sudo ./install.sh
   ```

The installer will copy all scripts to `/opt/openitro/`, create helper executables in `/usr/local/bin/`, set up the systemd background daemon (`openitrod`), install the desktop shortcut, and configure the application icon.

---

## Usage

### Graphical Interface (GUI)
- Open the application using the **NitroSense key** on your keyboard.
- Alternatively, launch it from your application menu ("OpeNitro") or by typing:
  ```bash
  openitro-gui
  ```

### Command Line Interface (CLI)
Query status or control settings directly from your terminal:
```bash
# Print current status and sensors info
openitro-cli --status

# Output status in JSON format
openitro-cli --status --json

# Set performance mode
openitro-cli --power quiet     # options: quiet, default, extreme

# Toggle battery charging limit
openitro-cli --battery-limit on

# Set CPU fan to manual with speed level 150
openitro-cli --cpu-fan manual --cpu-speed 150
```

### Managing the Background Service
The daemon runs as a systemd service:
```bash
# Check daemon service status
systemctl status openitrod

# Restart daemon service
sudo systemctl restart openitrod
```

---

## License
MIT License.
