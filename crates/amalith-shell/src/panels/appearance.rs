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

use super::{draw_eye, draw_paint_swatch, metric_footer_h, metric_pad, metric_row_h, Action, Ctx, EffectMenuChoice, ID};

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
    /// A Stroke row's weight field — click to type a new value, or
    /// scroll to nudge it. Computed at a fixed offset regardless of the
    /// "Fill"/"Stroke" label's own text width (this is a pure-geometry
    /// layout function with no `TextContext` to measure with) — a Fill
    /// row never reads this, so it's harmless that one exists for it too.
    weight: Rect,
}

fn row_layout(r: Rect) -> RowLayout {
    let x = r.x0 + metric_pad();
    let eye = Rect::new(x, r.y0, x + ui_px(18.0), r.y1);
    let kind_glyph = Point::new(eye.x1 + ui_px(11.0), r.center().y);
    let swatch_x = eye.x1 + ui_px(24.0);
    let swatch = Rect::new(swatch_x, r.center().y - metric_swatch() * 0.4, swatch_x + metric_swatch() * 0.8, r.center().y + metric_swatch() * 0.4);
    let weight_x = swatch.x1 + ui_px(58.0);
    let weight = Rect::new(weight_x, r.y0 + ui_px(4.0), weight_x + ui_px(46.0), r.y1 - ui_px(4.0));
    RowLayout { eye, kind_glyph, swatch, weight }
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

/// One on-screen row: a stack item's own row (`Slot::Item`), or one entry
/// in that item's own live-effect stack (`Slot::Effect(item_idx,
/// effect_idx)`) — Illustrator's own nested "fx" rows, one per effect in
/// `item.effects()`, in stack order. An item can carry any number of
/// these (stacking the same effect twice is allowed, matching real
/// Illustrator), not just zero-or-one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Slot {
    Item(usize),
    Effect(usize, usize),
}

fn display_slots(items: &[AppearanceItem]) -> Vec<Slot> {
    let mut out = Vec::new();
    for (idx, item) in display_rows(items) {
        out.push(Slot::Item(idx));
        for effect_idx in 0..item.effects().len() {
            out.push(Slot::Effect(idx, effect_idx));
        }
    }
    out
}

/// The nested effect row's label — dispatches on effect kind. A future
/// effect kind just adds a match arm here, nothing else in this file
/// changes.
fn effect_label(e: &amalith_core::Effect) -> String {
    use amalith_core::Effect;
    match e {
        Effect::Offset(fx) => format!("Offset Path  {:.1}px", fx.amount),
        Effect::ZigZag(fx) => format!("Zig Zag  {:.0}px", fx.size),
        Effect::PuckerBloat(fx) => format!("{}  {:.0}%", if fx.amount < 0.0 { "Pucker" } else { "Bloat" }, fx.amount.abs()),
        Effect::Roughen(fx) => format!("Roughen  {:.0}px", fx.size),
        Effect::Transform(_) => "Transform".to_string(),
        Effect::Tweak(fx) => format!("Tweak  {:.0}% / {:.0}%", fx.horizontal, fx.vertical),
        Effect::Twist(fx) => format!("Twist  {:.0}°", fx.angle),
    }
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
    for (row, slot) in display_slots(&ctx.appearance_items).into_iter().enumerate() {
        let r = row_rect(list, row);
        if r.y1 < list.y0 || r.y0 > list.y1 {
            continue;
        }
        let idx = match slot {
            Slot::Effect(idx, effect_idx) => {
                let item = &ctx.appearance_items[idx];
                let el = effect_row_layout(r);
                let x = r.x0 + metric_pad() + ui_px(28.0);
                draw_fx_glyph(scene, Point::new(x, r.center().y), th.text_dim);
                let label = effect_label(&item.effects()[effect_idx]);
                text.draw(scene, &label, 11.5, th.text_dim, x + ui_px(14.0), r.center().y + ui_px(4.0));
                draw_trash_glyph(scene, el.trash.center(), th.text_dim);
                continue;
            }
            Slot::Item(idx) => idx,
        };
        let item = &ctx.appearance_items[idx];
        let l = row_layout(r);
        let selected = ctx.appearance_selected == Some(idx);
        if selected {
            scene.fill(Fill::NonZero, ID, th.accent.with_alpha(0.18), None, &r);
        }

        let visible = item.visible();
        let ink = if visible { th.text_dim } else { th.border };
        draw_eye(scene, l.eye.center().x, l.eye.center().y, visible, ink);

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
                    text.draw(scene, &pct, 11.0, th.text_dim, r.x1 - metric_pad() - w, r.center().y + ui_px(4.0));
                }
            }
            AppearanceItem::Stroke { paint, width, opacity, .. } => {
                draw_stroke_glyph(scene, l.kind_glyph, ink);
                draw_paint_swatch(scene, text, th, l.swatch, *paint, selected, false);
                let label_x = l.swatch.x1 + ui_px(10.0);
                let name_ink = if visible { th.text } else { th.border };
                text.draw(scene, "Stroke", 12.0, name_ink, label_x, r.center().y + ui_px(4.0));

                let editing = ctx.appearance_width_edit.and_then(|(i, buf)| (i == idx).then_some(buf));
                let shown = match editing {
                    Some(buf) => buf.to_string(),
                    None => format!("{width:.1}pt"),
                };
                scene.fill(Fill::NonZero, ID, th.bg, None, &l.weight);
                scene.stroke(
                    &Stroke::new(if editing.is_some() { ui_px(1.3) } else { ui_px(1.0) }),
                    ID,
                    if editing.is_some() { th.accent } else { th.text_dim.with_alpha(0.4) },
                    None,
                    &l.weight,
                );
                let shown_disp = if editing.is_some() { format!("{shown}|") } else { shown };
                let w = text.measure(&shown_disp, 11.0);
                text.draw(scene, &shown_disp, 11.0, name_ink, l.weight.center().x - w * 0.5, l.weight.center().y + ui_px(3.5));

                if *opacity != 1.0 {
                    let pct = format!("{:.0}%", (opacity * 100.0).round());
                    let w = text.measure(&pct, 11.0);
                    text.draw(scene, &pct, 11.0, th.text_dim, r.x1 - metric_pad() - w, r.center().y + ui_px(4.0));
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
    if ctx.appearance_fx_menu {
        paint_fx_menu(scene, text, th, body);
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

/// Left-to-right, matching Illustrator's own footer: New Fill, New
/// Stroke, fx (the *one* entry point for adding a live effect to the
/// selected row — no per-row icon, and no per-effect-type icon either;
/// more effect types just add more rows to the menu `fx` opens), then a
/// gap, Clear Effects, Duplicate, Delete.
struct Footer {
    add_fill: Rect,
    add_stroke: Rect,
    fx: Rect,
    clear_effects: Rect,
    duplicate: Rect,
    delete: Rect,
}

fn footer_layout(body: Rect) -> Footer {
    let strip = Rect::new(body.x0, body.y1 - metric_footer_h(), body.x1, body.y1);
    let w = ui_px(50.0);
    let fx_w = ui_px(30.0);
    let gap = ui_px(6.0);
    let icon_w = ui_px(22.0);
    let y0 = strip.y0 + (strip.height() - ui_px(20.0)) * 0.5;
    let y1 = y0 + ui_px(20.0);
    let x0 = strip.x0 + metric_pad();
    let add_fill = Rect::new(x0, y0, x0 + w, y1);
    let add_stroke = Rect::new(add_fill.x1 + gap, y0, add_fill.x1 + gap + w, y1);
    let fx = Rect::new(add_stroke.x1 + gap, y0, add_stroke.x1 + gap + fx_w, y1);
    let delete = Rect::new(strip.x1 - metric_pad() - icon_w, y0, strip.x1 - metric_pad(), y1);
    let duplicate = Rect::new(delete.x0 - gap - icon_w, y0, delete.x0 - gap, y1);
    let clear_effects = Rect::new(duplicate.x0 - gap - icon_w, y0, duplicate.x0 - gap, y1);
    Footer { add_fill, add_stroke, fx, clear_effects, duplicate, delete }
}

fn paint_footer(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, body: Rect, ctx: &Ctx) {
    let f = footer_layout(body);
    scene.fill(Fill::NonZero, ID, th.splitter, None, &Rect::new(body.x0, f.add_fill.y0 - ui_px(6.0), body.x1, f.add_fill.y0 - ui_px(6.0) + 1.0));

    let has_target = !ctx.appearance_items.is_empty() || ctx.selection.len() == 1;
    let has_selected = ctx.appearance_selected.is_some();
    let has_any_effect = ctx.appearance_items.iter().any(|i| !i.effects().is_empty());
    text_button(scene, text, th, f.add_fill, "+ Fill", has_target, ctx.pointer);
    text_button(scene, text, th, f.add_stroke, "+ Stroke", has_target, ctx.pointer);
    fx_button(scene, text, th, f.fx, has_selected, ctx.appearance_fx_menu, ctx.pointer);
    icon_button(scene, th, f.clear_effects, IconKind::ClearEffects, has_any_effect, ctx.pointer);
    icon_button(scene, th, f.duplicate, IconKind::Duplicate, has_selected, ctx.pointer);
    icon_button(scene, th, f.delete, IconKind::Trash, has_selected, ctx.pointer);
}

/// The footer's "fx ▾" button — an italic "fx" mark plus a small
/// dropdown caret, matching Illustrator's own Appearance-panel fx icon.
fn fx_button(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, r: Rect, enabled: bool, open: bool, pointer: Point) {
    let hot = enabled && (r.contains(pointer) || open);
    let bg = if hot { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, if open { th.accent } else { th.text_dim.with_alpha(0.5) }, None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if enabled { th.text } else { th.text_dim.with_alpha(0.4) };
    let label = "fx";
    let w = text.measure(label, 11.5);
    text.draw(scene, label, 11.5, ink, r.center().x - w * 0.5 - ui_px(3.0), r.center().y + ui_px(4.0));
    let cx = r.x1 - ui_px(8.0);
    let cy = r.center().y;
    let mut caret = BezPath::new();
    caret.move_to((cx - ui_px(3.0), cy - ui_px(1.5)));
    caret.line_to((cx, cy + ui_px(2.0)));
    caret.line_to((cx + ui_px(3.0), cy - ui_px(1.5)));
    scene.stroke(&Stroke::new(ui_px(1.1)), ID, ink, None, &caret);
}

/// The popup the footer's fx button opens — Offset Path plus every
/// Distort & Transform effect `effectdlg` knows about. A future effect
/// kind is one more entry here, not a new button or a new dialog family
/// unless it genuinely needs one.
const FX_MENU_ITEM_H: f64 = 26.0;
const FX_MENU_W: f64 = 150.0;

fn fx_menu_entries() -> Vec<(&'static str, EffectMenuChoice)> {
    let mut v = vec![("Offset Path…", EffectMenuChoice::Offset)];
    for kind in crate::effectdlg::EffectKind::ALL {
        v.push((kind.menu_label(), EffectMenuChoice::Distort(kind)));
    }
    v
}

/// Where the fx menu sits, opening upward from the fx button — clamped to
/// `body` itself rather than just "however tall `entry_count` rows want
/// to be": the Appearance panel's own Stack-mode flyout preview is a
/// small fixed size (`layout::metric_flyout_h`, independent of how many
/// rows this panel actually has), so a 7-entry menu opening upward
/// unclamped can compute a top edge *above* the flyout's own body and get
/// silently clipped away entirely by its clip layer — the menu would
/// exist but never actually be visible. Anchoring the bottom at the fx
/// button and clamping the top to `body.y0` keeps at least the top of the
/// menu on-screen even when there isn't room for the whole thing; the
/// per-entry rows past whatever last fits still exist for
/// `fx_menu_entry_rect` (nothing currently scrolls this popup), but they
/// land inside the always-visible common case (a docked/floating panel,
/// which has plenty of room).
fn fx_menu_rect(body: Rect, entry_count: usize) -> Rect {
    let f = footer_layout(body);
    let h = ui_px(FX_MENU_ITEM_H) * entry_count as f64;
    let y1 = (f.fx.y0 - ui_px(6.0)).max(body.y0 + ui_px(4.0));
    let y0 = (y1 - h).max(body.y0 + ui_px(4.0));
    Rect::new(f.fx.x0, y0, f.fx.x0 + ui_px(FX_MENU_W), y1)
}

fn fx_menu_entry_rect(body: Rect, entry_count: usize, i: usize) -> Rect {
    let menu = fx_menu_rect(body, entry_count);
    let y = menu.y0 + i as f64 * ui_px(FX_MENU_ITEM_H);
    Rect::new(menu.x0, y, menu.x1, y + ui_px(FX_MENU_ITEM_H))
}

fn paint_fx_menu(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, body: Rect) {
    let entries = fx_menu_entries();
    let menu = fx_menu_rect(body, entries.len());
    scene.fill(Fill::NonZero, ID, th.panel_bg, None, &menu.to_rounded_rect(ui_px(4.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &menu.to_rounded_rect(ui_px(4.0)));
    for (i, (label, _)) in entries.iter().enumerate() {
        let r = fx_menu_entry_rect(body, entries.len(), i);
        text.draw(scene, label, 12.0, th.text, r.x0 + ui_px(10.0), r.center().y + ui_px(4.0));
    }
}

fn text_button(scene: &mut Scene, text: &mut TextContext, th: &crate::theme::Theme, r: Rect, label: &str, enabled: bool, pointer: Point) {
    let hot = enabled && r.contains(pointer);
    let bg = if hot { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.text_dim.with_alpha(0.5), None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if enabled { th.text } else { th.text_dim.with_alpha(0.4) };
    let w = text.measure(label, 11.0);
    text.draw(scene, label, 11.0, ink, r.center().x - w * 0.5, r.center().y + ui_px(3.5));
}

enum IconKind {
    Duplicate,
    Trash,
    ClearEffects,
}

fn icon_button(scene: &mut Scene, th: &crate::theme::Theme, r: Rect, kind: IconKind, enabled: bool, pointer: Point) {
    let hot = enabled && r.contains(pointer);
    let bg = if hot { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, th.text_dim.with_alpha(0.5), None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if enabled { th.text_dim } else { th.text_dim.with_alpha(0.35) };
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
        // A plain circle-slash — "clear every live effect," Illustrator's
        // own "no" glyph, distinct from Trash's per-row delete.
        IconKind::ClearEffects => {
            let s = Stroke::new(ui_px(1.1));
            scene.stroke(&s, ID, ink, None, &Circle::new(c, ui_px(6.0)));
            let d = ui_px(6.0) * std::f64::consts::FRAC_1_SQRT_2;
            scene.stroke(&s, ID, ink, None, &Line::new((c.x - d, c.y + d), (c.x + d, c.y - d)));
        }
    }
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    hit_items(&ctx.appearance_items, ctx.appearance_selected, ctx.appearance_fx_menu, body, p)
}

/// Which Stroke item's weight field (if any) `p` is over — used by
/// `App::appearance_width_field_at_pointer` to decide whether a press
/// lands on the field currently being edited (stay in it) or elsewhere
/// (commit it), the same "is the pointer still on this field" check
/// every other embedded numeric field in the shell makes.
pub fn row_weight_field_at(items: &[AppearanceItem], body: Rect, p: Point) -> Option<usize> {
    let list = list_rect(body);
    if !list.contains(p) {
        return None;
    }
    let slots = display_slots(items);
    let row = ((p.y - list.y0) / metric_row_h()).floor() as usize;
    let Slot::Item(idx) = *slots.get(row)? else {
        return None;
    };
    if !matches!(items[idx], AppearanceItem::Stroke { .. }) {
        return None;
    }
    let r = row_rect(list, row);
    row_layout(r).weight.contains(p).then_some(idx)
}

/// The real hit-test logic, independent of `Ctx` so it's unit-testable
/// without constructing one (mirrors how `paragraph.rs`'s alignment-tick
/// math is split out from its own `Ctx`-taking `paint`/`hit`).
fn hit_items(items: &[AppearanceItem], selected: Option<usize>, fx_menu_open: bool, body: Rect, p: Point) -> Action {
    // The fx menu is modal while open: its own entry commits, anything
    // else (including re-clicking the fx button) just closes it — same
    // "first click away just dismisses" convention as every other
    // popover in the shell (the Stack-mode flyout, the panel hamburger).
    if fx_menu_open {
        let entries = fx_menu_entries();
        for (i, &(_, choice)) in entries.iter().enumerate() {
            if fx_menu_entry_rect(body, entries.len(), i).contains(p) {
                return match selected {
                    Some(idx) => Action::AppearanceAddEffect(idx, choice),
                    None => Action::AppearanceToggleFxMenu,
                };
            }
        }
        return Action::AppearanceToggleFxMenu;
    }
    let f = footer_layout(body);
    if f.add_fill.contains(p) {
        return Action::AppearanceAddFill;
    }
    if f.add_stroke.contains(p) {
        return Action::AppearanceAddStroke;
    }
    if f.fx.contains(p) && selected.is_some() {
        return Action::AppearanceToggleFxMenu;
    }
    if f.clear_effects.contains(p) && items.iter().any(|i| !i.effects().is_empty()) {
        return Action::AppearanceClearEffects;
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
    let Some(&slot) = slots.get(row) else {
        return Action::None;
    };
    let r = row_rect(list, row);
    match slot {
        Slot::Effect(idx, effect_idx) => {
            let el = effect_row_layout(r);
            if el.trash.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
                return Action::AppearanceRemoveEffect(idx, effect_idx);
            }
            Action::OpenEffectDialog(idx, effect_idx)
        }
        Slot::Item(idx) => {
            let l = row_layout(r);
            if l.eye.contains(p) {
                return Action::AppearanceToggleVisible(idx);
            }
            if matches!(items[idx], AppearanceItem::Stroke { .. }) && l.weight.contains(p) {
                return Action::BeginAppearanceWidthEdit(idx);
            }
            if l.swatch.inflate(ui_px(3.0), ui_px(3.0)).contains(p) {
                return Action::OpenAppearanceItemPicker(idx);
            }
            Action::AppearanceSelect(idx)
        }
    }
}

pub fn tip(body: Rect, p: Point, ctx: &Ctx) -> Option<&'static str> {
    if ctx.appearance_fx_menu {
        return None;
    }
    let f = footer_layout(body);
    if f.add_fill.contains(p) {
        return Some("Add New Fill");
    }
    if f.add_stroke.contains(p) {
        return Some("Add New Stroke");
    }
    if f.fx.contains(p) {
        return Some("Add a live effect to the selected row");
    }
    if f.clear_effects.contains(p) {
        return Some("Clear Effects");
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
    let &slot = slots.get(row)?;
    let idx = match slot {
        Slot::Effect(..) => return Some("Click to edit, trash to remove"),
        Slot::Item(idx) => idx,
    };
    let r = row_rect(list, row);
    let l = row_layout(r);
    if matches!(ctx.appearance_items[idx], AppearanceItem::Stroke { .. }) && l.weight.contains(p) {
        return Some("Click to edit, or scroll to nudge");
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
            AppearanceItem::Fill { paint: Paint::Solid(amalith_core::Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, effects: Vec::new() },
            AppearanceItem::Stroke {
                paint: Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 1.0)),
                width: 2.0,
                style: amalith_core::StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                effects: Vec::new(),
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
        assert_eq!(hit_items(&items, None, false, b, top_row_center), Action::AppearanceSelect(1));
        let second_row_center = row_rect(list, 1).center();
        assert_eq!(hit_items(&items, None, false, b, second_row_center), Action::AppearanceSelect(0));
    }

    #[test]
    fn clicking_the_eye_toggles_that_rows_own_item() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        let r = row_rect(list, 0);
        let eye = row_layout(r).eye.center();
        assert_eq!(hit_items(&items, None, false, b, eye), Action::AppearanceToggleVisible(1));
    }

    #[test]
    fn clicking_the_swatch_opens_that_items_picker() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        let r = row_rect(list, 1);
        let swatch = row_layout(r).swatch.center();
        assert_eq!(hit_items(&items, None, false, b, swatch), Action::OpenAppearanceItemPicker(0));
    }

    #[test]
    fn duplicate_and_delete_are_inert_without_a_selected_row() {
        let items = two_items();
        let b = body();
        let f = footer_layout(b);
        assert_eq!(hit_items(&items, None, false, b, f.duplicate.center()), Action::None);
        assert_eq!(hit_items(&items, None, false, b, f.delete.center()), Action::None);
        assert_eq!(hit_items(&items, Some(0), false, b, f.duplicate.center()), Action::AppearanceDuplicate);
        assert_eq!(hit_items(&items, Some(0), false, b, f.delete.center()), Action::AppearanceDelete);
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
        assert_eq!(hit_items(&[], None, false, b, f.add_fill.center()), Action::AppearanceAddFill);
        assert_eq!(hit_items(&[], None, false, b, f.add_stroke.center()), Action::AppearanceAddStroke);
    }

    fn item_with_offset() -> Vec<AppearanceItem> {
        vec![
            AppearanceItem::Fill { paint: Paint::Solid(amalith_core::Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, effects: Vec::new() },
            AppearanceItem::Stroke {
                paint: Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 1.0)),
                width: 2.0,
                style: amalith_core::StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                effects: vec![amalith_core::Effect::Offset(amalith_core::OffsetEffect { amount: -2.0, join: amalith_core::LineJoin::Miter, miter_limit: 4.0 })],
            },
        ]
    }

    #[test]
    fn display_slots_inserts_a_nested_effect_row_right_under_its_item() {
        let items = item_with_offset();
        let slots = display_slots(&items);
        // Display order is reversed (Stroke=1 on top); it carries the
        // effect, so its nested row (idx 1, effect 0) follows directly,
        // then the plain Fill row (idx 0) with no nested row of its own.
        assert_eq!(slots, vec![Slot::Item(1), Slot::Effect(1, 0), Slot::Item(0)]);
    }

    #[test]
    fn display_slots_shows_one_nested_row_per_stacked_effect_on_the_same_item() {
        let mut items = two_items();
        let fx = amalith_core::OffsetEffect { amount: 1.0, join: amalith_core::LineJoin::Miter, miter_limit: 4.0 };
        items[1].effects_mut().extend([amalith_core::Effect::Offset(fx), amalith_core::Effect::Offset(fx)]);
        let slots = display_slots(&items);
        assert_eq!(slots, vec![Slot::Item(1), Slot::Effect(1, 0), Slot::Effect(1, 1), Slot::Item(0)]);
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
    fn the_footer_fx_button_toggles_the_menu_only_with_a_row_selected() {
        let items = two_items();
        let b = body();
        let f = footer_layout(b);
        assert_eq!(hit_items(&items, None, false, b, f.fx.center()), Action::None, "nothing selected — fx has no target row");
        assert_eq!(hit_items(&items, Some(1), false, b, f.fx.center()), Action::AppearanceToggleFxMenu);
    }

    #[test]
    fn picking_offset_path_from_the_open_fx_menu_adds_it_to_the_selected_row() {
        let items = two_items();
        let b = body();
        let entries = fx_menu_entries();
        let entry = fx_menu_entry_rect(b, entries.len(), 0).center();
        assert_eq!(
            hit_items(&items, Some(1), true, b, entry),
            Action::AppearanceAddEffect(1, EffectMenuChoice::Offset)
        );
    }

    #[test]
    fn picking_a_distort_effect_from_the_open_fx_menu_adds_that_kind_to_the_selected_row() {
        let items = two_items();
        let b = body();
        let entries = fx_menu_entries();
        // Entry 1 is the first Distort & Transform kind (Zig Zag) —
        // entry 0 is always Offset Path.
        let entry = fx_menu_entry_rect(b, entries.len(), 1).center();
        assert_eq!(
            hit_items(&items, Some(1), true, b, entry),
            Action::AppearanceAddEffect(1, EffectMenuChoice::Distort(crate::effectdlg::EffectKind::ZigZag))
        );
    }

    #[test]
    fn clicking_anywhere_else_while_the_fx_menu_is_open_just_closes_it() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        // A totally unrelated row click, while the menu is modal-open,
        // closes the menu instead of selecting that row — a second click
        // is needed to actually select it.
        assert_eq!(
            hit_items(&items, Some(1), true, b, row_rect(list, 1).center()),
            Action::AppearanceToggleFxMenu
        );
    }

    #[test]
    fn clear_effects_only_fires_when_some_item_actually_has_an_effect() {
        let plain = two_items();
        let with_fx = item_with_offset();
        let b = body();
        let f = footer_layout(b);
        assert_eq!(hit_items(&plain, None, false, b, f.clear_effects.center()), Action::None);
        assert_eq!(hit_items(&with_fx, None, false, b, f.clear_effects.center()), Action::AppearanceClearEffects);
    }

    #[test]
    fn clicking_a_strokes_weight_field_begins_editing_it() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        // Topmost displayed row (real index 1) is the Stroke.
        let l = row_layout(row_rect(list, 0));
        assert_eq!(hit_items(&items, None, false, b, l.weight.center()), Action::BeginAppearanceWidthEdit(1));
    }

    #[test]
    fn clicking_a_fills_row_at_the_weight_position_does_nothing_special() {
        let items = two_items();
        let b = body();
        let list = list_rect(b);
        // Second displayed row (real index 0) is the Fill — no weight field.
        let l = row_layout(row_rect(list, 1));
        assert_eq!(hit_items(&items, None, false, b, l.weight.center()), Action::AppearanceSelect(0));
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
        assert_eq!(hit_items(&items, None, false, b, el.trash.center()), Action::AppearanceRemoveEffect(1, 0));
        assert_eq!(hit_items(&items, None, false, b, effect_row.center()), Action::OpenEffectDialog(1, 0));
    }

    /// The Appearance panel's own Stack-mode flyout preview is a small
    /// fixed size (`layout::metric_flyout_h`, ~220px before UI scale) —
    /// far shorter than a 7-entry fx menu opening upward unclamped would
    /// need (7 * 26px = 182px, plus the footer/padding above it easily
    /// pushes the naive top edge above `body.y0`). Before clamping,
    /// `fx_menu_rect` would return a rect already entirely above the
    /// flyout's own clip region — invisible, not just cramped.
    #[test]
    fn fx_menu_rect_stays_inside_a_short_body_instead_of_opening_above_it() {
        let entries = fx_menu_entries();
        let short_body = Rect::new(0.0, 0.0, 240.0, 130.0);
        let menu = fx_menu_rect(short_body, entries.len());
        assert!(menu.y0 >= short_body.y0, "the menu's top edge must stay inside the body, not float off above it");
        assert!(menu.y1 <= short_body.y1 + 1.0, "the menu's bottom edge (anchored at the fx button) must also stay inside the body");
    }

    #[test]
    fn fx_menu_rect_is_unclamped_and_sits_above_the_button_in_a_tall_body() {
        let entries = fx_menu_entries();
        let tall_body = body();
        let f = footer_layout(tall_body);
        let menu = fx_menu_rect(tall_body, entries.len());
        assert!((menu.y1 - (f.fx.y0 - ui_px(6.0))).abs() < 0.5, "with room to spare, the menu still anchors right above the fx button");
        assert!((menu.height() - ui_px(FX_MENU_ITEM_H) * entries.len() as f64).abs() < 0.5, "and shows every entry at full height");
    }
}
