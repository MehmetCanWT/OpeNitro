//! Shared domain models and IPC protocol types for OpeNitro.

use serde::{Deserialize, Serialize};

/// Operating mode for an individual cooling fan (CPU or GPU).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    #[default]
    Auto,
    Turbo,
    Manual,
    Curve,
}

impl FanMode {
    pub fn label(self) -> &'static str {
        match self {
            FanMode::Auto => "AUTO",
            FanMode::Turbo => "MAX",
            FanMode::Manual => "MANUAL",
            FanMode::Curve => "CURVE",
        }
    }

    pub fn ec_mode(self) -> EcFanMode {
        match self {
            FanMode::Auto => EcFanMode::Auto,
            FanMode::Turbo => EcFanMode::Turbo,
            FanMode::Manual | FanMode::Curve => EcFanMode::Manual,
        }
    }
}

/// Low-level hardware fan mode written to the Embedded Controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EcFanMode {
    #[default]
    Auto,
    Turbo,
    Manual,
}

/// System-wide thermal & power performance profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PowerMode {
    Quiet,
    #[default]
    Default,
    Extreme,
}

impl PowerMode {
    pub fn label(self) -> &'static str {
        match self {
            PowerMode::Quiet => "Quiet",
            PowerMode::Default => "Balanced",
            PowerMode::Extreme => "Extreme",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            PowerMode::Quiet => "Lower power draw and acoustic floor for light tasks",
            PowerMode::Default => "Adaptive thermal and clock scaling for everyday use",
            PowerMode::Extreme => "Maximum thermal headroom and peak turbo boost",
        }
    }

    /// Map to standard Linux ACPI `platform_profile` string.
    pub fn platform_profile_str(self) -> &'static str {
        match self {
            PowerMode::Quiet => "low-power",
            PowerMode::Default => "balanced",
            PowerMode::Extreme => "performance",
        }
    }

    /// Parse standard Linux ACPI `platform_profile` string.
    pub fn from_platform_profile(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low-power" | "quiet" => Some(PowerMode::Quiet),
            "balanced" | "balanced-performance" => Some(PowerMode::Default),
            "performance" => Some(PowerMode::Extreme),
            _ => None,
        }
    }
}

/// Target cooling unit identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanUnit {
    Cpu,
    Gpu,
}

impl FanUnit {
    pub fn label(self) -> &'static str {
        match self {
            FanUnit::Cpu => "CPU",
            FanUnit::Gpu => "GPU",
        }
    }
}

/// Linux Kernel Lockdown LSM state (`/sys/kernel/security/lockdown`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LockdownMode {
    None,
    Integrity,
    Confidentiality,
    #[default]
    Unknown,
}

impl LockdownMode {
    pub fn label(self) -> &'static str {
        match self {
            LockdownMode::None => "None",
            LockdownMode::Integrity => "Integrity",
            LockdownMode::Confidentiality => "Confidentiality",
            LockdownMode::Unknown => "Unknown",
        }
    }

    pub fn is_locked_down(self) -> bool {
        matches!(
            self,
            LockdownMode::Integrity | LockdownMode::Confidentiality
        )
    }
}

/// Kernel integrity & anti-cheat compatibility posture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SecurityPosture {
    pub lockdown: LockdownMode,
    pub secure_boot: Option<bool>,
    pub tpm_present: bool,
    pub eac_linux_v2_ready: bool,
}

/// Active hardware control transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EcBackendKind {
    /// Standard in-tree `ec_sys` debugfs (`/sys/kernel/debug/ec/ec0/io`, lockdown=none).
    EcSys,
    /// Signed kernel sysfs interfaces (`platform_profile` / `power_supply`) — EAC Lockdown-safe.
    SignedSysfs,
    /// Unprivileged read-only `sysfs` telemetry.
    #[default]
    Locked,
}

impl EcBackendKind {
    pub fn label(self) -> &'static str {
        match self {
            EcBackendKind::EcSys => "ec_sys debugfs",
            EcBackendKind::SignedSysfs => "Signed Kernel Sysfs",
            EcBackendKind::Locked => "Sysfs Only (Locked)",
        }
    }

    pub fn is_unlocked(self) -> bool {
        !matches!(self, EcBackendKind::Locked)
    }

    pub fn supports_raw_fan_ec(self) -> bool {
        matches!(self, EcBackendKind::EcSys)
    }
}

/// Static DMI & hardware layout metadata detected at startup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HardwareInfo {
    pub vendor: String,
    pub model: String,
    pub bios_version: String,
    pub register_profile: String,
    pub acpi_platform_profile: Option<String>,
    pub acpi_profile_choices: Vec<String>,
    pub hwmon_sources: Vec<String>,
    pub security: SecurityPosture,
}

/// Complete telemetry snapshot returned on every poll cycle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TelemetrySnapshot {
    pub backend: EcBackendKind,
    pub ec_error: Option<String>,
    pub hardware: HardwareInfo,
    pub cpu_temp: Option<u8>,
    pub gpu_temp: Option<u8>,
    pub sys_temp: Option<u8>,
    pub nvme_temp: Option<u8>,
    pub cpu_rpm: Option<u16>,
    pub gpu_rpm: Option<u16>,
    pub cpu_fan_mode: Option<EcFanMode>,
    pub gpu_fan_mode: Option<EcFanMode>,
    pub cpu_manual_speed: Option<u8>,
    pub gpu_manual_speed: Option<u8>,
    pub power_mode: Option<PowerMode>,
    pub ac_plugged: Option<bool>,
    pub battery_percent: Option<u8>,
    pub battery_status: Option<String>,
    pub battery_power_watts: Option<f32>,
    pub battery_limit_active: Option<bool>,
    pub kb_backlight_timeout: Option<bool>,
    pub usb_charge_poweroff: Option<bool>,
    pub conflicting_daemon: Option<String>,
    pub nbfc_active: bool,
}

/// Request sent from the GUI process to the elevated `--ec-helper` child over stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum HelperRequest {
    Init,
    GetEcSnapshot,
    SetPowerMode {
        mode: PowerMode,
    },
    SetFanMode {
        unit: FanUnit,
        mode: EcFanMode,
        speed_pct: u8,
    },
    SetFanSpeed {
        unit: FanUnit,
        speed_pct: u8,
    },
    SetBatteryLimit {
        enabled: bool,
    },
    SetKbTimeout {
        enabled: bool,
    },
    SetUsbCharge {
        enabled: bool,
    },
    StopConflictingDaemon {
        name: String,
    },
    Quit,
}

/// Hardware state snapshot returned by the elevated helper.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EcSnapshot {
    pub backend: EcBackendKind,
    pub register_profile: String,
    pub cpu_temp: Option<u8>,
    pub gpu_temp: Option<u8>,
    pub sys_temp: Option<u8>,
    pub cpu_rpm: Option<u16>,
    pub gpu_rpm: Option<u16>,
    pub cpu_fan_mode: Option<EcFanMode>,
    pub gpu_fan_mode: Option<EcFanMode>,
    pub cpu_manual_speed: Option<u8>,
    pub gpu_manual_speed: Option<u8>,
    pub power_mode: Option<PowerMode>,
    pub ac_plugged: Option<bool>,
    pub battery_limit_active: Option<bool>,
    pub kb_backlight_timeout: Option<bool>,
    pub usb_charge_poweroff: Option<bool>,
    pub nbfc_active: bool,
}

/// Response returned from `--ec-helper` over stdout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HelperResponse {
    Ready {
        backend: EcBackendKind,
        register_profile: String,
    },
    Snapshot {
        data: EcSnapshot,
    },
    Ok {
        message: String,
    },
    Error {
        message: String,
    },
}
