//! Align panel — Illustrator Align: objects, distribute, spacing, Align To.

use amalith_commands::{AlignKind, AlignTo};
use vello::kurbo::{BezPath, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{Action, Ctx, ID, PAD};

const BTN: f64 = 26.0;
const GAP: f64 = 4.0;
/// Gap between the two trios in an icon row (H-align | V-align, and the
/// matching distribute pair). A fixed, modest break — not "shove the
/// second trio to the far edge" — so a wide panel doesn't tear the row
/// in half. Illustrator uses roughly this.
const GROUP_GAP: f64 = 22.0;
/// Section rhythm, shared by all four blocks so the panel reads as one
/// system (matches the Transform / Pathfinder panels' spacing feel).
const LABEL_DROP: f64 = 12.0; // section top → label baseline
const LABEL_TO_ROW: f64 = 10.0; // label baseline → icon row top
const ROW_TO_RULE: f64 = 14.0; // icon row bottom → divider
const AFTER_RULE: f64 = 14.0; // divider → next section top

struct L {
    lab_align: Point,
    align: [Rect; 6],
    rule_a: Rect,
    lab_dist: Point,
    dist: [Rect; 6],
    rule_b: Rect,
    lab_space: Point,
    space_h: Rect,
    space_v: Rect,
    space_field: Rect,
    rule_c: Rect,
    lab_to: Point,
    to: [Rect; 3],
    bottom: f64,
}

fn group3(x0: f64, y: f64) -> [Rect; 3] {
    std::array::from_fn(|i| {
        let x = x0 + i as f64 * (BTN + GAP);
        Rect::new(x, y, x + BTN, y + BTN)
    })
}

/// Two trios left-packed from `x0` with a fixed [`GROUP_GAP`] between
/// them. Only tightens the gap if the panel is too narrow to fit both
/// trios at that spacing (never overlaps, never overflows `x1`).
fn six(x0: f64, x1: f64, y: f64) -> [Rect; 6] {
    let g3 = 3.0 * BTN + 2.0 * GAP;
    let left = group3(x0, y);
    let want = x0 + g3 + GROUP_GAP;
    let max = (x1 - g3).max(x0 + g3 + 6.0);
    let right = group3(want.min(max), y);
    [left[0], left[1], left[2], right[0], right[1], right[2]]
}

fn layout(body: Rect) -> L {
    let x0 = body.x0 + PAD;
    let x1 = body.x1 - PAD;
    let mut y = body.y0 + PAD;

    // Every section: label, icon row, full-width divider — same cadence.
    let section_label = |y: &mut f64| {
        let p = Point::new(x0, *y + LABEL_DROP);
        *y += LABEL_DROP + LABEL_TO_ROW;
        p
    };
    let section_rule = |y: &mut f64| {
        *y += BTN + ROW_TO_RULE;
        let r = Rect::new(x0, *y, x1, *y + 1.0);
        *y += 1.0 + AFTER_RULE;
        r
    };

    let lab_align = section_label(&mut y);
    let align = six(x0, x1, y);
    let rule_a = section_rule(&mut y);

    let lab_dist = section_label(&mut y);
    let dist = six(x0, x1, y);
    let rule_b = section_rule(&mut y);

    // Distribute Spacing: the two even-spacing buttons, then a field that
    // runs to the panel edge like any other full-width control.
    let lab_space = section_label(&mut y);
    let space_h = Rect::new(x0, y, x0 + BTN, y + BTN);
    let space_v = Rect::new(x0 + BTN + GAP, y, x0 + 2.0 * BTN + GAP, y + BTN);
    let field_x0 = space_v.x1 + 8.0;
    let space_field = Rect::new(field_x0, y + 2.0, x1.max(field_x0 + 48.0), y + BTN - 2.0);
    let rule_c = section_rule(&mut y);

    // Align To: its own stacked section, left-aligned trio.
    let lab_to = section_label(&mut y);
    let to = group3(x0, y);
    y += BTN + PAD;

    L {
        lab_align,
        align,
        rule_a,
        lab_dist,
        dist,
        rule_b,
        lab_space,
        space_h,
        space_v,
        space_field,
        rule_c,
        lab_to,
        to,
        bottom: y,
    }
}

pub fn natural_height() -> f64 {
    layout(Rect::new(0.0, 0.0, 280.0, 400.0)).bottom
}

// Align: H left/center/right, then V top/center/bottom.
const ALIGN: [AlignKind; 6] = [
    AlignKind::HLeft,
    AlignKind::HCenter,
    AlignKind::HRight,
    AlignKind::VTop,
    AlignKind::VCenter,
    AlignKind::VBottom,
];
// Distribute: V top/center/bottom, then H left/center/right (Illustrator order).
const DIST: [AlignKind; 6] = [
    AlignKind::DistVTop,
    AlignKind::DistVCenter,
    AlignKind::DistVBottom,
    AlignKind::DistHLeft,
    AlignKind::DistHCenter,
    AlignKind::DistHRight,
];
const TO: [AlignTo; 3] = [AlignTo::Artboard, AlignTo::Selection, AlignTo::KeyObject];

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = layout(body);
    let th = ctx.theme;

    text.draw(scene, "Align Objects:", 11.0, th.text_dim, l.lab_align.x, l.lab_align.y);
    for (r, kind) in l.align.iter().zip(ALIGN) {
        paint_btn(scene, *r, th, r.contains(ctx.pointer), false);
        paint_align_icon(scene, *r, kind, th.text);
    }

    scene.fill(Fill::NonZero, ID, th.splitter, None, &l.rule_a);
    text.draw(scene, "Distribute Objects:", 11.0, th.text_dim, l.lab_dist.x, l.lab_dist.y);
    for (r, kind) in l.dist.iter().zip(DIST) {
        paint_btn(scene, *r, th, r.contains(ctx.pointer), false);
        paint_dist_icon(scene, *r, kind, th.text);
    }

    scene.fill(Fill::NonZero, ID, th.splitter, None, &l.rule_b);
    text.draw(
        scene,
        "Distribute Spacing:",
        11.0,
        th.text_dim,
        l.lab_space.x,
        l.lab_space.y,
    );
    paint_btn(scene, l.space_h, th, l.space_h.contains(ctx.pointer), false);
    paint_space_icon(scene, l.space_h, true, th.text);
    paint_btn(scene, l.space_v, th, l.space_v.contains(ctx.pointer), false);
    paint_space_icon(scene, l.space_v, false, th.text);

    let editing = ctx.align_spacing_edit.is_some();
    let rr = l.space_field.to_rounded_rect(3.0);
    scene.fill(
        Fill::NonZero,
        ID,
        if editing || l.space_field.contains(ctx.pointer) {
            th.strip_bg
        } else {
            th.bg
        },
        None,
        &rr,
    );
    scene.stroke(
        &Stroke::new(if editing { 1.5 } else { 1.0 }),
        ID,
        if editing { th.accent } else { th.border },
        None,
        &rr,
    );
    let label = ctx
        .align_spacing_edit
        .map(str::to_string)
        .unwrap_or_else(|| {
            ctx.align_spacing
                .map(|v| format!("{v} px"))
                .unwrap_or_else(|| "Auto".into())
        });
    text.draw(
        scene,
        &label,
        11.0,
        th.text,
        l.space_field.x0 + 8.0,
        l.space_field.center().y + 4.0,
    );

    scene.fill(Fill::NonZero, ID, th.splitter, None, &l.rule_c);
    text.draw(scene, "Align To:", 11.0, th.text_dim, l.lab_to.x, l.lab_to.y);
    for (r, to) in l.to.iter().zip(TO) {
        let on = ctx.align_to == to;
        let dim = to == AlignTo::KeyObject && ctx.selection.len() < 2;
        let hot = r.contains(ctx.pointer);
        if on || hot {
            let rr = r.to_rounded_rect(3.0);
            scene.fill(
                Fill::NonZero,
                ID,
                if on { th.bg } else { th.strip_bg },
                None,
                &rr,
            );
        }
        let ink = if on {
            th.accent
        } else if dim {
            th.text_dim
        } else {
            th.text
        };
        paint_to_icon(scene, *r, to, ink);
    }
}

fn paint_btn(scene: &mut Scene, r: Rect, th: &crate::theme::Theme, hot: bool, on: bool) {
    if !hot && !on {
        return;
    }
    let rr = r.to_rounded_rect(4.0);
    scene.fill(
        Fill::NonZero,
        ID,
        if on {
            th.accent.with_alpha(0.22)
        } else {
            th.strip_bg
        },
        None,
        &rr,
    );
}

fn bar(scene: &mut Scene, r: Rect, ink: Color) {
    scene.fill(Fill::NonZero, ID, ink, None, &r.to_rounded_rect(0.8));
}

pub(crate) fn paint_align_icon(scene: &mut Scene, r: Rect, kind: AlignKind, ink: Color) {
    let c = r.center();
    match kind {
        AlignKind::HLeft | AlignKind::HCenter | AlignKind::HRight => {
            let (w1, w2, h) = (13.0, 8.0, 4.0);
            let y1 = r.y0 + 6.0;
            let y2 = r.y1 - 6.0 - h;
            let (x1, x2, lx) = match kind {
                AlignKind::HLeft => (r.x0 + 6.0, r.x0 + 6.0, r.x0 + 6.0),
                AlignKind::HRight => (r.x1 - 6.0 - w1, r.x1 - 6.0 - w2, r.x1 - 6.0),
                _ => (c.x - w1 * 0.5, c.x - w2 * 0.5, c.x),
            };
            bar(scene, Rect::new(x1, y1, x1 + w1, y1 + h), ink);
            bar(scene, Rect::new(x2, y2, x2 + w2, y2 + h), ink);
            scene.stroke(
                &Stroke::new(1.2),
                ID,
                ink,
                None,
                &Line::new((lx, r.y0 + 4.0), (lx, r.y1 - 4.0)),
            );
        }
        AlignKind::VTop | AlignKind::VCenter | AlignKind::VBottom => {
            let (h1, h2, w) = (13.0, 8.0, 4.0);
            let x1 = r.x0 + 6.0;
            let x2 = r.x1 - 6.0 - w;
            let (y1, y2, ly) = match kind {
                AlignKind::VTop => (r.y0 + 6.0, r.y0 + 6.0, r.y0 + 6.0),
                AlignKind::VBottom => (r.y1 - 6.0 - h1, r.y1 - 6.0 - h2, r.y1 - 6.0),
                _ => (c.y - h1 * 0.5, c.y - h2 * 0.5, c.y),
            };
            bar(scene, Rect::new(x1, y1, x1 + w, y1 + h1), ink);
            bar(scene, Rect::new(x2, y2, x2 + w, y2 + h2), ink);
            scene.stroke(
                &Stroke::new(1.2),
                ID,
                ink,
                None,
                &Line::new((r.x0 + 4.0, ly), (r.x1 - 4.0, ly)),
            );
        }
        _ => {}
    }
}

pub(crate) fn paint_dist_icon(scene: &mut Scene, r: Rect, kind: AlignKind, ink: Color) {
    let vert = matches!(
        kind,
        AlignKind::DistVTop | AlignKind::DistVCenter | AlignKind::DistVBottom
    );
    if vert {
        let widths = [14.0, 8.0, 11.0];
        let h = 3.5;
        let gap = 3.0;
        let total = 3.0 * h + 2.0 * gap;
        let y0 = r.center().y - total * 0.5;
        for (i, &w) in widths.iter().enumerate() {
            let y = y0 + i as f64 * (h + gap);
            let x = r.center().x - w * 0.5;
            let b = Rect::new(x, y, x + w, y + h);
            bar(scene, b, ink);
            let ty = match kind {
                AlignKind::DistVTop => b.y0,
                AlignKind::DistVBottom => b.y1,
                _ => b.center().y,
            };
            scene.stroke(
                &Stroke::new(1.1),
                ID,
                ink,
                None,
                &Line::new((b.x0 - 1.5, ty), (b.x1 + 1.5, ty)),
            );
        }
    } else {
        let heights = [14.0, 8.0, 11.0];
        let w = 3.5;
        let gap = 3.0;
        let total = 3.0 * w + 2.0 * gap;
        let x0 = r.center().x - total * 0.5;
        for (i, &h) in heights.iter().enumerate() {
            let x = x0 + i as f64 * (w + gap);
            let y = r.center().y - h * 0.5;
            let b = Rect::new(x, y, x + w, y + h);
            bar(scene, b, ink);
            let tx = match kind {
                AlignKind::DistHLeft => b.x0,
                AlignKind::DistHRight => b.x1,
                _ => b.center().x,
            };
            scene.stroke(
                &Stroke::new(1.1),
                ID,
                ink,
                None,
                &Line::new((tx, b.y0 - 1.5), (tx, b.y1 + 1.5)),
            );
        }
    }
}

fn paint_space_icon(scene: &mut Scene, r: Rect, horiz: bool, ink: Color) {
    if horiz {
        let a = Rect::new(r.x0 + 8.0, r.y0 + 5.5, r.x0 + 16.0, r.y0 + 13.5);
        let b = Rect::new(r.x0 + 13.0, r.y0 + 12.5, r.x0 + 21.0, r.y0 + 20.5);
        bar(scene, a, ink);
        bar(scene, b, ink);
        let x = r.x0 + 4.5;
        let y = r.center().y;
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x, y - 3.0), (x, y + 3.0)));
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x + 4.0, y - 3.0), (x + 4.0, y + 3.0)));
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x, y), (x + 4.0, y)));
    } else {
        let a = Rect::new(r.x0 + 6.0, r.y0 + 10.0, r.x0 + 12.0, r.y1 - 5.0);
        let b = Rect::new(r.x0 + 14.0, r.y0 + 10.0, r.x0 + 20.0, r.y1 - 5.0);
        bar(scene, a, ink);
        bar(scene, b, ink);
        let x = r.center().x;
        let y = r.y0 + 4.5;
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x - 3.0, y), (x + 3.0, y)));
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x - 3.0, y + 4.0), (x + 3.0, y + 4.0)));
        scene.stroke(&Stroke::new(1.2), ID, ink, None, &Line::new((x, y), (x, y + 4.0)));
    }
}

pub(crate) fn paint_to_icon(scene: &mut Scene, r: Rect, to: AlignTo, ink: Color) {
    match to {
        AlignTo::Artboard => {
            let p = Rect::new(r.x0 + 7.0, r.y0 + 5.5, r.x1 - 7.0, r.y1 - 5.5);
            let fold = 4.5;
            let mut path = BezPath::new();
            path.move_to((p.x0, p.y0));
            path.line_to((p.x1 - fold, p.y0));
            path.line_to((p.x1 - fold, p.y0 + fold));
            path.line_to((p.x1, p.y0 + fold));
            path.line_to((p.x1, p.y1));
            path.line_to((p.x0, p.y1));
            path.close_path();
            scene.fill(Fill::NonZero, ID, ink, None, &path);
        }
        AlignTo::Selection => {
            paint_marquee(scene, r.inset(5.5), ink);
        }
        AlignTo::KeyObject => {
            let box_ = Rect::new(r.x0 + 5.0, r.y0 + 4.5, r.x1 - 8.0, r.y1 - 8.0);
            paint_marquee(scene, box_, ink);
            paint_pointer(scene, Point::new(r.center().x + 1.0, r.center().y - 1.0), ink);
        }
    }
}

fn paint_marquee(scene: &mut Scene, r: Rect, ink: Color) {
    scene.stroke(
        &Stroke::new(1.2).with_dashes(0.0, [2.2, 1.8]),
        ID,
        ink,
        None,
        &r,
    );
    let d = 2.4;
    let pts = [
        Point::new(r.x0, r.y0),
        Point::new(r.center().x, r.y0),
        Point::new(r.x1, r.y0),
        Point::new(r.x1, r.center().y),
        Point::new(r.x1, r.y1),
        Point::new(r.center().x, r.y1),
        Point::new(r.x0, r.y1),
        Point::new(r.x0, r.center().y),
    ];
    for p in pts {
        scene.fill(
            Fill::NonZero,
            ID,
            ink,
            None,
            &Rect::from_center_size(p, (d, d)),
        );
    }
}

fn paint_pointer(scene: &mut Scene, tip: Point, ink: Color) {
    let mut p = BezPath::new();
    p.move_to(tip);
    p.line_to((tip.x + 6.0, tip.y + 10.0));
    p.line_to((tip.x + 3.4, tip.y + 10.0));
    p.line_to((tip.x + 5.2, tip.y + 14.0));
    p.line_to((tip.x + 3.8, tip.y + 14.6));
    p.line_to((tip.x + 1.8, tip.y + 10.4));
    p.line_to((tip.x - 0.4, tip.y + 13.0));
    p.close_path();
    scene.fill(Fill::NonZero, ID, ink, None, &p);
    scene.stroke(&Stroke::new(0.8), ID, ink, None, &p);
}

pub fn hit(body: Rect, p: Point, _ctx: &Ctx) -> Action {
    let l = layout(body);
    for (r, kind) in l.align.iter().zip(ALIGN) {
        if r.contains(p) {
            return Action::Align(kind);
        }
    }
    for (r, kind) in l.dist.iter().zip(DIST) {
        if r.contains(p) {
            return Action::Align(kind);
        }
    }
    if l.space_h.contains(p) {
        return Action::Align(AlignKind::DistHSpace);
    }
    if l.space_v.contains(p) {
        return Action::Align(AlignKind::DistVSpace);
    }
    if l.space_field.contains(p) {
        return Action::BeginAlignSpacingEdit;
    }
    for (r, to) in l.to.iter().zip(TO) {
        if r.contains(p) {
            return Action::SetAlignTo(to);
        }
    }
    Action::None
}

pub fn tip(body: Rect, p: Point, _ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    let align = [
        "Horizontal Align Left",
        "Horizontal Align Center",
        "Horizontal Align Right",
        "Vertical Align Top",
        "Vertical Align Center",
        "Vertical Align Bottom",
    ];
    for (r, t) in l.align.iter().zip(align) {
        if r.contains(p) {
            return Some(t);
        }
    }
    let dist = [
        "Vertical Distribute Top",
        "Vertical Distribute Center",
        "Vertical Distribute Bottom",
        "Horizontal Distribute Left",
        "Horizontal Distribute Center",
        "Horizontal Distribute Right",
    ];
    for (r, t) in l.dist.iter().zip(dist) {
        if r.contains(p) {
            return Some(t);
        }
    }
    if l.space_h.contains(p) {
        return Some("Horizontal Distribute Space");
    }
    if l.space_v.contains(p) {
        return Some("Vertical Distribute Space");
    }
    if l.space_field.contains(p) {
        return Some("Distribute Spacing (Auto or px)");
    }
    let to = [
        "Align to Artboard",
        "Align to Selection",
        "Align to Key Object",
    ];
    for (r, t) in l.to.iter().zip(to) {
        if r.contains(p) {
            return Some(t);
        }
    }
    None
}

/// Spacing field under `p`, for click-outside commit.
pub fn spacing_field_at(body: Rect, p: Point) -> bool {
    layout(body).space_field.contains(p)
}

pub fn menu(ctx: &Ctx) -> Vec<super::MenuEntry> {
    vec![super::MenuEntry::Item {
        id: "cancel-key",
        label: "Cancel Key Object",
        checked: ctx.key_object.is_some(),
    }]
}
