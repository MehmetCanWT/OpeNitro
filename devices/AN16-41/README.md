# OpeNitro Support Guide for Acer Nitro 16 (AN16-41)

This guide contains compatibility details, specific Embedded Controller (EC) register mappings, and usage instructions for the **Acer Nitro 16 (AN16-41)** series when running **OpeNitro**.

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
| **RGB Keyboard Backlight** | **🟢 Yes** | 🟢 Yes (4-Zone RGB via WMI facer module). |

---

## 🔍 Model-Specific Notes

*   **EC Register Layout:** Standard EC registers.
*   **RGB Customization:** Requires the `facer` WMI kernel module loaded to communicate via `/dev/acer-gkbbl-0`.

---

## 🛠️ How to use OpeNitro on this model

1.  **Status Check:** Run `openitro-cli --status` in terminal to verify that temperature sensors and fan speeds are correctly mapped and reporting proper values.
2.  **Toggle Settings:** Open the OpeNitro GUI (`openitro-gui`) to toggle Battery Limit, CoolBoost, Keyboard Backlight Timeout, and USB power-off charging.
3.  **Configuring RGB:** If you have the `facer` kernel driver installed, the 'RGB KEYBOARD CONTROL' section will be displayed at the bottom of the GUI. You can dynamically choose a mode (Static, Breath, Neon, Wave, etc.) and pick colors and speeds.
