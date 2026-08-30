//! Artboards panel: a numbered list with sizes, inline rename, and a
//! footer button strip.

use vello::kurbo::{Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;

use super::{
    draw_name_field, panel_footer_rects, row_rect, paint_panel_footer, Action, Ctx, RenameId, FOOTER_H,
    ID, PAD, ROW_H,
};

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    for (i, ab) in ctx.doc.artboards().iter().enumerate() {
        let r = row_rect(body, i);
        if ctx.selected_artboard == Some(ab.id) {
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.select_blue.with_alpha(0.22),
                None,
                &r,
            );
        }
        let w = ab.rect.width().round() as i64;
        let h = ab.rect.height().round() as i64;
        text.draw(
            scene,
            &format!("{:02}", i + 1),
            12.0,
            ctx.theme.text_dim,
            body.x0 + PAD,
            r.y0 + ROW_H * 0.5 + 4.0,
        );
        let editing = match ctx.renaming {
            Some((RenameId::Artboard(a), buf)) if a == ab.id => Some(buf),
            _ => None,
        };
        draw_name_field(scene, text, ctx.theme, body.x0 + PAD + 34.0, r, &ab.name, ctx.theme.text, editing);
        if editing.is_none() {
            text.draw(
                scene,
                &format!("{w} × {h}"),
                11.0,
                ctx.theme.text_dim,
                body.x1 - PAD - 90.0,
                r.y0 + ROW_H * 0.5 + 4.0,
            );
        }
    }
    // Hairline separators.
    for i in 1..ctx.doc.artboards().len() {
        let y = body.y0 + i as f64 * ROW_H;
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            ctx.theme.border,
            None,
            &vello::kurbo::Line::new((body.x0, y), (body.x1, y)),
        );
    }

    // Footer: reordering artboards isn't wired yet, so up/down are
    // disabled; add and delete are always live.
    paint_panel_footer(scene, body, ctx.theme, ctx.pointer, [false, false, true, true]);
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    if local.y >= body.y1 - FOOTER_H {
        let [_, _, add, del] = panel_footer_rects(body);
        return if add.contains(local) {
            Action::NewArtboard
        } else if del.contains(local) {
            Action::DeleteArtboard
        } else {
            Action::None
        };
    }
    let row = ((local.y - body.y0) / ROW_H).floor();
    if row >= 0.0 {
        if let Some(ab) = ctx.doc.artboards().get(row as usize) {
            // The number column (before the name field) is the "snap the
            // view back onto this artboard" target; the rest of the row
            // just selects / renames.
            let num_x1 = body.x0 + PAD + 30.0;
            if local.x < num_x1 {
                return Action::FocusArtboard(ab.id);
            }
            return Action::SelectArtboard(ab.id);
        }
    }
    Action::None
}
