//! Root GPUI Kit (`https://gpui-kit.com/`) desktop application view for OpeNitro.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::base::{Selectable as _, StyledExt as _};
use gpui_kit::component::{
    ActiveTheme as _, Collapsible as _, Disableable as _, Icon, IconName, Sizable as _, Theme,
    ThemeMode, WindowExt as _,
    button::{Button, ButtonGroup, ButtonVariants as _},
    chart::AreaChart,
    group_box::{GroupBox, GroupBoxVariants as _},
    h_flex,
    notification::Notification,
    progress::Progress,
    scroll::ScrollableElement as _,
    separator::Separator,
    sidebar::{
        Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
        SidebarToggleButton,
    },
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
    tag::Tag,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, Window, div, px,
};

use crate::backend::config::{AppConfig, AppThemeKind};
use crate::backend::curves::{CurvePreset, FanCurve, HysteresisTracker};
use crate::backend::privilege::HardwareController;
use crate::backend::types::{EcFanMode, FanMode, FanUnit, PowerMode, TelemetrySnapshot};
use crate::ui::curve_canvas::{CurveChanged, CurveEditor};
use crate::ui::fan_rotor::render_fan_rotor;

const HISTORY_CAPACITY: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePage {
    Dashboard,
    Curves,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToggleKind {
    BatteryLimit,
    KbTimeout,
    UsbCharge,
}

#[derive(Clone)]
struct ThermalSample {
    label: SharedString,
    cpu: f64,
    gpu: f64,
}

/// Main OpeNitro desktop application state.
pub struct OpeNitroApp {
    focus_handle: FocusHandle,
    controller: Arc<HardwareController>,
    config: AppConfig,
    telemetry: TelemetrySnapshot,
    active_page: ActivePage,
    nav_collapsed_override: Option<bool>,
    unlocking: bool,
    stopping_daemon: bool,

    // Bounded thermal history & session extremes
    cpu_min: Option<u8>,
    cpu_max: Option<u8>,
    gpu_min: Option<u8>,
    gpu_max: Option<u8>,
    history: VecDeque<ThermalSample>,
    sample_seq: u64,

    // Fan animation & hysteresis curve state
    cpu_angle: f32,
    gpu_angle: f32,
    cpu_hysteresis: HysteresisTracker,
    gpu_hysteresis: HysteresisTracker,
    last_curve_cpu_speed: Option<u8>,
    last_curve_gpu_speed: Option<u8>,
    last_cpu_write: Option<Instant>,
    last_gpu_write: Option<Instant>,
    last_power_write: Option<Instant>,
    last_battery_write: Option<Instant>,
    last_kb_write: Option<Instant>,
    last_usb_write: Option<Instant>,
    pending_battery_verify: Option<bool>,
    config_existed_at_boot: bool,
    saved_config_applied: bool,

    // Reusable stateful controls (no per-frame allocation)
    editing_curve_unit: FanUnit,
    curve_editor: Entity<CurveEditor>,
    cpu_speed_slider: Entity<SliderState>,
    gpu_speed_slider: Entity<SliderState>,
    hysteresis_slider: Entity<SliderState>,

    // Owned tasks & subscriptions dropped cleanly when window closes
    _subscriptions: Vec<Subscription>,
    _poll_task: Task<()>,
    _anim_task: Task<()>,
}

impl OpeNitroApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let config_existed_at_boot = AppConfig::exists();
        let config = AppConfig::load();
        apply_theme_mode(config.theme, window, cx);

        let controller = Arc::new(HardwareController::new());
        let initial_telemetry = controller.poll_telemetry();

        let curve_editor = cx.new(|_| CurveEditor::new(config.cpu_curve.clone()));

        let cpu_speed_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .default_value(SliderValue::Single(config.cpu_manual_speed as f32))
                .step(1.0)
        });

        let gpu_speed_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .default_value(SliderValue::Single(config.gpu_manual_speed as f32))
                .step(1.0)
        });

        let hysteresis_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(8.0)
                .default_value(SliderValue::Single(config.cpu_curve.hysteresis_c as f32))
                .step(1.0)
        });

        let mut subscriptions = Vec::with_capacity(4);

        subscriptions.push(cx.subscribe_in(
            &cpu_speed_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(val) => {
                    this.config.cpu_manual_speed = val.start().round().clamp(0.0, 100.0) as u8;
                    cx.notify();
                }
                SliderEvent::Release(val) => {
                    let pct = val.start().round().clamp(0.0, 100.0) as u8;
                    this.set_manual_speed(FanUnit::Cpu, pct, window, cx);
                }
            },
        ));

        subscriptions.push(cx.subscribe_in(
            &gpu_speed_slider,
            window,
            |this, _, event: &SliderEvent, window, cx| match event {
                SliderEvent::Change(val) => {
                    this.config.gpu_manual_speed = val.start().round().clamp(0.0, 100.0) as u8;
                    cx.notify();
                }
                SliderEvent::Release(val) => {
                    let pct = val.start().round().clamp(0.0, 100.0) as u8;
                    this.set_manual_speed(FanUnit::Gpu, pct, window, cx);
                }
            },
        ));

        subscriptions.push(cx.subscribe(
            &hysteresis_slider,
            |this, _, event: &SliderEvent, cx| {
                let (SliderEvent::Change(val) | SliderEvent::Release(val)) = event;
                let hyst = val.start().round().clamp(0.0, 8.0) as u8;
                match this.editing_curve_unit {
                    FanUnit::Cpu => {
                        this.config.cpu_curve.hysteresis_c = hyst;
                        let c = this.config.cpu_curve.clone();
                        this.curve_editor.update(cx, |ed, cx| ed.set_curve(c, cx));
                    }
                    FanUnit::Gpu => {
                        this.config.gpu_curve.hysteresis_c = hyst;
                        let c = this.config.gpu_curve.clone();
                        this.curve_editor.update(cx, |ed, cx| ed.set_curve(c, cx));
                    }
                }
                if matches!(event, SliderEvent::Release(_)) {
                    let _ = this.config.save();
                }
                cx.notify();
            },
        ));

        subscriptions.push(cx.subscribe(&curve_editor, |this, _, ev: &CurveChanged, cx| {
            let curve = ev.curve.clone();
            match this.editing_curve_unit {
                FanUnit::Cpu => this.config.cpu_curve = curve,
                FanUnit::Gpu => this.config.gpu_curve = curve,
            }
            let _ = this.config.save();
            cx.notify();
        }));

        let poll_task = Self::spawn_polling_loop(&controller, window, cx);
        let anim_task = Self::spawn_animation_loop(window, cx);

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            controller,
            config,
            telemetry: initial_telemetry.clone(),
            active_page: ActivePage::Dashboard,
            nav_collapsed_override: None,
            unlocking: false,
            stopping_daemon: false,
            cpu_min: None,
            cpu_max: None,
            gpu_min: None,
            gpu_max: None,
            history: VecDeque::with_capacity(HISTORY_CAPACITY),
            sample_seq: 0,
            cpu_angle: 0.0,
            gpu_angle: 0.0,
            cpu_hysteresis: HysteresisTracker::default(),
            gpu_hysteresis: HysteresisTracker::default(),
            last_curve_cpu_speed: None,
            last_curve_gpu_speed: None,
            last_cpu_write: None,
            last_gpu_write: None,
            last_power_write: None,
            last_battery_write: None,
            last_kb_write: None,
            last_usb_write: None,
            pending_battery_verify: None,
            config_existed_at_boot,
            saved_config_applied: false,
            editing_curve_unit: FanUnit::Cpu,
            curve_editor,
            cpu_speed_slider,
            gpu_speed_slider,
            hysteresis_slider,
            _subscriptions: subscriptions,
            _poll_task: poll_task,
            _anim_task: anim_task,
        };

        if initial_telemetry.backend.is_unlocked() && app.config_existed_at_boot {
            app.telemetry = initial_telemetry;
            app.apply_saved_config(window, cx);
        } else {
            app.ingest_telemetry(initial_telemetry, window, cx);
        }
        app
    }

    fn spawn_polling_loop(
        controller: &Arc<HardwareController>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<()> {
        let controller = controller.clone();
        let this = cx.entity().downgrade();
        window.spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(1))
                    .await;

                let ctrl = controller.clone();
                let snap = cx
                    .background_executor()
                    .spawn(async move { ctrl.poll_telemetry() })
                    .await;

                let updated = this.update_in(cx, |app, window, cx| {
                    app.ingest_telemetry(snap, window, cx);
                    app.evaluate_fan_curves(cx);
                });
                if updated.is_err() {
                    break;
                }
            }
        })
    }

    fn spawn_animation_loop(window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        let this = cx.entity().downgrade();
        window.spawn(cx, async move |cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(50))
                    .await;

                let updated = this.update_in(cx, |app, window, cx| {
                    if app.active_page != ActivePage::Dashboard || !window.is_window_active() {
                        return;
                    }
                    let cpu_rpm = app.telemetry.cpu_rpm.unwrap_or(0);
                    let gpu_rpm = app.telemetry.gpu_rpm.unwrap_or(0);
                    if cpu_rpm == 0 && gpu_rpm == 0 {
                        return;
                    }
                    if cpu_rpm > 0 {
                        let step = ((cpu_rpm as f32) / 5000.0 * 21.0).clamp(2.0, 32.0);
                        app.cpu_angle = (app.cpu_angle + step) % 360.0;
                    }
                    if gpu_rpm > 0 {
                        let step = ((gpu_rpm as f32) / 5000.0 * 21.0).clamp(2.0, 32.0);
                        app.gpu_angle = (app.gpu_angle + step) % 360.0;
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
    }

    fn ingest_telemetry(
        &mut self,
        snap: TelemetrySnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut changed = snap != self.telemetry;

        if let Some(t) = snap.cpu_temp.filter(|&v| v > 0) {
            self.cpu_min = Some(self.cpu_min.map_or(t, |m| m.min(t)));
            self.cpu_max = Some(self.cpu_max.map_or(t, |m| m.max(t)));
        }
        if let Some(t) = snap.gpu_temp.filter(|&v| v > 0) {
            self.gpu_min = Some(self.gpu_min.map_or(t, |m| m.min(t)));
            self.gpu_max = Some(self.gpu_max.map_or(t, |m| m.max(t)));
        }

        if snap.cpu_temp.is_some() || snap.gpu_temp.is_some() {
            if let Some(gpu_t) = snap.gpu_temp.filter(|&v| v > 0) {
                for s in &mut self.history {
                    if s.gpu == 0.0 {
                        s.gpu = gpu_t as f64;
                    }
                }
            }
            self.sample_seq += 1;
            if self.history.len() >= HISTORY_CAPACITY {
                self.history.pop_front();
            }
            let gpu_val = snap
                .gpu_temp
                .or_else(|| self.history.back().map(|s| s.gpu as u8))
                .unwrap_or(0) as f64;
            self.history.push_back(ThermalSample {
                label: SharedString::from(format!("{}s", self.sample_seq)),
                cpu: snap.cpu_temp.unwrap_or(0) as f64,
                gpu: gpu_val,
            });
            changed = true;
        }

        let active_temp = match self.editing_curve_unit {
            FanUnit::Cpu => snap.cpu_temp,
            FanUnit::Gpu => snap.gpu_temp,
        };
        self.curve_editor
            .update(cx, |ed, cx| ed.set_live_temp(active_temp, cx));

        let in_grace = |ts: Option<Instant>| ts.is_some_and(|t| t.elapsed() < Duration::from_millis(2500));
        let cpu_grace = in_grace(self.last_cpu_write);
        let gpu_grace = in_grace(self.last_gpu_write);
        let power_grace = in_grace(self.last_power_write);
        let battery_grace = in_grace(self.last_battery_write);
        let kb_grace = in_grace(self.last_kb_write);
        let usb_grace = in_grace(self.last_usb_write);

        let sync_hardware_state = snap.backend.is_unlocked() || snap.nbfc_active;
        if snap.backend.is_unlocked() {
            if !power_grace {
                if let Some(pm) = snap.power_mode {
                    self.config.power_mode = pm;
                }
            }
            if !battery_grace {
                if let Some(bat) = snap.battery_limit_active {
                    if self.pending_battery_verify.take() == Some(true) && !bat {
                        window.push_notification(
                            Notification::warning(
                                "EC/BIOS reverted 80% Battery Limit — this firmware requires an Acer WMI driver (79772EC5); OpeNitro avoids out-of-tree kernel modules for EAC safety.",
                            ),
                            cx,
                        );
                    }
                    if self.config.battery_limit_active != bat {
                        self.config.battery_limit_active = bat;
                        let _ = self.config.save();
                    }
                }
            }
            if !kb_grace {
                if let Some(kb) = snap.kb_backlight_timeout {
                    self.config.kb_backlight_timeout = kb;
                }
            }
            if !usb_grace {
                if let Some(usb) = snap.usb_charge_poweroff {
                    self.config.usb_charge_poweroff = usb;
                }
            }
        }

        if sync_hardware_state {
            if !cpu_grace && self.config.cpu_fan_mode != FanMode::Curve {
                if let Some(ec_mode) = snap.cpu_fan_mode {
                    self.config.cpu_fan_mode = ec_mode_to_ui(ec_mode);
                }
                if let Some(spd) = snap.cpu_manual_speed {
                    if self.config.cpu_manual_speed != spd {
                        self.config.cpu_manual_speed = spd;
                        self.cpu_speed_slider
                            .update(cx, |s, cx| s.set_value(spd as f32, window, cx));
                    }
                }
            }

            if !gpu_grace && self.config.gpu_fan_mode != FanMode::Curve {
                if let Some(ec_mode) = snap.gpu_fan_mode {
                    self.config.gpu_fan_mode = ec_mode_to_ui(ec_mode);
                }
                if let Some(spd) = snap.gpu_manual_speed {
                    if self.config.gpu_manual_speed != spd {
                        self.config.gpu_manual_speed = spd;
                        self.gpu_speed_slider
                            .update(cx, |s, cx| s.set_value(spd as f32, window, cx));
                    }
                }
            }
        }

        self.telemetry = snap;
        if changed {
            cx.notify();
        }
    }

    fn evaluate_fan_curves(&mut self, cx: &mut Context<Self>) {
        if !self.telemetry.backend.supports_raw_fan_ec() && !self.telemetry.nbfc_active {
            return;
        }

        if self.config.cpu_fan_mode == FanMode::Curve {
            if let Some(temp) = self.telemetry.cpu_temp {
                let target = self
                    .config
                    .cpu_curve
                    .evaluate_with_hysteresis(temp, &mut self.cpu_hysteresis);
                let ec_drifted = self.telemetry.cpu_fan_mode != Some(EcFanMode::Manual)
                    || self
                        .telemetry
                        .cpu_manual_speed
                        .is_some_and(|s| s.abs_diff(target) >= 2);
                let should_write = ec_drifted
                    || self
                        .last_curve_cpu_speed
                        .is_none_or(|prev| prev.abs_diff(target) >= 2);
                if should_write {
                    let ctrl = self.controller.clone();
                    self.last_curve_cpu_speed = Some(target);
                    self.last_cpu_write = Some(Instant::now());
                    cx.background_spawn(async move {
                        let _ = ctrl.set_fan_mode(FanUnit::Cpu, EcFanMode::Manual, target);
                    })
                    .detach();
                }
            }
        }

        if self.config.gpu_fan_mode == FanMode::Curve {
            if let Some(temp) = self.telemetry.gpu_temp {
                let target = self
                    .config
                    .gpu_curve
                    .evaluate_with_hysteresis(temp, &mut self.gpu_hysteresis);
                let ec_drifted = self.telemetry.gpu_fan_mode != Some(EcFanMode::Manual)
                    || self
                        .telemetry
                        .gpu_manual_speed
                        .is_some_and(|s| s.abs_diff(target) >= 2);
                let should_write = ec_drifted
                    || self
                        .last_curve_gpu_speed
                        .is_none_or(|prev| prev.abs_diff(target) >= 2);
                if should_write {
                    let ctrl = self.controller.clone();
                    self.last_curve_gpu_speed = Some(target);
                    self.last_gpu_write = Some(Instant::now());
                    cx.background_spawn(async move {
                        let _ = ctrl.set_fan_mode(FanUnit::Gpu, EcFanMode::Manual, target);
                    })
                    .detach();
                }
            }
        }
    }

    fn trigger_unlock(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.unlocking {
            return;
        }
        self.unlocking = true;
        cx.notify();

        let controller = self.controller.clone();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { controller.unlock() })
                    .await;

                let _ = this.update_in(cx, |app, window, cx| {
                    app.unlocking = false;
                    match result {
                        Ok(kind) => {
                            if app.config_existed_at_boot && !app.saved_config_applied {
                                app.apply_saved_config(window, cx);
                            } else {
                                let snap = app.controller.poll_telemetry();
                                app.ingest_telemetry(snap, window, cx);
                            }
                            window.push_notification(
                                Notification::success(format!(
                                    "Hardware control unlocked via {}",
                                    kind.label()
                                )),
                                cx,
                            );
                        }
                        Err(err) => {
                            window.push_notification(
                                Notification::warning(format!("Unlock unavailable: {err}")),
                                cx,
                            );
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
    }

    /// Re-apply saved persistent settings from `config.json` once hardware control is unlocked.
    fn apply_saved_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.saved_config_applied = true;
        let now = Instant::now();
        self.last_power_write = Some(now);
        self.last_battery_write = Some(now);
        self.last_kb_write = Some(now);
        self.last_usb_write = Some(now);
        self.last_cpu_write = Some(now);
        self.last_gpu_write = Some(now);

        let saved = self.config.clone();
        let cpu_temp = self.telemetry.cpu_temp.unwrap_or(55);
        let gpu_temp = self.telemetry.gpu_temp.unwrap_or(55);
        let cpu_speed = if saved.cpu_fan_mode == FanMode::Curve {
            let s = saved
                .cpu_curve
                .evaluate_with_hysteresis(cpu_temp, &mut self.cpu_hysteresis);
            self.last_curve_cpu_speed = Some(s);
            s
        } else {
            saved.cpu_manual_speed
        };
        let gpu_speed = if saved.gpu_fan_mode == FanMode::Curve {
            let s = saved
                .gpu_curve
                .evaluate_with_hysteresis(gpu_temp, &mut self.gpu_hysteresis);
            self.last_curve_gpu_speed = Some(s);
            s
        } else {
            saved.gpu_manual_speed
        };

        let ctrl = self.controller.clone();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let snap = cx
                    .background_executor()
                    .spawn(async move {
                        let _ = ctrl.set_power_mode(saved.power_mode);
                        let _ = ctrl.set_battery_limit(saved.battery_limit_active);
                        let _ = ctrl.set_kb_timeout(saved.kb_backlight_timeout);
                        let _ = ctrl.set_usb_charge(saved.usb_charge_poweroff);
                        let _ = ctrl.set_fan_mode(FanUnit::Cpu, saved.cpu_fan_mode.ec_mode(), cpu_speed);
                        let _ = ctrl.set_fan_mode(FanUnit::Gpu, saved.gpu_fan_mode.ec_mode(), gpu_speed);
                        ctrl.poll_telemetry()
                    })
                    .await;
                let _ = this.update_in(cx, |app, window, cx| {
                    app.ingest_telemetry(snap, window, cx);
                });
            })
            .detach();
    }

    fn set_fan_mode(
        &mut self,
        unit: FanUnit,
        mode: FanMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let speed = match unit {
            FanUnit::Cpu => {
                self.last_cpu_write = Some(Instant::now());
                self.config.cpu_fan_mode = mode;
                if mode == FanMode::Curve {
                    self.cpu_hysteresis.reset();
                    self.last_curve_cpu_speed = None;
                    let temp = self.telemetry.cpu_temp.unwrap_or(55);
                    self.config
                        .cpu_curve
                        .evaluate_with_hysteresis(temp, &mut self.cpu_hysteresis)
                } else {
                    self.config.cpu_manual_speed
                }
            }
            FanUnit::Gpu => {
                self.last_gpu_write = Some(Instant::now());
                self.config.gpu_fan_mode = mode;
                if mode == FanMode::Curve {
                    self.gpu_hysteresis.reset();
                    self.last_curve_gpu_speed = None;
                    let temp = self.telemetry.gpu_temp.unwrap_or(55);
                    self.config
                        .gpu_curve
                        .evaluate_with_hysteresis(temp, &mut self.gpu_hysteresis)
                } else {
                    self.config.gpu_manual_speed
                }
            }
        };
        let _ = self.config.save();

        if !self.telemetry.backend.is_unlocked() && !self.telemetry.nbfc_active {
            window.push_notification(
                Notification::warning("Unlock Hardware Control first to apply fan changes"),
                cx,
            );
            cx.notify();
            return;
        }

        let ctrl = self.controller.clone();
        let ec_mode = mode.ec_mode();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move { ctrl.set_fan_mode(unit, ec_mode, speed) })
                    .await;
                let _ = this.update_in(cx, |app, window, cx| match res {
                    Ok(()) => {
                        let snap = app.controller.poll_telemetry();
                        app.ingest_telemetry(snap, window, cx);
                    }
                    Err(err) => window.push_notification(Notification::error(err), cx),
                });
            })
            .detach();
    }

    fn set_manual_speed(
        &mut self,
        unit: FanUnit,
        pct: u8,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match unit {
            FanUnit::Cpu => {
                self.last_cpu_write = Some(Instant::now());
                self.config.cpu_manual_speed = pct;
            }
            FanUnit::Gpu => {
                self.last_gpu_write = Some(Instant::now());
                self.config.gpu_manual_speed = pct;
            }
        }
        let _ = self.config.save();

        if !self.telemetry.backend.is_unlocked() && !self.telemetry.nbfc_active {
            return;
        }

        let ctrl = self.controller.clone();
        window
            .spawn(cx, async move |cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move { ctrl.set_fan_speed(unit, pct) })
                    .await;
                if let Err(err) = res {
                    let _ = cx.update(|window, cx| {
                        window.push_notification(Notification::error(err), cx);
                    });
                }
            })
            .detach();
    }

    fn set_power_mode(&mut self, mode: PowerMode, window: &mut Window, cx: &mut Context<Self>) {
        self.last_power_write = Some(Instant::now());
        self.config.power_mode = mode;
        let _ = self.config.save();

        if !self.telemetry.backend.is_unlocked() {
            window.push_notification(
                Notification::warning("Unlock Hardware Control first to switch performance mode"),
                cx,
            );
            cx.notify();
            return;
        }

        let ctrl = self.controller.clone();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move { ctrl.set_power_mode(mode) })
                    .await;
                let _ = this.update_in(cx, |app, window, cx| match res {
                    Ok(()) => {
                        let snap = app.controller.poll_telemetry();
                        app.ingest_telemetry(snap, window, cx);
                        window.push_notification(
                            Notification::success(format!("Profile switched to {}", mode.label())),
                            cx,
                        );
                    }
                    Err(err) => {
                        app.last_power_write = None;
                        let snap = app.controller.poll_telemetry();
                        app.ingest_telemetry(snap, window, cx);
                        window.push_notification(Notification::error(err), cx);
                    }
                });
            })
            .detach();
    }

    fn apply_toggle(
        &mut self,
        kind: ToggleKind,
        label: &'static str,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Some(Instant::now());
        match kind {
            ToggleKind::BatteryLimit => {
                self.last_battery_write = now;
                self.pending_battery_verify = Some(enabled);
                self.config.battery_limit_active = enabled;
            }
            ToggleKind::KbTimeout => {
                self.last_kb_write = now;
                self.config.kb_backlight_timeout = enabled;
            }
            ToggleKind::UsbCharge => {
                self.last_usb_write = now;
                self.config.usb_charge_poweroff = enabled;
            }
        }
        let _ = self.config.save();

        if !self.telemetry.backend.is_unlocked() {
            window.push_notification(
                Notification::warning("Unlock Hardware Control first to modify hardware settings"),
                cx,
            );
            cx.notify();
            return;
        }

        let ctrl = self.controller.clone();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let res = cx
                    .background_executor()
                    .spawn(async move {
                        match kind {
                            ToggleKind::BatteryLimit => ctrl.set_battery_limit(enabled),
                            ToggleKind::KbTimeout => ctrl.set_kb_timeout(enabled),
                            ToggleKind::UsbCharge => ctrl.set_usb_charge(enabled),
                        }
                    })
                    .await;
                let _ = this.update_in(cx, |app, window, cx| match res {
                    Ok(()) => {
                        let snap = app.controller.poll_telemetry();
                        app.ingest_telemetry(snap, window, cx);
                        window.push_notification(
                            Notification::success(format!(
                                "{label} {}",
                                if enabled { "enabled" } else { "disabled" }
                            )),
                            cx,
                        );
                    }
                    Err(err) => {
                        match kind {
                            ToggleKind::BatteryLimit => {
                                app.last_battery_write = None;
                                app.pending_battery_verify = None;
                            }
                            ToggleKind::KbTimeout => app.last_kb_write = None,
                            ToggleKind::UsbCharge => app.last_usb_write = None,
                        }
                        let snap = app.controller.poll_telemetry();
                        app.ingest_telemetry(snap, window, cx);
                        window.push_notification(Notification::error(err), cx);
                    }
                });
            })
            .detach();
    }

    fn switch_curve_unit(&mut self, unit: FanUnit, window: &mut Window, cx: &mut Context<Self>) {
        self.editing_curve_unit = unit;
        let curve = match unit {
            FanUnit::Cpu => self.config.cpu_curve.clone(),
            FanUnit::Gpu => self.config.gpu_curve.clone(),
        };
        let live_temp = match unit {
            FanUnit::Cpu => self.telemetry.cpu_temp,
            FanUnit::Gpu => self.telemetry.gpu_temp,
        };
        let hyst = curve.hysteresis_c as f32;
        self.hysteresis_slider
            .update(cx, |s, cx| s.set_value(hyst, window, cx));
        self.curve_editor.update(cx, |ed, cx| {
            ed.set_curve(curve, cx);
            ed.set_live_temp(live_temp, cx);
        });
        cx.notify();
    }

    fn apply_curve_preset(
        &mut self,
        preset: CurvePreset,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut curve = match self.editing_curve_unit {
            FanUnit::Cpu => self.config.cpu_curve.clone(),
            FanUnit::Gpu => self.config.gpu_curve.clone(),
        };
        curve.points = preset.points();
        curve.normalize();

        match self.editing_curve_unit {
            FanUnit::Cpu => self.config.cpu_curve = curve.clone(),
            FanUnit::Gpu => self.config.gpu_curve = curve.clone(),
        }
        let _ = self.config.save();
        self.curve_editor
            .update(cx, |ed, cx| ed.set_curve(curve, cx));
        window.push_notification(
            Notification::info(format!(
                "Applied {} preset to {} curve",
                preset.label(),
                self.editing_curve_unit.label()
            )),
            cx,
        );
        cx.notify();
    }

    fn stop_conflicting_daemon(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.stopping_daemon {
            return;
        }
        self.stopping_daemon = true;
        cx.notify();

        let ctrl = self.controller.clone();
        let this = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let target = name.clone();
                let res = cx
                    .background_executor()
                    .spawn(async move { ctrl.stop_conflicting_daemon(&target) })
                    .await;
                let _ = this.update_in(cx, |app, window, cx| {
                    app.stopping_daemon = false;
                    match res {
                        Ok(()) => {
                            let snap = app.controller.poll_telemetry();
                            app.ingest_telemetry(snap, window, cx);
                            window.push_notification(
                                Notification::success(format!(
                                    "Stopped {name} — OpeNitro now has exclusive fan control"
                                )),
                                cx,
                            );
                        }
                        Err(err) => window.push_notification(Notification::error(err), cx),
                    }
                    cx.notify();
                });
            })
            .detach();
    }

    fn reset_all_defaults(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.config = AppConfig::default();
        let _ = self.config.save();
        apply_theme_mode(self.config.theme, window, cx);
        self.cpu_speed_slider
            .update(cx, |s, cx| s.set_value(self.config.cpu_manual_speed as f32, window, cx));
        self.gpu_speed_slider
            .update(cx, |s, cx| s.set_value(self.config.gpu_manual_speed as f32, window, cx));
        self.switch_curve_unit(self.editing_curve_unit, window, cx);
        window.push_notification(Notification::info("Configuration reset to defaults"), cx);
        cx.notify();
    }

    fn render_sidebar(&self, collapsed: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.active_page;
        let unlocked = self.telemetry.backend.is_unlocked();

        Sidebar::new("openitro-sidebar")
            .collapsed(collapsed)
            .collapsible(true)
            .header(
                SidebarHeader::new().child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .when(!collapsed, |this| {
                            this.child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(Icon::new(IconName::Cpu).text_color(cx.theme().primary))
                                    .child(div().font_semibold().text_sm().child("OpeNitro")),
                            )
                        })
                        .child(
                            SidebarToggleButton::new()
                                .collapsed(collapsed)
                                .accessibility_label("Toggle navigation sidebar")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.nav_collapsed_override = Some(!collapsed);
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .child(
                SidebarGroup::new("System").child(
                    SidebarMenu::new()
                        .child(
                            SidebarMenuItem::new("Dashboard")
                                .icon(IconName::LayoutDashboard)
                                .active(active == ActivePage::Dashboard)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_page = ActivePage::Dashboard;
                                    cx.notify();
                                })),
                        )
                        .child(
                            SidebarMenuItem::new("Fan Curves")
                                .icon(IconName::Settings2)
                                .active(active == ActivePage::Curves)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_page = ActivePage::Curves;
                                    cx.notify();
                                })),
                        )
                        .child(
                            SidebarMenuItem::new("Settings & Integrity")
                                .icon(IconName::Settings)
                                .active(active == ActivePage::Settings)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_page = ActivePage::Settings;
                                    cx.notify();
                                })),
                        ),
                ),
            )
            .footer(
                SidebarFooter::new()
                    .collapsed(collapsed)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Icon::new(if unlocked {
                                IconName::CircleCheck
                            } else {
                                IconName::TriangleAlert
                            }).text_color(if unlocked {
                                cx.theme().green
                            } else {
                                cx.theme().yellow
                            }))
                            .when(!collapsed, |this| {
                                this.child(
                                    v_flex()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_medium()
                                                .child(self.telemetry.backend.label()),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                                        ),
                                )
                            }),
                    ),
            )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hw = &self.telemetry.hardware;
        let ac_text = match (
            self.telemetry.ac_plugged,
            self.telemetry.battery_percent,
            self.telemetry.battery_power_watts,
        ) {
            (Some(true), Some(pct), Some(w)) if w > 0.2 => format!("AC · {pct}% ({w:.1}W)"),
            (Some(true), Some(pct), _) => format!("AC · {pct}%"),
            (Some(true), None, _) => "AC Power".to_string(),
            (Some(false), Some(pct), Some(w)) if w > 0.2 => format!("Battery {pct}% (-{w:.1}W)"),
            (Some(false), Some(pct), _) => format!("Battery {pct}%"),
            _ => "Power Unknown".to_string(),
        };
        let sys_text = self
            .telemetry
            .sys_temp
            .map_or_else(|| "SYS --°C".to_string(), |t| format!("SYS {t}°C"));
        let lockdown_tag = if hw.security.eac_linux_v2_ready {
            Tag::success().outline().small().child("EAC v2 Ready")
        } else if hw.security.lockdown.is_locked_down() {
            Tag::info().outline().small().child("Lockdown Active")
        } else {
            Tag::secondary().outline().small().child("Standard Kernel")
        };

        h_flex()
            .w_full()
            .items_center()
            .justify_between()
            .gap_3()
            .flex_wrap()
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_xl().font_bold().child(match self.active_page {
                        ActivePage::Dashboard => "System Telemetry & Cooling",
                        ActivePage::Curves => "Custom Fan Curves",
                        ActivePage::Settings => "Hardware, Security & Preferences",
                    }))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{} · {}", hw.model, hw.register_profile)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .flex_wrap()
                    .child(lockdown_tag)
                    .when(self.telemetry.nbfc_active, |this| {
                        this.child(Tag::info().outline().small().child("NBFC Active"))
                    })
                    .child(Tag::secondary().outline().small().child(sys_text))
                    .child(Tag::secondary().outline().small().child(ac_text))
                    .child(if self.telemetry.backend.is_unlocked() {
                        Tag::success()
                            .outline()
                            .small()
                            .child(self.telemetry.backend.label())
                            .into_any_element()
                    } else {
                        Button::new("unlock-ec-header")
                            .primary()
                            .small()
                            .label(if self.unlocking {
                                "Unlocking…"
                            } else {
                                "Unlock Control"
                            })
                            .disabled(self.unlocking)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.trigger_unlock(window, cx);
                            }))
                            .into_any_element()
                    }),
            )
    }

    fn render_dashboard(&self, is_narrow: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let unlocked = self.telemetry.backend.is_unlocked();
        let cpu_temp = self.telemetry.cpu_temp;
        let gpu_temp = self.telemetry.gpu_temp;
        let history_vec: Vec<ThermalSample> = self.history.iter().cloned().collect();
        let conflict = self.telemetry.conflicting_daemon.clone();
        let nbfc_active = self.telemetry.nbfc_active;

        let cpu_card = self.render_fan_card(
            FanUnit::Cpu,
            "CPU Thermal & Fan",
            cpu_temp,
            self.cpu_min,
            self.cpu_max,
            self.telemetry.cpu_rpm,
            self.cpu_angle,
            self.config.cpu_fan_mode,
            self.config.cpu_manual_speed,
            &self.config.cpu_curve,
            &self.cpu_speed_slider,
            is_narrow,
            cx,
        );
        let gpu_card = self.render_fan_card(
            FanUnit::Gpu,
            "GPU Thermal & Fan",
            gpu_temp,
            self.gpu_min,
            self.gpu_max,
            self.telemetry.gpu_rpm,
            self.gpu_angle,
            self.config.gpu_fan_mode,
            self.config.gpu_manual_speed,
            &self.config.gpu_curve,
            &self.gpu_speed_slider,
            is_narrow,
            cx,
        );

        v_flex()
            .w_full()
            .gap_4()
            .when(!unlocked, |this| {
                this.child(status_strip(
                    cx.theme().warning,
                    IconName::TriangleAlert,
                    "Read-Only Telemetry",
                    "Unlock once via Polkit to enable EC fan control, performance profiles, and Battery Care.",
                    Button::new("unlock-ec-banner")
                        .primary()
                        .small()
                        .label(if self.unlocking {
                            "Authenticating…"
                        } else {
                            "Unlock Control"
                        })
                        .disabled(self.unlocking)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.trigger_unlock(window, cx);
                        })),
                    cx,
                ))
            })
            .when(!self.telemetry.hardware.register_verified, |this| {
                this.child(status_strip(
                    cx.theme().warning,
                    IconName::TriangleAlert,
                    "Unverified Model — Read-Only",
                    format!(
                        "{} is not in the supported list yet, so raw EC writes are disabled. Open an issue with an EC dump on GitHub to add support.",
                        self.telemetry.hardware.model
                    ),
                    div(),
                    cx,
                ))
            })
            .when(nbfc_active, |this| {
                this.child(status_strip(
                    cx.theme().cyan,
                    IconName::CircleCheck,
                    "Fan Control Delegated to nbfc_service",
                    "Auto, Max, Manual, and Curve speeds route cleanly through `nbfc` and persist after exit.",
                    Button::new("stop-nbfc-btn")
                        .outline()
                        .small()
                        .label(if self.stopping_daemon {
                            "Switching…"
                        } else {
                            "Switch to Direct EC"
                        })
                        .disabled(self.stopping_daemon)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.stop_conflicting_daemon(
                                "nbfc_service".to_string(),
                                window,
                                cx,
                            );
                        })),
                    cx,
                ))
            })
            .when_some(conflict, |this, daemon_name| {
                let stop_target = daemon_name.clone();
                this.child(status_strip(
                    cx.theme().red,
                    IconName::TriangleAlert,
                    format!("Conflicting Controller: {daemon_name}"),
                    "External fan daemon is overwriting EC fan registers.",
                    Button::new("stop-conflict-btn")
                        .danger()
                        .small()
                        .label(if self.stopping_daemon {
                            "Stopping…".to_string()
                        } else {
                            format!("Stop {daemon_name}")
                        })
                        .disabled(self.stopping_daemon)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.stop_conflicting_daemon(stop_target.clone(), window, cx);
                        })),
                    cx,
                ))
            })
            .child(if is_narrow {
                v_flex()
                    .w_full()
                    .gap_4()
                    .child(cpu_card)
                    .child(gpu_card)
                    .into_any_element()
            } else {
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_stretch()
                    .child(cpu_card)
                    .child(gpu_card)
                    .into_any_element()
            })
            .child(
                GroupBox::new()
                    .fill()
                    .title("Performance Mode Profile")
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .flex_wrap()
                            .child(
                                ButtonGroup::new("power-profile-group")
                                    .children([
                                        Button::new("pm-quiet")
                                            .label("Quiet")
                                            .selected(self.config.power_mode == PowerMode::Quiet)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.set_power_mode(PowerMode::Quiet, window, cx);
                                            })),
                                        Button::new("pm-default")
                                            .label("Balanced")
                                            .selected(self.config.power_mode == PowerMode::Default)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.set_power_mode(PowerMode::Default, window, cx);
                                            })),
                                        Button::new("pm-extreme")
                                            .label("Extreme")
                                            .selected(self.config.power_mode == PowerMode::Extreme)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.set_power_mode(PowerMode::Extreme, window, cx);
                                            })),
                                    ]),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.config.power_mode.description()),
                            ),
                    ),
            )
            .child(
                GroupBox::new()
                    .fill()
                    .title(match self.telemetry.nvme_temp {
                        Some(nvme) => format!("Live Thermal History (60s) · NVMe SSD: {nvme}°C"),
                        None => "Live Thermal History (60s)".to_string(),
                    })
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div().w_full().h(px(180.0)).child(
                                    AreaChart::new(history_vec)
                                        .id("thermal-history-chart")
                                        .appear(false)
                                        .x(|s| s.label.clone())
                                        .y(|s| s.cpu)
                                        .stroke(cx.theme().red)
                                        .fill(cx.theme().red.opacity(0.18))
                                        .name("CPU °C")
                                        .linear()
                                        .y(|s| s.gpu)
                                        .stroke(cx.theme().cyan)
                                        .fill(cx.theme().cyan.opacity(0.14))
                                        .name("GPU °C")
                                        .linear(),
                                ),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .child(
                                        h_flex()
                                            .gap_4()
                                            .child(
                                                Tag::custom(
                                                    cx.theme().red.opacity(0.14),
                                                    cx.theme().red,
                                                    cx.theme().red.opacity(0.4),
                                                )
                                                .small()
                                                .child(format!(
                                                    "CPU {}",
                                                    cpu_temp.map_or("--°C".to_string(), |t| format!("{t}°C"))
                                                )),
                                            )
                                            .child(
                                                Tag::custom(
                                                    cx.theme().cyan.opacity(0.14),
                                                    cx.theme().cyan,
                                                    cx.theme().cyan.opacity(0.4),
                                                )
                                                .small()
                                                .child(format!(
                                                    "GPU {}",
                                                    gpu_temp.map_or("--°C".to_string(), |t| format!("{t}°C"))
                                                )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("1s sampling · hover chart to inspect"),
                                    ),
                            ),
                    ),
            )
    }

    #[allow(clippy::too_many_arguments)]
    fn render_fan_card(
        &self,
        unit: FanUnit,
        title: &'static str,
        temp: Option<u8>,
        min_t: Option<u8>,
        max_t: Option<u8>,
        rpm: Option<u16>,
        angle: f32,
        mode: FanMode,
        manual_speed: u8,
        curve: &FanCurve,
        speed_slider: &Entity<SliderState>,
        is_narrow: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let t_color = temp_color(temp, cx);
        let temp_str = temp.map_or_else(|| "--°C".to_string(), |t| format!("{t}°C"));
        let minmax_str = format!(
            "MIN {}°C · MAX {}°C",
            min_t.map_or("--".to_string(), |v| v.to_string()),
            max_t.map_or("--".to_string(), |v| v.to_string()),
        );
        let rpm_val = rpm.unwrap_or(0);
        let rpm_str = rpm.map_or_else(|| "-- RPM".to_string(), |r| format!("{r} RPM"));
        let load_pct = temp.unwrap_or(0) as f32;
        let unit_accent = match unit {
            FanUnit::Cpu => cx.theme().primary,
            FanUnit::Gpu => cx.theme().cyan,
        };
        let unit_tag = unit.label().to_lowercase();
        let fan_ec_supported = !self.telemetry.backend.is_unlocked()
            || self.telemetry.backend.supports_raw_fan_ec()
            || self.telemetry.nbfc_active;

        let wrapper = if is_narrow {
            div().w_full()
        } else {
            div().flex_1().min_w(px(260.0))
        };

        wrapper.child(
            GroupBox::new().fill().title(title).child(
                v_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_size(px(34.0))
                                            .font_bold()
                                            .text_color(t_color)
                                            .child(temp_str),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(minmax_str),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(render_fan_rotor(
                                        angle,
                                        rpm_val,
                                        px(74.0),
                                        unit_accent,
                                        cx.theme().secondary,
                                        cx.theme().border,
                                    ))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_semibold()
                                            .text_color(unit_accent)
                                            .child(rpm_str),
                                    ),
                            ),
                    )
                    .child(
                        Progress::new(format!("{unit_tag}-temp-progress"))
                            .value(load_pct)
                            .color(t_color),
                    )
                    .child(Separator::horizontal())
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Fan Control Mode"),
                                    )
                                    .child(match mode {
                                        FanMode::Auto => Tag::info().outline().small().child("AUTO").into_any_element(),
                                        FanMode::Turbo => Tag::danger().outline().small().child("MAX").into_any_element(),
                                        FanMode::Manual => Tag::secondary().outline().small().child(format!("MANUAL · {manual_speed}%")).into_any_element(),
                                        FanMode::Curve => Tag::success().outline().small().child("CURVE").into_any_element(),
                                    }),
                            )
                            .child(
                                ButtonGroup::new(format!("{unit_tag}-mode-group"))
                                    .disabled(!fan_ec_supported)
                                    .children([
                                        Button::new(format!("{unit_tag}-auto"))
                                            .small()
                                            .label("Auto")
                                            .selected(mode == FanMode::Auto)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_fan_mode(unit, FanMode::Auto, window, cx);
                                            })),
                                        Button::new(format!("{unit_tag}-turbo"))
                                            .small()
                                            .label("Max")
                                            .selected(mode == FanMode::Turbo)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_fan_mode(unit, FanMode::Turbo, window, cx);
                                            })),
                                        Button::new(format!("{unit_tag}-manual"))
                                            .small()
                                            .label("Manual")
                                            .selected(mode == FanMode::Manual)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_fan_mode(unit, FanMode::Manual, window, cx);
                                            })),
                                        Button::new(format!("{unit_tag}-curve"))
                                            .small()
                                            .label("Curve")
                                            .selected(mode == FanMode::Curve)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.set_fan_mode(unit, FanMode::Curve, window, cx);
                                            })),
                                    ]),
                            ),
                    )
                    .child(if !fan_ec_supported {
                        div()
                            .p_2()
                            .rounded(px(6.0))
                            .bg(cx.theme().secondary)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Kernel Lockdown active: raw EC fan override disabled for anti-cheat integrity. Use Performance Mode above.")
                            .into_any_element()
                    } else {
                        match mode {
                            FanMode::Manual => v_flex()
                                .gap_2()
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Manual Speed"),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .text_color(unit_accent)
                                                .child(format!("{manual_speed}%")),
                                        ),
                                )
                                .child(Slider::new(speed_slider))
                                .into_any_element(),
                            FanMode::Curve => {
                                let target = temp.map(|t| curve.evaluate(t)).unwrap_or(50);
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .p_2()
                                    .rounded(px(6.0))
                                    .bg(cx.theme().secondary)
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "Curve target: {target}% (Hysteresis {}°C)",
                                                curve.hysteresis_c
                                            )),
                                    )
                                    .child(
                                        Button::new(format!("edit-curve-{unit_tag}"))
                                            .small()
                                            .label("Edit Curve")
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.active_page = ActivePage::Curves;
                                                this.switch_curve_unit(unit, window, cx);
                                            })),
                                    )
                                    .into_any_element()
                            }
                            FanMode::Turbo => div()
                                .px_3()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(cx.theme().secondary.opacity(0.6))
                                .border_1()
                                .border_color(cx.theme().border.opacity(0.5))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if self.telemetry.nbfc_active {
                                    "Full 100% fan override active via nbfc_service"
                                } else {
                                    "Full Turbo RPM override active in EC"
                                })
                                .into_any_element(),
                            FanMode::Auto => div()
                                .px_3()
                                .py_2()
                                .rounded(px(8.0))
                                .bg(cx.theme().secondary.opacity(0.6))
                                .border_1()
                                .border_color(cx.theme().border.opacity(0.5))
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if self.telemetry.nbfc_active {
                                    "Managed by nbfc_service thermal config".to_string()
                                } else {
                                    format!(
                                        "BIOS EC thermal curve active (last manual: {manual_speed}%)"
                                    )
                                })
                                .into_any_element(),
                        }
                    }),
            ),
        )
    }

    fn render_curves_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let unit = self.editing_curve_unit;
        let curve = match unit {
            FanUnit::Cpu => &self.config.cpu_curve,
            FanUnit::Gpu => &self.config.gpu_curve,
        };
        let is_curve_active = match unit {
            FanUnit::Cpu => self.config.cpu_fan_mode == FanMode::Curve,
            FanUnit::Gpu => self.config.gpu_fan_mode == FanMode::Curve,
        };
        let selected_idx = self.curve_editor.read(cx).selected_index();
        let selected_point = curve.points.get(selected_idx).copied();

        v_flex()
            .w_full()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .flex_wrap()
                    .child(
                        ButtonGroup::new("curve-unit-selector").children([
                            Button::new("cu-cpu")
                                .label("CPU Fan Curve")
                                .selected(unit == FanUnit::Cpu)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.switch_curve_unit(FanUnit::Cpu, window, cx);
                                })),
                            Button::new("cu-gpu")
                                .label("GPU Fan Curve")
                                .selected(unit == FanUnit::Gpu)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.switch_curve_unit(FanUnit::Gpu, window, cx);
                                })),
                        ]),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                ButtonGroup::new("curve-presets").children([
                                    Button::new("preset-silent").small().label("Silent").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.apply_curve_preset(CurvePreset::Silent, window, cx);
                                        }),
                                    ),
                                    Button::new("preset-balanced")
                                        .small()
                                        .label("Balanced")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_curve_preset(
                                                CurvePreset::Balanced,
                                                window,
                                                cx,
                                            );
                                        })),
                                    Button::new("preset-gaming").small().label("Gaming").on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.apply_curve_preset(CurvePreset::Gaming, window, cx);
                                        }),
                                    ),
                                    Button::new("preset-aggro")
                                        .small()
                                        .label("Aggressive")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.apply_curve_preset(
                                                CurvePreset::Aggressive,
                                                window,
                                                cx,
                                            );
                                        })),
                                ]),
                            )
                            .child(
                                Button::new("activate-curve-btn")
                                    .primary()
                                    .small()
                                    .label(if is_curve_active {
                                        "Curve Active"
                                    } else {
                                        "Activate Curve"
                                    })
                                    .disabled(is_curve_active)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.set_fan_mode(unit, FanMode::Curve, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                GroupBox::new()
                    .fill()
                    .title(format!("{} Interactive Fan Curve", unit.label()))
                    .child(
                        v_flex()
                            .gap_4()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Drag any control node (30°C–100°C × 0%–100%) or select a node below to fine-tune."),
                            )
                            .child(self.curve_editor.clone())
                            .child(
                                h_flex()
                                    .w_full()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .flex_wrap()
                                    .child(
                                        v_flex()
                                            .w(px(240.0))
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(format!(
                                                        "Hysteresis: {}°C",
                                                        curve.hysteresis_c
                                                    )),
                                            )
                                            .child(Slider::new(&self.hysteresis_slider)),
                                    )
                                    .child(
                                        h_flex().gap_2().flex_wrap().children(
                                            curve.points.iter().enumerate().map(|(idx, pt)| {
                                                let active = idx == selected_idx;
                                                Button::new(format!("curve-node-{idx}"))
                                                    .small()
                                                    .selected(active)
                                                    .label(format!(
                                                        "{}°C → {}%",
                                                        pt.temp_c, pt.speed_pct
                                                    ))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.curve_editor.update(cx, |ed, cx| {
                                                            ed.select_point(idx, cx);
                                                        });
                                                    }))
                                            }),
                                        ),
                                    ),
                            )
                            .when_some(selected_point, |this, pt| {
                                this.child(
                                    h_flex()
                                        .p_3()
                                        .rounded(px(8.0))
                                        .bg(cx.theme().secondary)
                                        .items_center()
                                        .justify_between()
                                        .flex_wrap()
                                        .gap_3()
                                        .child(div().text_xs().font_medium().child(format!(
                                            "Selected Node #{}: {}°C at {}% speed",
                                            selected_idx + 1,
                                            pt.temp_c,
                                            pt.speed_pct
                                        )))
                                        .child(
                                            h_flex()
                                                .gap_3()
                                                .child(
                                                    ButtonGroup::new("nudge-temp").children([
                                                        Button::new("t-minus")
                                                            .small()
                                                            .label("-2°C")
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.curve_editor.update(cx, |ed, cx| {
                                                                    ed.nudge_selected(-2, 0, cx)
                                                                });
                                                            })),
                                                        Button::new("t-plus")
                                                            .small()
                                                            .label("+2°C")
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.curve_editor.update(cx, |ed, cx| {
                                                                    ed.nudge_selected(2, 0, cx)
                                                                });
                                                            })),
                                                    ]),
                                                )
                                                .child(
                                                    ButtonGroup::new("nudge-speed").children([
                                                        Button::new("s-minus")
                                                            .small()
                                                            .label("-5%")
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.curve_editor.update(cx, |ed, cx| {
                                                                    ed.nudge_selected(0, -5, cx)
                                                                });
                                                            })),
                                                        Button::new("s-plus")
                                                            .small()
                                                            .label("+5%")
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.curve_editor.update(cx, |ed, cx| {
                                                                    ed.nudge_selected(0, 5, cx)
                                                                });
                                                            })),
                                                    ]),
                                                ),
                                        ),
                                )
                            }),
                    ),
            )
    }

    fn render_settings_page(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let hw = &self.telemetry.hardware;
        let sec = &hw.security;
        let hwmon_list = if hw.hwmon_sources.is_empty() {
            "None detected".to_string()
        } else {
            hw.hwmon_sources.join(", ")
        };

        v_flex()
            .w_full()
            .gap_5()
            .child(
                GroupBox::new()
                    .fill()
                    .title("Hardware & Power Features")
                    .child(
                        v_flex()
                            .gap_4()
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Switch::new("sw-battery-limit")
                                            .checked(self.config.battery_limit_active)
                                            .label("80% Battery Charge Limit (Battery Care)")
                                            .on_change(cx.listener(|this, next, window, cx| {
                                                let enabled = *next;
                                                this.apply_toggle(
                                                    ToggleKind::BatteryLimit,
                                                    "80% Battery Care",
                                                    enabled,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Caps charging at 80% via EC or kernel sysfs"),
                                    ),
                            )
                            .child(Separator::horizontal())
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Switch::new("sw-kb-timeout")
                                            .checked(self.config.kb_backlight_timeout)
                                            .label("Keyboard Backlight 30s Timeout")
                                            .on_change(cx.listener(|this, next, window, cx| {
                                                let enabled = *next;
                                                this.apply_toggle(
                                                    ToggleKind::KbTimeout,
                                                    "Keyboard 30s timeout",
                                                    enabled,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Turns off keyboard LEDs after 30s idle"),
                                    ),
                            )
                            .child(Separator::horizontal())
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .flex_wrap()
                                    .gap_2()
                                    .child(
                                        Switch::new("sw-usb-charge")
                                            .checked(self.config.usb_charge_poweroff)
                                            .label("USB Power-Off Charging")
                                            .on_change(cx.listener(|this, next, window, cx| {
                                                let enabled = *next;
                                                this.apply_toggle(
                                                    ToggleKind::UsbCharge,
                                                    "USB power-off charging",
                                                    enabled,
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Powers USB ports while laptop is suspended/off"),
                                    ),
                            ),
                    ),
            )
            .child(
                GroupBox::new()
                    .fill()
                    .title("Appearance")
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("gpui-kit native theme mode"),
                            )
                            .child(
                                ButtonGroup::new("theme-mode-group").children([
                                    Button::new("theme-dark")
                                        .small()
                                        .icon(IconName::Moon)
                                        .label("Dark")
                                        .selected(self.config.theme == AppThemeKind::Dark)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.config.theme = AppThemeKind::Dark;
                                            let _ = this.config.save();
                                            apply_theme_mode(AppThemeKind::Dark, window, cx);
                                            cx.notify();
                                        })),
                                    Button::new("theme-light")
                                        .small()
                                        .icon(IconName::Sun)
                                        .label("Light")
                                        .selected(self.config.theme == AppThemeKind::Light)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.config.theme = AppThemeKind::Light;
                                            let _ = this.config.save();
                                            apply_theme_mode(AppThemeKind::Light, window, cx);
                                            cx.notify();
                                        })),
                                ]),
                            ),
                    ),
            )
            .child(
                GroupBox::new()
                    .fill()
                    .title("Anti-Cheat & Kernel Integrity Diagnostics")
                    .child(
                        v_flex()
                            .gap_2()
                            .child(diag_row("Vendor / Model", &format!("{} {}", hw.vendor, hw.model), cx))
                            .child(diag_row("BIOS Version", &hw.bios_version, cx))
                            .child(diag_row("Active Control Transport", self.telemetry.backend.label(), cx))
                            .child(diag_row("Kernel Lockdown LSM", sec.lockdown.label(), cx))
                            .child(diag_row(
                                "UEFI Secure Boot",
                                match sec.secure_boot {
                                    Some(true) => "Enabled",
                                    Some(false) => "Disabled",
                                    None => "Not readable unprivileged",
                                },
                                cx,
                            ))
                            .child(diag_row(
                                "Hardware TPM 2.0",
                                if sec.tpm_present { "Present" } else { "Not detected" },
                                cx,
                            ))
                            .child(diag_row(
                                "Epic EAC Linux v2 Posture",
                                if sec.eac_linux_v2_ready {
                                    "Compliant (Signed + Lockdown + TPM2)"
                                } else {
                                    "No kernel tampering by OpeNitro (host lockdown/SB depends on distro)"
                                },
                                cx,
                            ))
                            .child(diag_row(
                                "ACPI Platform Profile",
                                hw.acpi_platform_profile.as_deref().unwrap_or("Not exposed"),
                                cx,
                            ))
                            .child(diag_row("Sysfs hwmon Drivers", &hwmon_list, cx))
                            .child(diag_row(
                                "Config File",
                                &AppConfig::default_path().display().to_string(),
                                cx,
                            ))
                            .child(Separator::horizontal())
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Zero systemd units · zero modprobe · zero /dev/port"),
                                    )
                                    .child(
                                        Button::new("reset-defaults-btn")
                                            .danger()
                                            .small()
                                            .label("Reset Defaults")
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.reset_all_defaults(window, cx);
                                            })),
                                    ),
                            ),
                    ),
            )
    }
}

impl Render for OpeNitroApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vw = window.viewport_size().width.as_f32();
        let is_ultra_narrow = vw < 620.0;
        let is_narrow = vw < 780.0;
        let sidebar_collapsed = self.nav_collapsed_override.unwrap_or(vw < 940.0);

        let page_content = match self.active_page {
            ActivePage::Dashboard => self.render_dashboard(is_narrow, cx).into_any_element(),
            ActivePage::Curves => self.render_curves_page(cx).into_any_element(),
            ActivePage::Settings => self.render_settings_page(cx).into_any_element(),
        };

        let mut viewport = div()
            .id("main-viewport")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scrollbar();

        viewport = if is_ultra_narrow {
            viewport.p_3()
        } else if is_narrow {
            viewport.p_4()
        } else {
            viewport.p_6()
        };

        h_flex()
            .id("openitro-root")
            .track_focus(&self.focus_handle)
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .size_full()
            .overflow_hidden()
            .when(!is_ultra_narrow, |this| {
                this.child(self.render_sidebar(sidebar_collapsed, cx))
            })
            .child(
                viewport.child(
                    v_flex()
                        .w_full()
                        .max_w(px(1400.0))
                        .gap_5()
                        .when(is_ultra_narrow, |this| {
                            this.child(
                                ButtonGroup::new("compact-top-nav").children([
                                    Button::new("top-dash")
                                        .small()
                                        .icon(IconName::LayoutDashboard)
                                        .label("Dashboard")
                                        .selected(self.active_page == ActivePage::Dashboard)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_page = ActivePage::Dashboard;
                                            cx.notify();
                                        })),
                                    Button::new("top-curves")
                                        .small()
                                        .icon(IconName::Settings2)
                                        .label("Curves")
                                        .selected(self.active_page == ActivePage::Curves)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_page = ActivePage::Curves;
                                            cx.notify();
                                        })),
                                    Button::new("top-settings")
                                        .small()
                                        .icon(IconName::Settings)
                                        .label("Settings")
                                        .selected(self.active_page == ActivePage::Settings)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.active_page = ActivePage::Settings;
                                            cx.notify();
                                        })),
                                ]),
                            )
                        })
                        .child(self.render_header(cx))
                        .child(page_content),
                ),
            )
    }
}

fn apply_theme_mode(kind: AppThemeKind, window: &mut Window, cx: &mut Context<OpeNitroApp>) {
    let mode = match kind {
        AppThemeKind::Dark => ThemeMode::Dark,
        AppThemeKind::Light => ThemeMode::Light,
    };
    Theme::change(mode, Some(window), cx);
}

fn temp_color(temp_c: Option<u8>, cx: &gpui_kit::App) -> Hsla {
    match temp_c {
        None | Some(0) => cx.theme().muted_foreground,
        Some(t) if t >= 85 => cx.theme().red,
        Some(t) if t >= 70 => cx.theme().warning,
        Some(t) if t <= 48 => cx.theme().green,
        Some(_) => cx.theme().cyan,
    }
}

fn diag_row(label: &str, value: &str, cx: &gpui_kit::App) -> impl IntoElement {
    h_flex()
        .w_full()
        .justify_between()
        .gap_4()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(
            div()
                .text_xs()
                .font_medium()
                .child(value.to_string()),
        )
}

fn ec_mode_to_ui(mode: EcFanMode) -> FanMode {
    match mode {
        EcFanMode::Auto => FanMode::Auto,
        EcFanMode::Turbo => FanMode::Turbo,
        EcFanMode::Manual => FanMode::Manual,
    }
}

fn status_strip(
    accent: Hsla,
    icon: IconName,
    title: impl Into<SharedString>,
    subtitle: impl Into<SharedString>,
    action: impl IntoElement,
    cx: &gpui_kit::App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .px_3p5()
        .py_2p5()
        .rounded(px(10.0))
        .bg(accent.opacity(0.08))
        .border_1()
        .border_color(accent.opacity(0.28))
        .items_center()
        .justify_between()
        .flex_wrap()
        .gap_3()
        .child(
            h_flex()
                .flex_1()
                .min_w(px(220.0))
                .items_center()
                .gap_3()
                .child(
                    div()
                        .p_2()
                        .rounded(px(8.0))
                        .bg(accent.opacity(0.16))
                        .child(Icon::new(icon).text_color(accent)),
                )
                .child(
                    v_flex()
                        .gap_0p5()
                        .child(div().text_xs().font_semibold().child(title.into()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(subtitle.into()),
                        ),
                ),
        )
        .child(action)
}
