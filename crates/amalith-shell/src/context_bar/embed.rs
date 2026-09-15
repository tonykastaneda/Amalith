//! Image actions: Embed for linked images and Image Trace for any image.

use crate::metrics::px as ui_px;

use vello::kurbo::{Point, Rect};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;
use crate::theme::Theme;

use super::{Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Embed,
    applies: |ctx| ctx.embed_target.is_some() || ctx.trace_target,
    measure: |ctx| {
        ui_px(
            if ctx.embed_target.is_some() {
                72.0
            } else {
                0.0
            } + if ctx.trace_target {
                if ctx.embed_target.is_some() {
                    120.0
                } else {
                    112.0
                }
            } else {
                0.0
            },
        )
    },
    paint,
    hit,
};

fn button_row(r: Rect) -> Rect {
    let half = ui_px(11.5).min((r.height() - ui_px(8.0)).max(0.0) * 0.5);
    Rect::new(r.x0, r.center().y - half, r.x1, r.center().y + half)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let r = button_row(r);
    if ctx.embed_target.is_some() {
        button(
            scene,
            text,
            ctx.theme,
            Rect::new(r.x0, r.y0, r.x0 + ui_px(72.), r.y1),
            "Embed",
        );
    }
    if ctx.trace_target {
        button(scene, text, ctx.theme, trace_rect(r, ctx), "Image Trace");
    }
}

fn trace_rect(r: Rect, ctx: &Ctx) -> Rect {
    Rect::new(
        r.x0 + if ctx.embed_target.is_some() {
            ui_px(80.)
        } else {
            0.
        },
        r.y0,
        r.x1,
        r.y1,
    )
}

fn hit(r: Rect, local: Point, ctx: &Ctx) -> Action {
    let r = button_row(r);
    if r.contains(local) {
        if ctx.trace_target && trace_rect(r, ctx).contains(local) {
            return Action::StartImageTrace;
        }
        if local.x <= r.x0 + ui_px(72.) {
            if let Some(id) = ctx.embed_target {
                return Action::EmbedAsset(id);
            }
        }
    }
    Action::None
}

fn button(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str) {
    scene.fill(
        Fill::NonZero,
        ID,
        theme.bg,
        None,
        &r.to_rounded_rect(ui_px(4.0)),
    );
    scene.stroke(
        &vello::kurbo::Stroke::new(ui_px(1.0)),
        ID,
        theme.text_dim.with_alpha(0.5),
        None,
        &r.to_rounded_rect(ui_px(4.0)),
    );
    let w = text.measure(label, 13.0);
    text.draw(
        scene,
        label,
        13.0,
        theme.text,
        r.x0 + (r.width() - w) * 0.5,
        r.y0 + r.height() * 0.5 + ui_px(4.5),
    );
}
