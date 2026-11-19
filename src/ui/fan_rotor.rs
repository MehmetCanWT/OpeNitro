//! GPU-accelerated spinning fan rotor visualization using GPUI `PathBuilder`.

use gpui_kit::{
    Hsla, IntoElement, ParentElement as _, PathBuilder, Pixels, Point, Styled as _, canvas, div,
    point, px,
};

/// Render an animated 7-blade cooling fan rotor scaled to `size_px`.
pub fn render_fan_rotor(
    angle_deg: f32,
    rpm: u16,
    size_px: Pixels,
    accent: Hsla,
    surface: Hsla,
    border: Hsla,
) -> impl IntoElement {
    let active = rpm > 0;
    let blade_color = if active {
        accent.opacity(0.88)
    } else {
        border.opacity(0.75)
    };
    let hub_color = if active { accent } else { border };
    let ring_color = if active { accent.opacity(0.35) } else { border };

    div().size(size_px).flex_none().child(
        canvas(
            move |_, _, _| {},
            move |bounds, _, window, _| {
                let cx = bounds.origin.x.as_f32() + bounds.size.width.as_f32() * 0.5;
                let cy = bounds.origin.y.as_f32() + bounds.size.height.as_f32() * 0.5;
                let radius = bounds.size.width.as_f32().min(bounds.size.height.as_f32()) * 0.5 - 3.0;
                if radius <= 4.0 {
                    return;
                }

                // Outer shroud ring
                let mut ring = PathBuilder::stroke(px(2.5));
                paint_circle_path(&mut ring, cx, cy, radius, 40);
                if let Ok(path) = ring.build() {
                    window.paint_path(path, ring_color);
                }

                // Inner casing disk
                let mut disk = PathBuilder::fill();
                paint_circle_path(&mut disk, cx, cy, radius - 2.0, 36);
                if let Ok(path) = disk.build() {
                    window.paint_path(path, surface.opacity(0.9));
                }

                // 7 curved impeller blades
                let num_blades = 7;
                let base_rad = angle_deg.to_radians();
                for i in 0..num_blades {
                    let theta = base_rad + (i as f32) * std::f32::consts::TAU / (num_blades as f32);
                    let blade = build_blade_path(cx, cy, radius * 0.25, radius * 0.88, theta);
                    if let Ok(path) = blade.build() {
                        window.paint_path(path, blade_color);
                    }
                }

                // Center hub
                let mut hub = PathBuilder::fill();
                paint_circle_path(&mut hub, cx, cy, radius * 0.26, 24);
                if let Ok(path) = hub.build() {
                    window.paint_path(path, hub_color);
                }

                // Hub center cap
                let mut cap = PathBuilder::fill();
                paint_circle_path(&mut cap, cx, cy, radius * 0.11, 16);
                if let Ok(path) = cap.build() {
                    window.paint_path(path, surface);
                }
            },
        )
        .size_full(),
    )
}

fn pt(x: f32, y: f32) -> Point<Pixels> {
    point(px(x), px(y))
}

fn paint_circle_path(builder: &mut PathBuilder, cx: f32, cy: f32, r: f32, segments: usize) {
    for i in 0..=segments {
        let a = (i as f32) * std::f32::consts::TAU / (segments as f32);
        let p = pt(cx + r * a.cos(), cy + r * a.sin());
        if i == 0 {
            builder.move_to(p);
        } else {
            builder.line_to(p);
        }
    }
    builder.close();
}

fn build_blade_path(cx: f32, cy: f32, inner_r: f32, outer_r: f32, angle: f32) -> PathBuilder {
    let mut b = PathBuilder::fill();
    let root_a = angle - 0.18;
    let tip_a = angle + 0.26;
    let mid_r = (inner_r + outer_r) * 0.55;

    let p0 = pt(cx + inner_r * root_a.cos(), cy + inner_r * root_a.sin());
    let ctrl1 = pt(
        cx + mid_r * (angle - 0.35).cos(),
        cy + mid_r * (angle - 0.35).sin(),
    );
    let p_tip = pt(cx + outer_r * tip_a.cos(), cy + outer_r * tip_a.sin());
    let ctrl2 = pt(
        cx + mid_r * (angle + 0.42).cos(),
        cy + mid_r * (angle + 0.42).sin(),
    );
    let p_back = pt(
        cx + inner_r * (angle + 0.28).cos(),
        cy + inner_r * (angle + 0.28).sin(),
    );

    b.move_to(p0);
    b.curve_to(p_tip, ctrl1);
    b.curve_to(p_back, ctrl2);
    b.close();
    b
}
