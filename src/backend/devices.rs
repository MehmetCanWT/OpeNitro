//! Device detection via Linux DMI sysfs and Acer Embedded Controller register layouts.

use std::fs;
use std::path::Path;

use crate::backend::types::{EcFanMode, FanUnit, PowerMode};

/// Embedded Controller register offsets and value encodings for Acer Nitro / Predator models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterMap {
    pub profile_name: &'static str,
    /// `false` for models not listed in `supported_devices.md`: EC writes are refused
    /// and register-decoded values are not shown until the layout is confirmed.
    pub verified: bool,

    // GPU Fan
    pub reg_gpu_fan_mode: u8,
    pub gpu_auto_val: u8,
    pub gpu_turbo_val: u8,
    pub gpu_manual_val: u8,
    pub reg_gpu_manual_speed: u8,

    // CPU Fan
    pub reg_cpu_fan_mode: u8,
    pub cpu_auto_val: u8,
    pub cpu_turbo_val: u8,
    pub cpu_manual_val: u8,
    pub reg_cpu_manual_speed: u8,

    // Fan RPM readouts
    pub reg_cpu_rpm_high: u8,
    pub reg_cpu_rpm_low: u8,
    pub reg_gpu_rpm_high: u8,
    pub reg_gpu_rpm_low: u8,

    // Temperature sensors
    pub reg_cpu_temp: u8,
    pub reg_gpu_temp: u8,
    pub reg_sys_temp: u8,

    // Power & battery
    pub reg_power_status: u8,
    pub power_plugged_val: u8,
    pub reg_battery_limit: u8,
    pub battery_limit_on: u8,
    pub battery_limit_off: u8,
    /// Bitmask enabling 80% battery charge limit in `reg_battery_limit` (0x40 on Acer Nitro/Predator).
    pub battery_limit_mask: u8,
    /// Bitmask enabling Linux manual/turbo fan control in `reg_battery_limit` (0x11 on AN515-47/55..58).
    pub fan_enable_mask: u8,

    // Nitro performance profile
    pub reg_nitro_mode: u8,
    pub nitro_quiet_val: u8,
    pub nitro_default_val: u8,
    pub nitro_extreme_val: u8,

    // Keyboard backlight 30s timeout
    pub reg_kb_timeout: u8,
    pub kb_timeout_on: u8,
    pub kb_timeout_off: u8,

    // USB power-off charging
    pub reg_usb_charge: u8,
    pub usb_charge_on: u8,
    pub usb_charge_off: u8,
    /// Bit in `reg_usb_charge` that disables power-off USB charging when set (`0x10`).
    pub usb_charge_disable_mask: u8,
}

impl RegisterMap {
    /// Standard Acer Nitro / Predator EC layout (AN515-55..58, AN515-45/46, AN16/17, ANV15/16, Helios).
    pub const fn standard() -> Self {
        Self {
            profile_name: "Standard Acer EC",
            verified: true,
            reg_gpu_fan_mode: 0x21,
            gpu_auto_val: 0x10,
            gpu_turbo_val: 0x20,
            gpu_manual_val: 0x30,
            reg_gpu_manual_speed: 0x3A,
            reg_cpu_fan_mode: 0x22,
            cpu_auto_val: 0x04,
            cpu_turbo_val: 0x08,
            cpu_manual_val: 0x0C,
            reg_cpu_manual_speed: 0x37,
            reg_cpu_rpm_high: 0x13,
            reg_cpu_rpm_low: 0x14,
            reg_gpu_rpm_high: 0x15,
            reg_gpu_rpm_low: 0x16,
            reg_cpu_temp: 0xB0,
            reg_gpu_temp: 0xB6,
            reg_sys_temp: 0xB3,
            reg_power_status: 0x00,
            power_plugged_val: 0x01,
            reg_battery_limit: 0x03,
            battery_limit_on: 0x51,
            battery_limit_off: 0x11,
            battery_limit_mask: 0x40,
            fan_enable_mask: 0x11,
            reg_nitro_mode: 0x2C,
            nitro_quiet_val: 0x00,
            nitro_default_val: 0x01,
            nitro_extreme_val: 0x04,
            reg_kb_timeout: 0x06,
            kb_timeout_on: 0x1E,
            kb_timeout_off: 0x00,
            reg_usb_charge: 0x08,
            usb_charge_on: 0x0F,
            usb_charge_off: 0x1F,
            usb_charge_disable_mask: 0x10,
        }
    }

    /// Select the register layout matching a DMI `product_name` string.
    pub fn for_model(model: &str) -> Self {
        let mut map = Self::standard();
        let upper = model.to_uppercase();
        if upper.contains("AN515-44") || upper.contains("AN515-43") {
            map.profile_name = "AN515-43/44 AMD Override";
            map.reg_gpu_temp = 0xB4;
            map.reg_sys_temp = 0xB0;
            map.battery_limit_on = 0x40;
            map.battery_limit_off = 0x00;
            map.fan_enable_mask = 0x00;
        } else if upper.contains("AN515-42") || upper.contains("AN515-51") {
            map.profile_name = "AN515-42/51 Legacy EC";
            map.battery_limit_on = 0x40;
            map.battery_limit_off = 0x00;
            map.fan_enable_mask = 0x00;
        } else if upper.contains("PREDATOR") || upper.contains("HELIOS") || upper.contains("TRITON")
        {
            map.profile_name = "Predator / Helios Base EC";
        } else if !is_verified_model(&upper) {
            map.profile_name = "Unverified Acer EC (read-only)";
            map.verified = false;
        }
        map
    }

    pub fn fan_mode_register(&self, unit: FanUnit) -> u8 {
        match unit {
            FanUnit::Cpu => self.reg_cpu_fan_mode,
            FanUnit::Gpu => self.reg_gpu_fan_mode,
        }
    }

    pub fn fan_speed_register(&self, unit: FanUnit) -> u8 {
        match unit {
            FanUnit::Cpu => self.reg_cpu_manual_speed,
            FanUnit::Gpu => self.reg_gpu_manual_speed,
        }
    }

    pub fn encode_fan_mode(&self, unit: FanUnit, mode: EcFanMode) -> u8 {
        match (unit, mode) {
            (FanUnit::Cpu, EcFanMode::Auto) => self.cpu_auto_val,
            (FanUnit::Cpu, EcFanMode::Turbo) => self.cpu_turbo_val,
            (FanUnit::Cpu, EcFanMode::Manual) => self.cpu_manual_val,
            (FanUnit::Gpu, EcFanMode::Auto) => self.gpu_auto_val,
            (FanUnit::Gpu, EcFanMode::Turbo) => self.gpu_turbo_val,
            (FanUnit::Gpu, EcFanMode::Manual) => self.gpu_manual_val,
        }
    }

    pub fn decode_fan_mode(&self, unit: FanUnit, raw: u8) -> EcFanMode {
        match unit {
            FanUnit::Cpu => {
                if raw == self.cpu_turbo_val {
                    EcFanMode::Turbo
                } else if raw == self.cpu_manual_val {
                    EcFanMode::Manual
                } else {
                    EcFanMode::Auto
                }
            }
            FanUnit::Gpu => {
                if raw == self.gpu_turbo_val {
                    EcFanMode::Turbo
                } else if raw == self.gpu_manual_val {
                    EcFanMode::Manual
                } else {
                    EcFanMode::Auto
                }
            }
        }
    }

    pub fn encode_power_mode(&self, mode: PowerMode) -> u8 {
        match mode {
            PowerMode::Quiet => self.nitro_quiet_val,
            PowerMode::Default => self.nitro_default_val,
            PowerMode::Extreme => self.nitro_extreme_val,
        }
    }

    pub fn decode_power_mode(&self, raw: u8) -> PowerMode {
        if raw == self.nitro_quiet_val {
            PowerMode::Quiet
        } else if raw == self.nitro_extreme_val {
            PowerMode::Extreme
        } else {
            PowerMode::Default
        }
    }

    /// Read-modify-write helper for the 80% battery limit bit in `reg_battery_limit`
    /// while preserving `fan_enable_mask` (so Acer Nitro EC receives `0x51` instead of bare `0x40`).
    pub fn update_battery_limit_byte(&self, current: u8, enabled: bool) -> u8 {
        if enabled {
            current | self.battery_limit_mask | self.fan_enable_mask
        } else {
            current & !self.battery_limit_mask
        }
    }

    /// Ensure manual fan control enable bits (`fan_enable_mask`) are preserved in `reg_battery_limit`.
    pub fn update_fan_enable_byte(&self, current: u8) -> u8 {
        current | self.fan_enable_mask
    }

    /// Decode whether the 80% battery charge limit bit is active.
    pub fn is_battery_limit_enabled(&self, raw: u8) -> bool {
        (raw & self.battery_limit_mask) != 0
    }

    /// Decode whether keyboard backlight 30s timeout is active.
    pub fn is_kb_timeout_enabled(&self, raw: u8) -> bool {
        raw != self.kb_timeout_off
    }

    /// Decode whether USB power-off charging is active (`usb_charge_disable_mask` cleared).
    pub fn is_usb_charge_enabled(&self, raw: u8) -> bool {
        (raw & self.usb_charge_disable_mask) == 0
    }

    /// Read-modify-write helper for USB power-off charging (`reg_usb_charge`).
    pub fn update_usb_charge_byte(&self, current: u8, enabled: bool) -> u8 {
        if enabled {
            current & !self.usb_charge_disable_mask
        } else {
            current | self.usb_charge_disable_mask
        }
    }

    /// Decode 16-bit fan RPM from the EC's low/high register pair.
    pub fn decode_rpm(&self, buffer: &[u8], unit: FanUnit) -> u16 {
        let (hi, lo) = match unit {
            FanUnit::Cpu => (self.reg_cpu_rpm_high, self.reg_cpu_rpm_low),
            FanUnit::Gpu => (self.reg_gpu_rpm_high, self.reg_gpu_rpm_low),
        };
        let high = *buffer.get(hi as usize).unwrap_or(&0) as u16;
        let low = *buffer.get(lo as usize).unwrap_or(&0) as u16;
        (low << 8) | high
    }
}

/// Models whose EC layout is confirmed (see `supported_devices.md`). Anything else is read-only.
const VERIFIED_MODELS: &[&str] = &[
    "AN515-45", "AN515-46", "AN515-55", "AN515-56", "AN515-57", "AN515-58", "AN517-55", "AN16-41",
    "AN16-42", "AN16-43", "AN17-41", "AN17-51", "ANV15-41", "ANV15-51", "ANV16-41",
];

fn is_verified_model(upper_model: &str) -> bool {
    VERIFIED_MODELS.iter().any(|m| upper_model.contains(m))
}

/// Read a DMI identifier field from `/sys/class/dmi/id/<field>`.
pub fn read_dmi_field(field: &str) -> String {
    read_dmi_field_from(Path::new("/sys/class/dmi/id"), field)
}

/// Read a DMI identifier field from an arbitrary base directory (testable).
pub fn read_dmi_field_from(base: &Path, field: &str) -> String {
    fs::read_to_string(base.join(field))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Unknown".to_string())
}
