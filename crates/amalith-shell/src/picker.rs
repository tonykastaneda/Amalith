//! Dialog-style HSV color picker with color-space readouts.

use amalith_core::Color as CoreColor;
use vello::kurbo::{Affine, BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::{Color, ColorStop, Fill, Gradient};
use vello::Scene;

use crate::panels::PaintSlot;
use crate::theme::Theme;

pub const W: f64 = 680.0;
pub const H: f64 = 386.0;

#[derive(Clone, Copy, Debug)]
pub struct Picker {
    pub slot: PaintSlot,
    /// Top-left of the popup, screen px.
    pub origin: Point,
    pub h: f32,
    pub s: f32,
    pub v: f32,
}

impl Picker {
    pub fn from_color(slot: PaintSlot, origin: Point, c: Option<CoreColor>) -> Self {
        let (h, s, v) = c
            .map(|c| rgb_to_hsv(c.r, c.g, c.b))
            .unwrap_or((0.0, 0.0, 0.0));
        Self {
            slot,
            origin,
            h,
            s,
            v,
        }
    }

    pub fn color(&self) -> CoreColor {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        CoreColor::rgb(r, g, b)
    }

    fn bounds(&self) -> Rect {
        Rect::new(
            self.origin.x,
            self.origin.y,
            self.origin.x + W,
            self.origin.y + H,
        )
    }
    fn sv_rect(&self) -> Rect {
        let b = self.bounds();
        Rect::new(b.x0 + 19.0, b.y0 + 26.0, b.x0 + 327.0, b.y0 + 334.0)
    }
    fn hue_rect(&self) -> Rect {
        let b = self.bounds();
        Rect::new(b.x0 + 350.0, b.y0 + 23.0, b.x0 + 380.0, b.y0 + 331.0)
    }
    fn ok_rect(&self) -> Rect {
        let b = self.bounds();
        // Same size and inset as New Document's primary "Create" button.
        Rect::new(b.x1 - 30.0 - 110.0, b.y1 - 14.0 - 34.0, b.x1 - 30.0, b.y1 - 14.0)
    }
    fn cancel_rect(&self) -> Rect {
        let ok = self.ok_rect();
        // Same size and gap as New Document's secondary "Close" button.
        Rect::new(ok.x0 - 14.0 - 96.0, ok.y0, ok.x0 - 14.0, ok.y1)
    }
}

/// Where a pointer at `p` (screen px) landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// New saturation / value (0..1).
    Sv(f32, f32),
    /// New hue (0..1).
    Hue(f32),
    Cancel,
    Ok,
    /// The dialog title bar, used to move the panel.
    Title,
    /// Somewhere else inside the popup — swallow it.
    Inside,
    /// Outside the popup — a click here dismisses it.
    Outside,
}

pub fn hit(pk: &Picker, p: Point) -> Hit {
    let sv = pk.sv_rect();
    if sv.contains(p) {
        let s = ((p.x - sv.x0) / sv.width()).clamp(0.0, 1.0) as f32;
        let v = 1.0 - ((p.y - sv.y0) / sv.height()).clamp(0.0, 1.0) as f32;
        return Hit::Sv(s, v);
    }
    let hr = pk.hue_rect();
    if hr.contains(p) {
        let t = ((p.y - hr.y0) / hr.height()).clamp(0.0, 1.0) as f32;
        let h = if t == 0.0 { 0.0 } else { 1.0 - t };
        return Hit::Hue(h);
    }
    if pk.cancel_rect().contains(p) {
        return Hit::Cancel;
    }
    if pk.ok_rect().contains(p) {
        return Hit::Ok;
    }
    let b = pk.bounds();
    if Rect::new(b.x0, b.y0, b.x1, b.y0 + 26.0).contains(p) {
        return Hit::Title;
    }
    if pk.bounds().contains(p) {
        return Hit::Inside;
    }
    Hit::Outside
}

/// Like [`hit`] but always returns an Sv/Hue value for a drag in progress,
/// clamping the pointer into whichever region the drag started in.
pub fn drag_value(pk: &Picker, p: Point, in_hue: bool) -> (f32, f32, f32) {
    if in_hue {
        let hr = pk.hue_rect();
        let t = ((p.y - hr.y0) / hr.height()).clamp(0.0, 1.0) as f32;
        let h = if t == 0.0 { 0.0 } else { 1.0 - t };
        (h, pk.s, pk.v)
    } else {
        let sv = pk.sv_rect();
        let s = ((p.x - sv.x0) / sv.width()).clamp(0.0, 1.0) as f32;
        let v = 1.0 - ((p.y - sv.y0) / sv.height()).clamp(0.0, 1.0) as f32;
        (pk.h, s, v)
    }
}

pub fn paint(
    scene: &mut Scene,
    pk: &Picker,
    text_color: Color,
    theme: &Theme,
    text: &mut crate::text::TextContext,
) {
    let b = pk.bounds();
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &b);
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &b);

    // SV square: white→hue horizontally, then transparent→black vertically.
    let sv = pk.sv_rect();
    let (hr, hg, hb) = hsv_to_rgb(pk.h, 1.0, 1.0);
    let hue = Color::new([hr, hg, hb, 1.0]);
    let horiz = Gradient::new_linear((sv.x0, sv.y0), (sv.x1, sv.y0))
        .with_stops([Color::new([1.0, 1.0, 1.0, 1.0]), hue]);
    scene.fill(Fill::NonZero, Affine::IDENTITY, &horiz, None, &sv);
    let vert = Gradient::new_linear((sv.x0, sv.y0), (sv.x0, sv.y1)).with_stops([
        Color::new([0.0, 0.0, 0.0, 0.0]),
        Color::new([0.0, 0.0, 0.0, 1.0]),
    ]);
    scene.fill(Fill::NonZero, Affine::IDENTITY, &vert, None, &sv);
    // SV cursor.
    let cx = sv.x0 + pk.s as f64 * sv.width();
    let cy = sv.y0 + (1.0 - pk.v as f64) * sv.height();
    let cursor = Circle::new((cx, cy), 5.0);
    scene.stroke(
        &Stroke::new(3.0),
        Affine::IDENTITY,
        Color::BLACK,
        None,
        &cursor,
    );
    scene.stroke(
        &Stroke::new(1.0),
        Affine::IDENTITY,
        Color::WHITE,
        None,
        &cursor,
    );

    // Hue strip.
    let hrect = pk.hue_rect();
    let stops: [ColorStop; 7] = [
        stop(0.0, 0.0),
        stop(1.0 / 6.0, 5.0 / 6.0),
        stop(2.0 / 6.0, 4.0 / 6.0),
        stop(3.0 / 6.0, 3.0 / 6.0),
        stop(4.0 / 6.0, 2.0 / 6.0),
        stop(5.0 / 6.0, 1.0 / 6.0),
        stop(1.0, 0.0),
    ];
    let hue_grad =
        Gradient::new_linear((hrect.x0, hrect.y0), (hrect.x0, hrect.y1)).with_stops(stops);
    scene.fill(Fill::NonZero, Affine::IDENTITY, &hue_grad, None, &hrect);
    let hy = if pk.h == 0.0 {
        hrect.y0
    } else {
        hrect.y0 + (1.0 - pk.h as f64) * hrect.height()
    };
    let marker = theme.text_dim;
    let mut left = BezPath::new();
    left.move_to((hrect.x0 - 10.0, hy - 5.0));
    left.line_to((hrect.x0 - 2.0, hy));
    left.line_to((hrect.x0 - 10.0, hy + 5.0));
    left.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, marker, None, &left);
    let mut right = BezPath::new();
    right.move_to((hrect.x1 + 10.0, hy - 5.0));
    right.line_to((hrect.x1 + 2.0, hy));
    right.line_to((hrect.x1 + 10.0, hy + 5.0));
    right.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, marker, None, &right);

    let (r, g, blue) = hsv_to_rgb(pk.h, pk.s, pk.v);
    let rgb = [r, g, blue];
    let k = 1.0 - r.max(g).max(blue);
    let cmy = if k >= 1.0 - f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [
            (1.0 - r - k) / (1.0 - k),
            (1.0 - g - k) / (1.0 - k),
            (1.0 - blue - k) / (1.0 - k),
        ]
    };
    let left_values = [
        format!("{}°", (pk.h * 360.0).round() as i32 % 360),
        format!("{}%", (pk.s * 100.0).round() as i32),
        format!("{}%", (pk.v * 100.0).round() as i32),
        format!("{}", (rgb[0] * 255.0).round() as i32),
        format!("{}", (rgb[1] * 255.0).round() as i32),
        format!("{}", (rgb[2] * 255.0).round() as i32),
    ];
    let right_values = [
        format!("{}%", (cmy[0] * 100.0).round() as i32),
        format!("{}%", (cmy[1] * 100.0).round() as i32),
        format!("{}%", (cmy[2] * 100.0).round() as i32),
        format!("{}%", (k * 100.0).round() as i32),
    ];
    let labels = ["H:", "S:", "B:", "R:", "G:", "B:"];
    let cmyk_labels = ["C:", "M:", "Y:", "K:"];
    let field_bg = theme.bg;
    for (i, (label, value)) in labels.iter().zip(left_values.iter()).enumerate() {
        let y = b.y0 + 32.0 + i as f64 * 30.0;
        let radio = Circle::new((b.x0 + 418.0, y), 7.0);
        scene.stroke(&Stroke::new(1.2), Affine::IDENTITY, marker, None, &radio);
        if i == 0 {
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &radio);
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.panel_bg,
                None,
                &Circle::new((b.x0 + 418.0, y), 2.5),
            );
        }
        text.draw(scene, label, 14.0, text_color, b.x0 + 430.0, y + 5.0);
        draw_field(scene, text, theme, text_color, field_bg, b.x0 + 450.0, y - 12.0, 80.0, value);
    }
    for (i, (label, value)) in cmyk_labels.iter().zip(right_values.iter()).enumerate() {
        let y = b.y0 + 32.0 + i as f64 * 30.0;
        text.draw(scene, label, 14.0, text_color, b.x0 + 545.0, y + 5.0);
        draw_field(scene, text, theme, text_color, field_bg, b.x0 + 565.0, y - 12.0, 80.0, value);
    }
    let hex = format!(
        "{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (blue * 255.0).round() as u8
    );
    text.draw(scene, "#", 20.0, marker, b.x0 + 413.0, b.y0 + 229.0);
    draw_field(scene, text, theme, text_color, field_bg, b.x0 + 430.0, b.y0 + 211.0, 90.0, &hex);

    draw_button(scene, text, theme, text_color, pk.cancel_rect(), "Cancel", false);
    draw_button(scene, text, theme, text_color, pk.ok_rect(), "OK", true);
}

fn draw_field(
    scene: &mut Scene,
    text: &mut crate::text::TextContext,
    theme: &Theme,
    text_color: Color,
    bg: Color,
    x: f64,
    y: f64,
    width: f64,
    value: &str,
) {
    let field = Rect::new(x, y, x + width, y + 25.0).to_rounded_rect(3.0);
    scene.fill(Fill::NonZero, Affine::IDENTITY, bg, None, &field);
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &field);
    text.draw(scene, value, 14.0, text_color, x + 7.0, y + 17.0);
}

fn draw_button(
    scene: &mut Scene,
    text: &mut crate::text::TextContext,
    theme: &Theme,
    _text_color: Color,
    rect: Rect,
    label: &str,
    primary: bool,
) {
    // Same chrome as New Document's Close / Create buttons: sharp corners,
    // accent fill on the primary, strip-active + hairline on the secondary.
    let fill = if primary {
        theme.accent
    } else {
        theme.strip_active
    };
    scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &rect);
    if !primary {
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            theme.text_dim.with_alpha(0.6),
            None,
            &rect,
        );
    }
    let col = if primary {
        Color::from_rgb8(0xff, 0xff, 0xff)
    } else {
        theme.text
    };
    let w = text.measure(label, 12.5);
    text.draw(
        scene,
        label,
        12.5,
        col,
        rect.x0 + (rect.width() - w) * 0.5,
        rect.y0 + rect.height() * 0.5 + 4.5,
    );
}

fn stop(offset: f32, hue: f32) -> ColorStop {
    let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
    ColorStop {
        offset,
        color: Color::new([r, g, b, 1.0]).into(),
    }
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor() as i32;
    let f = h - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

pub fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        (((g - b) / d) % 6.0) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h.rem_euclid(1.0), s, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_near(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn rgb_hsv_round_trip_preserves_color() {
        for (r, g, b) in [
            (0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0),
            (1.0, 0.0, 0.0),
            (0.12, 0.57, 0.91),
            (0.8, 0.35, 0.1),
        ] {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            let (rr, gg, bb) = hsv_to_rgb(h, s, v);
            assert_near(rr, r);
            assert_near(gg, g);
            assert_near(bb, b);
        }
    }

    #[test]
    fn picker_hit_values_match_visual_extents() {
        let pk = Picker::from_color(PaintSlot::Fill, Point::new(20.0, 30.0), None);
        let sv = pk.sv_rect();
        assert_eq!(hit(&pk, Point::new(sv.x0, sv.y0)), Hit::Sv(0.0, 1.0));
        assert_eq!(hit(&pk, sv.center()), Hit::Sv(0.5, 0.5));
        assert_eq!(
            drag_value(&pk, Point::new(sv.x1 + 20.0, sv.y1 + 20.0), false),
            (0.0, 1.0, 0.0)
        );

        let hue = pk.hue_rect();
        assert_eq!(hit(&pk, Point::new(hue.x0, hue.y0)), Hit::Hue(0.0));
        assert_eq!(hit(&pk, hue.center()), Hit::Hue(0.5));
        assert_eq!(
            drag_value(&pk, Point::new(hue.x1, hue.y1 + 20.0), true),
            (0.0, 0.0, 0.0)
        );
        assert_eq!(hit(&pk, pk.cancel_rect().center()), Hit::Cancel);
        assert_eq!(hit(&pk, pk.ok_rect().center()), Hit::Ok);
    }
}
