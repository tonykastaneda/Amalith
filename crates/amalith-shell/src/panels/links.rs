//! Links panel: one row per document asset (Linked or Embedded), with a
//! status badge and a Relink / Go to Link / Update Link footer — the
//! Illustrator-style Links/Embedded-image manager (no CC Libraries; see
//! `crate::canvas::link_status` for the Ok/Modified/Missing classifier).

use amalith_core::Document;
use vello::kurbo::{Circle, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::canvas::{link_status, LinkStatus};
use crate::text::TextContext;
use crate::theme::Theme;

use super::{Action, Ctx, MenuEntry, FOOTER_H, ID, PAD, ROW_H};

/// The scrollable row list (between the top and the footer — no search
/// strip, unlike Layers).
fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0, body.x1, body.y1 - FOOTER_H)
}

fn clamp_scroll(raw: f64, n_rows: usize, list_h: f64) -> f64 {
    let max = (n_rows as f64 * ROW_H - list_h).max(0.0);
    raw.clamp(0.0, max)
}

/// Full height the Links panel wants: every row + footer.
pub(super) fn content_height(doc: &Document) -> f64 {
    doc.assets().len() as f64 * ROW_H + FOOTER_H
}

fn status_color(status: LinkStatus, theme: &Theme) -> Color {
    match status {
        LinkStatus::Ok => Color::from_rgb8(0x3f, 0xb9, 0x50),
        LinkStatus::Modified => Color::from_rgb8(0xe0, 0x9a, 0x1e),
        LinkStatus::Missing => Color::from_rgb8(0xe0, 0x40, 0x40),
        LinkStatus::Embedded => theme.text_dim,
    }
}

fn status_label(status: LinkStatus) -> &'static str {
    match status {
        LinkStatus::Ok => "Linked",
        LinkStatus::Modified => "Modified",
        LinkStatus::Missing => "Missing",
        LinkStatus::Embedded => "Embedded",
    }
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let list = list_rect(body);
    let assets = ctx.doc.assets();
    let scroll = clamp_scroll(ctx.links_scroll, assets.len(), list.height());

    scene.push_clip_layer(Fill::NonZero, ID, &list);
    if assets.is_empty() {
        text.draw(
            scene,
            "No linked or embedded images",
            12.0,
            ctx.theme.text_dim,
            body.x0 + PAD,
            list.y0 + ROW_H * 0.5 + 4.0,
        );
    }
    for (i, asset) in assets.iter().enumerate() {
        let ry = list.y0 + i as f64 * ROW_H - scroll;
        if ry + ROW_H < list.y0 || ry > list.y1 {
            continue;
        }
        let r = Rect::new(list.x0, ry, list.x1, ry + ROW_H);
        if ctx.selected_asset == Some(asset.id) {
            scene.fill(Fill::NonZero, ID, ctx.theme.accent.with_alpha(0.22), None, &r);
        }
        let status = link_status(&asset.source);
        let baseline = r.y0 + ROW_H * 0.5 + 4.0;
        scene.fill(
            Fill::NonZero,
            ID,
            status_color(status, ctx.theme),
            None,
            &Circle::new((list.x0 + PAD + 4.0, r.center().y), 3.5),
        );
        text.draw(scene, &asset.name, 12.0, ctx.theme.text, list.x0 + PAD + 16.0, baseline);
        let label = status_label(status);
        let w = text.measure(label, 11.0);
        text.draw(scene, label, 11.0, ctx.theme.text_dim, r.x1 - PAD - w, baseline);
    }
    for i in 1..assets.len() {
        let y = list.y0 + i as f64 * ROW_H - scroll;
        scene.stroke(&Stroke::new(1.0), ID, ctx.theme.border, None, &Line::new((list.x0, y), (list.x1, y)));
    }
    scene.pop_layer();

    let content_h = assets.len() as f64 * ROW_H;
    if content_h > list.height() + 0.5 {
        let frac = (list.height() / content_h).min(1.0);
        let th = (list.height() * frac).max(24.0);
        let ty = list.y0 + (list.height() - th) * (scroll / (content_h - list.height()));
        scene.fill(
            Fill::NonZero,
            ID,
            ctx.theme.text_dim.with_alpha(0.5),
            None,
            &Rect::new(list.x1 - 4.0, ty, list.x1 - 1.0, ty + th).to_rounded_rect(1.5),
        );
    }

    paint_footer(scene, text, ctx, body);
}

/// (Relink, Go to Link, Update Link) rects, right-aligned along the footer.
fn footer_buttons(body: Rect) -> [Rect; 3] {
    let w = 88.0;
    let gap = 8.0;
    let cy = body.y1 - FOOTER_H * 0.5;
    let mut x1 = body.x1 - PAD;
    let mut rects = [Rect::ZERO; 3];
    for k in (0..3).rev() {
        let r = Rect::new(x1 - w, cy - 11.0, x1, cy + 11.0);
        rects[k] = r;
        x1 = r.x0 - gap;
    }
    rects
}

fn paint_footer(scene: &mut Scene, text: &mut TextContext, ctx: &Ctx, body: Rect) {
    let strip = Rect::new(body.x0, body.y1 - FOOTER_H, body.x1, body.y1);
    scene.fill(Fill::NonZero, ID, ctx.theme.strip_bg, None, &strip);
    scene.fill(
        Fill::NonZero,
        ID,
        ctx.theme.border,
        None,
        &Rect::new(strip.x0, strip.y0, strip.x1, strip.y0 + 1.0),
    );

    let asset = ctx.selected_asset.and_then(|id| ctx.doc.asset(id));
    let is_linked = asset.is_some_and(|a| a.is_linked());
    let has_any = asset.is_some();
    let [relink, goto, update] = footer_buttons(body);
    for (r, label, enabled) in [
        (relink, "Relink", is_linked),
        (goto, "Go to Link", has_any),
        (update, "Update Link", is_linked),
    ] {
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
        &Stroke::new(1.0),
        ID,
        ink.with_alpha(if enabled { 0.6 } else { 0.4 }),
        None,
        &r.to_rounded_rect(4.0),
    );
    let w = text.measure(label, 11.0);
    text.draw(scene, label, 11.0, ink, r.x0 + (r.width() - w) * 0.5, r.y0 + r.height() * 0.5 + 4.0);
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    if local.y >= body.y1 - FOOTER_H {
        let [relink, goto, update] = footer_buttons(body);
        let Some(id) = ctx.selected_asset else { return Action::None };
        let is_linked = ctx.doc.asset(id).is_some_and(|a| a.is_linked());
        if relink.contains(local) && is_linked {
            return Action::RelinkAsset(id);
        }
        if goto.contains(local) {
            return Action::GoToLinkAsset(id);
        }
        if update.contains(local) && is_linked {
            return Action::UpdateLinkAsset(id);
        }
        return Action::None;
    }
    let list = list_rect(body);
    let assets = ctx.doc.assets();
    let scroll = clamp_scroll(ctx.links_scroll, assets.len(), list.height());
    let i = ((local.y - list.y0 + scroll) / ROW_H).floor();
    if i < 0.0 {
        return Action::None;
    }
    match assets.get(i as usize) {
        Some(a) => Action::SelectAsset(a.id),
        None => Action::None,
    }
}

/// Hamburger flyout: Embed / Unembed, whichever applies to the selected row.
pub(super) fn menu(ctx: &Ctx) -> Vec<MenuEntry> {
    let Some(asset) = ctx.selected_asset.and_then(|id| ctx.doc.asset(id)) else {
        return Vec::new();
    };
    if asset.is_linked() {
        vec![MenuEntry::Item { id: "embed", label: "Embed Image", checked: false }]
    } else {
        vec![MenuEntry::Item { id: "unembed", label: "Unembed Image", checked: false }]
    }
}
