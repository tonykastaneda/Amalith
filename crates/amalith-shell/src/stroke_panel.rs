//! The Stroke flyout: a compact take on Illustrator's Stroke panel,
//! opened from the "Stroke" link in the options bar. Weight, cap, corner
//! (join), miter limit, alignment, and a dash / gap pair — everything
//! about a stroke except its paint.
//!
//! Like the options-bar number fields, every value here is driven by its
//! stepper arrows and the scroll wheel; there is no keyboard entry.

use amalith_core::{LineCap, LineJoin, StrokeAlign, StrokeStyle};
use vello::kurbo::{Affine, BezPath, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;

pub const W: f64 = 240.0;
pub const H: f64 = 232.0;

const PAD: f64 = 12.0;
/// X where a row's controls begin (past its label).
const CTRL_X: f64 = 60.0;
const BTN_W: f64 = 26.0;
const BTN_H: f64 = 20.0;
const BTN_GAP: f64 = 7.0;
const FIELD_W: f64 = 46.0;
const STEP_W: f64 = 13.0;

/// What a click in the flyout asks for. `Inside` = a harmless click on
/// the panel body (swallow it); `Outside` = close the flyout.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Inside,
    Outside,
    WeightStep(i32),
    LimitStep(i32),
    Cap(LineCap),
    Join(LineJoin),
    Align(StrokeAlign),
    ToggleDashed,
    DashStep(i32),
    GapStep(i32),
}

pub struct Layout {
    pub panel: Rect,
    weight_field: Rect,
    weight_up: Rect,
    weight_down: Rect,
    cap: [Rect; 3],
    join: [Rect; 3],
    limit_field: Rect,
    limit_up: Rect,
    limit_down: Rect,
    align: [Rect; 3],
    dash_check: Rect,
    dash_field: Rect,
    dash_up: Rect,
    dash_down: Rect,
    gap_field: Rect,
    gap_up: Rect,
    gap_down: Rect,
}

fn field_at(x: f64, cy: f64) -> (Rect, Rect, Rect) {
    let field = Rect::new(x, cy - 10.0, x + FIELD_W, cy + 10.0);
    let up = Rect::new(x + FIELD_W, cy - 10.0, x + FIELD_W + STEP_W, cy);
    let down = Rect::new(x + FIELD_W, cy, x + FIELD_W + STEP_W, cy + 10.0);
    (field, up, down)
}

fn btn_row(x0: f64, cy: f64) -> [Rect; 3] {
    std::array::from_fn(|i| {
        let x = x0 + i as f64 * (BTN_W + BTN_GAP);
        Rect::new(x, cy - BTN_H * 0.5, x + BTN_W, cy + BTN_H * 0.5)
    })
}

/// Build the flyout's rects with its top-left at `origin`.
pub fn layout(origin: Point) -> Layout {
    let panel = Rect::new(origin.x, origin.y, origin.x + W, origin.y + H);
    let cx = panel.x0 + CTRL_X;
    let row = |i: f64| panel.y0 + 22.0 + i * 30.0;

    let (weight_field, weight_up, weight_down) = field_at(cx, row(0.0));
    let cap = btn_row(cx, row(1.0));
    let join = btn_row(cx, row(2.0));
    let (limit_field, limit_up, limit_down) = field_at(cx, row(3.0));
    let align = btn_row(cx, row(4.0));

    let dash_check = Rect::new(panel.x0 + PAD, row(5.0) - 7.0, panel.x0 + PAD + 14.0, row(5.0) + 7.0);
    let (dash_field, dash_up, dash_down) = field_at(cx, row(6.0));
    let gap_x = cx + FIELD_W + STEP_W + 34.0;
    let (gap_field, gap_up, gap_down) = field_at(gap_x, row(6.0));

    Layout {
        panel,
        weight_field,
        weight_up,
        weight_down,
        cap,
        join,
        limit_field,
        limit_up,
        limit_down,
        align,
        dash_check,
        dash_field,
        dash_up,
        dash_down,
        gap_field,
        gap_up,
        gap_down,
    }
}

pub fn hit(lay: &Layout, style: &StrokeStyle, p: Point) -> Hit {
    if !lay.panel.contains(p) {
        return Hit::Outside;
    }
    if lay.weight_up.contains(p) {
        return Hit::WeightStep(1);
    }
    if lay.weight_down.contains(p) {
        return Hit::WeightStep(-1);
    }
    for (i, r) in lay.cap.iter().enumerate() {
        if r.contains(p) {
            return Hit::Cap([LineCap::Butt, LineCap::Round, LineCap::Square][i]);
        }
    }
    for (i, r) in lay.join.iter().enumerate() {
        if r.contains(p) {
            return Hit::Join([LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][i]);
        }
    }
    if style.join == LineJoin::Miter {
        if lay.limit_up.contains(p) {
            return Hit::LimitStep(1);
        }
        if lay.limit_down.contains(p) {
            return Hit::LimitStep(-1);
        }
    }
    for (i, r) in lay.align.iter().enumerate() {
        if r.contains(p) {
            return Hit::Align([StrokeAlign::Center, StrokeAlign::Inside, StrokeAlign::Outside][i]);
        }
    }
    if lay.dash_check.contains(p) || dash_label_rect(lay).contains(p) {
        return Hit::ToggleDashed;
    }
    if style.dashed {
        if lay.dash_up.contains(p) {
            return Hit::DashStep(1);
        }
        if lay.dash_down.contains(p) {
            return Hit::DashStep(-1);
        }
        if lay.gap_up.contains(p) {
            return Hit::GapStep(1);
        }
        if lay.gap_down.contains(p) {
            return Hit::GapStep(-1);
        }
    }
    Hit::Inside
}

/// Scroll wheel over the flyout: same targets as the stepper arrows.
pub fn scroll_hit(lay: &Layout, style: &StrokeStyle, p: Point) -> Option<Hit> {
    if !lay.panel.contains(p) {
        return None;
    }
    let col = |f: Rect, u: Rect, d: Rect| {
        Rect::new(f.x0, f.y0, u.x1.max(d.x1), f.y1).contains(p)
    };
    if col(lay.weight_field, lay.weight_up, lay.weight_down) {
        return Some(Hit::WeightStep(0));
    }
    if style.join == LineJoin::Miter && col(lay.limit_field, lay.limit_up, lay.limit_down) {
        return Some(Hit::LimitStep(0));
    }
    if style.dashed {
        if col(lay.dash_field, lay.dash_up, lay.dash_down) {
            return Some(Hit::DashStep(0));
        }
        if col(lay.gap_field, lay.gap_up, lay.gap_down) {
            return Some(Hit::GapStep(0));
        }
    }
    None
}

fn dash_label_rect(lay: &Layout) -> Rect {
    Rect::new(
        lay.dash_check.x1 + 4.0,
        lay.dash_check.y0,
        lay.dash_check.x1 + 78.0,
        lay.dash_check.y1,
    )
}

// ---- painting ------------------------------------------------------------

pub fn paint(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    lay: &Layout,
    style: &StrokeStyle,
    weight: f64,
) {
    let p = lay.panel;
    // Drop shadow + body.
    scene.fill(
        Fill::NonZero,
        ID,
        Color::from_rgb8(0, 0, 0).with_alpha(0.28),
        None,
        &p.with_origin(Point::new(p.x0 + 3.0, p.y0 + 4.0)),
    );
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &p);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &p);

    let label = |text: &mut TextContext, scene: &mut Scene, s: &str, y: f64, dim: bool| {
        let c = if dim { theme.text_dim.with_alpha(0.5) } else { theme.text_dim };
        text.draw(scene, s, 11.0, c, p.x0 + PAD, y + 4.0);
    };
    let row_cy = |i: f64| p.y0 + 22.0 + i * 30.0;

    // Weight.
    label(text, scene, "Weight", row_cy(0.0), false);
    num_field(scene, text, theme, lay.weight_field, lay.weight_up, lay.weight_down, &fmt_px(weight), false);

    // Cap.
    label(text, scene, "Cap", row_cy(1.0), false);
    for (i, r) in lay.cap.iter().enumerate() {
        let sel = style.cap == [LineCap::Butt, LineCap::Round, LineCap::Square][i];
        toggle(scene, *r, theme, sel);
        cap_glyph(scene, *r, i, glyph_color(theme, sel));
    }

    // Corner (join).
    label(text, scene, "Corner", row_cy(2.0), false);
    for (i, r) in lay.join.iter().enumerate() {
        let sel = style.join == [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel][i];
        toggle(scene, *r, theme, sel);
        join_glyph(scene, *r, i, glyph_color(theme, sel));
    }

    // Miter limit — only meaningful with a miter join.
    let miter = style.join == LineJoin::Miter;
    label(text, scene, "Limit", row_cy(3.0), !miter);
    num_field(
        scene,
        text,
        theme,
        lay.limit_field,
        lay.limit_up,
        lay.limit_down,
        &format!("{:.0} x", style.miter_limit),
        !miter,
    );

    // Align stroke.
    label(text, scene, "Align", row_cy(4.0), false);
    for (i, r) in lay.align.iter().enumerate() {
        let sel = style.align == [StrokeAlign::Center, StrokeAlign::Inside, StrokeAlign::Outside][i];
        toggle(scene, *r, theme, sel);
        align_glyph(scene, *r, i, glyph_color(theme, sel));
    }

    // Dashed line.
    checkbox(scene, lay.dash_check, theme, style.dashed);
    text.draw(
        scene,
        "Dashed Line",
        11.0,
        theme.text_dim,
        lay.dash_check.x1 + 6.0,
        lay.dash_check.y0 + 11.0,
    );

    let pair = dash_gap(style);
    label(text, scene, "Dash", row_cy(6.0), !style.dashed);
    num_field(
        scene,
        text,
        theme,
        lay.dash_field,
        lay.dash_up,
        lay.dash_down,
        &format!("{:.0}", pair.0),
        !style.dashed,
    );
    text.draw(
        scene,
        "Gap",
        11.0,
        if style.dashed { theme.text_dim } else { theme.text_dim.with_alpha(0.5) },
        lay.gap_field.x0 - 30.0,
        lay.gap_field.y0 + 14.0,
    );
    num_field(
        scene,
        text,
        theme,
        lay.gap_field,
        lay.gap_up,
        lay.gap_down,
        &format!("{:.0}", pair.1),
        !style.dashed,
    );
}

fn fmt_px(v: f64) -> String {
    if (v.fract()).abs() < 0.01 {
        format!("{v:.0} px")
    } else {
        format!("{v:.2} px")
    }
}

/// First dash / gap pair, defaulting to a sensible 12 / 6 when empty.
pub fn dash_gap(style: &StrokeStyle) -> (f64, f64) {
    let (d, g) = (style.dash[0], style.dash[1]);
    if d <= 0.0 && g <= 0.0 {
        (12.0, 6.0)
    } else {
        (d, g)
    }
}

fn glyph_color(theme: &Theme, sel: bool) -> Color {
    if sel {
        Color::from_rgb8(0xff, 0xff, 0xff)
    } else {
        theme.text_dim
    }
}

fn toggle(scene: &mut Scene, r: Rect, theme: &Theme, sel: bool) {
    if sel {
        scene.fill(Fill::NonZero, ID, theme.accent, None, &r);
    } else {
        scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    }
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        if sel { theme.accent } else { theme.border },
        None,
        &r,
    );
}

fn checkbox(scene: &mut Scene, r: Rect, theme: &Theme, on: bool) {
    scene.fill(Fill::NonZero, ID, if on { theme.accent } else { theme.bg }, None, &r);
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        if on { theme.accent } else { theme.border },
        None,
        &r,
    );
    if on {
        let mut tick = BezPath::new();
        tick.move_to((r.x0 + 3.0, r.center().y));
        tick.line_to((r.center().x - 1.0, r.y1 - 3.5));
        tick.line_to((r.x1 - 3.0, r.y0 + 3.5));
        scene.stroke(&Stroke::new(1.6), ID, Color::from_rgb8(0xff, 0xff, 0xff), None, &tick);
    }
}

fn num_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    field: Rect,
    up: Rect,
    down: Rect,
    value: &str,
    dim: bool,
) {
    let border = theme.text_dim.with_alpha(if dim { 0.25 } else { 0.5 });
    let ink = if dim { theme.text_dim.with_alpha(0.5) } else { theme.text };
    scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &field);
    text.draw(scene, value, 11.0, ink, field.x0 + 5.0, field.y0 + field.height() * 0.5 + 4.0);

    let stepper = Rect::new(up.x0, up.y0, up.x1, down.y1);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &stepper);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &stepper);
    let cx = stepper.x0 + stepper.width() * 0.5;
    let mut t = BezPath::new();
    t.move_to((cx - 3.0, up.y1 - 3.0));
    t.line_to((cx + 3.0, up.y1 - 3.0));
    t.line_to((cx, up.y0 + 3.0));
    t.close_path();
    scene.fill(Fill::NonZero, ID, border, None, &t);
    let mut b = BezPath::new();
    b.move_to((cx - 3.0, down.y0 + 3.0));
    b.line_to((cx + 3.0, down.y0 + 3.0));
    b.line_to((cx, down.y1 - 3.0));
    b.close_path();
    scene.fill(Fill::NonZero, ID, border, None, &b);
}

fn cap_glyph(scene: &mut Scene, r: Rect, i: usize, c: Color) {
    let cy = r.center().y;
    let x0 = r.x0 + 5.0;
    let x1 = r.x1 - 8.0;
    let w = 4.0;
    match i {
        0 => {
            // Butt — line stops flat.
            scene.stroke(&Stroke::new(w), ID, c, None, &Line::new((x0, cy), (x1, cy)));
        }
        1 => {
            // Round — semicircle past the end.
            scene.stroke(&Stroke::new(w), ID, c, None, &Line::new((x0, cy), (x1, cy)));
            scene.fill(
                Fill::NonZero,
                ID,
                c,
                None,
                &vello::kurbo::Circle::new((x1, cy), w * 0.5),
            );
        }
        _ => {
            // Projecting — square overhangs by half the weight.
            scene.stroke(
                &Stroke::new(w),
                ID,
                c,
                None,
                &Line::new((x0, cy), (x1 + w * 0.5, cy)),
            );
        }
    }
    // End marker.
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        c.with_alpha(0.6),
        None,
        &Line::new((x1, cy - 6.0), (x1, cy + 6.0)),
    );
}

fn join_glyph(scene: &mut Scene, r: Rect, i: usize, c: Color) {
    let a = Point::new(r.x0 + 6.0, r.y1 - 5.0);
    let corner = Point::new(r.x0 + 6.0, r.y0 + 6.0);
    let b = Point::new(r.x1 - 5.0, r.y0 + 6.0);
    let w = 3.0;
    match i {
        0 => {
            let mut p = BezPath::new();
            p.move_to(a);
            p.line_to(corner);
            p.line_to(b);
            scene.stroke(&Stroke::new(w).with_join(vello::kurbo::Join::Miter), ID, c, None, &p);
        }
        1 => {
            let mut p = BezPath::new();
            p.move_to(a);
            p.line_to(Point::new(corner.x, corner.y + 3.0));
            p.quad_to(corner, Point::new(corner.x + 3.0, corner.y));
            p.line_to(b);
            scene.stroke(&Stroke::new(w), ID, c, None, &p);
        }
        _ => {
            let mut p = BezPath::new();
            p.move_to(a);
            p.line_to((corner.x, corner.y + 3.0));
            p.line_to((corner.x + 3.0, corner.y));
            p.line_to(b);
            scene.stroke(&Stroke::new(w).with_join(vello::kurbo::Join::Bevel), ID, c, None, &p);
        }
    }
}

fn align_glyph(scene: &mut Scene, r: Rect, i: usize, c: Color) {
    // A shape edge (thin) with the stroke band drawn on one side of it.
    let edge_y = r.center().y;
    let x0 = r.x0 + 5.0;
    let x1 = r.x1 - 5.0;
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        c.with_alpha(0.55),
        None,
        &Line::new((x0, edge_y), (x1, edge_y)),
    );
    let band = 3.0;
    let (y0, y1) = match i {
        0 => (edge_y - band, edge_y + band),
        1 => (edge_y, edge_y + band * 2.0),
        _ => (edge_y - band * 2.0, edge_y),
    };
    scene.fill(
        Fill::NonZero,
        ID,
        c,
        None,
        &Rect::new(x0, y0, x1, y1),
    );
}
