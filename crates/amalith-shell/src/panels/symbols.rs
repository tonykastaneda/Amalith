//! Symbols panel: one row per pooled symbol definition, with a New /
//! Place / Edit / Delete workflow — Illustrator's Symbols panel, minus
//! its thumbnail art and drag-to-canvas placement (click-to-place via the
//! footer "Place" button instead; see `amalith-commands`'
//! `Command::DefineSymbol`/`PlaceSymbolInstance` docs for why that's a
//! reasonable first cut). A row shows the same diamond glyph as the
//! panel's own tab icon in place of a live-rendered thumbnail — real
//! per-definition raster thumbnails are a natural fast-follow (the same
//! `canvas::export_scene` + `App::render_scene_to_rgba` pipeline
//! `app/thumbnails.rs` already uses for Home-screen previews), not
//! implemented yet.

use crate::metrics::px as ui_px;

use amalith_core::Document;
use vello::kurbo::{Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::dock::{PanelId, PanelKind};
use crate::text::TextContext;
use crate::theme::Theme;

use super::{Action, Ctx, MenuEntry, metric_footer_h, ID, metric_pad, metric_row_h};

/// The scrollable row list (between the top and the footer).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0, body.x1, body.y1 - metric_footer_h())
}

fn clamp_scroll(raw: f64, n_rows: usize, list_h: f64) -> f64 {
    let max = (n_rows as f64 * metric_row_h() - list_h).max(0.0);
    raw.clamp(0.0, max)
}

/// Full height the Symbols panel wants: every row + footer.
pub(super) fn content_height(doc: &Document) -> f64 {
    doc.symbols().len() as f64 * metric_row_h() + metric_footer_h()
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let list = list_rect(body);
    let symbols = ctx.doc.symbols();
    let scroll = clamp_scroll(ctx.symbols_scroll, symbols.len(), list.height());

    // "Drop here to make a symbol" — a canvas object drag is hovering this
    // panel right now (see `App::docked_symbols_panel_body_at`). A light
    // tint under the content plus a solid frame on top, so it reads at a
    // glance without hiding the row list underneath.
    if ctx.symbols_drop_hover {
        scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.12), None, &body);
    }

    scene.push_clip_layer(Fill::NonZero, ID, &list);
    if symbols.is_empty() {
        text.draw(
            scene,
            "No symbols — select artwork and choose New Symbol",
            12.0,
            ctx.theme.text_dim,
            body.x0 + metric_pad(),
            list.y0 + metric_row_h() * 0.5 + ui_px(4.0),
        );
    }
    for (i, def) in symbols.iter().enumerate() {
        let ry = list.y0 + i as f64 * metric_row_h() - scroll;
        if ry + metric_row_h() < list.y0 || ry > list.y1 {
            continue;
        }
        let r = Rect::new(list.x0, ry, list.x1, ry + metric_row_h());
        if ctx.selected_symbol == Some(def.id) {
            scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.22), None, &r);
        }
        let baseline = r.y0 + metric_row_h() * 0.5 + ui_px(4.0);
        let glyph_rect = Rect::new(
            list.x0 + metric_pad(),
            r.y0 + ui_px(6.0),
            list.x0 + metric_pad() + ui_px(20.0),
            r.y1 - ui_px(6.0),
        );
        crate::panel_icon::draw(scene, PanelId(PanelKind::Symbols), glyph_rect, ctx.theme.text_dim);
        text.draw(scene, &def.name, 12.0, ctx.theme.text, glyph_rect.x1 + ui_px(8.0), baseline);
    }
    for i in 1..symbols.len() {
        let y = list.y0 + i as f64 * metric_row_h() - scroll;
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, ctx.theme.border, None, &Line::new((list.x0, y), (list.x1, y)));
    }
    scene.pop_layer();

    let content_h = symbols.len() as f64 * metric_row_h();
    if content_h > list.height() + 0.5 {
        let frac = (list.height() / content_h).min(1.0);
        let th = (list.height() * frac).max(ui_px(24.0));
        let ty = list.y0 + (list.height() - th) * (scroll / (content_h - list.height()));
        scene.fill(
            Fill::NonZero,
            ID,
            ctx.theme.text_dim.with_alpha(0.5),
            None,
            &Rect::new(list.x1 - ui_px(4.0), ty, list.x1 - 1.0, ty + th).to_rounded_rect(ui_px(1.5)),
        );
    }

    paint_footer(scene, text, ctx, body);

    if ctx.symbols_drop_hover {
        scene.stroke(&Stroke::new(ui_px(2.0)), ID, ctx.theme.accent, None, &body);
    }
}

/// (Place, Edit, Delete) rects, right-aligned along the footer.
fn footer_buttons(body: Rect) -> [Rect; 3] {
    let w = ui_px(60.0);
    let gap = ui_px(8.0);
    let cy = body.y1 - metric_footer_h() * 0.5;
    let mut x1 = body.x1 - metric_pad();
    let mut rects = [Rect::ZERO; 3];
    for k in (0..3).rev() {
        let r = Rect::new(x1 - w, cy - ui_px(11.0), x1, cy + ui_px(11.0));
        rects[k] = r;
        x1 = r.x0 - gap;
    }
    rects
}

fn paint_footer(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let strip = Rect::new(body.x0, body.y1 - metric_footer_h(), body.x1, body.y1);
    scene.fill(Fill::NonZero, ID, ctx.theme.strip_bg, None, &strip);
    scene.fill(
        Fill::NonZero,
        ID,
        ctx.theme.border,
        None,
        &Rect::new(strip.x0, strip.y0, strip.x1, strip.y0 + 1.0),
    );

    let has_selection = ctx.selected_symbol.is_some();
    let [place, edit, delete] = footer_buttons(body);
    for (r, label, enabled) in [(place, "Place", has_selection), (edit, "Edit", has_selection), (delete, "Delete", has_selection)] {
        button(scene, text, ctx.theme, r, label, enabled, r.contains(ctx.pointer));
    }
}

fn button(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    label: &str,
    enabled: bool,
    hot: bool,
) {
    let ink = if !enabled {
        theme.border
    } else if hot {
        theme.text
    } else {
        theme.text_dim
    };
    scene.stroke(
        &Stroke::new(ui_px(1.0)),
        ID,
        ink.with_alpha(if enabled { 0.6 } else { 0.4 }),
        None,
        &r.to_rounded_rect(ui_px(4.0)),
    );
    let w = text.measure(label, 11.0);
    text.draw(scene, label, 11.0, ink, r.x0 + (r.width() - w) * 0.5, r.y0 + r.height() * 0.5 + ui_px(4.0));
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    if local.y >= body.y1 - metric_footer_h() {
        let [place, edit, delete] = footer_buttons(body);
        let Some(id) = ctx.selected_symbol else { return Action::None };
        if place.contains(local) {
            return Action::PlaceSymbolInstance(id);
        }
        if edit.contains(local) {
            return Action::EditSymbolDefinition(id);
        }
        if delete.contains(local) {
            return Action::DeleteSymbolDefinition(id);
        }
        return Action::None;
    }
    let list = list_rect(body);
    let symbols = ctx.doc.symbols();
    let scroll = clamp_scroll(ctx.symbols_scroll, symbols.len(), list.height());
    let i = ((local.y - list.y0 + scroll) / metric_row_h()).floor();
    if i < 0.0 {
        return Action::None;
    }
    match symbols.get(i as usize) {
        Some(def) => Action::SelectSymbol(def.id),
        None => Action::None,
    }
}

/// Hamburger flyout: New Symbol always; Rename only with a row selected.
pub(super) fn menu(ctx: &Ctx) -> Vec<MenuEntry> {
    let mut entries = vec![MenuEntry::Item { id: "new-symbol", label: "New Symbol from Selection", checked: false }];
    if ctx.selected_symbol.is_some() {
        entries.push(MenuEntry::Item { id: "rename", label: "Rename", checked: false });
    }
    entries
}
