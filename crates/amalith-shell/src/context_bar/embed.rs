//! The "Embed" button — shown only while a single Linked image is selected.
//! Clicking it copies the file's bytes into the document and switches the
//! asset over to Embedded (see `App::embed_asset`).

use vello::kurbo::{Point, Rect};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;
use crate::theme::Theme;

use super::{Ctx, SegKind, Segment, ID};

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Embed,
    applies: |ctx| ctx.embed_target.is_some(),
    measure: |_| 72.0,
    paint,
    hit,
};

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    button(scene, text, ctx.theme, r, "Embed");
}

fn hit(r: Rect, local: Point, ctx: &Ctx) -> Action {
    if r.contains(local) {
        if let Some(id) = ctx.embed_target {
            return Action::EmbedAsset(id);
        }
    }
    Action::None
}

fn button(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str) {
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r.to_rounded_rect(4.0));
    scene.stroke(
        &vello::kurbo::Stroke::new(1.0),
        ID,
        theme.text_dim.with_alpha(0.5),
        None,
        &r.to_rounded_rect(4.0),
    );
    let w = text.measure(label, 13.0);
    text.draw(
        scene,
        label,
        13.0,
        theme.text,
        r.x0 + (r.width() - w) * 0.5,
        r.y0 + r.height() * 0.5 + 4.5,
    );
}
