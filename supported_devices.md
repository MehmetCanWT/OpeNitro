# 📋 Supported Laptops & Compatibility

This page lists confirmed, compatible, and planned laptop models for **OpeNitro**.

> [!IMPORTANT]
> If your laptop is not listed below, please **[open a GitHub Issue](https://github.com/trwinner9/OpeNitro/issues)**.
> To help us verify and expand support, please include the output of the following command:
> ```bash
> cat /sys/class/dmi/id/product_name
> ```

---

## 💻 Compatibility Matrix

OpeNitro is model-agnostic and interfaces directly with the Embedded Controller (EC). In order to maximize compatibility across different generations of hardware, the daemon dynamically loads register overrides at startup.

| Model Series | Fan Speed Controls | 80% Battery Limit | Acer CoolBoost | Keyboard Timeout | RGB Backlight | Status & Verification |
| :--- | :---: | :---: | :---: | :---: | :---: | :--- |
| [Acer Nitro 5 (AN515-57)](devices/AN515-57/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-57/README.md) | **Fully Tested** (Primary Dev Machine) |
| [Acer Nitro 5 (AN515-46)](devices/AN515-46/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN515-46/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN515-45)](devices/AN515-45/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-45/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN515-55)](devices/AN515-55/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-55/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN515-56)](devices/AN515-56/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-56/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN515-58)](devices/AN515-58/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN515-58/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN517-55)](devices/AN517-55/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN517-55/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 16 (AN16-41)](devices/AN16-41/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN16-41/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 16 (AN16-42)](devices/AN16-42/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN16-42/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 16 (AN16-43)](devices/AN16-43/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN16-43/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 17 (AN17-41)](devices/AN17-41/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN17-41/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 17 (AN17-51)](devices/AN17-51/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/AN17-51/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro V 15 (ANV15-41)](devices/ANV15-41/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/ANV15-41/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro V 15 (ANV15-51)](devices/ANV15-51/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/ANV15-51/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro V 16 (ANV16-41)](devices/ANV16-41/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/ANV16-41/README.md) | **Fully Supported** (Standard EC) |
| [Acer Nitro 5 (AN515-44)](devices/AN515-44/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-44/README.md) | **Fully Supported** (Via overrides) |
| [Acer Nitro 5 (AN515-43)](devices/AN515-43/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🔴 Red Only](devices/AN515-43/README.md) | **Fully Supported** (Via overrides) |
| [Predator Helios 300](devices/Predator-Helios-300/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/Predator-Helios-300/README.md) | Compatible (Base ACPI/EC map) |
| [Predator Helios 16/18](devices/Predator-Helios-16-18/README.md) | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | [🟢 Yes (WMI)](devices/Predator-Helios-16-18/README.md) | Compatible (Base ACPI/EC map) |
| [Predator Triton Series](devices/Predator-Triton-Series/README.md) | **🟡 Experimental** | **🟢 Yes** | **🟡 Experimental** | **🟡 Experimental** | [🟢 Yes (WMI)](devices/Predator-Triton-Series/README.md) | Shared ACPI charging offsets |

---

## 🔍 How register overrides work (AN515-44 / AN515-43)

On older AMD-based platforms (such as the Nitro 5 AN515-44/43), Acer used slightly different register configurations. OpeNitro detects this automatically at initialization:

* **GPU temperature offset** is remapped from `0xB6` to `0xB4`.
* **System temperature offset** is remapped from `0xB3` to `0xB0`.
* **Battery charge limits** are remapped:
  * Enable: `0x40` (instead of `0x51`)
  * Disable: `0x00` (instead of `0x11`)

---

## 🛠️ Verification & Feedback

To double check compatibility on a new Acer machine:
1. Run `openitro-cli --status` to ensure correct CPU/GPU fan RPM and temperatures are read.
2. Toggle the features inside `openitro-gui` and check for any mismatch warning in `sudo journalctl -u openitrod -f`.
