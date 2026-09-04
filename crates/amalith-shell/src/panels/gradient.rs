//! Gradient panel — Illustrator's Gradient panel.
//!
//! Top to bottom: the gradient type (Linear / Radial; Freeform is shown
//! but not yet supported), an angle field (linear) or aspect field
//! (radial), the stop slider (the live ramp with draggable stop handles
//! and midpoint diamonds), and the selected stop's colour swatch with
//! Location / Opacity fields.
//!
//! Interaction:
//! - click the ramp track (just below the bar) to add a stop there;
//! - drag a stop handle to move it; drag it well below the bar to delete it;
//! - click a handle to select it; double-click it to open the colour picker;
//! - the ▾ / ▴ chevrons nudge the numeric next to them.

use super::{Action, Ctx, ID, PAD};
use crate::text::TextContext;
use amalith_core::{Gradient, GradientKind};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

pub const NATURAL_H: f64 = 210.0;

pub fn natural_height() -> f64 {
    NATURAL_H
}

/// A selected-stop / geometry numeric a chevron or scroll nudges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradField {
    /// Linear-axis angle, degrees.
    Angle,
    /// Radial ellipse aspect ratio.
    Aspect,
    /// Selected stop's location on the slider, 0..1.
    Location,
    /// Selected stop's opacity, 0..1.
    Opacity,
}

const BAR_H: f64 = 22.0;
const STOP_W: f64 = 14.0;
const STOP_H: f64 = 13.0;
/// Drag a stop this far below the bar's bottom to drop it.
pub const REMOVE_DROP: f64 = 26.0;

struct L {
    type_lin: Rect,
    type_rad: Rect,
    type_free: Rect,
    geom_label: Point,
    geom_dn: Rect,
    geom_val: Rect,
    geom_up: Rect,
    bar: Rect,
    /// Clickable strip holding the stop handles (below the bar).
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
    bottom: f64,
}

fn stepper(x1: f64, y: f64, w: f64) -> (Rect, Rect, Rect) {
    let ch = 14.0;
    let up = Rect::new(x1 - ch, y, x1, y + 18.0);
    let val = Rect::new(x1 - ch - w, y, x1 - ch, y + 18.0);
    let dn = Rect::new(val.x0 - ch, y, val.x0, y + 18.0);
    (dn, val, up)
}

fn layout(body: Rect) -> L {
    let x0 = body.x0 + PAD;
    let x1 = body.x1 - PAD;
    let mut y = body.y0 + PAD;

    let bt = 24.0;
    let type_lin = Rect::new(x0, y, x0 + bt, y + bt);
    let type_rad = Rect::new(x0 + bt + 6.0, y, x0 + 2.0 * bt + 6.0, y + bt);
    let type_free = Rect::new(x0 + 2.0 * bt + 12.0, y, x0 + 3.0 * bt + 12.0, y + bt);
    // Angle / aspect stepper, right-aligned on the type row.
    let geom_label = Point::new(type_free.x1 + 14.0, y + 13.0);
    let (geom_dn, geom_val, geom_up) = stepper(x1, y + 3.0, 40.0);
    y += bt + 14.0;

    let bar = Rect::new(x0, y, x1, y + BAR_H);
    let track = Rect::new(x0 - STOP_W, bar.y1, x1 + STOP_W, bar.y1 + STOP_H + 6.0);
    y = track.y1 + 12.0;

    let swatch = Rect::new(x0, y, x0 + 22.0, y + 20.0);
    let loc_label = Point::new(swatch.x1 + 12.0, y + 14.0);
    let (loc_dn, loc_val, loc_up) = stepper((x0 + x1) * 0.5 + 26.0, y + 1.0, 40.0);
    let op_label = Point::new(loc_up.x1 + 14.0, y + 14.0);
    let (op_dn, op_val, op_up) = stepper(x1, y + 1.0, 40.0);
    y += 20.0 + PAD;

    L {
        type_lin,
        type_rad,
        type_free,
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
        bottom: y,
    }
}

/// Screen x of a stop at slider position `off` (0..1).
fn stop_x(bar: Rect, off: f32) -> f64 {
    bar.x0 + off.clamp(0.0, 1.0) as f64 * bar.width()
}

/// House-shaped handle rect for a stop centered at `off`.
fn stop_rect(bar: Rect, off: f32) -> Rect {
    let cx = stop_x(bar, off);
    Rect::new(cx - STOP_W * 0.5, bar.y1 + 2.0, cx + STOP_W * 0.5, bar.y1 + 2.0 + STOP_H)
}

fn checker(scene: &mut Scene, r: Rect) {
    let s = 5.0;
    let cols = (r.width() / s).ceil() as i64;
    let rows = (r.height() / s).ceil() as i64;
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

/// Sample the ramp across `bar` (honoring per-stop opacity, over a
/// checkerboard so alpha reads).
fn paint_ramp(scene: &mut Scene, bar: Rect, g: &Gradient) {
    checker(scene, bar);
    let n = bar.width().ceil().max(1.0) as i64;
    for i in 0..n {
        let t = i as f32 / n as f32;
        let c = g.sample(t);
        let x = bar.x0 + i as f64;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([c.r, c.g, c.b, c.a]),
            None,
            &Rect::new(x, bar.y0, x + 1.0, bar.y1),
        );
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    let l = layout(body);
    let grad = ctx.gradient.as_ref();
    let kind = grad.map(|(g, _)| g.kind);

    // --- Type buttons -------------------------------------------------
    let type_btn = |scene: &mut Scene, r: Rect, on: bool| {
        scene.fill(Fill::NonZero, ID, th.panel_bg, None, &r);
        scene.stroke(
            &Stroke::new(if on { 1.6 } else { 1.0 }),
            ID,
            if on { th.accent } else { th.border },
            None,
            &r,
        );
        Rect::new(r.x0 + 4.0, r.y0 + 4.0, r.x1 - 4.0, r.y1 - 4.0)
    };
    // Linear: a left→right grey ramp.
    let li = type_btn(scene, l.type_lin, kind == Some(GradientKind::Linear));
    for i in 0..li.width() as i64 {
        let t = i as f32 / li.width().max(1.0) as f32;
        let v = 0.15 + t * 0.8;
        let x = li.x0 + i as f64;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([v, v, v, 1.0]),
            None,
            &Rect::new(x, li.y0, x + 1.0, li.y1),
        );
    }
    // Radial: concentric rings.
    let ri = type_btn(scene, l.type_rad, kind == Some(GradientKind::Radial));
    for k in 0..3 {
        let f = 1.0 - k as f64 * 0.32;
        let rr = Rect::from_center_size(ri.center(), (ri.width() * f, ri.height() * f));
        let v = 0.15 + k as f32 * 0.33;
        scene.fill(Fill::NonZero, ID, Color::new([v, v, v, 1.0]), None, &rr.to_ellipse());
    }
    // Freeform: greyed scatter of dots (not yet supported).
    let fi = type_btn(scene, l.type_free, false);
    for (dx, dy) in [(0.25, 0.3), (0.7, 0.25), (0.4, 0.7), (0.78, 0.72)] {
        let p = Point::new(fi.x0 + fi.width() * dx, fi.y0 + fi.height() * dy);
        scene.fill(
            Fill::NonZero,
            ID,
            th.border,
            None,
            &Rect::from_center_size(p, (3.0, 3.0)).to_ellipse(),
        );
    }

    // --- Angle / aspect field --------------------------------------------
    let (glabel, gval) = match kind {
        Some(GradientKind::Radial) => (
            "Aspect",
            grad.map(|(g, _)| format!("{:.2}", g.aspect)).unwrap_or_default(),
        ),
        _ => (
            "Angle",
            grad.map(|(g, _)| format!("{:.0}°", g.angle_deg())).unwrap_or_default(),
        ),
    };
    text.draw(scene, glabel, 11.0, th.text_dim, l.geom_label.x, l.geom_label.y);
    field_box(scene, text, th, l.geom_val, &gval);
    chevron(scene, l.geom_dn, false, th.text_dim);
    chevron(scene, l.geom_up, true, th.text_dim);

    // --- The ramp + stops ---------------------------------------------
    match grad {
        None => {
            checker(scene, l.bar);
            scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.bar);
            text.draw(
                scene,
                "No gradient — pick Linear or Radial, or click a shape's Gradient fill.",
                10.5,
                th.text_dim,
                l.bar.x0,
                l.track.y1 + 4.0,
            );
        }
        Some((g, sel)) => {
            paint_ramp(scene, l.bar, g);
            scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.bar);

            // Midpoint diamonds between consecutive stops (on the bar's top).
            for pair in g.stops.windows(2) {
                let mid = pair[0].offset + (pair[1].offset - pair[0].offset) * pair[0].midpoint;
                let cx = stop_x(l.bar, mid);
                let mut d = BezPath::new();
                d.move_to((cx, l.bar.y0 - 5.0));
                d.line_to((cx + 3.5, l.bar.y0 - 1.5));
                d.line_to((cx, l.bar.y0 + 2.0));
                d.line_to((cx - 3.5, l.bar.y0 - 1.5));
                d.close_path();
                scene.fill(Fill::NonZero, ID, th.text_dim, None, &d);
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
                    &Stroke::new(if on { 1.8 } else { 1.0 }),
                    ID,
                    if on { th.accent } else { th.text_dim },
                    None,
                    &h,
                );
            }
        }
    }

    // --- Selected-stop colour + location + opacity ----------------------
    if let Some((g, sel)) = grad {
        let stop = g.stops.get(*sel).copied().unwrap_or(g.stops[0]);
        checker(scene, l.swatch);
        let c = stop.color;
        scene.fill(
            Fill::NonZero,
            ID,
            Color::new([c.r, c.g, c.b, c.a * stop.opacity]),
            None,
            &l.swatch,
        );
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.swatch);

        text.draw(scene, "Loc", 11.0, th.text_dim, l.loc_label.x, l.loc_label.y);
        field_box(scene, text, th, l.loc_val, &format!("{:.0}%", stop.offset * 100.0));
        chevron(scene, l.loc_dn, false, th.text_dim);
        chevron(scene, l.loc_up, true, th.text_dim);

        text.draw(scene, "Opac", 11.0, th.text_dim, l.op_label.x, l.op_label.y);
        field_box(scene, text, th, l.op_val, &format!("{:.0}%", stop.opacity * 100.0));
        chevron(scene, l.op_dn, false, th.text_dim);
        chevron(scene, l.op_up, true, th.text_dim);
    }
    let _ = l.bottom;
}

fn field_box(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, r: Rect, s: &str) {
    scene.fill(Fill::NonZero, ID, th.bg, None, &r);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &r);
    text.draw(scene, s, 11.0, th.text, r.x0 + 4.0, r.y0 + 13.0);
}

fn chevron(scene: &mut Scene, r: Rect, up: bool, color: Color) {
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

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);

    if l.type_lin.contains(p) {
        return Action::GradientKind(GradientKind::Linear);
    }
    if l.type_rad.contains(p) {
        return Action::GradientKind(GradientKind::Radial);
    }
    if l.type_free.contains(p) {
        return Action::None; // Freeform not supported yet
    }

    let geom_field = match ctx.gradient.as_ref().map(|(g, _)| g.kind) {
        Some(GradientKind::Radial) => GradField::Aspect,
        _ => GradField::Angle,
    };
    if l.geom_dn.contains(p) {
        return Action::GradientStep(geom_field, -step_of(geom_field));
    }
    if l.geom_up.contains(p) {
        return Action::GradientStep(geom_field, step_of(geom_field));
    }

    if ctx.gradient.is_some() {
        if l.loc_dn.contains(p) {
            return Action::GradientStep(GradField::Location, -step_of(GradField::Location));
        }
        if l.loc_up.contains(p) {
            return Action::GradientStep(GradField::Location, step_of(GradField::Location));
        }
        if l.op_dn.contains(p) {
            return Action::GradientStep(GradField::Opacity, -step_of(GradField::Opacity));
        }
        if l.op_up.contains(p) {
            return Action::GradientStep(GradField::Opacity, step_of(GradField::Opacity));
        }
        if l.swatch.contains(p) {
            return Action::GradientStopPicker;
        }

        if let Some((g, _)) = ctx.gradient.as_ref() {
            // A stop handle?
            for (i, stop) in g.stops.iter().enumerate() {
                if stop_rect(l.bar, stop.offset).inflate(2.0, 2.0).contains(p) {
                    return Action::GradientSelectStop { index: i, bar: l.bar };
                }
            }
            // Empty track → add a stop where clicked.
            if l.track.contains(p) || l.bar.contains(p) {
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

/// The numeric field under `p`, for scroll-to-nudge (mirrors the Transform
/// panel's `xform_field_at`).
pub fn field_at(body: Rect, p: Point, kind: Option<GradientKind>) -> Option<GradField> {
    let l = layout(body);
    let geom = match kind {
        Some(GradientKind::Radial) => GradField::Aspect,
        _ => GradField::Angle,
    };
    if l.geom_val.inflate(14.0, 2.0).contains(p) {
        return Some(geom);
    }
    if l.loc_val.inflate(14.0, 2.0).contains(p) {
        return Some(GradField::Location);
    }
    if l.op_val.inflate(14.0, 2.0).contains(p) {
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
    } else if l.bar.contains(p) || l.track.contains(p) {
        Some("Click to add a stop · drag a stop off to delete")
    } else {
        None
    }
}
