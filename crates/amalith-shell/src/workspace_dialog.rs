//! Windows ▸ Workspace's two small modals — naming a new workspace, and
//! deleting saved ones — each a centered card painted directly over the
//! main canvas (no native OS dialog), the same modal pattern as
//! `about.rs`/`newdoc.rs`.

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

const CARD_W: f64 = 340.0;
const CARD_H: f64 = 132.0;

fn card_rect(viewport: Rect) -> Rect {
    let c = viewport.center();
    Rect::new(c.x - CARD_W / 2.0, c.y - CARD_H / 2.0, c.x + CARD_W / 2.0, c.y + CARD_H / 2.0)
}

/// Field, Cancel, OK — in that order — inside an already-positioned card.
fn prompt_rects(card: Rect) -> (Rect, Rect, Rect) {
    let field = Rect::new(card.x0 + 20.0, card.y0 + 48.0, card.x1 - 20.0, card.y0 + 48.0 + 30.0);
    let btn_w = 74.0;
    let btn_h = 28.0;
    let ok = Rect::new(card.x1 - 20.0 - btn_w, card.y1 - 20.0 - btn_h, card.x1 - 20.0, card.y1 - 20.0);
    let cancel = Rect::new(ok.x0 - 10.0 - btn_w, ok.y0, ok.x0 - 10.0, ok.y1);
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

fn paint_button(scene: &mut Scene, text: &mut TextContext, r: Rect, label: &str, theme: &Theme, primary: bool) {
    let rr = r.to_rounded_rect(5.0);
    if primary {
        scene.fill(Fill::NonZero, ID, theme.accent, None, &rr);
    } else {
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    }
    let color = if primary { theme.on_accent } else { theme.text };
    let w = text.measure(label, 12.5);
    let x = r.x0 + (r.width() - w) * 0.5;
    let y = r.y0 + r.height() * 0.5 + 4.5;
    text.draw(scene, label, 12.5, color, x, y);
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, viewport: Rect, p: &NamePrompt, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, SCRIM, None, &viewport);
    let card = card_rect(viewport);
    let rr = card.to_rounded_rect(10.0);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    text.draw(scene, "New Workspace", 14.0, theme.text, card.x0 + 20.0, card.y0 + 30.0);

    let (field, cancel, ok) = prompt_rects(card);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &field);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &field);
    let baseline = field.y0 + field.height() * 0.5 + 4.5;
    if !p.buf.is_empty() {
        text.draw(scene, &p.buf, 13.0, theme.text, field.x0 + 8.0, baseline);
    }
    let caret_x = field.x0 + 8.0 + text.measure(&p.buf, 13.0);
    let caret = Rect::new(caret_x, field.y0 + 6.0, caret_x + 1.0, field.y1 - 6.0);
    scene.fill(Fill::NonZero, ID, theme.text, None, &caret);

    paint_button(scene, text, cancel, "Cancel", theme, false);
    paint_button(scene, text, ok, "OK", theme, true);
}

// ------------------------------------------------------------ Manage Workspaces

const MANAGE_W: f64 = 320.0;
const ROW_H: f64 = 32.0;
const LIST_TOP: f64 = 48.0;
const FOOTER_H: f64 = 56.0;
const EMPTY_H: f64 = 60.0;

fn manage_card_rect(viewport: Rect, count: usize) -> Rect {
    let list_h = if count == 0 { EMPTY_H } else { count as f64 * ROW_H };
    let h = (LIST_TOP + list_h + FOOTER_H).min(viewport.height() - 40.0);
    let c = viewport.center();
    Rect::new(c.x - MANAGE_W / 2.0, c.y - h / 2.0, c.x + MANAGE_W / 2.0, c.y + h / 2.0)
}

fn manage_row_rect(card: Rect, i: usize) -> Rect {
    let y0 = card.y0 + LIST_TOP + i as f64 * ROW_H;
    Rect::new(card.x0 + 16.0, y0, card.x1 - 16.0, y0 + ROW_H)
}

fn delete_rect(row: Rect) -> Rect {
    Rect::new(row.x1 - 24.0, row.y0 + (row.height() - 18.0) * 0.5, row.x1, row.y0 + (row.height() + 18.0) * 0.5)
}

fn done_rect(card: Rect) -> Rect {
    let w = 74.0;
    let h = 28.0;
    Rect::new(card.x1 - 20.0 - w, card.y1 - 20.0 - h, card.x1 - 20.0, card.y1 - 20.0)
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
    let rr = card.to_rounded_rect(10.0);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    text.draw(scene, "Manage Workspaces", 14.0, theme.text, card.x0 + 20.0, card.y0 + 30.0);

    if names.is_empty() {
        text.draw(
            scene,
            "No saved workspaces yet.",
            12.5,
            theme.text_dim,
            card.x0 + 20.0,
            card.y0 + LIST_TOP + EMPTY_H * 0.5 + 4.0,
        );
    } else {
        for (i, name) in names.iter().enumerate() {
            let row = manage_row_rect(card, i);
            if i > 0 {
                let sep = Rect::new(row.x0, row.y0 - 0.5, row.x1, row.y0 + 0.5);
                scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
            }
            let baseline = row.y0 + row.height() * 0.5 + 4.5;
            text.draw(scene, name, 12.5, theme.text, row.x0, baseline);
            let del = delete_rect(row);
            crate::chrome::paint_x(scene, del, theme.text_dim, 3.0);
        }
    }

    paint_button(scene, text, done_rect(card), "Done", theme, true);
}
