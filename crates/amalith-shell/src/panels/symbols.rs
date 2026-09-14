//! Symbol browsing with cached artwork previews and inline renaming.

use crate::metrics::px as ui_px;

use amalith_core::Document;
use vello::kurbo::{Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::prefs::SymbolsView;
use crate::text::TextContext;
use crate::theme::Theme;

use super::{metric_footer_h, metric_pad, metric_row_h, Action, Ctx, MenuEntry, ID};

/// The scrollable row list (between the top and the footer).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0, body.x1, body.y1 - metric_footer_h())
}

fn clamp_scroll(raw: f64, content_h: f64, list_h: f64) -> f64 {
    let max = (content_h - list_h).max(0.0);
    raw.clamp(0.0, max)
}

/// Full height the Symbols panel wants: every row + footer.
fn grid_metrics(width: f64) -> (usize, f64) {
    let cols = (width / ui_px(72.0)).floor().max(1.0) as usize;
    (cols, width / cols as f64)
}
pub(super) fn content_height(doc: &Document, width: f64, view: SymbolsView) -> f64 {
    let n = doc.symbols().len();
    let h = if view == SymbolsView::List {
        n as f64 * metric_row_h()
    } else {
        let (cols, cell) = grid_metrics(width);
        n.div_ceil(cols) as f64 * cell
    };
    h + metric_footer_h()
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let list = list_rect(body);
    let symbols = ctx.doc.symbols();
    let content_h = content_height(ctx.doc, body.width(), ctx.symbols_view) - metric_footer_h();
    let scroll = clamp_scroll(ctx.symbols_scroll, content_h, list.height());

    // "Drop here to make a symbol" — a canvas object drag is hovering this
    // panel right now (see `App::docked_symbols_panel_body_at`). A light
    // tint under the content plus a solid frame on top, so it reads at a
    // glance without hiding the row list underneath.
    if ctx.symbols_drop_hover {
        scene.fill(
            Fill::NonZero,
            ID,
            ctx.theme.accent.with_alpha(0.12),
            None,
            &body,
        );
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
    ctx.symbol_tiles.borrow_mut().clear();
    let (cols, cell) = grid_metrics(list.width());
    for (i, def) in symbols.iter().enumerate() {
        if ctx.symbols_view == SymbolsView::Thumbnails {
            let x = list.x0 + (i % cols) as f64 * cell;
            let y = list.y0 + (i / cols) as f64 * cell - scroll;
            let tile = Rect::new(x, y, x + cell, y + cell).inflate(-ui_px(3.0), -ui_px(3.0));
            if tile.y1 < list.y0 || tile.y0 > list.y1 {
                continue;
            }
            ctx.symbol_tiles
                .borrow_mut()
                .push((tile.intersect(list), def.id));
            let selected = ctx.selected_symbol == Some(def.id);
            scene.fill(
                Fill::NonZero,
                ID,
                if selected {
                    ctx.theme.accent.with_alpha(0.22)
                } else {
                    ctx.theme.bg
                },
                None,
                &tile,
            );
            thumbnail(scene, ctx, def.id, tile.inflate(-ui_px(6.0), -ui_px(6.0)));
            scene.stroke(
                &Stroke::new(ui_px(if selected { 2.0 } else { 1.0 })),
                ID,
                if selected {
                    ctx.theme.accent
                } else {
                    ctx.theme.border
                },
                None,
                &tile,
            );
            if let Some((super::RenameId::Symbol(id), buf)) = ctx.renaming {
                if id == def.id {
                    super::draw_name_field(
                        scene,
                        text,
                        ctx.theme,
                        tile.x0 + ui_px(5.0),
                        Rect::new(tile.x0, tile.y1 - metric_row_h(), tile.x1, tile.y1),
                        &def.name,
                        ctx.theme.text,
                        Some(buf),
                    );
                }
            }
            continue;
        }
        let ry = list.y0 + i as f64 * metric_row_h() - scroll;
        if ry + metric_row_h() < list.y0 || ry > list.y1 {
            continue;
        }
        let r = Rect::new(list.x0, ry, list.x1, ry + metric_row_h());
        if ctx.selected_symbol == Some(def.id) {
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.accent.with_alpha(0.22),
                None,
                &r,
            );
        }
        let glyph_rect = Rect::new(
            list.x0 + metric_pad(),
            r.center().y - ui_px(10.0),
            list.x0 + metric_pad() + ui_px(20.0),
            r.center().y + ui_px(10.0),
        );
        thumbnail(scene, ctx, def.id, glyph_rect);
        let editing = match ctx.renaming {
            Some((super::RenameId::Symbol(id), buf)) if id == def.id => Some(buf),
            _ => None,
        };
        super::draw_name_field(
            scene,
            text,
            ctx.theme,
            glyph_rect.x1 + ui_px(8.0),
            r,
            &def.name,
            ctx.theme.text,
            editing,
        );
    }
    for i in 1..if ctx.symbols_view == SymbolsView::List {
        symbols.len()
    } else {
        0
    } {
        let y = list.y0 + i as f64 * metric_row_h() - scroll;
        scene.stroke(
            &Stroke::new(ui_px(1.0)),
            ID,
            ctx.theme.border,
            None,
            &Line::new((list.x0, y), (list.x1, y)),
        );
    }
    scene.pop_layer();

    if content_h > list.height() + 0.5 {
        let frac = (list.height() / content_h).min(1.0);
        let th = (list.height() * frac).max(ui_px(24.0));
        let ty = list.y0 + (list.height() - th) * (scroll / (content_h - list.height()));
        scene.fill(
            Fill::NonZero,
            ID,
            ctx.theme.text_dim.with_alpha(0.5),
            None,
            &Rect::new(list.x1 - ui_px(4.0), ty, list.x1 - 1.0, ty + th)
                .to_rounded_rect(ui_px(1.5)),
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
    for (r, label, enabled) in [
        (place, "Place", has_selection),
        (edit, "Edit", has_selection),
        (delete, "Delete", has_selection),
    ] {
        button(
            scene,
            text,
            ctx.theme,
            r,
            label,
            enabled,
            r.contains(ctx.pointer),
        );
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
    text.draw(
        scene,
        label,
        11.0,
        ink,
        r.x0 + (r.width() - w) * 0.5,
        r.y0 + r.height() * 0.5 + ui_px(4.0),
    );
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    if local.y >= body.y1 - metric_footer_h() {
        let [place, edit, delete] = footer_buttons(body);
        let Some(id) = ctx.selected_symbol else {
            return Action::None;
        };
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
    let content_h = content_height(ctx.doc, body.width(), ctx.symbols_view) - metric_footer_h();
    let scroll = clamp_scroll(ctx.symbols_scroll, content_h, list.height());
    if !list.contains(local) {
        return Action::None;
    }
    if ctx.symbols_view == SymbolsView::Thumbnails {
        return ctx
            .symbol_tiles
            .borrow()
            .iter()
            .find(|(r, _)| r.contains(local))
            .map_or(Action::None, |(_, id)| Action::SelectSymbol(*id));
    }
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
    let mut entries = vec![
        MenuEntry::Item {
            id: "symbols-thumbnails",
            label: "Thumbnails",
            checked: ctx.symbols_view == SymbolsView::Thumbnails,
        },
        MenuEntry::Item {
            id: "symbols-list",
            label: "List",
            checked: ctx.symbols_view == SymbolsView::List,
        },
        MenuEntry::Item {
            id: "new-symbol",
            label: "New Symbol from Selection",
            checked: false,
        },
    ];
    if ctx.selected_symbol.is_some() {
        entries.push(MenuEntry::Item {
            id: "rename",
            label: "Rename",
            checked: false,
        });
    }
    entries
}

fn thumbnail(scene: &mut Scene, ctx: &Ctx, id: amalith_core::SymbolId, r: Rect) {
    if let Some(image) = ctx.symbol_thumbnails.get(&id) {
        let scale = (r.width() / image.width as f64).min(r.height() / image.height as f64);
        let xf = vello::kurbo::Affine::translate((
            r.center().x - image.width as f64 * scale * 0.5,
            r.center().y - image.height as f64 * scale * 0.5,
        )) * vello::kurbo::Affine::scale(scale);
        scene.draw_image(image, xf);
    }
}
