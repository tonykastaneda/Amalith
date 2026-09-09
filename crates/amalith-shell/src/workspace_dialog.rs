//! Windows ▸ Workspace's two small modals — naming a new workspace, and
//! deleting saved ones — each a centered card painted directly over the
//! main canvas (no native OS dialog), the same modal pattern as
//! `about.rs`/`newdoc.rs`.

use crate::metrics::px as ui_px;

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;
const SCRIM: Color = Color::from_rgba8(0, 0, 0, 140);

// ---------------------------------------------------------------- New Workspace

/// Windows ▸ Workspace ▸ New Workspace… — a single text field + Cancel/OK.
/// `fresh` mirrors `Rename`'s: true until the first keystroke, so typing
/// starts from an empty buffer rather than appending to a placeholder.
pub struct NamePrompt {
    pub buf: String,
    pub fresh: bool,
}

impl NamePrompt {
    pub fn new() -> Self {
        Self { buf: String::new(), fresh: true }
    }
}

fn metric_card_w() -> f64 { crate::metrics::with(|m| m.workspace_dialog_card_w) }
fn metric_card_h() -> f64 { crate::metrics::with(|m| m.workspace_dialog_card_h) }

fn card_rect(viewport: Rect) -> Rect {
    let c = viewport.center();
    Rect::new(c.x - metric_card_w() / 2.0, c.y - metric_card_h() / 2.0, c.x + metric_card_w() / 2.0, c.y + metric_card_h() / 2.0)
}

/// Field, Cancel, OK — in that order — inside an already-positioned card.
fn prompt_rects(card: Rect) -> (Rect, Rect, Rect) {
    let field = Rect::new(card.x0 + ui_px(20.0), card.y0 + ui_px(48.0), card.x1 - ui_px(20.0), card.y0 + ui_px(48.0) + ui_px(30.0));
    let btn_w = ui_px(74.0);
    let btn_h = ui_px(28.0);
    let ok = Rect::new(card.x1 - ui_px(20.0) - btn_w, card.y1 - ui_px(20.0) - btn_h, card.x1 - ui_px(20.0), card.y1 - ui_px(20.0));
    let cancel = Rect::new(ok.x0 - ui_px(10.0) - btn_w, ok.y0, ok.x0 - ui_px(10.0), ok.y1);
    (field, cancel, ok)
}

pub enum Hit {
    Backdrop,
    Field,
    Cancel,
    Ok,
    None,
}

pub fn hit(viewport: Rect, p: Point) -> Hit {
    let card = card_rect(viewport);
    if !card.contains(p) {
        return Hit::Backdrop;
    }
    let (field, cancel, ok) = prompt_rects(card);
    if field.contains(p) {
        Hit::Field
    } else if cancel.contains(p) {
        Hit::Cancel
    } else if ok.contains(p) {
        Hit::Ok
    } else {
        Hit::None
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, viewport: Rect, p: &NamePrompt, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, SCRIM, None, &viewport);
    let card = card_rect(viewport);
    let rr = card.to_rounded_rect(ui_px(10.0));
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &rr);
    text.draw(scene, "New Workspace", 14.0, theme.text, card.x0 + ui_px(20.0), card.y0 + ui_px(30.0));

    let (field, cancel, ok) = prompt_rects(card);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &field);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &field);
    let baseline = field.y0 + field.height() * 0.5 + ui_px(4.5);
    if !p.buf.is_empty() {
        text.draw(scene, &p.buf, 13.0, theme.text, field.x0 + ui_px(8.0), baseline);
    }
    let caret_x = field.x0 + ui_px(8.0) + text.measure(&p.buf, 13.0);
    let caret = Rect::new(caret_x, field.y0 + ui_px(6.0), caret_x + 1.0, field.y1 - ui_px(6.0));
    scene.fill(Fill::NonZero, ID, theme.text, None, &caret);

    crate::widgets::button(scene, text, theme, cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, ok, "OK", true);
}

// ------------------------------------------------------------ Manage Workspaces

fn metric_manage_w() -> f64 { crate::metrics::with(|m| m.workspace_dialog_manage_w) }
fn metric_row_h() -> f64 { crate::metrics::with(|m| m.workspace_dialog_row_h) }
fn metric_list_top() -> f64 { crate::metrics::with(|m| m.workspace_dialog_list_top) }
fn metric_footer_h() -> f64 { crate::metrics::with(|m| m.workspace_dialog_footer_h) }
fn metric_empty_h() -> f64 { crate::metrics::with(|m| m.workspace_dialog_empty_h) }

fn manage_card_rect(viewport: Rect, count: usize) -> Rect {
    let list_h = if count == 0 { metric_empty_h() } else { count as f64 * metric_row_h() };
    let h = (metric_list_top() + list_h + metric_footer_h()).min(viewport.height() - ui_px(40.0));
    let c = viewport.center();
    Rect::new(c.x - metric_manage_w() / 2.0, c.y - h / 2.0, c.x + metric_manage_w() / 2.0, c.y + h / 2.0)
}

fn manage_row_rect(card: Rect, i: usize) -> Rect {
    let y0 = card.y0 + metric_list_top() + i as f64 * metric_row_h();
    Rect::new(card.x0 + ui_px(16.0), y0, card.x1 - ui_px(16.0), y0 + metric_row_h())
}

fn delete_rect(row: Rect) -> Rect {
    Rect::new(row.x1 - ui_px(24.0), row.y0 + (row.height() - ui_px(18.0)) * 0.5, row.x1, row.y0 + (row.height() + ui_px(18.0)) * 0.5)
}

fn done_rect(card: Rect) -> Rect {
    let w = ui_px(74.0);
    let h = ui_px(28.0);
    Rect::new(card.x1 - ui_px(20.0) - w, card.y1 - ui_px(20.0) - h, card.x1 - ui_px(20.0), card.y1 - ui_px(20.0))
}

pub enum ManageHit {
    Backdrop,
    Delete(usize),
    Done,
    None,
}

pub fn manage_hit(viewport: Rect, names: &[String], p: Point) -> ManageHit {
    let card = manage_card_rect(viewport, names.len());
    if !card.contains(p) {
        return ManageHit::Backdrop;
    }
    for i in 0..names.len() {
        if delete_rect(manage_row_rect(card, i)).contains(p) {
            return ManageHit::Delete(i);
        }
    }
    if done_rect(card).contains(p) {
        return ManageHit::Done;
    }
    ManageHit::None
}

pub fn paint_manage(scene: &mut Scene, text: &mut TextContext, viewport: Rect, names: &[String], theme: &Theme) {
    scene.fill(Fill::NonZero, ID, SCRIM, None, &viewport);
    let card = manage_card_rect(viewport, names.len());
    let rr = card.to_rounded_rect(ui_px(10.0));
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &rr);
    text.draw(scene, "Manage Workspaces", 14.0, theme.text, card.x0 + ui_px(20.0), card.y0 + ui_px(30.0));

    if names.is_empty() {
        text.draw(
            scene,
            "No saved workspaces yet.",
            12.5,
            theme.text_dim,
            card.x0 + ui_px(20.0),
            card.y0 + metric_list_top() + metric_empty_h() * 0.5 + ui_px(4.0),
        );
    } else {
        for (i, name) in names.iter().enumerate() {
            let row = manage_row_rect(card, i);
            if i > 0 {
                let sep = Rect::new(row.x0, row.y0 - 0.5, row.x1, row.y0 + 0.5);
                scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
            }
            let baseline = row.y0 + row.height() * 0.5 + ui_px(4.5);
            text.draw(scene, name, 12.5, theme.text, row.x0, baseline);
            let del = delete_rect(row);
            crate::chrome::paint_x(scene, del, theme.text_dim, 3.0);
        }
    }

    crate::widgets::button(scene, text, theme, done_rect(card), "Done", true);
}
