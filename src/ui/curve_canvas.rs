//! Interactive GPUI `canvas` Fan Curve Editor (`30°C..100°C` x `0%..100%`).

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::{
    Bounds, Context, DispatchPhase, EventEmitter, Hsla, InteractiveElement as _, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _, PathBuilder,
    Pixels, Point, Render, Styled as _, Window, canvas, div, fill, point, px, size,
};

use crate::backend::curves::{CurvePoint, FanCurve};

const MIN_TEMP: f32 = 30.0;
const MAX_TEMP: f32 = 100.0;
const PAD_LEFT: f32 = 24.0;
const PAD_RIGHT: f32 = 20.0;
const PAD_TOP: f32 = 18.0;
const PAD_BOTTOM: f32 = 22.0;

/// Emitted whenever the user modifies a curve point via drag or fine-tuning.
pub struct CurveChanged {
    pub curve: FanCurve,
}

/// Interactive fan curve editor entity.
pub struct CurveEditor {
    curve: FanCurve,
    live_temp: Option<u8>,
    selected_idx: usize,
    dragging_idx: Option<usize>,
    bounds: Option<Bounds<Pixels>>,
}

impl EventEmitter<CurveChanged> for CurveEditor {}

impl CurveEditor {
    pub fn new(curve: FanCurve) -> Self {
        let mut normalized = curve;
        normalized.normalize();
        Self {
            curve: normalized,
            live_temp: None,
            selected_idx: 0,
            dragging_idx: None,
            bounds: None,
        }
    }

    pub fn curve(&self) -> &FanCurve {
        &self.curve
    }

    pub fn selected_index(&self) -> usize {
        self.selected_idx
            .min(self.curve.points.len().saturating_sub(1))
    }

    pub fn set_curve(&mut self, mut curve: FanCurve, cx: &mut Context<Self>) {
        curve.normalize();
        self.curve = curve;
        self.selected_idx = self
            .selected_idx
            .min(self.curve.points.len().saturating_sub(1));
        cx.notify();
    }

    pub fn set_live_temp(&mut self, temp: Option<u8>, cx: &mut Context<Self>) {
        if self.live_temp != temp {
            self.live_temp = temp;
            cx.notify();
        }
    }

    pub fn nudge_selected(&mut self, delta_temp: i8, delta_speed: i8, cx: &mut Context<Self>) {
        let idx = self.selected_index();
        if idx >= self.curve.points.len() {
            return;
        }
        let current = self.curve.points[idx];
        let next_temp = (current.temp_c as i16 + delta_temp as i16).clamp(30, 100) as u8;
        let next_speed = (current.speed_pct as i16 + delta_speed as i16).clamp(0, 100) as u8;
        self.update_point(idx, next_temp, next_speed, cx);
    }

    pub fn select_point(&mut self, idx: usize, cx: &mut Context<Self>) {
        if idx < self.curve.points.len() {
            self.selected_idx = idx;
            cx.notify();
        }
    }

    fn update_point(&mut self, idx: usize, temp_c: u8, speed_pct: u8, cx: &mut Context<Self>) {
        let len = self.curve.points.len();
        if idx >= len {
            return;
        }

        let min_t = if idx == 0 {
            30
        } else {
            self.curve.points[idx - 1].temp_c.saturating_add(2)
        };
        let max_t = if idx + 1 >= len {
            100
        } else {
            self.curve.points[idx + 1].temp_c.saturating_sub(2)
        };

        let clamped_t = temp_c.clamp(min_t.min(max_t), max_t.max(min_t));
        let clamped_s = speed_pct.min(100);

        let next = CurvePoint::new(clamped_t, clamped_s);
        if self.curve.points[idx] != next {
            self.curve.points[idx] = next;
            cx.emit(CurveChanged {
                curve: self.curve.clone(),
            });
            cx.notify();
        }
    }

    fn plot_rect(bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
        let left = bounds.origin.x.as_f32() + PAD_LEFT;
        let top = bounds.origin.y.as_f32() + PAD_TOP;
        let width = (bounds.size.width.as_f32() - PAD_LEFT - PAD_RIGHT).max(40.0);
        let height = (bounds.size.height.as_f32() - PAD_TOP - PAD_BOTTOM).max(40.0);
        (left, top, width, height)
    }

    fn point_to_px(p: CurvePoint, left: f32, top: f32, width: f32, height: f32) -> Point<Pixels> {
        let tx = ((p.temp_c as f32 - MIN_TEMP) / (MAX_TEMP - MIN_TEMP)).clamp(0.0, 1.0);
        let sy = (p.speed_pct as f32 / 100.0).clamp(0.0, 1.0);
        point(px(left + tx * width), px(top + (1.0 - sy) * height))
    }

    fn px_to_values(&self, pos: Point<Pixels>) -> Option<(u8, u8)> {
        let bounds = self.bounds?;
        let (left, top, width, height) = Self::plot_rect(bounds);
        let nx = ((pos.x.as_f32() - left) / width).clamp(0.0, 1.0);
        let ny = (1.0 - (pos.y.as_f32() - top) / height).clamp(0.0, 1.0);
        let temp = (MIN_TEMP + nx * (MAX_TEMP - MIN_TEMP)).round() as u8;
        let speed = (ny * 100.0).round() as u8;
        Some((temp, speed))
    }

    fn on_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(bounds) = self.bounds else {
            return;
        };
        let (left, top, width, height) = Self::plot_rect(bounds);

        let mut best_idx = 0usize;
        let mut best_dist = f32::MAX;
        for (idx, pt) in self.curve.points.iter().enumerate() {
            let p_px = Self::point_to_px(*pt, left, top, width, height);
            let dx = event.position.x.as_f32() - p_px.x.as_f32();
            let dy = event.position.y.as_f32() - p_px.y.as_f32();
            let dist = dx * dx + dy * dy;
            if dist < best_dist {
                best_dist = dist;
                best_idx = idx;
            }
        }

        self.selected_idx = best_idx;
        self.dragging_idx = Some(best_idx);
        if let Some((t, s)) = self.px_to_values(event.position) {
            self.update_point(best_idx, t, s, cx);
        } else {
            cx.notify();
        }
    }

    fn on_drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(idx) = self.dragging_idx else {
            return;
        };
        if let Some((t, s)) = self.px_to_values(event.position) {
            self.update_point(idx, t, s, cx);
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging_idx.take().is_some() {
            cx.notify();
        }
    }
}

impl Render for CurveEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let accent = theme.primary;
        let border = theme.border;
        let surface_bg = theme.background;
        let fg = theme.foreground;
        let info = theme.cyan;

        let points = self.curve.points.clone();
        let selected_idx = self.selected_index();
        let dragging_idx = self.dragging_idx;
        let live_temp = self.live_temp;
        let live_target = live_temp.map(|t| self.curve.evaluate(t));

        div()
            .id("fan-curve-canvas")
            .relative()
            .w_full()
            .h(px(230.0))
            .rounded(px(8.0))
            .bg(surface_bg)
            .border_1()
            .border_color(border)
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.on_mouse_down(event, cx)),
            )
            .child(
                canvas(move |bounds, _, _cx| bounds, {
                    let entity = cx.entity().clone();
                    move |bounds, _, window, cx| {
                        entity.update(cx, |this, _cx| {
                            this.bounds = Some(bounds);
                        });

                        window.on_mouse_event({
                            let entity = entity.clone();
                            move |event: &MouseMoveEvent, phase, _window, cx| {
                                if phase != DispatchPhase::Bubble {
                                    return;
                                }
                                entity.update(cx, |this, cx| this.on_drag_move(event, cx));
                            }
                        });

                        window.on_mouse_event({
                            let entity = entity.clone();
                            move |event: &MouseUpEvent, phase, _window, cx| {
                                if phase != DispatchPhase::Bubble
                                    || event.button != MouseButton::Left
                                {
                                    return;
                                }
                                entity.update(cx, |this, cx| this.end_drag(cx));
                            }
                        });

                        let (left, top, width, height) = Self::plot_rect(bounds);

                        for pct in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                            let y = top + (1.0 - pct) * height;
                            window.paint_quad(fill(
                                Bounds::new(point(px(left), px(y)), size(px(width), px(1.0))),
                                border.opacity(0.55),
                            ));
                        }

                        for temp in [30.0_f32, 45.0, 60.0, 75.0, 90.0, 100.0] {
                            let nx = (temp - MIN_TEMP) / (MAX_TEMP - MIN_TEMP);
                            let x = left + nx * width;
                            window.paint_quad(fill(
                                Bounds::new(point(px(x), px(top)), size(px(1.0), px(height))),
                                border.opacity(0.4),
                            ));
                        }

                        if points.len() >= 2 {
                            let px_pts: Vec<Point<Pixels>> = points
                                .iter()
                                .map(|&p| Self::point_to_px(p, left, top, width, height))
                                .collect();

                            let bottom_y = top + height;
                            let mut area = PathBuilder::fill();
                            area.move_to(point(px(left), px(bottom_y)));
                            area.line_to(point(px(left), px_pts[0].y));
                            for &pt in &px_pts {
                                area.line_to(pt);
                            }
                            area.line_to(point(px(left + width), px_pts[px_pts.len() - 1].y));
                            area.line_to(point(px(left + width), px(bottom_y)));
                            area.close();
                            if let Ok(path) = area.build() {
                                window.paint_path(path, accent.opacity(0.15));
                            }

                            let mut line = PathBuilder::stroke(px(2.5));
                            line.move_to(point(px(left), px_pts[0].y));
                            for &pt in &px_pts {
                                line.line_to(pt);
                            }
                            line.line_to(point(px(left + width), px_pts[px_pts.len() - 1].y));
                            if let Ok(path) = line.build() {
                                window.paint_path(path, accent);
                            }

                            if let (Some(temp), Some(speed)) = (live_temp, live_target) {
                                let tx = ((temp as f32 - MIN_TEMP) / (MAX_TEMP - MIN_TEMP))
                                    .clamp(0.0, 1.0);
                                let lx = left + tx * width;
                                let ly = top + (1.0 - speed as f32 / 100.0) * height;
                                window.paint_quad(fill(
                                    Bounds::new(point(px(lx), px(top)), size(px(1.5), px(height))),
                                    info.opacity(0.7),
                                ));
                                paint_circle(window, lx, ly, 5.0, info);
                            }

                            for (idx, &pt) in px_pts.iter().enumerate() {
                                let is_active =
                                    idx == selected_idx || dragging_idx == Some(idx);
                                let outer_r = if is_active { 8.0 } else { 6.0 };
                                let px_x = pt.x.as_f32();
                                let px_y = pt.y.as_f32();
                                paint_circle(window, px_x, px_y, outer_r, accent);
                                paint_circle(window, px_x, px_y, outer_r - 2.5, fg);
                            }
                        }
                    }
                })
                .size_full(),
            )
    }
}

fn paint_circle(window: &mut Window, cx: f32, cy: f32, r: f32, color: Hsla) {
    let mut b = PathBuilder::fill();
    for i in 0..=18 {
        let a = (i as f32) * std::f32::consts::TAU / 18.0;
        let p = point(px(cx + r * a.cos()), px(cy + r * a.sin()));
        if i == 0 {
            b.move_to(p);
        } else {
            b.line_to(p);
        }
    }
    b.close();
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}
