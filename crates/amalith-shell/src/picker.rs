//! A minimal HSV color picker popup: a saturation/value square, a hue
//! strip, a current-color preview, and a "None" button. Drawn as an
//! overlay; the app routes pointer events to [`hit`] while it is open.

use amalith_core::Color as CoreColor;
use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, ColorStop, Fill, Gradient};
use vello::Scene;

use crate::panels::PaintSlot;
use crate::theme::Theme;

pub const W: f64 = 232.0;
pub const H: f64 = 214.0;

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
        Rect::new(
            b.x0 + 12.0,
            b.y0 + 12.0,
            b.x0 + 12.0 + 160.0,
            b.y0 + 12.0 + 160.0,
        )
    }
    fn hue_rect(&self) -> Rect {
        let b = self.bounds();
        Rect::new(
            b.x1 - 12.0 - 20.0,
            b.y0 + 12.0,
            b.x1 - 12.0,
            b.y0 + 12.0 + 160.0,
        )
    }
    fn none_rect(&self) -> Rect {
        let b = self.bounds();
        Rect::new(
            b.x0 + 12.0,
            b.y1 - 12.0 - 22.0,
            b.x0 + 12.0 + 70.0,
            b.y1 - 12.0,
        )
    }
    fn preview_rect(&self) -> Rect {
        let b = self.bounds();
        Rect::new(
            b.x1 - 12.0 - 70.0,
            b.y1 - 12.0 - 22.0,
            b.x1 - 12.0,
            b.y1 - 12.0,
        )
    }
}

/// Where a pointer at `p` (screen px) landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// New saturation / value (0..1).
    Sv(f32, f32),
    /// New hue (0..1).
    Hue(f32),
    /// The "None" button.
    NoneButton,
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
        let h = ((p.y - hr.y0) / hr.height()).clamp(0.0, 1.0) as f32;
        return Hit::Hue(h);
    }
    if pk.none_rect().contains(p) {
        return Hit::NoneButton;
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
        let h = ((p.y - hr.y0) / hr.height()).clamp(0.0, 1.0) as f32;
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
    scene.stroke(
        &Stroke::new(1.5),
        Affine::IDENTITY,
        Color::new([1.0, 1.0, 1.0, 1.0]),
        None,
        &vello::kurbo::Circle::new((cx, cy), 5.0),
    );

    // Hue strip.
    let hrect = pk.hue_rect();
    let stops: [ColorStop; 7] = [
        stop(0.0, 0.0),
        stop(1.0 / 6.0, 1.0 / 6.0),
        stop(2.0 / 6.0, 2.0 / 6.0),
        stop(3.0 / 6.0, 3.0 / 6.0),
        stop(4.0 / 6.0, 4.0 / 6.0),
        stop(5.0 / 6.0, 5.0 / 6.0),
        stop(1.0, 1.0),
    ];
    let hue_grad =
        Gradient::new_linear((hrect.x0, hrect.y0), (hrect.x0, hrect.y1)).with_stops(stops);
    scene.fill(Fill::NonZero, Affine::IDENTITY, &hue_grad, None, &hrect);
    let hy = hrect.y0 + pk.h as f64 * hrect.height();
    scene.stroke(
        &Stroke::new(1.5),
        Affine::IDENTITY,
        Color::new([1.0, 1.0, 1.0, 1.0]),
        None,
        &Rect::new(hrect.x0 - 2.0, hy - 2.0, hrect.x1 + 2.0, hy + 2.0),
    );

    // None button + preview.
    let nb = pk.none_rect();
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_bg, None, &nb);
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &nb);
    text.draw(scene, "None", 11.0, text_color, nb.x0 + 10.0, nb.y0 + 15.0);

    let pv = pk.preview_rect();
    let (r, g, b_) = hsv_to_rgb(pk.h, pk.s, pk.v);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::new([r, g, b_, 1.0]),
        None,
        &pv,
    );
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &pv);
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
