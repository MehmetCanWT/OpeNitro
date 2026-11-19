//! Built-in CLI mode for headless status queries and EC control.

use crate::backend::privilege::HardwareController;
use crate::backend::types::{EcFanMode, FanUnit, PowerMode};

/// Handle CLI arguments if present. Returns `Some(exit_code)` when CLI mode ran,
/// or `None` when the binary should launch the GPUI desktop window.
pub fn maybe_run_cli(args: &[String]) -> Option<i32> {
    if args.len() <= 1 {
        return None;
    }

    let flags = &args[1..];
    if flags.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Some(0);
    }

    let controller = HardwareController::new();
    let mut json_output = false;
    let mut show_status = false;
    let mut power_mode: Option<PowerMode> = None;
    let mut cpu_fan: Option<EcFanMode> = None;
    let mut cpu_speed: u8 = 65;
    let mut gpu_fan: Option<EcFanMode> = None;
    let mut gpu_speed: u8 = 65;
    let mut battery_limit: Option<bool> = None;
    let mut kb_timeout: Option<bool> = None;
    let mut usb_charge: Option<bool> = None;

    let mut i = 0;
    while i < flags.len() {
        match flags[i].as_str() {
            "--status" | "-s" => show_status = true,
            "--json" | "-j" => json_output = true,
            "--power" | "-p" => {
                i += 1;
                let Some(val) = flags.get(i) else {
                    eprintln!("Missing value for --power (quiet|default|extreme)");
                    return Some(2);
                };
                power_mode = match val.to_lowercase().as_str() {
                    "quiet" => Some(PowerMode::Quiet),
                    "default" | "balanced" => Some(PowerMode::Default),
                    "extreme" => Some(PowerMode::Extreme),
                    other => {
                        eprintln!("Invalid power mode '{other}' (expected quiet|default|extreme)");
                        return Some(2);
                    }
                };
            }
            "--cpu-fan" => {
                i += 1;
                let Some(val) = flags.get(i) else {
                    eprintln!("Missing value for --cpu-fan (auto|turbo|manual)");
                    return Some(2);
                };
                cpu_fan = parse_fan_mode(val);
                if cpu_fan.is_none() {
                    eprintln!("Invalid fan mode '{val}' (expected auto|turbo|manual)");
                    return Some(2);
                }
            }
            "--cpu-speed" => {
                i += 1;
                let Some(val) = flags.get(i).and_then(|s| s.parse::<u8>().ok()) else {
                    eprintln!("Invalid --cpu-speed (expected 0-100)");
                    return Some(2);
                };
                cpu_speed = val.min(100);
            }
            "--gpu-fan" => {
                i += 1;
                let Some(val) = flags.get(i) else {
                    eprintln!("Missing value for --gpu-fan (auto|turbo|manual)");
                    return Some(2);
                };
                gpu_fan = parse_fan_mode(val);
                if gpu_fan.is_none() {
                    eprintln!("Invalid fan mode '{val}' (expected auto|turbo|manual)");
                    return Some(2);
                }
            }
            "--gpu-speed" => {
                i += 1;
                let Some(val) = flags.get(i).and_then(|s| s.parse::<u8>().ok()) else {
                    eprintln!("Invalid --gpu-speed (expected 0-100)");
                    return Some(2);
                };
                gpu_speed = val.min(100);
            }
            "--battery-limit" | "-b" => {
                i += 1;
                let Some(val) = flags.get(i).and_then(|s| parse_on_off(s)) else {
                    eprintln!("Invalid --battery-limit (expected on|off)");
                    return Some(2);
                };
                battery_limit = Some(val);
            }
            "--kb-timeout" | "-k" => {
                i += 1;
                let Some(val) = flags.get(i).and_then(|s| parse_on_off(s)) else {
                    eprintln!("Invalid --kb-timeout (expected on|off)");
                    return Some(2);
                };
                kb_timeout = Some(val);
            }
            "--usb-charge" | "-u" => {
                i += 1;
                let Some(val) = flags.get(i).and_then(|s| parse_on_off(s)) else {
                    eprintln!("Invalid --usb-charge (expected on|off)");
                    return Some(2);
                };
                usb_charge = Some(val);
            }
            other => {
                eprintln!("Unknown option: {other}");
                print_help();
                return Some(2);
            }
        }
        i += 1;
    }

    let mut exit_code = 0;

    if let Some(mode) = power_mode {
        if let Err(e) = controller.set_power_mode(mode) {
            eprintln!("Error setting power mode: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("Power mode -> {}", mode.label());
        }
    }
    if let Some(mode) = cpu_fan {
        if let Err(e) = controller.set_fan_mode(FanUnit::Cpu, mode, cpu_speed) {
            eprintln!("Error setting CPU fan: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("CPU fan -> {:?} ({cpu_speed}%)", mode);
        }
    }
    if let Some(mode) = gpu_fan {
        if let Err(e) = controller.set_fan_mode(FanUnit::Gpu, mode, gpu_speed) {
            eprintln!("Error setting GPU fan: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("GPU fan -> {:?} ({gpu_speed}%)", mode);
        }
    }
    if let Some(enabled) = battery_limit {
        if let Err(e) = controller.set_battery_limit(enabled) {
            eprintln!("Error setting battery limit: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("Battery limit -> {}", if enabled { "80%" } else { "100%" });
        }
    }
    if let Some(enabled) = kb_timeout {
        if let Err(e) = controller.set_kb_timeout(enabled) {
            eprintln!("Error setting keyboard timeout: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("Keyboard 30s timeout -> {}", if enabled { "on" } else { "off" });
        }
    }
    if let Some(enabled) = usb_charge {
        if let Err(e) = controller.set_usb_charge(enabled) {
            eprintln!("Error setting USB charging: {e} (run with sudo for EC writes)");
            exit_code = 1;
        } else {
            println!("USB power-off charging -> {}", if enabled { "on" } else { "off" });
        }
    }

    if show_status {
        let snap = controller.poll_telemetry();
        if json_output {
            match serde_json::to_string_pretty(&snap) {
                Ok(text) => println!("{text}"),
                Err(e) => {
                    eprintln!("Serialization error: {e}");
                    return Some(1);
                }
            }
        } else {
            println!("OpeNitro — {} ({})", snap.hardware.model, snap.backend.label());
            println!(
                "  Integrity: Lockdown={}, TPM2={}, EAC-v2 Ready={}",
                snap.hardware.security.lockdown.label(),
                snap.hardware.security.tpm_present,
                snap.hardware.security.eac_linux_v2_ready
            );
            println!(
                "  CPU: {}°C  |  GPU: {}°C  |  SYS: {}°C",
                opt_deg(snap.cpu_temp),
                opt_deg(snap.gpu_temp),
                opt_deg(snap.sys_temp)
            );
            println!(
                "  CPU Fan: {} RPM ({:?})  |  GPU Fan: {} RPM ({:?})",
                opt_u16(snap.cpu_rpm),
                snap.cpu_fan_mode,
                opt_u16(snap.gpu_rpm),
                snap.gpu_fan_mode
            );
            println!(
                "  Power: {:?}  |  AC: {:?}  |  Battery: {}% ({})",
                snap.power_mode,
                snap.ac_plugged,
                opt_deg(snap.battery_percent),
                snap.battery_status.as_deref().unwrap_or("Unknown")
            );
        }
    }

    Some(exit_code)
}

fn parse_fan_mode(s: &str) -> Option<EcFanMode> {
    match s.to_lowercase().as_str() {
        "auto" => Some(EcFanMode::Auto),
        "turbo" | "max" => Some(EcFanMode::Turbo),
        "manual" => Some(EcFanMode::Manual),
        _ => None,
    }
}

fn parse_on_off(s: &str) -> Option<bool> {
    match s.to_lowercase().as_str() {
        "on" | "1" | "true" => Some(true),
        "off" | "0" | "false" => Some(false),
        _ => None,
    }
}

fn opt_deg(v: Option<u8>) -> String {
    v.map_or_else(|| "--".to_string(), |x| x.to_string())
}

fn opt_u16(v: Option<u16>) -> String {
    v.map_or_else(|| "--".to_string(), |x| x.to_string())
}

fn print_help() {
    println!(
        "\
OpeNitro — Acer Nitro & Predator Linux System Controller
Run with no arguments to open the GPU-accelerated desktop GUI.

USAGE:
  openitro [OPTIONS]

OPTIONS:
  -s, --status                  Print live thermal & fan status
  -j, --json                    Output status as JSON (with --status)
  -p, --power <quiet|default|extreme>
  -b, --battery-limit <on|off>  Toggle 80% battery charge limit
  -k, --kb-timeout <on|off>     Toggle keyboard backlight 30s timeout
  -u, --usb-charge <on|off>     Toggle USB power-off charging
      --cpu-fan <auto|turbo|manual>
      --cpu-speed <0-100>       Manual CPU fan percentage
      --gpu-fan <auto|turbo|manual>
      --gpu-speed <0-100>       Manual GPU fan percentage
  -h, --help                    Show this help"
    );
}
