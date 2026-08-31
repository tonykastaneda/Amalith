//! Swatches panel: the fill / stroke chips, a stroke-width row, and the
//! preset colour grid.

use amalith_core::{Color as CoreColor, Paint};
use vello::kurbo::{Point, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{draw_paint_swatch, palette, Action, Ctx, PaintSlot, ID, PAD, SWATCH};

const STROKE_WIDTHS: [f64; 5] = [1.0, 2.0, 4.0, 8.0, 16.0];

struct SwatchLayout {
    fill: Rect,
    stroke: Rect,
    widths: Vec<(f64, Rect)>,
    swatches: Vec<(Paint, Rect)>,
}

fn swatch_layout(body: Rect) -> SwatchLayout {
    let fill = Rect::new(body.x0 + PAD, body.y0 + 10.0, body.x0 + PAD + 24.0, body.y0 + 34.0);
    let stroke = fill.with_origin(Point::new(fill.x0 + 14.0, fill.y0 + 14.0));

    let wx = stroke.x1 + 18.0;
    let widths = STROKE_WIDTHS
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let x = wx + i as f64 * 26.0;
            (*w, Rect::new(x, body.y0 + 14.0, x + 22.0, body.y0 + 36.0))
        })
        .collect();

    let top = body.y0 + 56.0;
    let cols = (((body.width() - PAD * 2.0) / (SWATCH + 4.0)).floor() as usize).max(1);
    let swatches = palette()
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let (col, row) = (i % cols, i / cols);
            let x = body.x0 + PAD + col as f64 * (SWATCH + 4.0);
            let y = top + row as f64 * (SWATCH + 4.0);
            (p, Rect::new(x, y, x + SWATCH, y + SWATCH))
        })
        .collect();

    SwatchLayout {
        fill,
        stroke,
        widths,
        swatches,
    }
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = swatch_layout(body);
    let rep = ctx.representative;
    let white = Color::from_rgb8(0xff, 0xff, 0xff);

    // Stroke behind, fill in front (Illustrator's overlap).
    draw_paint_swatch(
        scene,
        ctx.theme,
        l.stroke,
        rep.map(|a| a.stroke).unwrap_or(Paint::None),
        ctx.active_slot == PaintSlot::Stroke,
    );
    draw_paint_swatch(
        scene,
        ctx.theme,
        l.fill,
        rep.map(|a| a.fill)
            .unwrap_or(Paint::Solid(CoreColor::rgb(0.87, 0.87, 0.87))),
        ctx.active_slot == PaintSlot::Fill,
    );

    let cur_w = rep.map(|a| a.stroke_width);
    for (w, r) in &l.widths {
        let on = cur_w.is_some_and(|c| (c - *w).abs() < 0.01);
        scene.fill(
            Fill::NonZero,
            ID,
            if on { ctx.theme.accent } else { ctx.theme.strip_bg },
            None,
            r,
        );
        text.draw(
            scene,
            &format!("{}", *w as i64),
            11.0,
            if on { white } else { ctx.theme.text_dim },
            r.x0 + 6.0,
            r.y0 + 15.0,
        );
    }

    for (p, r) in &l.swatches {
        draw_paint_swatch(scene, ctx.theme, *r, *p, false);
    }
}

pub(super) fn hit(body: Rect, local: Point, _ctx: &Ctx) -> Action {
    let l = swatch_layout(body);
    if l.fill.contains(local) {
        return Action::OpenPicker(PaintSlot::Fill);
    }
    if l.stroke.contains(local) {
        return Action::OpenPicker(PaintSlot::Stroke);
    }
    for (w, r) in &l.widths {
        if r.contains(local) {
            return Action::SetStrokeWidth(*w);
        }
    }
    for (p, r) in &l.swatches {
        if r.contains(local) {
            return Action::SetPaint(*p);
        }
    }
    Action::None
}
