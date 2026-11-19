//! Thread-safe hardware control over standard Linux interfaces.
//!
//! Anti-cheat & Kernel Integrity policy:
//! - Never loads or unloads kernel modules (`no modprobe`).
//! - Never accesses raw I/O ports (`/dev/port` forbidden under Kernel Lockdown / EAC).
//! - Uses signed kernel `sysfs` (`platform_profile`, `charge_control_end_threshold`) when
//!   Kernel Lockdown is active (`integrity` / `confidentiality`).
//! - Uses already-present `ec_sys` debugfs (`/sys/kernel/debug/ec/ec0/io`) only when
//!   Kernel Lockdown is `none` and debugfs is writable.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::backend::devices::{RegisterMap, read_dmi_field};
use crate::backend::sysfs::{
    find_acer_wmi_battery_path, find_battery_threshold_path, read_security_posture,
    read_sysfs_snapshot,
};
use crate::backend::types::{EcBackendKind, EcFanMode, EcSnapshot, FanUnit, PowerMode};

const EC_SYS_PATH: &str = "/sys/kernel/debug/ec/ec0/io";
const PLATFORM_PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";

enum HardwareTransport {
    EcSys(File),
    SignedSysfs {
        platform_profile: Option<PathBuf>,
        battery_threshold: Option<PathBuf>,
    },
}

/// Thread-safe hardware controller handle.
pub struct EcDevice {
    transport: Mutex<HardwareTransport>,
    backend: EcBackendKind,
    registers: RegisterMap,
}

impl EcDevice {
    /// Open the safest available read/write control transport without modifying kernel state.
    pub fn open() -> Result<Self, String> {
        let model = read_dmi_field("product_name");
        let registers = RegisterMap::for_model(&model);
        let posture = read_security_posture();

        // Only open raw ec_sys debugfs when Kernel Lockdown is NOT active and the node already exists.
        if !posture.lockdown.is_locked_down() && Path::new(EC_SYS_PATH).exists() {
            if let Ok(file) = OpenOptions::new().read(true).write(true).open(EC_SYS_PATH) {
                return Ok(Self {
                    transport: Mutex::new(HardwareTransport::EcSys(file)),
                    backend: EcBackendKind::EcSys,
                    registers,
                });
            }
        }

        // Fallback to signed kernel sysfs interfaces (100% EAC Linux v2 & Lockdown compatible).
        let platform_profile = Path::new(PLATFORM_PROFILE_PATH)
            .exists()
            .then(|| PathBuf::from(PLATFORM_PROFILE_PATH))
            .filter(|p| OpenOptions::new().write(true).open(p).is_ok());

        let battery_threshold = find_battery_threshold_path()
            .or_else(find_acer_wmi_battery_path)
            .filter(|p| OpenOptions::new().write(true).open(p).is_ok());

        if platform_profile.is_some() || battery_threshold.is_some() {
            return Ok(Self {
                transport: Mutex::new(HardwareTransport::SignedSysfs {
                    platform_profile,
                    battery_threshold,
                }),
                backend: EcBackendKind::SignedSysfs,
                registers,
            });
        }

        let reason = if posture.lockdown.is_locked_down() {
            format!(
                "Kernel Lockdown ({}) active; no writable platform_profile/battery sysfs controls exposed",
                posture.lockdown.label()
            )
        } else {
            "No writable ec_sys debugfs or sysfs hardware controls available (run via pkexec/sudo)"
                .to_string()
        };
        Err(reason)
    }

    pub fn backend_kind(&self) -> EcBackendKind {
        self.backend
    }

    pub fn register_map(&self) -> RegisterMap {
        self.registers
    }

    /// Read all 256 bytes of EC register space when using `ec_sys`.
    fn read_ec_buffer(file: &mut File) -> Result<[u8; 256], String> {
        let mut buf = [0u8; 256];
        file.seek(SeekFrom::Start(0))
            .map_err(|e| format!("EC seek failed: {e}"))?;
        file.read_exact(&mut buf)
            .map_err(|e| format!("EC read failed: {e}"))?;
        Ok(buf)
    }

    fn read_ec_byte(file: &mut File, addr: u8) -> Result<u8, String> {
        let mut byte = [0u8; 1];
        file.seek(SeekFrom::Start(addr as u64))
            .map_err(|e| format!("EC seek failed: {e}"))?;
        file.read_exact(&mut byte)
            .map_err(|e| format!("EC read failed @ {addr:#04x}: {e}"))?;
        Ok(byte[0])
    }

    fn write_ec_byte(file: &mut File, addr: u8, value: u8) -> Result<(), String> {
        file.seek(SeekFrom::Start(addr as u64))
            .map_err(|e| format!("EC seek failed: {e}"))?;
        file.write_all(&[value])
            .map_err(|e| format!("EC write failed @ {addr:#04x}={value:#04x}: {e}"))?;
        file.flush().map_err(|e| format!("EC flush failed: {e}"))
    }

    fn ensure_fan_control_enabled(&self, file: &mut File) -> Result<(), String> {
        let r = self.registers;
        if r.fan_enable_mask == 0 {
            return Ok(());
        }
        let cur = Self::read_ec_byte(file, r.reg_battery_limit)?;
        let next = r.update_fan_enable_byte(cur);
        if next != cur {
            Self::write_ec_byte(file, r.reg_battery_limit, next)?;
        }
        Ok(())
    }

    /// Read and decode a full `EcSnapshot`.
    pub fn read_snapshot(&self) -> Result<EcSnapshot, String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        let r = self.registers;

        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                let buf = Self::read_ec_buffer(file)?;
                let cpu_t = buf[r.reg_cpu_temp as usize];
                let gpu_t = buf[r.reg_gpu_temp as usize];
                let sys_t = buf[r.reg_sys_temp as usize];

                Ok(EcSnapshot {
                    backend: EcBackendKind::EcSys,
                    register_profile: r.profile_name.to_string(),
                    cpu_temp: (cpu_t > 0).then_some(cpu_t),
                    gpu_temp: (gpu_t > 0).then_some(gpu_t),
                    sys_temp: (sys_t > 0).then_some(sys_t),
                    cpu_rpm: Some(r.decode_rpm(&buf, FanUnit::Cpu)),
                    gpu_rpm: Some(r.decode_rpm(&buf, FanUnit::Gpu)),
                    cpu_fan_mode: Some(
                        r.decode_fan_mode(FanUnit::Cpu, buf[r.reg_cpu_fan_mode as usize]),
                    ),
                    gpu_fan_mode: Some(
                        r.decode_fan_mode(FanUnit::Gpu, buf[r.reg_gpu_fan_mode as usize]),
                    ),
                    cpu_manual_speed: Some(buf[r.reg_cpu_manual_speed as usize].min(100)),
                    gpu_manual_speed: Some(buf[r.reg_gpu_manual_speed as usize].min(100)),
                    power_mode: Some(r.decode_power_mode(buf[r.reg_nitro_mode as usize])),
                    ac_plugged: Some((buf[r.reg_power_status as usize] & r.power_plugged_val) != 0),
                    battery_limit_active: Some(
                        find_acer_wmi_battery_path()
                            .and_then(|p| fs::read_to_string(p).ok())
                            .map(|s| s.trim() == "1")
                            .unwrap_or_else(|| {
                                r.is_battery_limit_enabled(buf[r.reg_battery_limit as usize])
                            }),
                    ),
                    kb_backlight_timeout: Some(
                        r.is_kb_timeout_enabled(buf[r.reg_kb_timeout as usize]),
                    ),
                    usb_charge_poweroff: Some(
                        r.is_usb_charge_enabled(buf[r.reg_usb_charge as usize]),
                    ),
                    nbfc_active: false,
                })
            }
            HardwareTransport::SignedSysfs { .. } => {
                let sys = read_sysfs_snapshot();
                Ok(EcSnapshot {
                    backend: EcBackendKind::SignedSysfs,
                    register_profile: r.profile_name.to_string(),
                    cpu_temp: sys.cpu_temp,
                    gpu_temp: sys.gpu_temp,
                    sys_temp: sys.acpi_temp,
                    cpu_rpm: sys.cpu_rpm,
                    gpu_rpm: sys.gpu_rpm,
                    power_mode: sys.power_mode,
                    ac_plugged: sys.ac_plugged,
                    battery_limit_active: find_acer_wmi_battery_path()
                        .and_then(|p| fs::read_to_string(p).ok())
                        .map(|s| s.trim() == "1")
                        .or_else(|| sys.charge_threshold.map(|t| t <= 85)),
                    ..Default::default()
                })
            }
        }
    }

    pub fn set_power_mode(&self, mode: PowerMode) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                let val = self.registers.encode_power_mode(mode);
                Self::write_ec_byte(file, self.registers.reg_nitro_mode, val)
            }
            HardwareTransport::SignedSysfs {
                platform_profile: Some(path),
                ..
            } => fs::write(path, format!("{}\n", mode.platform_profile_str()))
                .map_err(|e| format!("Failed writing platform_profile: {e}")),
            HardwareTransport::SignedSysfs {
                platform_profile: None,
                ..
            } => Err("Kernel platform_profile is not exposed on this machine".to_string()),
        }
    }

    pub fn set_fan_mode(&self, unit: FanUnit, mode: EcFanMode, speed_pct: u8) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                if mode != EcFanMode::Auto {
                    self.ensure_fan_control_enabled(file)?;
                }
                match mode {
                    EcFanMode::Manual => {
                        Self::write_ec_byte(
                            file,
                            self.registers.fan_speed_register(unit),
                            speed_pct.min(100),
                        )?;
                    }
                    EcFanMode::Turbo => {
                        Self::write_ec_byte(file, self.registers.fan_speed_register(unit), 100)?;
                    }
                    EcFanMode::Auto => {}
                }
                let val = self.registers.encode_fan_mode(unit, mode);
                Self::write_ec_byte(file, self.registers.fan_mode_register(unit), val)
            }
            HardwareTransport::SignedSysfs { .. } => Err(
                "Manual EC fan control requires Lockdown=none with ec_sys; use Performance Profile under Kernel Lockdown"
                    .to_string(),
            ),
        }
    }

    pub fn set_fan_speed(&self, unit: FanUnit, speed_pct: u8) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                self.ensure_fan_control_enabled(file)?;
                Self::write_ec_byte(
                    file,
                    self.registers.fan_speed_register(unit),
                    speed_pct.min(100),
                )
            }
            HardwareTransport::SignedSysfs { .. } => Err(
                "Manual EC fan speed requires Lockdown=none with ec_sys"
                    .to_string(),
            ),
        }
    }

    pub fn set_battery_limit(&self, enabled: bool) -> Result<(), String> {
        if let Some(wmi_path) = find_acer_wmi_battery_path() {
            if OpenOptions::new().write(true).open(&wmi_path).is_ok() {
                let val = if enabled { "1\n" } else { "0\n" };
                return fs::write(&wmi_path, val)
                    .map_err(|e| format!("Failed writing Acer WMI battery limit: {e}"));
            }
        }
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                let cur = Self::read_ec_byte(file, self.registers.reg_battery_limit)?;
                let next = self.registers.update_battery_limit_byte(cur, enabled);
                Self::write_ec_byte(file, self.registers.reg_battery_limit, next)
            }
            HardwareTransport::SignedSysfs {
                battery_threshold: Some(path),
                ..
            } => {
                let val = if enabled { "80\n" } else { "100\n" };
                fs::write(path, val).map_err(|e| format!("Failed writing charge threshold: {e}"))
            }
            HardwareTransport::SignedSysfs {
                battery_threshold: None,
                ..
            } => Err("Kernel charge_control_end_threshold is not exposed on this machine".to_string()),
        }
    }

    pub fn set_kb_timeout(&self, enabled: bool) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                let val = if enabled {
                    self.registers.kb_timeout_on
                } else {
                    self.registers.kb_timeout_off
                };
                Self::write_ec_byte(file, self.registers.reg_kb_timeout, val)
            }
            HardwareTransport::SignedSysfs { .. } => {
                Err("Keyboard timeout toggle requires ec_sys".to_string())
            }
        }
    }

    pub fn set_usb_charge(&self, enabled: bool) -> Result<(), String> {
        let mut guard = self
            .transport
            .lock()
            .map_err(|_| "Hardware mutex poisoned".to_string())?;
        match &mut *guard {
            HardwareTransport::EcSys(file) => {
                let cur = Self::read_ec_byte(file, self.registers.reg_usb_charge)?;
                let next = self.registers.update_usb_charge_byte(cur, enabled);
                Self::write_ec_byte(file, self.registers.reg_usb_charge, next)
            }
            HardwareTransport::SignedSysfs { .. } => {
                Err("USB power-off charging toggle requires ec_sys".to_string())
            }
        }
    }
}
