//! Paragraph panel — the Illustrator paragraph controls.
//!
//! Reads [`Ctx::text_align`] / [`Ctx::text_paragraph`] (the live text
//! edit, else a selected text object, else the new-text defaults) and
//! emits [`Action`]s the shell applies back.
//!
//! Live: the seven alignment modes, left / right / first-line indent,
//! space before / after, hyphenate toggle. Bullet / numbered lists are
//! greyed for now.

use crate::metrics::px as ui_px;

use amalith_core::TextAlign;
use vello::kurbo::{BezPath, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{Action, Ctx, ParaField, ID, metric_pad};

fn metric_btn() -> f64 { crate::metrics::with(|m| m.panels_paragraph_btn) }
fn metric_bgap() -> f64 { crate::metrics::with(|m| m.panels_paragraph_bgap) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.panels_paragraph_field_h) }
fn metric_row() -> f64 { crate::metrics::with(|m| m.panels_paragraph_row) }

/// Alignment buttons, left to right.
const ALIGNS: [TextAlign; 7] = [
    TextAlign::Start,
    TextAlign::Center,
    TextAlign::End,
    TextAlign::JustifyLeft,
    TextAlign::JustifyCenter,
    TextAlign::JustifyRight,
    TextAlign::JustifyAll,
];

struct Field {
    box_: Rect,
    up: Rect,
    down: Rect,
}

fn field(x: f64, y: f64, w: f64) -> Field {
    let box_ = Rect::new(x, y, x + w, y + metric_field_h());
    let sx = box_.x1 - ui_px(15.0);
    Field {
        box_,
        up: Rect::new(sx, y + 1.0, box_.x1, y + metric_field_h() / 2.0),
        down: Rect::new(sx, y + metric_field_h() / 2.0, box_.x1, y + metric_field_h() - 1.0),
    }
}

struct L {
    aligns: [Rect; 7],
    list_bullet: Rect,
    list_number: Rect,
    rule_a: Rect,
    indent_start: Field,
    indent_end: Field,
    indent_first: Field,
    rule_b: Rect,
    space_before: Field,
    space_after: Field,
    hyphenate: Rect,
    bottom: f64,
}

fn layout(body: Rect) -> L {
    let x = body.x0 + metric_pad();
    let w = body.width() - metric_pad() * 2.0;
    let half = (w - ui_px(8.0)) / 2.0;
    let mut y = body.y0 + metric_pad() + ui_px(4.0);

    // Seven alignment buttons, evenly spread across the width.
    let cell = (w - metric_bgap() * 6.0) / 7.0;
    let aligns = std::array::from_fn(|i| {
        let bx = x + i as f64 * (cell + metric_bgap());
        Rect::new(bx, y, bx + cell, y + metric_btn())
    });
    y += metric_btn() + ui_px(12.0);

    // List style dropdowns (greyed).
    let list_bullet = Rect::new(x, y, x + ui_px(46.0), y + metric_field_h());
    let list_number = Rect::new(x + ui_px(54.0), y, x + ui_px(100.0), y + metric_field_h());
    y += metric_field_h() + ui_px(12.0);

    let rule_a = Rect::new(x, y, x + w, y + 1.0);
    y += ui_px(13.0);

    let indent_start = field(x, y, half);
    let indent_end = field(x + half + ui_px(8.0), y, half);
    y += metric_row();
    let indent_first = field(x, y, half);
    y += metric_row();

    let rule_b = Rect::new(x, y, x + w, y + 1.0);
    y += ui_px(13.0);

    let space_before = field(x, y, half);
    let space_after = field(x + half + ui_px(8.0), y, half);
    y += metric_row() + ui_px(6.0);

    let hyphenate = Rect::new(x, y, x + ui_px(16.0), y + ui_px(16.0));
    let bottom = y + ui_px(16.0) + metric_pad();

    L {
        aligns,
        list_bullet,
        list_number,
        rule_a,
        indent_start,
        indent_end,
        indent_first,
        rule_b,
        space_before,
        space_after,
        hyphenate,
        bottom,
    }
}

pub fn natural_height() -> f64 {
    layout(Rect::new(0.0, 0.0, ui_px(240.0), ui_px(4000.0))).bottom
}

fn fmt_pt(v: f64) -> String {
    if v.fract().abs() < 0.05 {
        format!("{} pt", v.round() as i64)
    } else {
        format!("{v:.1} pt")
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = layout(body);
    let th = ctx.theme;
    let cur = ctx.text_align;
    let p = ctx.text_paragraph;

    for (r, a) in l.aligns.iter().zip(ALIGNS) {
        let on = cur == a;
        let hot = r.contains(ctx.pointer);
        let bg = if on {
            th.accent
        } else if hot {
            th.strip_bg
        } else {
            th.bg
        };
        scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r.to_rounded_rect(ui_px(4.0)));
        let ink = if on { th.on_accent } else { th.text };
        if ctx.text_vertical {
            align_glyph_vertical(scene, *r, a, ink);
        } else {
            align_glyph(scene, *r, a, ink);
        }
    }

    // List dropdowns — display only for now.
    for r in [l.list_bullet, l.list_number] {
        scene.fill(Fill::NonZero, ID, th.bg, None, &r.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r.to_rounded_rect(ui_px(4.0)));
        let c = Point::new(r.x1 - ui_px(9.0), r.center().y);
        let mut t = BezPath::new();
        t.move_to((c.x - ui_px(3.0), c.y - ui_px(2.0)));
        t.line_to((c.x + ui_px(3.0), c.y - ui_px(2.0)));
        t.line_to((c.x, c.y + ui_px(2.5)));
        t.close_path();
        scene.fill(Fill::NonZero, ID, th.text_dim.with_alpha(0.5), None, &t);
    }
    list_icon(scene, l.list_bullet, true, th.text_dim.with_alpha(0.6));
    list_icon(scene, l.list_number, false, th.text_dim.with_alpha(0.6));

    scene.fill(Fill::NonZero, ID, th.splitter, None, &l.rule_a);
    scene.fill(Fill::NonZero, ID, th.splitter, None, &l.rule_b);

    stepper(scene, text, th, &l.indent_start, IndentIcon::Left, &fmt_pt(p.indent_start), ctx.pointer);
    stepper(scene, text, th, &l.indent_end, IndentIcon::Right, &fmt_pt(p.indent_end), ctx.pointer);
    stepper(scene, text, th, &l.indent_first, IndentIcon::First, &fmt_pt(p.indent_first), ctx.pointer);
    stepper(scene, text, th, &l.space_before, IndentIcon::Before, &fmt_pt(p.space_before), ctx.pointer);
    stepper(scene, text, th, &l.space_after, IndentIcon::After, &fmt_pt(p.space_after), ctx.pointer);

    // Hyphenate checkbox.
    let hc = l.hyphenate;
    scene.fill(Fill::NonZero, ID, th.bg, None, &hc.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &hc.to_rounded_rect(ui_px(3.0)));
    if p.hyphenate {
        let mut check = BezPath::new();
        check.move_to((hc.x0 + ui_px(3.5), hc.center().y));
        check.line_to((hc.x0 + ui_px(6.5), hc.y1 - ui_px(4.0)));
        check.line_to((hc.x1 - ui_px(3.0), hc.y0 + ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.8)), ID, th.accent, None, &check);
    }
    text.draw(scene, "Hyphenate", 12.0, th.text, hc.x1 + ui_px(8.0), hc.center().y + ui_px(4.0));
}

#[derive(Clone, Copy)]
enum IndentIcon {
    Left,
    Right,
    First,
    Before,
    After,
}

fn stepper(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    f: &Field,
    icon: IndentIcon,
    value: &str,
    pointer: Point,
) {
    let rr = f.box_.to_rounded_rect(ui_px(4.0));
    scene.fill(Fill::NonZero, ID, th.bg, None, &rr);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &rr);
    indent_glyph(scene, f.box_, icon, th.text_dim);
    text.draw(scene, value, 12.0, th.text, f.box_.x0 + ui_px(20.0), f.box_.center().y + ui_px(4.0));
    let up_hot = f.up.contains(pointer);
    let dn_hot = f.down.contains(pointer);
    tri(scene, f.up.center(), true, if up_hot { th.text } else { th.text_dim });
    tri(scene, f.down.center(), false, if dn_hot { th.text } else { th.text_dim });
}

/// A small left-of-value glyph telling the fields apart.
fn indent_glyph(scene: &mut Scene, box_: Rect, icon: IndentIcon, ink: Color) {
    let cx = box_.x0 + ui_px(10.0);
    let cy = box_.center().y;
    let s = Stroke::new(ui_px(1.2));
    let bars = |scene: &mut Scene, from_left: bool| {
        for (i, wf) in [0.9f64, 0.6, 0.8].into_iter().enumerate() {
            let y = cy - ui_px(4.0) + i as f64 * ui_px(4.0);
            let (x0, x1) = if from_left {
                (cx - ui_px(4.0), cx - ui_px(4.0) + 8.0 * wf)
            } else {
                (cx + ui_px(4.0) - 8.0 * wf, cx + ui_px(4.0))
            };
            scene.stroke(&s, ID, ink, None, &Line::new((x0, y), (x1, y)));
        }
    };
    match icon {
        IndentIcon::Left => {
            scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(6.0), cy - ui_px(6.0)), (cx - ui_px(6.0), cy + ui_px(6.0))));
            bars(scene, true);
        }
        IndentIcon::Right => {
            scene.stroke(&s, ID, ink, None, &Line::new((cx + ui_px(6.0), cy - ui_px(6.0)), (cx + ui_px(6.0), cy + ui_px(6.0))));
            bars(scene, false);
        }
        IndentIcon::First => {
            let y = cy - ui_px(4.0);
            scene.stroke(&s, ID, ink, None, &Line::new((cx - 1.0, y), (cx + ui_px(4.0), y)));
            scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(4.0), y + ui_px(4.0)), (cx + ui_px(4.0), y + ui_px(4.0))));
            scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(4.0), y + ui_px(8.0)), (cx + ui_px(4.0), y + ui_px(8.0))));
        }
        IndentIcon::Before => {
            scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(5.0), cy - ui_px(5.0)), (cx + ui_px(5.0), cy - ui_px(5.0))));
            arrow(scene, cx, cy + ui_px(4.0), true, ink);
        }
        IndentIcon::After => {
            scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(5.0), cy + ui_px(5.0)), (cx + ui_px(5.0), cy + ui_px(5.0))));
            arrow(scene, cx, cy - ui_px(4.0), false, ink);
        }
    }
}

fn arrow(scene: &mut Scene, cx: f64, cy: f64, down: bool, ink: Color) {
    let d = if down { 1.0 } else { -1.0 };
    let s = Stroke::new(ui_px(1.2));
    scene.stroke(&s, ID, ink, None, &Line::new((cx, cy - 4.0 * d), (cx, cy + 4.0 * d)));
    let mut head = BezPath::new();
    head.move_to((cx - ui_px(2.5), cy));
    head.line_to((cx, cy + 4.0 * d));
    head.line_to((cx + ui_px(2.5), cy));
    scene.stroke(&s, ID, ink, None, &head);
}

/// The horizontal-line "text lines" glyph on an alignment button.
fn align_glyph(scene: &mut Scene, r: Rect, a: TextAlign, ink: Color) {
    let s = Stroke::new(ui_px(1.3));
    let cx = r.center().x;
    let full = r.width() * 0.62;
    let widths: [f64; 4] = if a.is_justified() {
        let last = match a {
            TextAlign::JustifyAll => 1.0,
            TextAlign::JustifyRight | TextAlign::JustifyCenter => 0.55,
            _ => 0.65,
        };
        [1.0, 1.0, 1.0, last]
    } else {
        [1.0, 0.6, 0.85, 0.45]
    };
    for (i, wf) in widths.into_iter().enumerate() {
        let y = r.y0 + r.height() * 0.28 + i as f64 * (r.height() * 0.15);
        let bw = full * wf;
        let (x0, x1) = match a {
            TextAlign::Center | TextAlign::JustifyCenter => (cx - bw / 2.0, cx + bw / 2.0),
            TextAlign::End | TextAlign::JustifyRight => (cx + full / 2.0 - bw, cx + full / 2.0),
            _ => (cx - full / 2.0, cx - full / 2.0 + bw),
        };
        scene.stroke(&s, ID, ink, None, &Line::new((x0, y), (x1, y)));
    }
}

/// The vertical-tick "text columns" glyph an alignment button draws when
/// the active text object is vertical (top-to-bottom columns instead of
/// left-to-right lines) — a mechanical transpose of [`align_glyph`]:
/// columns run right-to-left (matching vertical-writing order — column 0,
/// the tallest tick, sits furthest right, the same order
/// `vertical_text::VerticalLayout` lays real columns out in), and each
/// tick's vertical position maps `Start`/`End` to top/bottom the way
/// `align_glyph` maps them to left/right.
fn align_glyph_vertical(scene: &mut Scene, r: Rect, a: TextAlign, ink: Color) {
    let s = Stroke::new(ui_px(1.3));
    for (x, y0, y1) in align_ticks_vertical(r, a) {
        scene.stroke(&s, ID, ink, None, &Line::new((x, y0), (x, y1)));
    }
}

/// The 4 `(x, y0, y1)` tick segments [`align_glyph_vertical`] draws — split
/// out from the drawing call so the alignment math itself (which edge each
/// tick hugs) is unit-testable without a `Scene`.
fn align_ticks_vertical(r: Rect, a: TextAlign) -> [(f64, f64, f64); 4] {
    let cy = r.center().y;
    let full = r.height() * 0.62;
    let heights: [f64; 4] = if a.is_justified() {
        let last = match a {
            TextAlign::JustifyAll => 1.0,
            TextAlign::JustifyRight | TextAlign::JustifyCenter => 0.55,
            _ => 0.65,
        };
        [1.0, 1.0, 1.0, last]
    } else {
        [1.0, 0.6, 0.85, 0.45]
    };
    std::array::from_fn(|i| {
        let hf = heights[i];
        let x = r.x1 - r.width() * 0.28 - i as f64 * (r.width() * 0.15);
        let bh = full * hf;
        let (y0, y1) = match a {
            TextAlign::Center | TextAlign::JustifyCenter => (cy - bh / 2.0, cy + bh / 2.0),
            TextAlign::End | TextAlign::JustifyRight => (cy + full / 2.0 - bh, cy + full / 2.0),
            _ => (cy - full / 2.0, cy - full / 2.0 + bh),
        };
        (x, y0, y1)
    })
}

fn list_icon(scene: &mut Scene, r: Rect, bullet: bool, ink: Color) {
    let s = Stroke::new(ui_px(1.1));
    for i in 0..3 {
        let y = r.y0 + ui_px(6.0) + i as f64 * ui_px(5.0);
        if bullet {
            scene.fill(
                Fill::NonZero,
                ID,
                ink,
                None,
                &Rect::from_center_size(Point::new(r.x0 + ui_px(6.0), y), (ui_px(2.4), ui_px(2.4))),
            );
        } else {
            scene.stroke(
                &s,
                ID,
                ink,
                None,
                &Line::new((r.x0 + ui_px(4.0), y - 1.5), (r.x0 + ui_px(4.0), y + 1.5)),
            );
        }
        scene.stroke(&s, ID, ink, None, &Line::new((r.x0 + ui_px(12.0), y), (r.x0 + ui_px(26.0), y)));
    }
}

fn tri(scene: &mut Scene, c: Point, up: bool, color: Color) {
    let d = ui_px(3.0);
    let mut p = BezPath::new();
    if up {
        p.move_to((c.x - d, c.y + d * 0.6));
        p.line_to((c.x + d, c.y + d * 0.6));
        p.line_to((c.x, c.y - d * 0.6));
    } else {
        p.move_to((c.x - d, c.y - d * 0.6));
        p.line_to((c.x + d, c.y - d * 0.6));
        p.line_to((c.x, c.y + d * 0.6));
    }
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);
    for (r, a) in l.aligns.iter().zip(ALIGNS) {
        if r.contains(p) {
            return Action::SetTextAlign(a);
        }
    }
    let pg = ctx.text_paragraph;
    for (f, field, cur) in [
        (&l.indent_start, ParaField::IndentStart, pg.indent_start),
        (&l.indent_end, ParaField::IndentEnd, pg.indent_end),
        (&l.indent_first, ParaField::IndentFirst, pg.indent_first),
        (&l.space_before, ParaField::SpaceBefore, pg.space_before),
        (&l.space_after, ParaField::SpaceAfter, pg.space_after),
    ] {
        // First-line indent may go negative (hanging); the rest floor at 0.
        let lo = if field == ParaField::IndentFirst { -ui_px(10000.0) } else { 0.0 };
        if f.up.contains(p) {
            return Action::SetParagraphMetric(field, (cur + 1.0).max(lo));
        }
        if f.down.contains(p) {
            return Action::SetParagraphMetric(field, (cur - 1.0).max(lo));
        }
    }
    let hyph_row = Rect::new(l.hyphenate.x0, l.hyphenate.y0, l.hyphenate.x0 + ui_px(110.0), l.hyphenate.y1);
    if hyph_row.contains(p) {
        return Action::ToggleHyphenate;
    }
    Action::None
}

pub fn tip(body: Rect, p: Point, _ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    let aligns = [
        "Align Left",
        "Align Center",
        "Align Right",
        "Justify with last line aligned left",
        "Justify with last line aligned center",
        "Justify with last line aligned right",
        "Justify all lines",
    ];
    for (r, t) in l.aligns.iter().zip(aligns) {
        if r.contains(p) {
            return Some(t);
        }
    }
    for (f, t) in [
        (&l.indent_start, "Left Indent"),
        (&l.indent_end, "Right Indent"),
        (&l.indent_first, "First-line Left Indent"),
        (&l.space_before, "Space Before Paragraph"),
        (&l.space_after, "Space After Paragraph"),
    ] {
        if f.box_.contains(p) {
            return Some(t);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_align_ticks_run_right_to_left_matching_column_order() {
        let r = Rect::new(0.0, 0.0, 40.0, 24.0);
        let ticks = align_ticks_vertical(r, TextAlign::Start);
        for t in &ticks {
            assert!(t.2 > t.1, "each tick must be a real vertical segment");
        }
        let xs: Vec<f64> = ticks.iter().map(|t| t.0).collect();
        assert!(
            xs.windows(2).all(|w| w[0] > w[1]),
            "column 0 sits furthest right, matching VerticalLayout's own right-to-left order"
        );
    }

    #[test]
    fn vertical_align_start_hugs_the_top_edge() {
        let r = Rect::new(0.0, 0.0, 40.0, 100.0);
        let ticks = align_ticks_vertical(r, TextAlign::Start);
        let top = ticks[0].1;
        for t in &ticks {
            assert!((t.1 - top).abs() < 1e-9, "Start hugs a shared top edge, varying only how far each tick reaches down");
        }
    }

    #[test]
    fn vertical_align_end_hugs_the_bottom_edge() {
        let r = Rect::new(0.0, 0.0, 40.0, 100.0);
        let ticks = align_ticks_vertical(r, TextAlign::End);
        let bottom = ticks[0].2;
        for t in &ticks {
            assert!((t.2 - bottom).abs() < 1e-9, "End hugs a shared bottom edge, varying only how far each tick reaches up");
        }
    }

    #[test]
    fn vertical_align_center_centers_every_tick_on_the_same_midline() {
        let r = Rect::new(0.0, 0.0, 40.0, 100.0);
        let cy = r.center().y;
        let ticks = align_ticks_vertical(r, TextAlign::Center);
        for t in &ticks {
            assert!(((t.1 + t.2) / 2.0 - cy).abs() < 1e-9, "Center keeps every tick's own midpoint on the button's centerline");
        }
    }
}
