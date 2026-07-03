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

| Model Series | Fan Speed Controls | 80% Battery Limit | Acer CoolBoost | Keyboard Timeout | Status & Verification |
| :--- | :---: | :---: | :---: | :---: | :--- |
| **Acer Nitro 5 (AN515-57)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Tested** (Primary Development Machine) |
| **Acer Nitro 5 (AN515-46)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN515-45)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN515-55)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN515-56)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN515-58)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN517-55)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 16 (AN16-41)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 16 (AN16-42)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 16 (AN16-43)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 17 (AN17-41)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 17 (AN17-51)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro V 15 (ANV15-41)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro V 15 (ANV15-51)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro V 16 (ANV16-41)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Standard EC Registers) |
| **Acer Nitro 5 (AN515-44)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Via Automated EC overrides) |
| **Acer Nitro 5 (AN515-43)** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Supported** (Via Automated EC overrides) |
| **Predator Helios 300** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Compatible (Shares base ACPI / EC mapping) |
| **Predator Helios 16/18** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Compatible (Shares base ACPI / EC mapping) |
| **Predator Triton Series** | **🟡 Experimental** | **🟢 Yes** | **🟡 Experimental** | **🟡 Experimental** | Shared ACPI charging offsets |

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
