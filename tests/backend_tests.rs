//! Unit and integration tests for OpeNitro's pure-Rust backend.

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use openitro::backend::config::{AppConfig, AppThemeKind};
use openitro::backend::curves::{CurvePoint, CurvePreset, FanCurve, HysteresisTracker};
use openitro::backend::devices::{RegisterMap, read_dmi_field_from};
use openitro::backend::sysfs::{
    detect_conflicting_daemon_in, is_nvidia_gpu_active_in, parse_lockdown_str, parse_millidegrees,
    parse_nbfc_status, parse_nvidia_smi_temp,
};
use openitro::backend::types::{
    EcBackendKind, EcFanMode, FanMode, FanUnit, HelperRequest, HelperResponse, LockdownMode,
    PowerMode,
};

fn temp_dir_path(tag: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("openitro_test_{tag}_{nanos}"))
}

#[test]
fn register_map_standard_and_amd_overrides() {
    let std_map = RegisterMap::for_model("Nitro AN515-57");
    assert_eq!(std_map.reg_gpu_temp, 0xB6);
    assert_eq!(std_map.reg_sys_temp, 0xB3);
    assert_eq!(std_map.battery_limit_on, 0x51);
    assert_eq!(std_map.battery_limit_off, 0x11);

    let amd_map = RegisterMap::for_model("Nitro AN515-44");
    assert_eq!(amd_map.reg_gpu_temp, 0xB4);
    assert_eq!(amd_map.reg_sys_temp, 0xB0);
    assert_eq!(amd_map.battery_limit_on, 0x40);
    assert_eq!(amd_map.battery_limit_off, 0x00);

    let pred_map = RegisterMap::for_model("Predator PH315-54");
    assert_eq!(pred_map.profile_name, "Predator / Helios Base EC");
}

#[test]
fn register_map_encode_decode_modes_and_rpm() {
    let map = RegisterMap::standard();

    for unit in [FanUnit::Cpu, FanUnit::Gpu] {
        for mode in [EcFanMode::Auto, EcFanMode::Turbo, EcFanMode::Manual] {
            let encoded = map.encode_fan_mode(unit, mode);
            assert_eq!(map.decode_fan_mode(unit, encoded), mode);
        }
    }

    for pm in [PowerMode::Quiet, PowerMode::Default, PowerMode::Extreme] {
        let encoded = map.encode_power_mode(pm);
        assert_eq!(map.decode_power_mode(encoded), pm);
    }

    let mut ec_buf = [0u8; 256];
    // 4200 RPM = 0x1068 -> low reg (0x14) = 0x10, high reg (0x13) = 0x68
    ec_buf[map.reg_cpu_rpm_low as usize] = 0x10;
    ec_buf[map.reg_cpu_rpm_high as usize] = 0x68;
    assert_eq!(map.decode_rpm(&ec_buf, FanUnit::Cpu), 4200);
}

#[test]
fn fan_curve_interpolation_and_clamping() {
    let mut curve = FanCurve {
        points: vec![
            CurvePoint::new(80, 90),
            CurvePoint::new(40, 20),
            CurvePoint::new(60, 50),
        ],
        hysteresis_c: 3,
    };
    curve.normalize();

    assert_eq!(curve.points[0].temp_c, 40);
    assert_eq!(curve.points[1].temp_c, 60);
    assert_eq!(curve.points[2].temp_c, 80);

    // Below minimum / above maximum
    assert_eq!(curve.evaluate(25), 20);
    assert_eq!(curve.evaluate(95), 90);

    // Midpoint interpolation: halfway between (40,20) and (60,50) is 50°C -> 35%
    assert_eq!(curve.evaluate(50), 35);
    assert_eq!(curve.evaluate(70), 70);
}

#[test]
fn fan_curve_hysteresis_prevents_oscillation() {
    let curve = FanCurve {
        points: CurvePreset::Balanced.points(),
        hysteresis_c: 4,
    };
    let mut tracker = HysteresisTracker::default();

    let s1 = curve.evaluate_with_hysteresis(70, &mut tracker);
    // Small 2°C dip stays held at 70°C because hysteresis is 4°C
    let s2 = curve.evaluate_with_hysteresis(68, &mut tracker);
    assert_eq!(s1, s2);

    // 5°C drop exceeds hysteresis (4°C), so target speed steps down
    let s3 = curve.evaluate_with_hysteresis(65, &mut tracker);
    assert!(s3 < s1);
}

#[test]
fn sysfs_millidegrees_and_dmi_helpers() {
    assert_eq!(parse_millidegrees("54321\n"), Some(54));
    assert_eq!(parse_millidegrees("0\n"), None);
    assert_eq!(parse_millidegrees("-5000\n"), None);
    assert_eq!(parse_millidegrees("not-a-number"), None);

    let dir = temp_dir_path("dmi");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("product_name"), "  Nitro AN515-57 \n").unwrap();
    assert_eq!(read_dmi_field_from(&dir, "product_name"), "Nitro AN515-57");
    assert_eq!(read_dmi_field_from(&dir, "missing_file"), "Unknown");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn config_roundtrip_and_sanitization() {
    let dir = temp_dir_path("cfg");
    let path = dir.join("config.json");

    let cfg = AppConfig {
        cpu_fan_mode: FanMode::Curve,
        cpu_manual_speed: 85,
        power_mode: PowerMode::Extreme,
        battery_limit_active: true,
        theme: AppThemeKind::Light,
        ..Default::default()
    };
    cfg.save_to(&path).unwrap();

    let loaded = AppConfig::load_from(&path);
    assert_eq!(loaded, cfg);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lockdown_and_platform_profile_parsing() {
    assert_eq!(
        parse_lockdown_str("[none] integrity confidentiality"),
        LockdownMode::None
    );
    assert_eq!(
        parse_lockdown_str("none [integrity] confidentiality"),
        LockdownMode::Integrity
    );
    assert_eq!(
        parse_lockdown_str("none integrity [confidentiality]"),
        LockdownMode::Confidentiality
    );
    assert!(!LockdownMode::None.is_locked_down());
    assert!(LockdownMode::Integrity.is_locked_down());
    assert!(LockdownMode::Confidentiality.is_locked_down());

    assert_eq!(
        PowerMode::from_platform_profile("low-power\n"),
        Some(PowerMode::Quiet)
    );
    assert_eq!(
        PowerMode::from_platform_profile("balanced"),
        Some(PowerMode::Default)
    );
    assert_eq!(
        PowerMode::from_platform_profile("performance"),
        Some(PowerMode::Extreme)
    );
    assert!(EcBackendKind::EcSys.supports_raw_fan_ec());
    assert!(!EcBackendKind::SignedSysfs.supports_raw_fan_ec());
    assert!(!EcBackendKind::Locked.supports_raw_fan_ec());
}

#[test]
fn detects_conflicting_fan_daemon_from_proc() {
    let dir = temp_dir_path("proc");
    let pid_dir = dir.join("1234");
    fs::create_dir_all(&pid_dir).unwrap();
    fs::write(pid_dir.join("comm"), "bash\n").unwrap();
    assert_eq!(detect_conflicting_daemon_in(&dir), None);

    let nbfc_dir = dir.join("837");
    fs::create_dir_all(&nbfc_dir).unwrap();
    fs::write(nbfc_dir.join("comm"), "nbfc_service\n").unwrap();
    assert_eq!(
        detect_conflicting_daemon_in(&dir),
        Some("nbfc_service".to_string())
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn ipc_protocol_serialization_roundtrip() {
    let req = HelperRequest::SetFanMode {
        unit: FanUnit::Gpu,
        mode: EcFanMode::Manual,
        speed_pct: 75,
    };
    let line = serde_json::to_string(&req).unwrap();
    let decoded: HelperRequest = serde_json::from_str(&line).unwrap();
    match decoded {
        HelperRequest::SetFanMode {
            unit,
            mode,
            speed_pct,
        } => {
            assert_eq!(unit, FanUnit::Gpu);
            assert_eq!(mode, EcFanMode::Manual);
            assert_eq!(speed_pct, 75);
        }
        _ => panic!("unexpected request variant"),
    }

    let resp = HelperResponse::Ready {
        backend: EcBackendKind::EcSys,
        register_profile: "Standard Acer EC".to_string(),
    };
    let resp_line = serde_json::to_string(&resp).unwrap();
    let decoded_resp: HelperResponse = serde_json::from_str(&resp_line).unwrap();
    match decoded_resp {
        HelperResponse::Ready {
            backend,
            register_profile,
        } => {
            assert_eq!(backend, EcBackendKind::EcSys);
            assert_eq!(register_profile, "Standard Acer EC");
        }
        _ => panic!("unexpected response variant"),
    }
}

#[test]
fn register_map_bitmask_rmw_and_usb_kb_decoding() {
    let map = RegisterMap::for_model("Nitro AN515-57");
    assert_eq!(map.fan_enable_mask, 0x11);
    assert_eq!(map.battery_limit_mask, 0x40);

    // Starting from 0x00 (e.g. after nbfc reset): enabling battery sets 0x40 | 0x11 => 0x51
    // so Acer Nitro EC receives the valid combined control byte, never bare 0x40.
    let bat_on = map.update_battery_limit_byte(0x00, true);
    assert_eq!(bat_on, 0x51);
    assert!(map.is_battery_limit_enabled(bat_on));

    let both = map.update_fan_enable_byte(bat_on);
    assert_eq!(both, 0x51);
    assert!(map.is_battery_limit_enabled(both));

    // Disabling battery limit leaves manual fan enable bits intact (0x11).
    let fan_only = map.update_battery_limit_byte(both, false);
    assert_eq!(fan_only, 0x11);
    assert!(!map.is_battery_limit_enabled(fan_only));

    // USB power-off charge uses bit 0x10 as disable bit while preserving lower bits.
    assert!(map.is_usb_charge_enabled(0x0F));
    assert!(!map.is_usb_charge_enabled(0x1F));
    assert_eq!(map.update_usb_charge_byte(0x1F, true), 0x0F);
    assert_eq!(map.update_usb_charge_byte(0x0F, false), 0x1F);

    // Keyboard timeout any non-zero value is active.
    assert!(!map.is_kb_timeout_enabled(0x00));
    assert!(map.is_kb_timeout_enabled(0x1E));
    assert!(map.is_kb_timeout_enabled(0x01));

    let legacy = RegisterMap::for_model("Nitro AN515-51");
    assert_eq!(legacy.fan_enable_mask, 0x00);
}

#[test]
fn nvidia_smi_parser_and_pci_runtime_gate() {
    assert_eq!(parse_nvidia_smi_temp("40.56\n"), Some(41));
    assert_eq!(parse_nvidia_smi_temp("62\n"), Some(62));
    assert_eq!(parse_nvidia_smi_temp("0\n"), None);
    assert_eq!(parse_nvidia_smi_temp("N/A\n"), None);

    let dir = temp_dir_path("pci");
    let gpu = dir.join("0000:01:00.0");
    fs::create_dir_all(gpu.join("power")).unwrap();
    fs::write(gpu.join("vendor"), "0x10de\n").unwrap();
    fs::write(gpu.join("class"), "0x030000\n").unwrap();
    fs::write(gpu.join("power/runtime_status"), "suspended\n").unwrap();
    assert!(!is_nvidia_gpu_active_in(&dir));

    fs::write(gpu.join("power/runtime_status"), "active\n").unwrap();
    assert!(is_nvidia_gpu_active_in(&dir));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn parses_nbfc_status_output() {
    let sample = "\
Service enabled          : true
Read-only                : false
Selected Config Name     : Acer Nitro AN515-57 Custom
Temperature              : 45.20

Fan Display Name         : CPU fan
Auto Control Enabled     : true
Critical Mode Enabled    : false
Current Fan Speed        : 31.00
Target Fan Speed         : 31.00

Fan Display Name         : GPU fan
Auto Control Enabled     : false
Critical Mode Enabled    : false
Current Fan Speed        : 100.00
Target Fan Speed         : 100.00
";
    let parsed = parse_nbfc_status(sample);
    let cpu = parsed.cpu.expect("cpu fan");
    assert_eq!(cpu.mode, EcFanMode::Auto);
    assert_eq!(cpu.speed_pct, 31);

    let gpu = parsed.gpu.expect("gpu fan");
    assert_eq!(gpu.mode, EcFanMode::Turbo);
    assert_eq!(gpu.speed_pct, 100);
}
