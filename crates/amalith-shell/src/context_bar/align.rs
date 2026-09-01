//! Align cluster for the options bar, matching Illustrator's Control bar:
//! Align To dropdown, six align buttons, six distribute buttons.

use amalith_commands::AlignKind;
use vello::kurbo::{BezPath, Point, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::panels::{self, Action};
use crate::text::TextContext;

use super::{Ctx, SegKind, Segment, ID};

const BTN: f64 = 22.0;
const GAP: f64 = 2.0;
const GROUP: f64 = 8.0;
const DROP_W: f64 = 32.0;

const ALIGN: [AlignKind; 6] = [
    AlignKind::HLeft,
    AlignKind::HCenter,
    AlignKind::HRight,
    AlignKind::VTop,
    AlignKind::VCenter,
    AlignKind::VBottom,
];
const DIST: [AlignKind; 6] = [
    AlignKind::DistVTop,
    AlignKind::DistVCenter,
    AlignKind::DistVBottom,
    AlignKind::DistHLeft,
    AlignKind::DistHCenter,
    AlignKind::DistHRight,
];

/// dropdown + 6 align + 6 distribute, with a group gap in each six.
const WIDTH: f64 = DROP_W
    + 6.0
    + (6.0 * BTN + 5.0 * GAP + GROUP)
    + 8.0
    + (6.0 * BTN + 5.0 * GAP + GROUP);

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Align,
    applies: |ctx| ctx.selection_len > 0,
    measure: |_| WIDTH,
    paint,
    hit,
};

struct Parts {
    drop: Rect,
    align: [Rect; 6],
    dist: [Rect; 6],
}

fn six(x0: f64, y0: f64) -> (f64, [Rect; 6]) {
    let mut x = x0;
    let rects = std::array::from_fn(|i| {
        if i == 3 {
            x += GROUP;
        }
        let b = Rect::new(x, y0, x + BTN, y0 + BTN);
        x += BTN + GAP;
        b
    });
    (x, rects)
}

fn parts(r: Rect) -> Parts {
    let y0 = r.center().y - BTN * 0.5;
    let drop = Rect::new(r.x0, y0, r.x0 + DROP_W, y0 + BTN);
    let (x, align) = six(drop.x1 + 6.0, y0);
    let (_, dist) = six(x + 6.0, y0);
    Parts { drop, align, dist }
}

fn paint(scene: &mut Scene, _text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let p = parts(r);
    let th = ctx.theme;

    let drop_on = ctx.align_to_menu || p.drop.contains(ctx.pointer);
    if drop_on {
        scene.fill(
            Fill::NonZero,
            ID,
            if ctx.align_to_menu { th.bg } else { th.strip_bg },
            None,
            &p.drop.to_rounded_rect(3.0),
        );
    }
    paint_drop_icon(scene, p.drop, th.text);

    for (slot, kind) in p.align.iter().zip(ALIGN) {
        if slot.contains(ctx.pointer) {
            scene.fill(
                Fill::NonZero,
                ID,
                th.strip_bg,
                None,
                &slot.to_rounded_rect(3.0),
            );
        }
        panels::align::paint_align_icon(scene, *slot, kind, th.text);
    }
    for (slot, kind) in p.dist.iter().zip(DIST) {
        if slot.contains(ctx.pointer) {
            scene.fill(
                Fill::NonZero,
                ID,
                th.strip_bg,
                None,
                &slot.to_rounded_rect(3.0),
            );
        }
        panels::align::paint_dist_icon(scene, *slot, kind, th.text);
    }
}

/// 9-dot grid (Align To) plus a dropdown caret, like Illustrator's Control bar.
fn paint_drop_icon(scene: &mut Scene, r: Rect, ink: Color) {
    let grid = Rect::new(r.x0 + 4.0, r.y0 + 5.0, r.x0 + 18.0, r.y1 - 5.0);
    let d = 2.0;
    for row in 0..3 {
        for col in 0..3 {
            let p = Point::new(
                grid.x0 + 2.0 + col as f64 * 5.0,
                grid.y0 + 2.0 + row as f64 * 4.5,
            );
            scene.fill(
                Fill::NonZero,
                ID,
                ink,
                None,
                &Rect::from_center_size(p, (d, d)),
            );
        }
    }
    let cx = r.x1 - 7.0;
    let cy = r.center().y;
    let mut t = BezPath::new();
    t.move_to((cx - 3.0, cy - 1.5));
    t.line_to((cx + 3.0, cy - 1.5));
    t.line_to((cx, cy + 2.5));
    t.close_path();
    scene.fill(Fill::NonZero, ID, ink, None, &t);
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let p = parts(r);
    if p.drop.contains(local) {
        return Action::OpenAlignToMenu(p.drop);
    }
    for (slot, kind) in p.align.iter().zip(ALIGN) {
        if slot.contains(local) {
            return Action::Align(kind);
        }
    }
    for (slot, kind) in p.dist.iter().zip(DIST) {
        if slot.contains(local) {
            return Action::Align(kind);
        }
    }
    Action::None
}

pub(super) fn tip(r: Rect, local: Point) -> Option<&'static str> {
    let p = parts(r);
    if p.drop.contains(local) {
        return Some("Align To");
    }
    let align = [
        "Horizontal Align Left",
        "Horizontal Align Center",
        "Horizontal Align Right",
        "Vertical Align Top",
        "Vertical Align Center",
        "Vertical Align Bottom",
    ];
    for (slot, t) in p.align.iter().zip(align) {
        if slot.contains(local) {
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
    for (slot, t) in p.dist.iter().zip(dist) {
        if slot.contains(local) {
            return Some(t);
        }
    }
    None
}
