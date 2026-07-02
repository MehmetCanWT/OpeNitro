# Supported Devices

> [!IMPORTANT]
> If your device is not listed below, please **[open an issue](https://github.com/YOUR_USERNAME/OpeNitro/issues)** requesting support. Please include the output of `cat /sys/class/dmi/id/product_name` and your hardware details!

The following Acer laptops are confirmed or expected to be compatible with this application's Embedded Controller (EC) register mapping (ECS_AN515_46 layout):

| Device Model | Fan Control Status | Battery Limit (80%) Status | Performance Modes | Notes |
|:---|:---:|:---:|:---:|:---|
| **Acer Nitro 5 AN515-57** | **Fully Tested** | **Supported** | **Supported** | Main development and test machine (AN515-57-57DA). |
| **Acer Nitro 5 AN515-46** | Compatible | Supported | Supported | Shares the base ECS_AN515_46 EC layout. |
| **Acer Nitro 5 AN515-55** | Compatible | Supported | Supported | Verified with similar register maps. |
| **Acer Nitro 5 AN515-58** | Compatible | Supported | Supported | Verified with similar register maps. |
| **Acer Nitro 5 AN515-45** | Compatible | Supported | Supported | Fully compatible. |
| **Acer Nitro 5 AN515-47** | Compatible | Supported | Supported | Should work out of the box. |
| **Acer Predator Helios 300** | Experimental | Experimental | Experimental | Some models share Acer WMI fan control offsets. |
| **Acer Triton 300 / 500** | Experimental | Experimental | Experimental | Shares general Acer charging limit offsets. |

---

### How to Verify Your Model
You can check your exact laptop model string by running:
```bash
cat /sys/class/dmi/id/product_name
```
If it is in the `AN515` series, it should work perfectly with this tool. If you encounter issues, please submit an issue with your debug logs.
