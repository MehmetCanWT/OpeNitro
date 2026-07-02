# 📋 Supported Laptops & Compatibility

This page keeps track of confirmed, compatible, and planned laptop models for **OpeNitro**.

> [!IMPORTANT]
> If your laptop is not listed below, please **[open a GitHub Issue](https://github.com/trwinner9/OpeNitro/issues)**. 
> To help us verify support, please include the output of the following command:
> ```bash
> cat /sys/class/dmi/id/product_name
> ```

---

## 💻 Compatibility Matrix

The following Acer models share the same Embedded Controller (EC) register mappings and have been analyzed for compatibility:

| Model / series | Fan Control | Battery Care (80%) | Performance Profiles | Status & Verification |
| :--- | :---: | :---: | :---: | :--- |
| 🛡️ **Acer Nitro 5 AN515-57** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | **Fully Tested** (Primary Development Machine: AN515-57-57DA) |
| 💻 **Acer Nitro 5 AN515-46** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Expected compatible (Shares base ECS_AN515_46 EC layout) |
| 💻 **Acer Nitro 5 AN515-55** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Expected compatible (Matches base EC registers) |
| 💻 **Acer Nitro 5 AN515-58** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Expected compatible (Matches base EC registers) |
| 💻 **Acer Nitro 5 AN515-45** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Expected compatible (Matches base EC registers) |
| 💻 **Acer Nitro 5 AN515-47** | **🟢 Yes** | **🟢 Yes** | **🟢 Yes** | Expected compatible (Matches base EC registers) |
| 🚀 **Acer Predator Helios 300** | **🟡 Experimental** | **🟡 Experimental** | **🟡 Experimental** | Shared Acer WMI base fan offsets |
| 🚀 **Acer Triton 300 / 500** | **🟡 Experimental** | **🟡 Experimental** | **🟡 Experimental** | Shared Acer ACPI charging offsets |

---

## 🔍 How to Verify Your Model

To determine if your laptop matches the Acer Nitro series, open your terminal and run:

```bash
cat /sys/class/dmi/id/product_name
```

* If your system reports an **AN515** series model, it is highly likely to be compatible out-of-the-box.
* If your model is in the **Helios** or **Triton** range, basic telemetry will work, but fan manual settings should be tested carefully.

---

## 🛠️ Troubleshooting & Reporting

If OpeNitro doesn't work correctly on your device:
1. Stop the daemon: `sudo systemctl stop openitrod`
2. Run the daemon directly in foreground debug mode: `sudo /usr/local/bin/openitrod`
3. Open a GitHub issue and attach the console printouts so we can address it.
