//! Appearance panel — Illustrator's Appearance panel: an object's own
//! ordered stack of fills and strokes ([`AppearanceItem`]), each
//! independently editable.
//!
//! The toolbar/Color-panel Fill & Stroke swatches and this panel are the
//! *same* underlying data (`Appearance::items`), not two disconnected
//! things the way Illustrator's own toolbar and Appearance panel are —
//! see `Appearance`'s own doc comment. Clicking a swatch here or in the
//! toolbar edits (or, from the toolbar, creates) the topmost matching
//! item; there is no separate "current color" hiding anywhere else.
//!
//! V1 scope: shows/edits the single selected object's stack. Nothing to
//! show when zero or more than one object is selected — editing a stack
//! is inherently single-object (`Command::SetAppearanceItems` targets
//! one id); editing several objects' differently-shaped stacks at once
//! is a V2 concern. No drag-to-reorder yet (footer buttons only), no
//! blend modes, no effects — see the Appearance-panel plan for the
//! reasoning behind each of those V1 cuts.

use crate::metrics::px as ui_px;

use amalith_core::AppearanceItem;
use vello::kurbo::{BezPath, Circle, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{draw_paint_swatch, metric_footer_h, metric_pad, metric_row_h, Action, Ctx, ID};

fn metric_swatch() -> f64 { crate::metrics::with(|m| m.panels_swatch) }

/// The stack, reversed for display — Illustrator (and this app's own
/// Layers panel) shows the last-painted item on top. `usize` is the
/// item's real index into `ctx.appearance_items` (paint order), which is
/// what every `Action` here addresses.
fn display_rows(items: &[AppearanceItem]) -> Vec<(usize, &AppearanceItem)> {
    items.iter().enumerate().rev().collect()
}

fn list_rect(body: Rect) -> Rect {
    Rect::new(body.x0, body.y0 + ui_px(4.0), body.x1, body.y1 - metric_footer_h())
}

fn row_rect(list: Rect, row: usize) -> Rect {
    let y = list.y0 + row as f64 * metric_row_h();
    Rect::new(list.x0, y, list.x1, y + metric_row_h())
}

struct RowLayout {
    eye: Rect,
    kind_glyph: Point,
    swatch: Rect,
    /// The "ƒx" affordance — opens the Offset Path dialog retargeted at
    /// this row (see `Action::OpenOffsetEffectDialog`), whether or not it
    /// already carries an effect.
    fx: Rect,
}

fn row_layout(r: Rect) -> RowLayout {
    let x = r.x0 + metric_pad();
    let eye = Rect::new(x, r.y0, x + ui_px(18.0), r.y1);
    let kind_glyph = Point::new(eye.x1 + ui_px(11.0), r.center().y);
    let swatch_x = eye.x1 + ui_px(24.0);
    let swatch = Rect::new(swatch_x, r.center().y - metric_swatch() * 0.4, swatch_x + metric_swatch() * 0.8, r.center().y + metric_swatch() * 0.4);
    let fx_w = ui_px(18.0);
    let fx = Rect::new(r.x1 - metric_pad() - fx_w, r.y0 + (r.height() - fx_w) * 0.5, r.x1 - metric_pad(), r.y0 + (r.height() + fx_w) * 0.5);
    RowLayout { eye, kind_glyph, swatch, fx }
}

/// The nested "Offset Path" row directly under an item that carries a
/// live effect — just a delete affordance; clicking anywhere else on the
/// row re-opens the dialog to edit it.
struct EffectRowLayout {
    trash: Rect,
}

fn effect_row_layout(r: Rect) -> EffectRowLayout {
    let w = ui_px(16.0);
    let trash = Rect::new(r.x1 - metric_pad() - w, r.y0 + (r.height() - w) * 0.5, r.x1 - metric_pad(), r.y0 + (r.height() + w) * 0.5);
    EffectRowLayout { trash }
}

/// One on-screen row: a stack item's own row, or — only for an item that
/// carries a live Offset Path effect — the slim indented row directly
/// beneath it showing that effect (Illustrator's own nested "fx" rows).
/// The `bool` is `true` for that nested effect row.
fn display_slots(items: &[AppearanceItem]) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for (idx, item) in display_rows(items) {
        out.push((idx, false));
        if item.offset().is_some() {
            out.push((idx, true));
        }
    }
    out
}

pub fn natural_height(ctx: &Ctx) -> f64 {
    let slots = display_slots(&ctx.appearance_items).len().max(1);
    ui_px(4.0) + slots as f64 * metric_row_h() + metric_footer_h()
}

/// Where dragging the selected row to `pointer` would insert it:
/// `(display_row, real_index)`. `display_row` (`0..=item_count`,
/// top-to-bottom on screen) drives the drop-line indicator;
/// `real_index` (`0..=item_count`, into the stored bottom-to-top
/// `items` Vec) is what a `Vec::insert` wants. The two are mirror
/// images of each other because the panel displays the stack reversed
/// (see `Appearance::items`'s doc comment): `real_index = item_count -
/// display_row`. A flat list — no "into a container" band the way
/// `layers::drop_target` needs, just a rounded fractional row.
///
/// Assumes one screen row per item, which is what `paint`/`hit_items`
/// show when nothing in the stack carries a live Offset Path effect. An
/// item with an active effect adds a nested row under it (see
/// `display_slots`), which this pointer math doesn't account for — a
/// disclosed v1 gap, same spirit as the SVG multi-item export fallback:
/// dragging while an effect row is visible can land one row off from
/// where the pointer actually is.
pub fn drop_target(body: Rect, pointer: Point, item_count: usize) -> Option<(usize, usize)> {
    if item_count == 0 {
        return None;
    }
    let list = list_rect(body);
    if pointer.x < list.x0 || pointer.x > list.x1 || pointer.y < list.y0 - metric_row_h() * 0.5
        || pointer.y > list.y1 + metric_row_h() * 0.5
    {
        return None;
    }
    let f = ((pointer.y - list.y0) / metric_row_h()).clamp(0.0, item_count as f64);
    let display_row = f.round() as usize;
    let real_index = item_count - display_row;
    Some((display_row, real_index))
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    let list = list_rect(body);

    if ctx.appearance_items.is_empty() {
        let msg = if ctx.selection.len() > 1 {
            "Select a single object"
        } else {
            "No selection"
        };
        text.draw(scene, msg, 12.0, th.text_dim, body.x0 + metric_pad(), list.y0 + ui_px(18.0));
    }

    scene.push_clip_layer(Fill::NonZero, ID, &list);
    for (row, (idx, is_effect)) in display_slots(&ctx.appearance_items).into_iter().enumerate() {
        let r = row_rect(list, row);
        if r.y1 < list.y0 || r.y0 > list.y1 {
            continue;
        }
        let item = &ctx.appearance_items[idx];
        if is_effect {
            let el = effect_row_layout(r);
            let x = r.x0 + metric_pad() + ui_px(28.0);
            draw_fx_glyph(scene, Point::new(x, r.center().y), th.text_dim);
            let amount = item.offset().map(|fx| fx.amount).unwrap_or(0.0);
            let label = format!("Offset Path  {amount:.1}px");
            text.draw(scene, &label, 11.5, th.text_dim, x + ui_px(14.0), r.center().y + ui_px(4.0));
            draw_trash_glyph(scene, el.trash.center(), th.text_dim);
            continue;
        }
        let l = row_layout(r);
        let selected = ctx.appearance_selected == Some(idx);
        if selected {
            scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.18), None, &r);
        }

        let visible = item.visible();
        let ink = if visible { th.text_dim } else { th.border };
        draw_eye(scene, l.eye.center().x, l.eye.center().y, visible, ink);
        let fx_ink = if item.offset().is_some() { th.accent } else { th.border };
        draw_fx_glyph(scene, l.fx.center(), fx_ink);

        match item {
            AppearanceItem::Fill { paint, opacity, .. } => {
                draw_fill_glyph(scene, l.kind_glyph, ink);
                draw_paint_swatch(scene, text, th, l.swatch, *paint, selected, false);
                let label_x = l.swatch.x1 + ui_px(10.0);
                let name_ink = if visible { th.text } else { th.border };
                text.draw(scene, "Fill", 12.0, name_ink, label_x, r.center().y + ui_px(4.0));
                if *opacity != 1.0 {
                    let pct = format!("{:.0}%", (opacity * 100.0).round());
                    let w = text.measure(&pct, 11.0);
                    text.draw(scene, &pct, 11.0, th.text_dim, l.fx.x0 - ui_px(6.0) - w, r.center().y + ui_px(4.0));
                }
            }
            AppearanceItem::Stroke { paint, width, opacity, .. } => {
                draw_stroke_glyph(scene, l.kind_glyph, ink);
                draw_paint_swatch(scene, text, th, l.swatch, *paint, selected, false);
                let label_x = l.swatch.x1 + ui_px(10.0);
                let name_ink = if visible { th.text } else { th.border };
                let label = format!("Stroke  {width:.1}pt");
                text.draw(scene, &label, 12.0, name_ink, label_x, r.center().y + ui_px(4.0));
                if *opacity != 1.0 {
                    let pct = format!("{:.0}%", (opacity * 100.0).round());
                    let w = text.measure(&pct, 11.0);
                    text.draw(scene, &pct, 11.0, th.text_dim, l.fx.x0 - ui_px(6.0) - w, r.center().y + ui_px(4.0));
                }
            }
        }
    }
    if let Some(drop_row) = ctx.appearance_drop {
        let y = list.y0 + drop_row as f64 * metric_row_h();
        scene.fill(
            Fill::NonZero,
            ID,
            th.accent,
            None,
            &Rect::new(list.x0 + ui_px(3.0), y - 1.0, list.x1 - ui_px(3.0), y + 1.0),
        );
    }
    scene.pop_layer();

    paint_footer(scene, text, th, body, ctx);
}

fn draw_eye(scene: &mut Scene, cx: f64, cy: f64, open: bool, ink: Color) {
    let s = Stroke::new(ui_px(1.2));
    if open {
        let mut lid = BezPath::new();
        lid.move_to((cx - ui_px(6.0), cy));
        lid.quad_to((cx, cy - ui_px(5.0)), (cx + ui_px(6.0), cy));
        lid.quad_to((cx, cy + ui_px(5.0)), (cx - ui_px(6.0), cy));
        lid.close_path();
        scene.stroke(&s, ID, ink, None, &lid);
        scene.fill(Fill::NonZero, ID, ink, None, &Circle::new((cx, cy), ui_px(1.6)));
    } else {
        scene.stroke(&s, ID, ink, None, &Line::new((cx - ui_px(6.0), cy), (cx + ui_px(6.0), cy)));
    }
}

/// A filled square — Illustrator's own Fill-item glyph.
fn draw_fill_glyph(scene: &mut Scene, c: Point, ink: Color) {
    let r = ui_px(5.0);
    scene.fill(Fill::NonZero, ID, ink, None, &Rect::from_center_size(c, (r * 2.0, r * 2.0)));
}

/// An open ring — Illustrator's own Stroke-item glyph.
fn draw_stroke_glyph(scene: &mut Scene, c: Point, ink: Color) {
    scene.stroke(&Stroke::new(ui_px(1.6)), ID, ink, None, &Circle::new(c, ui_px(5.0)));
}

/// A small "ƒx" mark — Illustrator's own live-effect glyph, used both for
/// the item row's add/edit affordance and the nested effect row's icon.
fn draw_fx_glyph(scene: &mut Scene, c: Point, ink: Color) {
    let s = Stroke::new(ui_px(1.1));
    let mut f = BezPath::new();
    f.move_to((c.x - ui_px(4.5), c.y + ui_px(5.0)));
    f.line_to((c.x - ui_px(1.5), c.y - ui_px(5.0)));
    scene.stroke(&s, ID, ink, None, &f);
    scene.stroke(&s, ID, ink, None, &Line::new((c.x - ui_px(6.0), c.y), (c.x - ui_px(2.5), c.y)));
    let mut x = BezPath::new();
    x.move_to((c.x + ui_px(1.5), c.y - ui_px(4.0)));
    x.line_to((c.x + ui_px(6.5), c.y + ui_px(4.0)));
    scene.stroke(&s, ID, ink, None, &x);
    let mut x2 = BezPath::new();
    x2.move_to((c.x + ui_px(6.5), c.y - ui_px(4.0)));
    x2.line_to((c.x + ui_px(1.5), c.y + ui_px(4.0)));
    scene.stroke(&s, ID, ink, None, &x2);
}

/// A small trash-can mark — the nested effect row's delete affordance.
fn draw_trash_glyph(scene: &mut Scene, c: Point, ink: Color) {
    let s = Stroke::new(ui_px(1.0));
    scene.stroke(&s, ID, ink, None, &Line::new((c.x - ui_px(4.5), c.y - ui_px(4.0)), (c.x + ui_px(4.5), c.y - ui_px(4.0))));
    let can = Rect::new(c.x - ui_px(3.2), c.y - ui_px(3.2), c.x + ui_px(3.2), c.y + ui_px(4.5));
    scene.stroke(&s, ID, ink, None, &can);
}

struct Footer {
    add_fill: Rect,
    add_stroke: Rect,
    duplicate: Rect,
    delete: Rect,
}

fn footer_layout(body: Rect) -> Footer {
    let strip = Rect::new(body.x0, body.y1 - metric_footer_h(), body.x1, body.y1);
    let w = ui_px(58.0);
    let gap = ui_px(6.0);
    let y0 = strip.y0 + (strip.height() - ui_px(20.0)) * 0.5;
    let y1 = y0 + ui_px(20.0);
    let x0 = strip.x0 + metric_pad();
    Footer {
        add_fill: Rect::new(x0, y0, x0 + w, y1),
        add_stroke: Rect::new(x0 + w + gap, y0, x0 + 2.0 * w + gap, y1),
        duplicate: Rect::new(strip.x1 - metric_pad() - 2.0 * ui_px(22.0) - gap, y0, strip.x1 - metric_pad() - ui_px(22.0) - gap, y1),
        delete: Rect::new(strip.x1 - metric_pad() - ui_px(22.0), y0, strip.x1 - metric_pad(), y1),
    }
}

fn paint_footer(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, body: Rect, ctx: &Ctx) {
    let f = footer_layout(body);
    scene.fill(Fill::NonZero, ID, th.splitter, None, &Rect::new(body.x0, f.add_fill.y0 - ui_px(6.0), body.x1, f.add_fill.y0 - ui_px(6.0) + 1.0));

    let has_target = !ctx.appearance_items.is_empty() || ctx.selection.len() == 1;
    let has_selected = ctx.appearance_selected.is_some();
    text_button(scene, text, th, f.add_fill, "+ Fill", has_target, ctx.pointer);
    text_button(scene, text, th, f.add_stroke, "+ Stroke", has_target, ctx.pointer);
    icon_button(scene, th, f.duplicate, IconKind::Duplicate, has_selected, ctx.pointer);
    icon_button(scene, th, f.delete, IconKind::Trash, has_selected, ctx.pointer);
}

fn text_button(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, r: Rect, label: &str, enabled: bool, pointer: Point) {
    let hot = enabled && r.contains(pointer);
    let bg = if hot { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if enabled { th.text } else { th.border };
    let w = text.measure(label, 11.0);
    text.draw(scene, label, 11.0, ink, r.center().x - w * 0.5, r.center().y + ui_px(3.5));
}

enum IconKind {
    Duplicate,
    Trash,
}

fn icon_button(scene: &mut Scene, th: &crate::theme::Theme, r: Rect, kind: IconKind, enabled: bool, pointer: Point) {
    let hot = enabled && r.contains(pointer);
    let bg = if hot { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if enabled { th.text_dim } else { th.border };
    let c = r.center();
    match kind {
        IconKind::Duplicate => {
            let s = Stroke::new(ui_px(1.1));
            let back = Rect::from_center_size((c.x - ui_px(1.5), c.y - ui_px(1.5)), (ui_px(9.0), ui_px(9.0)));
            let front = Rect::from_center_size((c.x + ui_px(1.5), c.y + ui_px(1.5)), (ui_px(9.0), ui_px(9.0)));
            scene.stroke(&s, ID, ink, None, &back);
            scene.fill(Fill::NonZero, ID, bg, None, &front);
            scene.stroke(&s, ID, ink, None, &front);
        }
        IconKind::Trash => {
            let s = Stroke::new(ui_px(1.1));
            let lid = Line::new((c.x - ui_px(5.5), c.y - ui_px(4.5)), (c.x + ui_px(5.5), c.y - ui_px(4.5)));
            scene.stroke(&s, ID, ink, None, &lid);
            let can = Rect::new(c.x - ui_px(4.0), c.y - ui_px(4.0), c.x + ui_px(4.0), c.y + ui_px(5.5));
            scene.stroke(&s, ID, ink, None, &can);
            for dx in [-ui_px(1.8), 0.0, ui_px(1.8)] {
                scene.stroke(&s, ID, ink, None, &Line::new((c.x + dx, c.y - ui_px(2.5)), (c.x + dx, c.y + ui_px(3.5))));
            }
        }
    }
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    hit_items(&ctx.appearance_items, ctx.appearance_selected, body, p)
}

/// The real hit-test logic, independent of `Ctx` so it's unit-testable
/// without constructing one (mirrors how `paragraph.rs`'s alignment-tick
/// math is split out from its own `Ctx`-taking `paint`/`hit`).
fn hit_items(items: &[AppearanceItem], selected: Option<usize>, body: Rect, p: Point) -> Action {
    let f = footer_layout(body);
    if f.add_fill.contains(p) {
        return Action::AppearanceAddFill;
    }
    if f.add_stroke.contains(p) {
        return Action::AppearanceAddStroke;
    }
    if f.duplicate.contains(p) && selected.is_some() {
        return Action::AppearanceDuplicate;
    }
    if f.delete.contains(p) && selected.is_some() {
        return Action::AppearanceDelete;
    }
    let list = list_rect(body);
    if !list.contains(p) {
        return Action::None;
    }
    let slots = display_slots(items);
    let row = ((p.y - list.y0) / metric_row_h()).floor() as usize;
    let Some(&(idx, is_effect)) = slots.get(row) else {
        return Action::None;
    };
    let r = row_rect(list, row);
    if is_effect {
        let el = effect_row_layout(r);
        if el.trash.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
            return Action::AppearanceRemoveOffset(idx);
        }
        return Action::OpenOffsetEffectDialog(idx);
    }
    let l = row_layout(r);
    if l.eye.contains(p) {
        return Action::AppearanceToggleVisible(idx);
    }
    if l.fx.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
        return Action::OpenOffsetEffectDialog(idx);
    }
    if l.swatch.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
        return Action::OpenAppearanceItemPicker(idx);
    }
    Action::AppearanceSelect(idx)
}

pub fn tip(body: Rect, p: Point, ctx: &Ctx) -> Option<&'static str> {
    let f = footer_layout(body);
    if f.add_fill.contains(p) {
        return Some("Add New Fill");
    }
    if f.add_stroke.contains(p) {
        return Some("Add New Stroke");
    }
    if f.duplicate.contains(p) {
        return Some("Duplicate Item");
    }
    if f.delete.contains(p) {
        return Some("Delete Item");
    }
    let list = list_rect(body);
    if !list.contains(p) {
        return None;
    }
    let slots = display_slots(&ctx.appearance_items);
    let row = ((p.y - list.y0) / metric_row_h()).floor() as usize;
    let &(idx, is_effect) = slots.get(row)?;
    if is_effect {
        return Some("Offset Path — click to edit, trash to remove");
    }
    let r = row_rect(list, row);
    let l = row_layout(r);
    if l.fx.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
        return Some(if ctx.appearance_items[idx].offset().is_some() {
            "Edit Offset Path"
        } else {
            "Add Offset Path"
        });
    }
    Some(match &ctx.appearance_items[idx] {
        AppearanceItem::Fill { .. } => "Fill",
        AppearanceItem::Stroke { .. } => "Stroke",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::Paint;

    fn body() -> Rect {
        Rect::new(0.0, 0.0, 240.0, 300.0)
    }

    fn two_items() -> Vec<AppearanceItem> {
        vec![
            AppearanceItem::Fill { paint: Paint::Solid(amalith_core::Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, offset: None },
            AppearanceItem::Stroke {
                paint: Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 1.0)),
                width: 2.0,
                style: amalith_core::StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                offset: None,
            },
        ]
    }

    #[test]
    fn display_order_puts_the_last_painted_item_on_top() {
        let items = two_items();
        let rows = display_rows(&items);
        // Stored bottom-to-top (Fill=0 painted first, Stroke=1 painted
        // last, on top) — display reverses that, so row 0 (topmost on
        // screen) is the Stroke, index 1.
        assert_eq!(rows[0].0, 1);
        assert!(matches!(rows[0].1, AppearanceItem::Stroke { .. }));
        assert_eq!(rows[1].0, 0);
        assert!(matches!(rows[1].1, AppearanceItem::Fill { .. }));
    }

    #[test]
    fn clicking_a_row_selects_its_real_stack_index_not_its_display_row() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        let top_row_center = row_rect(list, 0).center();
        // The topmost displayed row is the Stroke (real index 1).
        assert_eq!(hit_items(&items, None, b, top_row_center), Action::AppearanceSelect(1));
        let second_row_center = row_rect(list, 1).center();
        assert_eq!(hit_items(&items, None, b, second_row_center), Action::AppearanceSelect(0));
    }

    #[test]
    fn clicking_the_eye_toggles_that_rows_own_item() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        let r = row_rect(list, 0);
        let eye = row_layout(r).eye.center();
        assert_eq!(hit_items(&items, None, b, eye), Action::AppearanceToggleVisible(1));
    }

    #[test]
    fn clicking_the_swatch_opens_that_items_picker() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        let r = row_rect(list, 1);
        let swatch = row_layout(r).swatch.center();
        assert_eq!(hit_items(&items, None, b, swatch), Action::OpenAppearanceItemPicker(0));
    }

    #[test]
    fn duplicate_and_delete_are_inert_without_a_selected_row() {
        let items = two_items();
        let b = body();
        let f = footer_layout(b);
        assert_eq!(hit_items(&items, None, b, f.duplicate.center()), Action::None);
        assert_eq!(hit_items(&items, None, b, f.delete.center()), Action::None);
        assert_eq!(hit_items(&items, Some(0), b, f.duplicate.center()), Action::AppearanceDuplicate);
        assert_eq!(hit_items(&items, Some(0), b, f.delete.center()), Action::AppearanceDelete);
    }

    #[test]
    fn drop_target_maps_screen_rows_to_stack_indices_in_reverse() {
        let b = body();
        let list = list_rect(b);
        // Dropping above the very first (topmost, display row 0) row
        // means "insert on top of everything" — the real end of the
        // bottom-to-top stack.
        let (row, real) = drop_target(b, Point::new(list.center().x, list.y0), 3).unwrap();
        assert_eq!(row, 0);
        assert_eq!(real, 3, "display row 0 inserts at the real top of a 3-item stack");
        // Dropping below the last display row means "insert underneath
        // everything" — real index 0.
        let (row, real) = drop_target(b, Point::new(list.center().x, list.y0 + 3.0 * metric_row_h()), 3).unwrap();
        assert_eq!(row, 3);
        assert_eq!(real, 0);
    }

    #[test]
    fn drop_target_is_none_for_an_empty_stack_or_far_outside_the_list() {
        let b = body();
        assert_eq!(drop_target(b, b.center(), 0), None, "nothing to reorder in an empty stack");
        assert_eq!(drop_target(b, Point::new(b.center().x, b.y1 + 500.0), 3), None);
    }

    #[test]
    fn add_fill_and_add_stroke_are_always_live() {
        let b = body();
        let f = footer_layout(b);
        assert_eq!(hit_items(&[], None, b, f.add_fill.center()), Action::AppearanceAddFill);
        assert_eq!(hit_items(&[], None, b, f.add_stroke.center()), Action::AppearanceAddStroke);
    }

    fn item_with_offset() -> Vec<AppearanceItem> {
        vec![
            AppearanceItem::Fill { paint: Paint::Solid(amalith_core::Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, offset: None },
            AppearanceItem::Stroke {
                paint: Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 1.0)),
                width: 2.0,
                style: amalith_core::StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                offset: Some(amalith_core::OffsetEffect { amount: -2.0, join: amalith_core::LineJoin::Miter, miter_limit: 4.0 }),
            },
        ]
    }

    #[test]
    fn display_slots_inserts_a_nested_effect_row_right_under_its_item() {
        let items = item_with_offset();
        let slots = display_slots(&items);
        // Display order is reversed (Stroke=1 on top); it carries the
        // effect, so its nested row (idx 1, is_effect) follows directly,
        // then the plain Fill row (idx 0) with no nested row of its own.
        assert_eq!(slots, vec![(1, false), (1, true), (0, false)]);
    }

    #[test]
    fn display_slots_grows_by_one_row_per_active_effect() {
        let plain = two_items();
        let with_fx = item_with_offset();
        // `natural_height` is `display_slots(...).len()` rows plus the
        // fixed chrome — checking the slot count directly avoids having
        // to build a full `Ctx` (theme/pointer/etc.) just for this.
        assert_eq!(display_slots(&with_fx).len(), display_slots(&plain).len() + 1);
    }

    #[test]
    fn clicking_a_row_without_an_effect_opens_the_dialog_to_add_one() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        // Topmost displayed row (real index 1, the Stroke) has no effect.
        let l = row_layout(row_rect(list, 0));
        assert_eq!(hit_items(&items, None, b, l.fx.center()), Action::OpenOffsetEffectDialog(1));
    }

    #[test]
    fn clicking_the_nested_rows_trash_removes_its_effect_and_elsewhere_edits_it() {
        let items = item_with_offset();
        let b = body();
        let list = list_rect(b);
        // Slot 0 = the Stroke's own row (has the effect), slot 1 = its
        // nested effect row.
        let effect_row = row_rect(list, 1);
        let el = effect_row_layout(effect_row);
        assert_eq!(hit_items(&items, None, b, el.trash.center()), Action::AppearanceRemoveOffset(1));
        assert_eq!(hit_items(&items, None, b, effect_row.center()), Action::OpenOffsetEffectDialog(1));
    }
}
