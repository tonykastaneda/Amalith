//! Gradient panel — Illustrator's Gradient panel.
//!
//! Rows: gradient type (Linear / Radial; Freeform shown but unsupported),
//! an Angle field (linear) / Aspect field (radial) with a Reverse button,
//! the stop slider (live ramp, draggable stop handles, draggable midpoint
//! diamonds), then the selected stop's colour swatch with Location and
//! Opacity fields.
//!
//! Every numeric field is click-to-edit (type a value, Enter to commit),
//! nudged by its ▾ / ▴ chevrons, and scroll-wheel adjustable. Click the
//! ramp to add a stop; drag a stop handle to move it, or off the bar to
//! delete it; double-click a stop (or click its swatch) for the picker.

use super::{Action, Ctx, ID, PAD};
use crate::text::TextContext;
use amalith_core::{Gradient, GradientKind};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

pub const NATURAL_H: f64 = 238.0;

pub fn natural_height() -> f64 {
    NATURAL_H
}

/// A numeric field the panel edits (typed, nudged, or scrolled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradField {
    /// Linear-axis angle, degrees.
    Angle,
    /// Radial ellipse aspect ratio.
    Aspect,
    /// Selected stop's location on the slider (shown as %).
    Location,
    /// Selected stop's opacity (shown as %).
    Opacity,
}

const BAR_H: f64 = 26.0;
const STOP_W: f64 = 16.0;
const STOP_H: f64 = 15.0;
const FIELD_H: f64 = 20.0;
const CHEV_W: f64 = 15.0;
/// Drag a stop this far below the bar's bottom to drop it.
pub const REMOVE_DROP: f64 = 30.0;

struct L {
    type_lin: Rect,
    type_rad: Rect,
    type_free: Rect,
    reverse: Rect,
    geom_label: Point,
    geom_dn: Rect,
    geom_val: Rect,
    geom_up: Rect,
    bar: Rect,
    /// Strip below the bar that holds the stop handles / catches add clicks.
    track: Rect,
    swatch: Rect,
    loc_label: Point,
    loc_dn: Rect,
    loc_val: Rect,
    loc_up: Rect,
    op_label: Point,
    op_dn: Rect,
    op_val: Rect,
    op_up: Rect,
}

/// A `[▾][ value ][▴]` cluster whose right edge sits at `x1`.
fn stepper(x1: f64, y: f64, w: f64) -> (Rect, Rect, Rect) {
    let up = Rect::new(x1 - CHEV_W, y, x1, y + FIELD_H);
    let val = Rect::new(x1 - CHEV_W - w, y, x1 - CHEV_W, y + FIELD_H);
    let dn = Rect::new(val.x0 - CHEV_W, y, val.x0, y + FIELD_H);
    (dn, val, up)
}

fn layout(body: Rect) -> L {
    let x0 = body.x0 + PAD;
    let x1 = body.x1 - PAD;
    let mut y = body.y0 + PAD;

    // Row 1 — type buttons.
    let bt = 26.0;
    let type_lin = Rect::new(x0, y, x0 + bt, y + bt);
    let type_rad = Rect::new(type_lin.x1 + 6.0, y, type_lin.x1 + 6.0 + bt, y + bt);
    let type_free = Rect::new(type_rad.x1 + 6.0, y, type_rad.x1 + 6.0 + bt, y + bt);
    let reverse = Rect::new(x1 - bt, y, x1, y + bt);
    y += bt + 12.0;

    // Row 2 — angle / aspect field (label left, stepper right).
    let geom_label = Point::new(x0, y + 14.0);
    let (geom_dn, geom_val, geom_up) = stepper(x1, y, 52.0);
    y += FIELD_H + 14.0;

    // The ramp + handle track.
    let bar = Rect::new(x0, y, x1, y + BAR_H);
    let track = Rect::new(x0, bar.y1, x1, bar.y1 + STOP_H + 8.0);
    y = track.y1 + 14.0;

    // Row 3 — swatch + Location.
    let swatch = Rect::new(x0, y, x0 + FIELD_H, y + FIELD_H);
    let loc_label = Point::new(swatch.x1 + 10.0, y + 14.0);
    let (loc_dn, loc_val, loc_up) = stepper(x1, y, 52.0);
    y += FIELD_H + 10.0;

    // Row 4 — Opacity.
    let op_label = Point::new(x0, y + 14.0);
    let (op_dn, op_val, op_up) = stepper(x1, y, 52.0);

    L {
        type_lin,
        type_rad,
        type_free,
        reverse,
        geom_label,
        geom_dn,
        geom_val,
        geom_up,
        bar,
        track,
        swatch,
        loc_label,
        loc_dn,
        loc_val,
        loc_up,
        op_label,
        op_dn,
        op_val,
        op_up,
    }
}

/// Screen x of a stop at slider position `off` (0..1).
fn stop_x(bar: Rect, off: f32) -> f64 {
    bar.x0 + off.clamp(0.0, 1.0) as f64 * bar.width()
}

/// House-shaped handle rect for a stop centred at `off`.
fn stop_rect(bar: Rect, off: f32) -> Rect {
    let cx = stop_x(bar, off);
    Rect::new(
        cx - STOP_W * 0.5,
        bar.y1 + 2.0,
        cx + STOP_W * 0.5,
        bar.y1 + 2.0 + STOP_H,
    )
}

/// Fractional slider position of the midpoint diamond between stops
/// `i` and `i + 1`.
fn midpoint_pos(g: &Gradient, i: usize) -> f32 {
    let a = g.stops[i];
    let b = g.stops[i + 1];
    a.offset + (b.offset - a.offset) * a.midpoint
}

fn mid_rect(bar: Rect, pos: f32) -> Rect {
    let cx = stop_x(bar, pos);
    Rect::new(cx - 5.0, bar.y0 - 8.0, cx + 5.0, bar.y0 + 1.0)
}

fn checker(scene: &mut Scene, r: Rect) {
    let s = 5.0;
    let cols = (r.width() / s).ceil().max(1.0) as i64;
    let rows = (r.height() / s).ceil().max(1.0) as i64;
    scene.fill(Fill::NonZero, ID, Color::from_rgb8(0xff, 0xff, 0xff), None, &r);
    for gy in 0..rows {
        for gx in 0..cols {
            if (gx + gy) % 2 == 0 {
                continue;
            }
            let c = Rect::new(
                r.x0 + gx as f64 * s,
                r.y0 + gy as f64 * s,
                (r.x0 + (gx + 1) as f64 * s).min(r.x1),
                (r.y0 + (gy + 1) as f64 * s).min(r.y1),
            );
            scene.fill(Fill::NonZero, ID, Color::from_rgb8(0xcc, 0xcc, 0xcc), None, &c);
        }
    }
}

/// The ramp is always shown fully opaque — it's reading the *colour*
/// sequence, not previewing alpha, so no checkerboard and no blending.
fn paint_ramp(scene: &mut Scene, bar: Rect, g: &Gradient) {
    let n = bar.width().ceil().max(1.0) as i64;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let c = g.sample(t);
        let x = bar.x0 + i as f64;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([c.r, c.g, c.b, 1.0]),
            None,
            &Rect::new(x, bar.y0, x + 1.0, bar.y1),
        );
    }
}

fn field_box(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    r: Rect,
    s: &str,
    editing: bool,
) {
    scene.fill(Fill::NonZero, ID, th.bg, None, &r);
    scene.stroke(
        &Stroke::new(if editing { 1.5 } else { 1.0 }),
        ID,
        if editing { th.accent } else { th.border },
        None,
        &r,
    );
    text.draw(scene, s, 11.0, th.text, r.x0 + 4.0, r.y0 + 14.0);
    if editing {
        let cx = r.x0 + 4.0 + text.measure(s, 11.0) + 1.0;
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            th.text,
            None,
            &vello::kurbo::Line::new((cx, r.y0 + 3.0), (cx, r.y1 - 3.0)),
        );
    }
}

fn chevron(scene: &mut Scene, r: Rect, up: bool, hot: bool, th: &crate::theme::Theme) {
    let color = if hot { th.text } else { th.text_dim };
    scene.fill(Fill::NonZero, ID, th.strip_bg, None, &r);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &r);
    let cx = r.center().x;
    let (ya, yb) = if up {
        (r.center().y + 2.5, r.center().y - 2.5)
    } else {
        (r.center().y - 2.5, r.center().y + 2.5)
    };
    let mut p = BezPath::new();
    p.move_to((cx - 3.5, ya));
    p.line_to((cx, yb));
    p.line_to((cx + 3.5, ya));
    scene.stroke(&Stroke::new(1.3), ID, color, None, &p);
}

fn value_of(g: &Gradient, field: GradField, sel: usize) -> String {
    let stop = g.stops.get(sel).copied().unwrap_or(g.stops[0]);
    match field {
        GradField::Angle => format!("{:.0}°", g.angle_deg()),
        GradField::Aspect => format!("{:.2}", g.aspect),
        GradField::Location => format!("{:.0}%", stop.offset * 100.0),
        GradField::Opacity => format!("{:.0}%", stop.opacity * 100.0),
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    let l = layout(body);
    let grad = ctx.gradient.as_ref();
    let kind = grad.map(|(g, _)| g.kind);
    let editing = ctx.gradient_edit;

    // A live field buffer for `field`, or `None` to show the real value.
    let live = |field: GradField| -> Option<&str> {
        editing.and_then(|(f, s)| (f == field).then_some(s))
    };

    // --- Type buttons ------------------------------------------------
    let type_btn = |scene: &mut Scene, r: Rect, on: bool| {
        scene.fill(Fill::NonZero, ID, th.strip_bg, None, &r);
        scene.stroke(
            &Stroke::new(if on { 1.8 } else { 1.0 }),
            ID,
            if on { th.accent } else { th.border },
            None,
            &r,
        );
        Rect::new(r.x0 + 4.0, r.y0 + 4.0, r.x1 - 4.0, r.y1 - 4.0)
    };
    let li = type_btn(scene, l.type_lin, kind == Some(GradientKind::Linear));
    for i in 0..li.width().max(1.0) as i64 {
        let t = i as f32 / li.width().max(1.0) as f32;
        let v = 0.12 + t * 0.82;
        let x = li.x0 + i as f64;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([v, v, v, 1.0]),
            None,
            &Rect::new(x, li.y0, x + 1.0, li.y1),
        );
    }
    let ri = type_btn(scene, l.type_rad, kind == Some(GradientKind::Radial));
    for k in 0..3 {
        let f = 1.0 - k as f64 * 0.32;
        let rr = Rect::from_center_size(ri.center(), (ri.width() * f, ri.height() * f));
        let v = 0.12 + k as f32 * 0.34;
        scene.fill(Fill::NonZero, ID, Color::new([v, v, v, 1.0]), None, &rr.to_ellipse());
    }
    let fi = type_btn(scene, l.type_free, false);
    for (dx, dy) in [(0.25, 0.3), (0.72, 0.26), (0.4, 0.72), (0.78, 0.7)] {
        let p = Point::new(fi.x0 + fi.width() * dx, fi.y0 + fi.height() * dy);
        scene.fill(
            Fill::NonZero,
            ID,
            th.text_dim,
            None,
            &Rect::from_center_size(p, (3.0, 3.0)).to_ellipse(),
        );
    }

    // Reverse button (⇄).
    {
        let r = l.reverse;
        scene.fill(Fill::NonZero, ID, th.strip_bg, None, &r);
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &r);
        let c = r.center();
        let col = if grad.is_some() { th.text } else { th.text_dim };
        for (dy, left) in [(-3.0, true), (3.0, false)] {
            let mut a = BezPath::new();
            let (x0, x1) = (c.x - 6.0, c.x + 6.0);
            a.move_to((x0, c.y + dy));
            a.line_to((x1, c.y + dy));
            let tip = if left { x1 } else { x0 };
            let dir = if left { -3.0 } else { 3.0 };
            a.move_to((tip, c.y + dy));
            a.line_to((tip + dir, c.y + dy - 2.5));
            a.move_to((tip, c.y + dy));
            a.line_to((tip + dir, c.y + dy + 2.5));
            scene.stroke(&Stroke::new(1.3), ID, col, None, &a);
        }
    }

    // --- Angle (read-only) / Aspect (editable) --------------------------
    // The linear angle is set only by dragging the gradient tool on the
    // canvas — it's shown here but not editable. The radial aspect ratio
    // *is* an editable field.
    if kind == Some(GradientKind::Radial) {
        text.draw(scene, "Aspect Ratio", 11.0, th.text_dim, l.geom_label.x, l.geom_label.y);
        let gval = live(GradField::Aspect)
            .map(str::to_string)
            .or_else(|| grad.map(|(g, s)| value_of(g, GradField::Aspect, *s)))
            .unwrap_or_default();
        field_box(scene, text, th, l.geom_val, &gval, live(GradField::Aspect).is_some());
        chevron(scene, l.geom_dn, false, true, th);
        chevron(scene, l.geom_up, true, true, th);
    } else {
        text.draw(scene, "Angle", 11.0, th.text_dim, l.geom_label.x, l.geom_label.y);
        let gval = grad.map(|(g, _)| format!("{:.0}°", g.angle_deg())).unwrap_or_default();
        // Plain read-out, right-aligned where the field would sit.
        let w = text.measure(&gval, 11.0);
        text.draw(scene, &gval, 11.0, th.text_dim, l.geom_up.x1 - w, l.geom_label.y);
    }

    // --- Ramp + stops ------------------------------------------------
    match grad {
        None => {
            checker(scene, l.bar);
            scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.bar);
            text.draw(
                scene,
                "No gradient. Click Linear or Radial above,",
                10.5,
                th.text_dim,
                l.bar.x0,
                l.track.y0 + 12.0,
            );
            text.draw(
                scene,
                "or select a shape with a gradient fill.",
                10.5,
                th.text_dim,
                l.bar.x0,
                l.track.y0 + 26.0,
            );
        }
        Some((g, sel)) => {
            paint_ramp(scene, l.bar, g);
            scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.bar);

            // Midpoint diamonds.
            for i in 0..g.stops.len().saturating_sub(1) {
                let r = mid_rect(l.bar, midpoint_pos(g, i));
                let c = r.center();
                let mut d = BezPath::new();
                d.move_to((c.x, r.y0));
                d.line_to((r.x1, c.y));
                d.line_to((c.x, r.y1));
                d.line_to((r.x0, c.y));
                d.close_path();
                scene.fill(Fill::NonZero, ID, th.strip_bg, None, &d);
                scene.stroke(&Stroke::new(1.0), ID, th.text_dim, None, &d);
            }

            // Stop handles.
            for (i, stop) in g.stops.iter().enumerate() {
                let r = stop_rect(l.bar, stop.offset);
                let cx = r.center().x;
                let mut h = BezPath::new();
                h.move_to((cx, r.y0));
                h.line_to((r.x1, r.y0 + 4.0));
                h.line_to((r.x1, r.y1));
                h.line_to((r.x0, r.y1));
                h.line_to((r.x0, r.y0 + 4.0));
                h.close_path();
                let c = stop.color;
                scene.fill(Fill::NonZero, ID, Color::new([c.r, c.g, c.b, 1.0]), None, &h);
                let on = i == *sel;
                scene.stroke(
                    &Stroke::new(if on { 2.0 } else { 1.0 }),
                    ID,
                    if on { th.accent } else { th.text_dim },
                    None,
                    &h,
                );
            }
        }
    }

    // --- Selected-stop swatch + Location + Opacity ----------------------
    if let Some((g, sel)) = grad {
        let stop = g.stops.get(*sel).copied().unwrap_or(g.stops[0]);
        let c = stop.color;
        scene.fill(Fill::NonZero, ID, Color::new([c.r, c.g, c.b, 1.0]), None, &l.swatch);
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.swatch);

        text.draw(scene, "Location", 11.0, th.text_dim, l.loc_label.x, l.loc_label.y);
        let lv = live(GradField::Location)
            .map(str::to_string)
            .unwrap_or_else(|| value_of(g, GradField::Location, *sel));
        field_box(scene, text, th, l.loc_val, &lv, live(GradField::Location).is_some());
        chevron(scene, l.loc_dn, false, true, th);
        chevron(scene, l.loc_up, true, true, th);

        text.draw(scene, "Opacity", 11.0, th.text_dim, l.op_label.x, l.op_label.y);
        let ov = live(GradField::Opacity)
            .map(str::to_string)
            .unwrap_or_else(|| value_of(g, GradField::Opacity, *sel));
        field_box(scene, text, th, l.op_val, &ov, live(GradField::Opacity).is_some());
        chevron(scene, l.op_dn, false, true, th);
        chevron(scene, l.op_up, true, true, th);
    }
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);

    if l.type_lin.contains(p) {
        return Action::GradientKind(GradientKind::Linear);
    }
    if l.type_rad.contains(p) {
        return Action::GradientKind(GradientKind::Radial);
    }
    if l.type_free.contains(p) {
        return Action::None;
    }
    if l.reverse.contains(p) {
        return Action::GradientReverse;
    }

    let has = ctx.gradient.is_some();
    let radial = ctx.gradient.as_ref().map(|(g, _)| g.kind) == Some(GradientKind::Radial);
    if has {
        // Only the radial Aspect field is editable here. The linear angle
        // is set by dragging the gradient tool on the canvas — the panel
        // just displays it.
        if radial {
            if l.geom_dn.contains(p) {
                return Action::GradientStep(GradField::Aspect, -step_of(GradField::Aspect));
            }
            if l.geom_up.contains(p) {
                return Action::GradientStep(GradField::Aspect, step_of(GradField::Aspect));
            }
            if l.geom_val.contains(p) {
                return Action::GradientBeginEdit(GradField::Aspect);
            }
        }

        if l.loc_dn.contains(p) {
            return Action::GradientStep(GradField::Location, -step_of(GradField::Location));
        }
        if l.loc_up.contains(p) {
            return Action::GradientStep(GradField::Location, step_of(GradField::Location));
        }
        if l.loc_val.contains(p) {
            return Action::GradientBeginEdit(GradField::Location);
        }
        if l.op_dn.contains(p) {
            return Action::GradientStep(GradField::Opacity, -step_of(GradField::Opacity));
        }
        if l.op_up.contains(p) {
            return Action::GradientStep(GradField::Opacity, step_of(GradField::Opacity));
        }
        if l.op_val.contains(p) {
            return Action::GradientBeginEdit(GradField::Opacity);
        }
        if l.swatch.contains(p) {
            return Action::GradientStopPicker;
        }

        if let Some((g, _)) = ctx.gradient.as_ref() {
            // Midpoint diamond?
            for i in 0..g.stops.len().saturating_sub(1) {
                if mid_rect(l.bar, midpoint_pos(g, i))
                    .inflate(3.0, 3.0)
                    .contains(p)
                {
                    return Action::GradientMidDrag { index: i, bar: l.bar };
                }
            }
            // Stop handle?
            for (i, stop) in g.stops.iter().enumerate() {
                if stop_rect(l.bar, stop.offset).inflate(3.0, 3.0).contains(p) {
                    return Action::GradientSelectStop { index: i, bar: l.bar };
                }
            }
            // Bar / track → add a stop.
            if l.bar.contains(p) || l.track.contains(p) {
                let off = ((p.x - l.bar.x0) / l.bar.width()).clamp(0.0, 1.0) as f32;
                return Action::GradientAddStop { offset: off };
            }
        }
    }

    Action::None
}

/// The nudge size for one chevron click / scroll tick on `field`.
pub fn step_of(field: GradField) -> f64 {
    match field {
        GradField::Angle => 1.0,
        GradField::Aspect => 0.05,
        GradField::Location => 0.01,
        GradField::Opacity => 0.05,
    }
}

/// Parse a typed field buffer into the value the App applies. `%` and `°`
/// suffixes are tolerated; Location / Opacity come back as a 0..1 fraction.
pub fn parse_field(field: GradField, buf: &str) -> Option<f64> {
    let t: String = buf
        .trim()
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let v: f64 = t.parse().ok()?;
    Some(match field {
        GradField::Angle => v,
        GradField::Aspect => v.clamp(0.05, 20.0),
        GradField::Location | GradField::Opacity => (v / 100.0).clamp(0.0, 1.0),
    })
}

/// The numeric field under `p`, for scroll-to-nudge / click-to-edit. The
/// linear Angle is deliberately excluded — it isn't editable from here.
pub fn field_at(body: Rect, p: Point, kind: Option<GradientKind>) -> Option<GradField> {
    let l = layout(body);
    if kind == Some(GradientKind::Radial) && l.geom_val.inflate(CHEV_W, 3.0).contains(p) {
        return Some(GradField::Aspect);
    }
    if l.loc_val.inflate(CHEV_W, 3.0).contains(p) {
        return Some(GradField::Location);
    }
    if l.op_val.inflate(CHEV_W, 3.0).contains(p) {
        return Some(GradField::Opacity);
    }
    None
}

pub fn tip(body: Rect, p: Point, _ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    if l.type_lin.contains(p) {
        Some("Linear gradient")
    } else if l.type_rad.contains(p) {
        Some("Radial gradient")
    } else if l.type_free.contains(p) {
        Some("Freeform gradient — not yet supported")
    } else if l.reverse.contains(p) {
        Some("Reverse gradient")
    } else if l.bar.contains(p) || l.track.contains(p) {
        Some("Click to add a stop · drag a stop off the bar to delete")
    } else {
        None
    }
}
