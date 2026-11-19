//! User configuration persistence (`~/.config/openitro/config.json`).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backend::curves::FanCurve;
use crate::backend::types::{FanMode, PowerMode};

/// Visual theme mode backed by `gpui-kit`'s native `ThemeMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppThemeKind {
    #[default]
    Dark,
    Light,
}

impl AppThemeKind {
    pub fn label(self) -> &'static str {
        match self {
            AppThemeKind::Dark => "Dark",
            AppThemeKind::Light => "Light",
        }
    }
}

/// Persistent OpeNitro user preferences and fan curves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub cpu_fan_mode: FanMode,
    pub gpu_fan_mode: FanMode,
    pub cpu_manual_speed: u8,
    pub gpu_manual_speed: u8,
    pub cpu_curve: FanCurve,
    pub gpu_curve: FanCurve,
    pub power_mode: PowerMode,
    pub battery_limit_active: bool,
    pub kb_backlight_timeout: bool,
    pub usb_charge_poweroff: bool,
    pub theme: AppThemeKind,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cpu_fan_mode: FanMode::Auto,
            gpu_fan_mode: FanMode::Auto,
            cpu_manual_speed: 65,
            gpu_manual_speed: 65,
            cpu_curve: FanCurve::default(),
            gpu_curve: FanCurve::default(),
            power_mode: PowerMode::Default,
            battery_limit_active: false,
            kb_backlight_timeout: true,
            usb_charge_poweroff: false,
            theme: AppThemeKind::Dark,
        }
    }
}

impl AppConfig {
    /// Resolve standard XDG config file path (`$XDG_CONFIG_HOME/openitro/config.json`).
    pub fn default_path() -> PathBuf {
        if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                return PathBuf::from(xdg).join("openitro").join("config.json");
            }
        }
        if let Ok(home) = env::var("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join("openitro")
                .join("config.json");
        }
        PathBuf::from(".openitro.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn exists() -> bool {
        Self::default_path().is_file()
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(raw) = fs::read_to_string(path) else {
            return Self::default();
        };
        let mut cfg: Self = serde_json::from_str(&raw).unwrap_or_default();
        cfg.cpu_curve.normalize();
        cfg.gpu_curve.normalize();
        cfg.cpu_manual_speed = cfg.cpu_manual_speed.min(100);
        cfg.gpu_manual_speed = cfg.gpu_manual_speed.min(100);
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::default_path())
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create config dir: {e}"))?;
            }
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        fs::write(path, json).map_err(|e| format!("Failed to write config: {e}"))
    }
}
