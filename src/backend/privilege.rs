//! Zero-systemd privilege bridge for hardware control.
//!
//! When running unprivileged inside a Wayland/X11 desktop session, the GUI spawns
//! `pkexec <self> --ec-helper` with a sanitized environment over anonymous
//! stdin/stdout JSON-line pipes. The helper terminates immediately when the
//! GUI exits or drops `HardwareController`.

use std::env;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use crate::backend::ec::EcDevice;
use crate::backend::sysfs::{
    detect_conflicting_fan_daemon, find_nbfc_bin, is_nbfc_ready, read_hardware_info,
    read_nbfc_status, read_sysfs_snapshot,
};
use crate::backend::types::{
    EcBackendKind, EcFanMode, EcSnapshot, FanUnit, HardwareInfo, HelperRequest, HelperResponse,
    PowerMode, TelemetrySnapshot,
};

const MAX_IPC_LINE_BYTES: u64 = 64 * 1024;

struct HelperProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    backend: EcBackendKind,
}

impl HelperProcess {
    fn spawn() -> Result<Self, String> {
        let exe = env::current_exe()
            .map_err(|e| format!("Failed to locate executable: {e}"))?
            .canonicalize()
            .map_err(|e| format!("Failed to resolve executable path: {e}"))?;

        let meta = std::fs::metadata(&exe)
            .map_err(|e| format!("Failed to inspect executable: {e}"))?;
        if !meta.is_file() {
            return Err("Resolved executable path is not a regular file".to_string());
        }

        let mut cmd = Command::new("pkexec");
        cmd.arg(&exe)
            .arg("--ec-helper")
            .env_clear()
            .env("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to launch pkexec: {e}"))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Missing helper stdin pipe".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Missing helper stdout pipe".to_string())?;
        let mut reader = BufReader::new(stdout);

        let line = read_bounded_line(&mut reader)?;
        if line.is_empty() {
            let output = child.wait_with_output();
            let stderr_msg = output
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "Authentication dismissed or helper exited".to_string());
            return Err(stderr_msg);
        }

        let resp: HelperResponse = serde_json::from_str(line.trim())
            .map_err(|e| format!("Invalid helper handshake: {e}"))?;

        match resp {
            HelperResponse::Ready { backend, .. } => Ok(Self {
                child,
                stdin,
                stdout: reader,
                backend,
            }),
            HelperResponse::Error { message } => {
                let _ = child.kill();
                let _ = child.wait();
                Err(message)
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                Err("Unexpected handshake response from helper".to_string())
            }
        }
    }

    fn send(&mut self, req: &HelperRequest) -> Result<HelperResponse, String> {
        let payload = serde_json::to_string(req).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{payload}").map_err(|e| format!("Helper pipe write failed: {e}"))?;
        self.stdin
            .flush()
            .map_err(|e| format!("Helper pipe flush failed: {e}"))?;

        let line = read_bounded_line(&mut self.stdout)?;
        if line.is_empty() {
            return Err("Hardware helper process exited unexpectedly".to_string());
        }

        serde_json::from_str(line.trim()).map_err(|e| format!("Invalid helper JSON: {e}"))
    }
}

impl Drop for HelperProcess {
    fn drop(&mut self) {
        if let Ok(payload) = serde_json::to_string(&HelperRequest::Quit) {
            let _ = writeln!(self.stdin, "{payload}");
            let _ = self.stdin.flush();
        }
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

fn read_bounded_line<R: BufRead>(reader: &mut R) -> Result<String, String> {
    let mut line = String::new();
    let n = reader
        .take(MAX_IPC_LINE_BYTES)
        .read_line(&mut line)
        .map_err(|e| format!("Helper pipe read failed: {e}"))?;
    if n as u64 >= MAX_IPC_LINE_BYTES {
        return Err("IPC response exceeded maximum line size".to_string());
    }
    Ok(line)
}

enum EcBridge {
    Direct(EcDevice),
    Helper(HelperProcess),
    Locked(Option<String>),
}

/// Thread-safe hardware controller coordinating elevated writes and unprivileged sysfs telemetry.
pub struct HardwareController {
    hardware_info: HardwareInfo,
    bridge: Mutex<EcBridge>,
}

impl HardwareController {
    pub fn new() -> Self {
        let mut hardware_info = read_hardware_info();
        let bridge = match EcDevice::open() {
            Ok(ec) => {
                hardware_info.register_profile = ec.register_map().profile_name.to_string();
                EcBridge::Direct(ec)
            }
            Err(_) => EcBridge::Locked(None),
        };

        Self {
            hardware_info,
            bridge: Mutex::new(bridge),
        }
    }

    pub fn hardware_info(&self) -> HardwareInfo {
        self.hardware_info.clone()
    }

    pub fn backend_kind(&self) -> EcBackendKind {
        let Ok(guard) = self.bridge.lock() else {
            return EcBackendKind::Locked;
        };
        match &*guard {
            EcBridge::Direct(ec) => ec.backend_kind(),
            EcBridge::Helper(h) => h.backend,
            EcBridge::Locked(_) => EcBackendKind::Locked,
        }
    }

    pub fn unlock(&self) -> Result<EcBackendKind, String> {
        {
            let guard = self
                .bridge
                .lock()
                .map_err(|_| "Controller mutex poisoned".to_string())?;
            match &*guard {
                EcBridge::Direct(ec) => return Ok(ec.backend_kind()),
                EcBridge::Helper(h) => return Ok(h.backend),
                EcBridge::Locked(_) => {}
            }
        }

        if let Ok(ec) = EcDevice::open() {
            let kind = ec.backend_kind();
            let mut guard = self
                .bridge
                .lock()
                .map_err(|_| "Controller mutex poisoned".to_string())?;
            *guard = EcBridge::Direct(ec);
            return Ok(kind);
        }

        match HelperProcess::spawn() {
            Ok(helper) => {
                let kind = helper.backend;
                let mut guard = self
                    .bridge
                    .lock()
                    .map_err(|_| "Controller mutex poisoned".to_string())?;
                *guard = EcBridge::Helper(helper);
                Ok(kind)
            }
            Err(err) => {
                if let Ok(mut guard) = self.bridge.lock() {
                    *guard = EcBridge::Locked(Some(err.clone()));
                }
                Err(err)
            }
        }
    }

    pub fn poll_telemetry(&self) -> TelemetrySnapshot {
        let sys = read_sysfs_snapshot();
        let nbfc_active = is_nbfc_ready();
        let conflict = detect_conflicting_fan_daemon()
            .filter(|name| !(nbfc_active && name == "nbfc_service"));

        let mut snap = TelemetrySnapshot {
            backend: EcBackendKind::Locked,
            ec_error: None,
            hardware: self.hardware_info.clone(),
            cpu_temp: sys.cpu_temp,
            gpu_temp: sys.gpu_temp,
            sys_temp: sys.acpi_temp,
            nvme_temp: sys.nvme_temp,
            cpu_rpm: sys.cpu_rpm,
            gpu_rpm: sys.gpu_rpm,
            cpu_fan_mode: None,
            gpu_fan_mode: None,
            cpu_manual_speed: None,
            gpu_manual_speed: None,
            power_mode: sys.power_mode,
            ac_plugged: sys.ac_plugged,
            battery_percent: sys.battery_percent,
            battery_status: sys.battery_status,
            battery_power_watts: sys.battery_power_watts,
            battery_limit_active: sys.charge_threshold.map(|t| t <= 85),
            kb_backlight_timeout: None,
            usb_charge_poweroff: None,
            conflicting_daemon: conflict,
            nbfc_active,
        };

        if nbfc_active {
            if let Some(nbfc) = read_nbfc_status() {
                if let Some(cpu) = nbfc.cpu {
                    snap.cpu_fan_mode = Some(cpu.mode);
                    snap.cpu_manual_speed = Some(cpu.speed_pct);
                }
                if let Some(gpu) = nbfc.gpu {
                    snap.gpu_fan_mode = Some(gpu.mode);
                    snap.gpu_manual_speed = Some(gpu.speed_pct);
                }
            }
        }

        let Ok(mut guard) = self.bridge.lock() else {
            return snap;
        };

        let ec_result: Option<Result<EcSnapshot, String>> = match &mut *guard {
            EcBridge::Direct(ec) => Some(ec.read_snapshot()),
            EcBridge::Helper(helper) => Some(match helper.send(&HelperRequest::GetEcSnapshot) {
                Ok(HelperResponse::Snapshot { data }) => Ok(data),
                Ok(HelperResponse::Error { message }) => Err(message),
                Ok(_) => Err("Unexpected response from helper".to_string()),
                Err(e) => Err(e),
            }),
            EcBridge::Locked(err) => {
                snap.ec_error = err.clone();
                None
            }
        };

        if let Some(res) = ec_result {
            match res {
                Ok(ec) => {
                    snap.backend = ec.backend;
                    snap.hardware.register_profile = ec.register_profile;
                    if let Some(t) = ec.cpu_temp {
                        snap.cpu_temp = Some(t);
                    }
                    if let Some(t) = ec.gpu_temp {
                        snap.gpu_temp = Some(t);
                    }
                    if let Some(t) = ec.sys_temp {
                        snap.sys_temp = Some(t);
                    }
                    if let Some(r) = ec.cpu_rpm {
                        snap.cpu_rpm = Some(r);
                    }
                    if let Some(r) = ec.gpu_rpm {
                        snap.gpu_rpm = Some(r);
                    }
                    if ec.cpu_fan_mode.is_some() {
                        snap.cpu_fan_mode = ec.cpu_fan_mode;
                    }
                    if ec.gpu_fan_mode.is_some() {
                        snap.gpu_fan_mode = ec.gpu_fan_mode;
                    }
                    if ec.cpu_manual_speed.is_some() {
                        snap.cpu_manual_speed = ec.cpu_manual_speed;
                    }
                    if ec.gpu_manual_speed.is_some() {
                        snap.gpu_manual_speed = ec.gpu_manual_speed;
                    }
                    if ec.power_mode.is_some() {
                        snap.power_mode = ec.power_mode;
                    }
                    if snap.ac_plugged.is_none() && ec.ac_plugged.is_some() {
                        snap.ac_plugged = ec.ac_plugged;
                    }
                    if ec.nbfc_active {
                        snap.nbfc_active = true;
                    }
                    if ec.battery_limit_active.is_some() {
                        snap.battery_limit_active = ec.battery_limit_active;
                    }
                    if ec.kb_backlight_timeout.is_some() {
                        snap.kb_backlight_timeout = ec.kb_backlight_timeout;
                    }
                    if ec.usb_charge_poweroff.is_some() {
                        snap.usb_charge_poweroff = ec.usb_charge_poweroff;
                    }
                }
                Err(err) => {
                    if matches!(&*guard, EcBridge::Helper(_))
                        && err.contains("exited unexpectedly")
                    {
                        *guard = EcBridge::Locked(Some(err.clone()));
                    }
                    snap.ec_error = Some(err);
                }
            }
        }

        snap
    }

    fn execute(&self, req: HelperRequest) -> Result<(), String> {
        let mut guard = self
            .bridge
            .lock()
            .map_err(|_| "Controller mutex poisoned".to_string())?;
        match &mut *guard {
            EcBridge::Direct(ec) => dispatch_direct(ec, req).map(|_| ()),
            EcBridge::Helper(helper) => match helper.send(&req)? {
                HelperResponse::Ok { .. } => Ok(()),
                HelperResponse::Error { message } => Err(message),
                _ => Err("Unexpected response from helper".to_string()),
            },
            EcBridge::Locked(_) => {
                Err("Hardware control is locked. Click Unlock Control first.".to_string())
            }
        }
    }

    pub fn set_power_mode(&self, mode: PowerMode) -> Result<(), String> {
        self.execute(HelperRequest::SetPowerMode { mode })
    }

    pub fn set_fan_mode(&self, unit: FanUnit, mode: EcFanMode, speed_pct: u8) -> Result<(), String> {
        if is_nbfc_ready() && apply_nbfc_fan_mode(unit, mode, speed_pct).is_ok() {
            return Ok(());
        }
        self.execute(HelperRequest::SetFanMode {
            unit,
            mode,
            speed_pct,
        })
    }

    pub fn set_fan_speed(&self, unit: FanUnit, speed_pct: u8) -> Result<(), String> {
        if is_nbfc_ready() && apply_nbfc_fan_mode(unit, EcFanMode::Manual, speed_pct).is_ok() {
            return Ok(());
        }
        self.execute(HelperRequest::SetFanSpeed { unit, speed_pct })
    }

    pub fn set_battery_limit(&self, enabled: bool) -> Result<(), String> {
        self.execute(HelperRequest::SetBatteryLimit { enabled })
    }

    pub fn set_kb_timeout(&self, enabled: bool) -> Result<(), String> {
        self.execute(HelperRequest::SetKbTimeout { enabled })
    }

    pub fn set_usb_charge(&self, enabled: bool) -> Result<(), String> {
        self.execute(HelperRequest::SetUsbCharge { enabled })
    }

    /// Stop an allowlisted conflicting fan daemon (`nbfc_service` / `openitrod`) on user request.
    pub fn stop_conflicting_daemon(&self, name: &str) -> Result<(), String> {
        validate_daemon_name(name)?;
        if self.backend_kind().is_unlocked() {
            return self.execute(HelperRequest::StopConflictingDaemon {
                name: name.to_string(),
            });
        }
        let status = Command::new("pkexec")
            .args(["systemctl", "stop", name])
            .env_clear()
            .env(
                "PATH",
                "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
            )
            .status()
            .map_err(|e| format!("Failed to invoke pkexec systemctl: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("Could not stop {name} (exit status {status})"))
        }
    }
}

fn validate_daemon_name(name: &str) -> Result<&'static str, String> {
    match name {
        "nbfc_service" => Ok("nbfc_service"),
        "openitrod" => Ok("openitrod"),
        _ => Err("Unsupported daemon target".to_string()),
    }
}

impl Default for HardwareController {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_nbfc_fan_mode(unit: FanUnit, mode: EcFanMode, speed_pct: u8) -> Result<(), String> {
    let bin = find_nbfc_bin().ok_or_else(|| "nbfc CLI binary not found".to_string())?;
    let fan_idx = match unit {
        FanUnit::Cpu => "0",
        FanUnit::Gpu => "1",
    };
    let speed_arg = match mode {
        EcFanMode::Turbo => "100".to_string(),
        EcFanMode::Manual => speed_pct.min(100).to_string(),
        EcFanMode::Auto => String::new(),
    };

    let mut cmd = Command::new(bin);
    cmd.arg("set");
    match mode {
        EcFanMode::Auto => {
            cmd.arg("-a");
        }
        EcFanMode::Turbo | EcFanMode::Manual => {
            cmd.args(["-s", &speed_arg]);
        }
    }
    cmd.args(["-f", fan_idx])
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to invoke {bin}: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let detail = if stderr.is_empty() {
            format!("exit status {}", output.status)
        } else {
            stderr
        };
        Err(format!("nbfc set failed: {detail}"))
    }
}

fn dispatch_direct(ec: &EcDevice, req: HelperRequest) -> Result<HelperResponse, String> {
    match req {
        HelperRequest::Init => Ok(HelperResponse::Ready {
            backend: ec.backend_kind(),
            register_profile: ec.register_map().profile_name.to_string(),
        }),
        HelperRequest::GetEcSnapshot => {
            let mut data = ec.read_snapshot()?;
            if is_nbfc_ready() {
                data.nbfc_active = true;
                if let Some(nbfc) = read_nbfc_status() {
                    if let Some(cpu) = nbfc.cpu {
                        data.cpu_fan_mode = Some(cpu.mode);
                        data.cpu_manual_speed = Some(cpu.speed_pct);
                    }
                    if let Some(gpu) = nbfc.gpu {
                        data.gpu_fan_mode = Some(gpu.mode);
                        data.gpu_manual_speed = Some(gpu.speed_pct);
                    }
                }
            }
            Ok(HelperResponse::Snapshot { data })
        }
        HelperRequest::SetPowerMode { mode } => {
            ec.set_power_mode(mode)?;
            Ok(HelperResponse::Ok {
                message: format!("Power mode set to {}", mode.label()),
            })
        }
        HelperRequest::SetFanMode {
            unit,
            mode,
            speed_pct,
        } => {
            if is_nbfc_ready() {
                apply_nbfc_fan_mode(unit, mode, speed_pct)?;
            } else {
                ec.set_fan_mode(unit, mode, speed_pct)?;
            }
            Ok(HelperResponse::Ok {
                message: format!("{} fan mode updated", unit.label()),
            })
        }
        HelperRequest::SetFanSpeed { unit, speed_pct } => {
            if is_nbfc_ready() {
                apply_nbfc_fan_mode(unit, EcFanMode::Manual, speed_pct)?;
            } else {
                ec.set_fan_speed(unit, speed_pct)?;
            }
            Ok(HelperResponse::Ok {
                message: format!("{} fan speed set to {}%", unit.label(), speed_pct.min(100)),
            })
        }
        HelperRequest::SetBatteryLimit { enabled } => {
            ec.set_battery_limit(enabled)?;
            Ok(HelperResponse::Ok {
                message: format!(
                    "80% battery limit {}",
                    if enabled { "enabled" } else { "disabled" }
                ),
            })
        }
        HelperRequest::SetKbTimeout { enabled } => {
            ec.set_kb_timeout(enabled)?;
            Ok(HelperResponse::Ok {
                message: format!(
                    "Keyboard 30s timeout {}",
                    if enabled { "enabled" } else { "disabled" }
                ),
            })
        }
        HelperRequest::SetUsbCharge { enabled } => {
            ec.set_usb_charge(enabled)?;
            Ok(HelperResponse::Ok {
                message: format!(
                    "USB power-off charging {}",
                    if enabled { "enabled" } else { "disabled" }
                ),
            })
        }
        HelperRequest::StopConflictingDaemon { name } => {
            let safe_name = validate_daemon_name(&name)?;
            let status = Command::new("systemctl")
                .args(["stop", safe_name])
                .env_clear()
                .env(
                    "PATH",
                    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
                )
                .status()
                .map_err(|e| format!("Failed to invoke systemctl: {e}"))?;
            if status.success() {
                Ok(HelperResponse::Ok {
                    message: format!("Stopped {safe_name}"),
                })
            } else {
                Err(format!("Failed to stop {safe_name} (status {status})"))
            }
        }
        HelperRequest::Quit => Ok(HelperResponse::Ok {
            message: "Exiting".to_string(),
        }),
    }
}

/// Entrypoint for `--ec-helper`, spawned inside `pkexec` by the unprivileged GUI.
pub fn run_ec_helper_loop() -> i32 {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let ec = match EcDevice::open() {
        Ok(ec) => ec,
        Err(message) => {
            if let Ok(line) = serde_json::to_string(&HelperResponse::Error { message }) {
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
            }
            return 1;
        }
    };

    let ready = HelperResponse::Ready {
        backend: ec.backend_kind(),
        register_profile: ec.register_map().profile_name.to_string(),
    };
    if let Ok(line) = serde_json::to_string(&ready) {
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    while let Ok(raw) = read_bounded_line(&mut reader) {
        if raw.is_empty() {
            break;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<HelperRequest>(trimmed) {
            Ok(HelperRequest::Quit) => {
                if let Ok(msg) = serde_json::to_string(&HelperResponse::Ok {
                    message: "Goodbye".to_string(),
                }) {
                    let _ = writeln!(out, "{msg}");
                    let _ = out.flush();
                }
                break;
            }
            Ok(req) => match dispatch_direct(&ec, req) {
                Ok(r) => r,
                Err(message) => HelperResponse::Error { message },
            },
            Err(e) => HelperResponse::Error {
                message: format!("Malformed request: {e}"),
            },
        };

        if let Ok(out_line) = serde_json::to_string(&resp) {
            if writeln!(out, "{out_line}").is_err() || out.flush().is_err() {
                break;
            }
        }
    }

    0
}
