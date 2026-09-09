//! Pathfinder panel — Illustrator-style shape modes and pathfinders.

use crate::metrics::px as ui_px;

use amalith_commands::PathfinderOp;
use vello::kurbo::{Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{Action, Ctx, ID, metric_pad};

fn metric_btn() -> f64 { crate::metrics::with(|m| m.panels_pathfinder_btn) }
fn metric_gap() -> f64 { crate::metrics::with(|m| m.panels_pathfinder_gap) }

struct L {
    modes: [Rect; 4],
    expand: Rect,
    finders: [Rect; 6],
    bottom: f64,
}

fn layout(body: Rect) -> L {
    let x0 = body.x0 + metric_pad();
    let mut y = body.y0 + ui_px(28.0);
    let modes = std::array::from_fn(|i| {
        let x = x0 + i as f64 * (metric_btn() + metric_gap());
        Rect::new(x, y, x + metric_btn(), y + metric_btn())
    });
    let expand = Rect::new(
        x0 + 4.0 * (metric_btn() + metric_gap()),
        y,
        x0 + 4.0 * (metric_btn() + metric_gap()) + ui_px(64.0),
        y + metric_btn(),
    );
    y += metric_btn() + ui_px(28.0);
    let finders = std::array::from_fn(|i| {
        let x = x0 + i as f64 * (metric_btn() + metric_gap());
        Rect::new(x, y, x + metric_btn(), y + metric_btn())
    });
    L {
        modes,
        expand,
        finders,
        bottom: y + metric_btn() + metric_pad(),
    }
}

pub fn natural_height() -> f64 {
    layout(Rect::new(0.0, 0.0, ui_px(280.0), ui_px(400.0))).bottom
}

fn can_expand(ctx: &Ctx) -> bool {
    ctx.selection.iter().any(|id| {
        ctx.doc
            .object(*id)
            .is_some_and(|o| amalith_commands::has_visible_stroke(&o.appearance))
    })
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = layout(body);
    let th = ctx.theme;
    text.draw(
        scene,
        "Shape Modes:",
        11.0,
        th.text_dim,
        body.x0 + metric_pad(),
        body.y0 + ui_px(18.0),
    );
    let mode_ops = [
        PathfinderOp::Unite,
        PathfinderOp::MinusFront,
        PathfinderOp::Intersect,
        PathfinderOp::Exclude,
    ];
    for (r, op) in l.modes.iter().zip(mode_ops) {
        paint_btn(scene, *r, th, r.contains(ctx.pointer), true);
        paint_mode_icon(scene, *r, op, th.text, th.bg);
    }
    if can_expand(ctx) {
        let hot = l.expand.contains(ctx.pointer);
        let rr = l.expand.to_rounded_rect(ui_px(4.0));
        scene.fill(
            Fill::NonZero,
            ID,
            if hot { th.strip_bg } else { th.bg },
            None,
            &rr,
        );
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &rr);
        let w = text.measure("Expand", 11.0);
        text.draw(
            scene,
            "Expand",
            11.0,
            th.text,
            l.expand.center().x - w * 0.5,
            l.expand.center().y + ui_px(4.0),
        );
    }

    text.draw(
        scene,
        "Pathfinders:",
        11.0,
        th.text_dim,
        body.x0 + metric_pad(),
        l.finders[0].y0 - ui_px(8.0),
    );
    let finder_ops = [
        PathfinderOp::Divide,
        PathfinderOp::Trim,
        PathfinderOp::Merge,
        PathfinderOp::Crop,
        PathfinderOp::Outline,
        PathfinderOp::MinusBack,
    ];
    for (r, op) in l.finders.iter().zip(finder_ops) {
        paint_btn(scene, *r, th, r.contains(ctx.pointer), true);
        paint_finder_icon(scene, *r, op, th.text);
    }
}

fn paint_btn(scene: &mut Scene, r: Rect, th: &crate::theme::Theme, hot: bool, _on: bool) {
    let rr = r.to_rounded_rect(ui_px(4.0));
    scene.fill(
        Fill::NonZero,
        ID,
        if hot { th.strip_bg } else { th.bg },
        None,
        &rr,
    );
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &rr);
}

fn two_sq(r: Rect) -> (Rect, Rect) {
    let s = ui_px(11.0);
    let a = Rect::new(r.x0 + ui_px(4.0), r.y0 + ui_px(5.0), r.x0 + ui_px(4.0) + s, r.y0 + ui_px(5.0) + s);
    let b = a + vello::kurbo::Vec2::new(ui_px(7.0), ui_px(6.0));
    (a, b)
}

fn fill_sq(scene: &mut Scene, r: Rect, c: Color) {
    scene.fill(Fill::NonZero, ID, c, None, &r.to_rounded_rect(ui_px(1.5)));
}

fn paint_mode_icon(scene: &mut Scene, r: Rect, op: PathfinderOp, ink: Color, bg: Color) {
    let (a, b) = two_sq(r);
    let dim = ink.with_alpha(0.35);
    match op {
        PathfinderOp::Unite => {
            fill_sq(scene, a, ink);
            fill_sq(scene, b, ink);
        }
        PathfinderOp::MinusFront => {
            fill_sq(scene, a, ink);
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, dim, None, &b);
        }
        PathfinderOp::Intersect => {
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, dim, None, &a);
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, dim, None, &b);
            let hit = a.intersect(b);
            if hit.width() > 0.0 && hit.height() > 0.0 {
                fill_sq(scene, hit, ink);
            }
        }
        PathfinderOp::Exclude => {
            fill_sq(scene, a, ink);
            fill_sq(scene, b, ink);
            let hit = a.intersect(b);
            if hit.width() > 0.0 && hit.height() > 0.0 {
                scene.fill(Fill::NonZero, ID, bg, None, &hit);
            }
        }
        _ => {}
    }
}

fn paint_finder_icon(scene: &mut Scene, r: Rect, op: PathfinderOp, ink: Color) {
    let (a, b) = two_sq(r);
    let dim = ink.with_alpha(0.4);
    match op {
        PathfinderOp::Divide => {
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, ink, None, &a);
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, ink, None, &b);
        }
        PathfinderOp::Trim => {
            fill_sq(scene, a, dim);
            fill_sq(scene, b, ink);
        }
        PathfinderOp::Merge => {
            fill_sq(scene, a, ink);
            fill_sq(scene, b, ink);
        }
        PathfinderOp::Crop => {
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, dim, None, &a);
            fill_sq(scene, b, ink);
        }
        PathfinderOp::Outline => {
            scene.stroke(&Stroke::new(ui_px(1.2)), ID, ink, None, &a);
            scene.stroke(&Stroke::new(ui_px(1.2)), ID, ink, None, &b);
        }
        PathfinderOp::MinusBack => {
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, dim, None, &a);
            fill_sq(scene, b, ink);
        }
        _ => {}
    }
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);
    let mode_ops = [
        PathfinderOp::Unite,
        PathfinderOp::MinusFront,
        PathfinderOp::Intersect,
        PathfinderOp::Exclude,
    ];
    for (r, op) in l.modes.iter().zip(mode_ops) {
        if r.contains(p) {
            return Action::Pathfinder(op);
        }
    }
    if can_expand(ctx) && l.expand.contains(p) {
        return Action::ExpandStroke;
    }
    let finder_ops = [
        PathfinderOp::Divide,
        PathfinderOp::Trim,
        PathfinderOp::Merge,
        PathfinderOp::Crop,
        PathfinderOp::Outline,
        PathfinderOp::MinusBack,
    ];
    for (r, op) in l.finders.iter().zip(finder_ops) {
        if r.contains(p) {
            return Action::Pathfinder(op);
        }
    }
    Action::None
}

pub fn tip(body: Rect, p: Point, ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    let modes = [
        (l.modes[0], "Unite"),
        (l.modes[1], "Minus Front"),
        (l.modes[2], "Intersect"),
        (l.modes[3], "Exclude"),
    ];
    for (r, t) in modes {
        if r.contains(p) {
            return Some(t);
        }
    }
    if can_expand(ctx) && l.expand.contains(p) {
        return Some("Expand Stroke");
    }
    let finders = [
        (l.finders[0], "Divide"),
        (l.finders[1], "Trim"),
        (l.finders[2], "Merge"),
        (l.finders[3], "Crop"),
        (l.finders[4], "Outline"),
        (l.finders[5], "Minus Back"),
    ];
    for (r, t) in finders {
        if r.contains(p) {
            return Some(t);
        }
    }
    None
}
