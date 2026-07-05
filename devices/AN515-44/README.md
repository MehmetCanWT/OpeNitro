# OpeNitro Support Guide for Acer Nitro 5 (AN515-44)

This guide contains compatibility details, specific Embedded Controller (EC) register mappings, and usage instructions for the **Acer Nitro 5 (AN515-44)** series when running **OpeNitro**.

---

## 🚀 Compatibility & Features

| Feature | Supported | Details |
| :--- | :---: | :--- |
| **CPU Fan Speed Control** | **🟢 Yes** | Accesses standard EC RPM registers. Supports Auto, Max, and Manual controls. |
| **GPU Fan Speed Control** | **🟢 Yes** | Accesses standard EC RPM registers. Supports Auto, Max, and Manual controls. |
| **80% Battery Charge Limit** | **🟢 Yes** | Preserves battery lifespan by limiting charge to 80%. |
| **Acer CoolBoost** | **🟢 Yes** | Dynamically increases fan speed bounds under high CPU/GPU load. |
| **Keyboard backlight timeout** | **🟢 Yes** | Disables the automatic 30-second power-saving timeout on the backlight. |
| **USB Power-off Charging** | **🟢 Yes** | Allows toggling USB power delivery while the system is off or sleeping. |
| **RGB Keyboard Backlight** | **🔴 No** | 🔴 Red backlight only. |

---

## 🔍 Model-Specific Notes

*   **EC Register Layout:** Utilizes custom EC registers. Remaps GPU Temp to 0xB4, System Temp to 0xB0, and Battery Charge limits to 0x40/0x00.
*   **RGB Customization:** This model features a single-color (Red or White) backlight. Static time-out prevention is fully supported, but color animation and zone adjustments are not hardware-supported.

---

## 🛠️ How to use OpeNitro on this model

1.  **Status Check:** Run `openitro-cli --status` in terminal to verify that temperature sensors and fan speeds are correctly mapped and reporting proper values.
2.  **Toggle Settings:** Open the OpeNitro GUI (`openitro-gui`) to toggle Battery Limit, CoolBoost, Keyboard Backlight Timeout, and USB power-off charging.
3.  **Configuring RGB:** Since this model has a static backlight, the RGB section will remain hidden. Use the 'Keyboard Backlight Timeout' switch to keep the keyboard lit permanently.
