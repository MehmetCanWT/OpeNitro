//! Unprivileged Linux sysfs telemetry & kernel integrity posture reader.
//!
//! Reads `/sys/class/hwmon`, `/sys/class/power_supply`, `/sys/class/dmi/id`,
//! `/sys/firmware/acpi/platform_profile`, and `/sys/kernel/security/lockdown`
//! without root privileges, kernel modifications, or external processes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::backend::devices::{RegisterMap, read_dmi_field};
use crate::backend::types::{EcFanMode, HardwareInfo, LockdownMode, PowerMode, SecurityPosture};

const PLATFORM_PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";
const PLATFORM_CHOICES_PATH: &str = "/sys/firmware/acpi/platform_profile_choices";

/// Unprivileged sensor snapshot gathered from standard Linux sysfs nodes.
#[derive(Debug, Clone, Default)]
pub struct SysfsSnapshot {
    pub cpu_temp: Option<u8>,
    pub gpu_temp: Option<u8>,
    pub nvme_temp: Option<u8>,
    pub acpi_temp: Option<u8>,
    pub cpu_rpm: Option<u16>,
    pub gpu_rpm: Option<u16>,
    pub power_mode: Option<PowerMode>,
    pub ac_plugged: Option<bool>,
    pub battery_percent: Option<u8>,
    pub battery_status: Option<String>,
    pub battery_power_watts: Option<f32>,
    pub charge_threshold: Option<u8>,
    pub hwmon_sources: Vec<String>,
}

/// Parse `/sys/kernel/security/lockdown` active mode (marked in brackets, e.g. `[integrity]`).
pub fn parse_lockdown_str(raw: &str) -> LockdownMode {
    if raw.contains("[confidentiality]") {
        LockdownMode::Confidentiality
    } else if raw.contains("[integrity]") {
        LockdownMode::Integrity
    } else if raw.contains("[none]") {
        LockdownMode::None
    } else {
        LockdownMode::Unknown
    }
}

/// Probe kernel lockdown, UEFI Secure Boot, and TPM 2.0 posture (read-only).
pub fn read_security_posture() -> SecurityPosture {
    let lockdown = fs::read_to_string("/sys/kernel/security/lockdown")
        .map(|s| parse_lockdown_str(&s))
        .unwrap_or(LockdownMode::Unknown);

    let secure_boot = read_secure_boot_flag(Path::new("/sys/firmware/efi/efivars"));
    let tpm_present =
        Path::new("/dev/tpmrm0").exists() || Path::new("/sys/class/tpm/tpm0").exists();
    let eac_linux_v2_ready =
        lockdown.is_locked_down() && tpm_present && secure_boot.unwrap_or(false);

    SecurityPosture {
        lockdown,
        secure_boot,
        tpm_present,
        eac_linux_v2_ready,
    }
}

fn read_secure_boot_flag(efivars_dir: &Path) -> Option<bool> {
    let entries = fs::read_dir(efivars_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("SecureBoot-") {
            if let Ok(bytes) = fs::read(entry.path()) {
                // Efivar format: 4-byte attributes + 1-byte boolean payload
                if bytes.len() >= 5 {
                    return Some(bytes[4] == 1);
                }
            }
        }
    }
    None
}

/// Probe static DMI, ACPI platform_profile, and kernel security posture.
pub fn read_hardware_info() -> HardwareInfo {
    let model = read_dmi_field("product_name");
    let vendor = read_dmi_field("sys_vendor");
    let bios_version = read_dmi_field("bios_version");
    let register_map = RegisterMap::for_model(&model);
    let register_profile = register_map.profile_name.to_string();
    let acpi_platform_profile = fs::read_to_string(PLATFORM_PROFILE_PATH)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let acpi_profile_choices = fs::read_to_string(PLATFORM_CHOICES_PATH)
        .map(|s| s.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();

    let snap = read_sysfs_snapshot();

    HardwareInfo {
        vendor,
        model,
        bios_version,
        register_profile,
        register_verified: register_map.verified,
        acpi_platform_profile,
        acpi_profile_choices,
        hwmon_sources: snap.hwmon_sources,
        security: read_security_posture(),
    }
}

/// Locate the battery `charge_control_end_threshold` or an already-loaded Acer WMI `health_mode` node.
pub fn find_battery_threshold_path() -> Option<PathBuf> {
    let entries = fs::read_dir("/sys/class/power_supply").ok()?;
    for entry in entries.flatten() {
        let dir = entry.path();
        let is_bat = fs::read_to_string(dir.join("type"))
            .map(|s| s.trim() == "Battery")
            .unwrap_or(false);
        if is_bat {
            let p = dir.join("charge_control_end_threshold");
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Check for an already-present Acer WMI battery health sysfs node (never loads modules).
pub fn find_acer_wmi_battery_path() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "/sys/bus/wmi/drivers/acer-wmi-battery/health_mode",
        "/sys/module/linuwu_sense/drivers/platform:acer-wmi/acer-wmi/nitro_sense/battery_limiter",
    ];
    CANDIDATES.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Read live telemetry from `/sys/class/hwmon`, `/sys/class/power_supply`, `platform_profile`, and unprivileged GPU sensors.
pub fn read_sysfs_snapshot() -> SysfsSnapshot {
    let mut snap = SysfsSnapshot::default();
    read_hwmon_dir(Path::new("/sys/class/hwmon"), &mut snap);
    if snap.gpu_temp.is_none() {
        snap.gpu_temp = read_nvidia_gpu_temp();
    }
    read_power_supply_dir(Path::new("/sys/class/power_supply"), &mut snap);
    snap.power_mode = fs::read_to_string(PLATFORM_PROFILE_PATH)
        .ok()
        .and_then(|s| PowerMode::from_platform_profile(&s));
    snap
}

/// Find allowlisted `nbfc` CLI executable on the host.
pub fn find_nbfc_bin() -> Option<&'static str> {
    const NBFC_PATHS: &[&str] = &["/usr/bin/nbfc", "/usr/local/bin/nbfc"];
    NBFC_PATHS.iter().copied().find(|p| Path::new(p).is_file())
}

/// Returns true when `nbfc_service` socket and `nbfc` CLI are both available for fan delegation.
pub fn is_nbfc_ready() -> bool {
    Path::new("/run/nbfc_service.socket").exists() && find_nbfc_bin().is_some()
}

/// Per-fan status parsed from `nbfc status -a`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NbfcFanStatus {
    pub mode: EcFanMode,
    pub speed_pct: u8,
}

/// Parsed CPU/GPU fan state reported by `nbfc status -a`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NbfcStatus {
    pub cpu: Option<NbfcFanStatus>,
    pub gpu: Option<NbfcFanStatus>,
}

/// Parse `nbfc status -a` text output into CPU (index 0) and GPU (index 1) states.
pub fn parse_nbfc_status(raw: &str) -> NbfcStatus {
    #[derive(Default)]
    struct Pending {
        auto: Option<bool>,
        target: Option<f32>,
        current: Option<f32>,
    }

    let mut fans: Vec<Pending> = Vec::new();
    let mut cur: Option<Pending> = None;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((key, val)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let val = val.trim();
        match key {
            "Fan Display Name" => {
                if let Some(prev) = cur.take() {
                    fans.push(prev);
                }
                cur = Some(Pending::default());
            }
            "Auto Control Enabled" => {
                if let Some(ref mut c) = cur {
                    c.auto = match val.to_ascii_lowercase().as_str() {
                        "true" | "1" | "yes" => Some(true),
                        "false" | "0" | "no" => Some(false),
                        _ => None,
                    };
                }
            }
            "Target Fan Speed" => {
                if let Some(ref mut c) = cur {
                    c.target = val.parse::<f32>().ok();
                }
            }
            "Current Fan Speed" => {
                if let Some(ref mut c) = cur {
                    c.current = val.parse::<f32>().ok();
                }
            }
            _ => {}
        }
    }
    if let Some(last) = cur.take() {
        fans.push(last);
    }

    let finish = |p: Pending| -> NbfcFanStatus {
        let pct = p
            .target
            .or(p.current)
            .map(|v| v.round().clamp(0.0, 100.0) as u8)
            .unwrap_or(0);
        let mode = match p.auto {
            Some(true) => EcFanMode::Auto,
            Some(false) if pct >= 100 => EcFanMode::Turbo,
            Some(false) => EcFanMode::Manual,
            None => EcFanMode::Auto,
        };
        NbfcFanStatus {
            mode,
            speed_pct: pct,
        }
    };

    let mut out = NbfcStatus::default();
    let mut iter = fans.into_iter();
    out.cpu = iter.next().map(finish);
    out.gpu = iter.next().map(finish);
    out
}

/// Query `/usr/bin/nbfc status -a` if NBFC is running and accessible.
pub fn read_nbfc_status() -> Option<NbfcStatus> {
    if !is_nbfc_ready() {
        return None;
    }
    let bin = find_nbfc_bin()?;
    let output = Command::new(bin)
        .args(["status", "-a"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let parsed = parse_nbfc_status(&text);
    if parsed.cpu.is_some() || parsed.gpu.is_some() {
        Some(parsed)
    } else {
        None
    }
}

/// Parse single-line `nvidia-smi --query-gpu=temperature.gpu --format=csv,noheader,nounits` output.
pub fn parse_nvidia_smi_temp(raw: &str) -> Option<u8> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(val) = trimmed.parse::<f32>() {
            if (1.0..=150.0).contains(&val) {
                return Some(val.round().clamp(1.0, 150.0) as u8);
            }
        }
    }
    None
}

/// Check PCI sysfs whether an NVIDIA display controller (`0x10de`, class `0x03*`) is currently active.
pub fn is_nvidia_gpu_active_in(pci_devices_dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(pci_devices_dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let dev = entry.path();
        let Ok(vendor) = fs::read_to_string(dev.join("vendor")) else {
            continue;
        };
        if !vendor.trim().eq_ignore_ascii_case("0x10de") {
            continue;
        }
        let Ok(class) = fs::read_to_string(dev.join("class")) else {
            continue;
        };
        if !class.trim().to_ascii_lowercase().starts_with("0x03") {
            continue;
        }
        let runtime = fs::read_to_string(dev.join("power/runtime_status"))
            .map(|s| s.trim().eq_ignore_ascii_case("active"))
            .unwrap_or(true);
        if runtime {
            return true;
        }
    }
    false
}

/// Read NVIDIA dGPU temperature via unprivileged `/usr/bin/nvidia-smi` only when the dGPU is active.
pub fn read_nvidia_gpu_temp() -> Option<u8> {
    if !is_nvidia_gpu_active_in(Path::new("/sys/bus/pci/devices")) {
        return None;
    }
    const NVIDIA_SMI_PATHS: &[&str] = &["/usr/bin/nvidia-smi", "/bin/nvidia-smi"];
    let bin = NVIDIA_SMI_PATHS
        .iter()
        .copied()
        .find(|p| Path::new(p).is_file())?;

    let output = Command::new(bin)
        .args([
            "--query-gpu=temperature.gpu",
            "--format=csv,noheader,nounits",
        ])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_nvidia_smi_temp(&String::from_utf8_lossy(&output.stdout))
}

/// Detect competing external fan daemons (e.g. `nbfc_service`) via read-only `/proc` scan.
pub fn detect_conflicting_fan_daemon() -> Option<String> {
    detect_conflicting_daemon_in(Path::new("/proc"))
}

/// Testable `/proc` scanner for competing fan control daemons.
pub fn detect_conflicting_daemon_in(proc_dir: &Path) -> Option<String> {
    const KNOWN_CONFLICTS: &[&str] = &["nbfc_service", "openitrod"];
    let entries = fs::read_dir(proc_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let pid_str = name.to_string_lossy();
        if !pid_str.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let Ok(comm) = fs::read_to_string(entry.path().join("comm")) {
            let trimmed = comm.trim();
            if KNOWN_CONFLICTS.contains(&trimmed) {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Parse millidegrees Celsius text (e.g. `"54000\n"`) into whole degrees `u8`.
pub fn parse_millidegrees(raw: &str) -> Option<u8> {
    let milli: i64 = raw.trim().parse().ok()?;
    if milli <= 0 || milli > 150_000 {
        return None;
    }
    Some((milli / 1000).clamp(0, 150) as u8)
}

fn read_hwmon_dir(base: &Path, snap: &mut SysfsSnapshot) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };

    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();

    for dir in dirs {
        let Ok(name) = fs::read_to_string(dir.join("name")) else {
            continue;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            continue;
        }
        snap.hwmon_sources.push(name.clone());

        let max_temp = read_max_temp_in_hwmon(&dir);
        match name.as_str() {
            "coretemp" | "k10temp" | "zenpower" | "cpu_thermal" => {
                if let Some(t) = max_temp {
                    snap.cpu_temp = Some(snap.cpu_temp.map_or(t, |prev| prev.max(t)));
                }
            }
            "amdgpu" | "nouveau" | "nvidia" => {
                if let Some(t) = max_temp {
                    snap.gpu_temp = Some(snap.gpu_temp.map_or(t, |prev| prev.max(t)));
                }
            }
            "nvme" => {
                if let Some(t) = max_temp {
                    snap.nvme_temp = Some(snap.nvme_temp.map_or(t, |prev| prev.max(t)));
                }
            }
            "acpitz" => {
                if let Some(t) = max_temp {
                    snap.acpi_temp = Some(snap.acpi_temp.map_or(t, |prev| prev.max(t)));
                }
            }
            "acer" | "acer_wmi" => {
                if let Ok(rpm) = fs::read_to_string(dir.join("fan1_input")) {
                    if let Ok(v) = rpm.trim().parse::<u16>() {
                        snap.cpu_rpm = Some(v);
                    }
                }
                if let Ok(rpm) = fs::read_to_string(dir.join("fan2_input")) {
                    if let Ok(v) = rpm.trim().parse::<u16>() {
                        snap.gpu_rpm = Some(v);
                    }
                }
            }
            _ => {}
        }
    }

    if snap.cpu_temp.is_none() {
        snap.cpu_temp = snap.acpi_temp;
    }
}

fn read_max_temp_in_hwmon(dir: &Path) -> Option<u8> {
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    let mut best: Option<u8> = None;
    for entry in entries.flatten() {
        let fname = entry.file_name();
        let fname = fname.to_string_lossy();
        if fname.starts_with("temp") && fname.ends_with("_input") {
            if let Ok(raw) = fs::read_to_string(entry.path()) {
                if let Some(deg) = parse_millidegrees(&raw) {
                    best = Some(best.map_or(deg, |p| p.max(deg)));
                }
            }
        }
    }
    best
}

fn read_power_supply_dir(base: &Path, snap: &mut SysfsSnapshot) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };

    let mut dirs: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    dirs.sort();

    for dir in dirs {
        let supply_type = fs::read_to_string(dir.join("type"))
            .unwrap_or_default()
            .trim()
            .to_string();

        if supply_type == "Mains" {
            if let Ok(online) = fs::read_to_string(dir.join("online")) {
                snap.ac_plugged = Some(online.trim() == "1");
            }
        } else if supply_type == "Battery" {
            if let Ok(cap) = fs::read_to_string(dir.join("capacity")) {
                if let Ok(pct) = cap.trim().parse::<u8>() {
                    snap.battery_percent = Some(pct.min(100));
                }
            }
            if let Ok(status) = fs::read_to_string(dir.join("status")) {
                let s = status.trim().to_string();
                if !s.is_empty() {
                    snap.battery_status = Some(s);
                }
            }
            if let Ok(thresh) = fs::read_to_string(dir.join("charge_control_end_threshold")) {
                if let Ok(t) = thresh.trim().parse::<u8>() {
                    snap.charge_threshold = Some(t);
                }
            }

            if let Ok(power_now) = fs::read_to_string(dir.join("power_now")) {
                if let Ok(uw) = power_now.trim().parse::<f64>() {
                    snap.battery_power_watts = Some((uw / 1_000_000.0) as f32);
                }
            } else if let (Ok(v_raw), Ok(c_raw)) = (
                fs::read_to_string(dir.join("voltage_now")),
                fs::read_to_string(dir.join("current_now")),
            ) {
                if let (Ok(uv), Ok(ua)) = (v_raw.trim().parse::<f64>(), c_raw.trim().parse::<f64>())
                {
                    snap.battery_power_watts = Some(((uv * ua) / 1_000_000_000_000.0) as f32);
                }
            }
        }
    }
}
