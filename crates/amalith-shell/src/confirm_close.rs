//! The "unsaved changes" confirmation modal — Save / Don't Save / Cancel,
//! shown before closing a tab, closing every tab, or quitting whenever an
//! open document has unsaved edits (`App::is_tab_dirty`). A centered card
//! over the main canvas, the same modal pattern as
//! `about.rs`/`workspace_dialog.rs`.

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;
const SCRIM: Color = Color::from_rgba8(0, 0, 0, 140);
const CARD_W: f64 = 380.0;
const CARD_H: f64 = 128.0;

fn card_rect(viewport: Rect) -> Rect {
    let c = viewport.center();
    Rect::new(c.x - CARD_W / 2.0, c.y - CARD_H / 2.0, c.x + CARD_W / 2.0, c.y + CARD_H / 2.0)
}

/// Cancel, Don't Save, Save — left to right, matching the usual macOS
/// convention (destructive action in the middle, default/primary action
/// rightmost).
fn button_rects(card: Rect) -> (Rect, Rect, Rect) {
    let h = 28.0;
    let y0 = card.y1 - 20.0 - h;
    let y1 = card.y1 - 20.0;
    let save = Rect::new(card.x1 - 20.0 - 76.0, y0, card.x1 - 20.0, y1);
    let dont_save_w = 96.0;
    let dont_save = Rect::new(save.x0 - 10.0 - dont_save_w, y0, save.x0 - 10.0, y1);
    let cancel = Rect::new(card.x0 + 20.0, y0, card.x0 + 20.0 + 76.0, y1);
    (cancel, dont_save, save)
}

pub enum Hit {
    Backdrop,
    Cancel,
    DontSave,
    Save,
    None,
}

pub fn hit(viewport: Rect, p: Point) -> Hit {
    let card = card_rect(viewport);
    if !card.contains(p) {
        return Hit::Backdrop;
    }
    let (cancel, dont_save, save) = button_rects(card);
    if cancel.contains(p) {
        Hit::Cancel
    } else if dont_save.contains(p) {
        Hit::DontSave
    } else if save.contains(p) {
        Hit::Save
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

/// `name` is the dirty document's title, for the prompt text.
pub fn paint(scene: &mut Scene, text: &mut TextContext, viewport: Rect, name: &str, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, SCRIM, None, &viewport);
    let card = card_rect(viewport);
    let rr = card.to_rounded_rect(10.0);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);

    let title = format!("Save changes to \u{201c}{name}\u{201d} before closing?");
    text.draw(scene, &title, 13.5, theme.text, card.x0 + 20.0, card.y0 + 34.0);
    let body = "Your changes will be lost if you don\u{2019}t save them.";
    text.draw(scene, body, 12.0, theme.text_dim, card.x0 + 20.0, card.y0 + 58.0);

    let (cancel, dont_save, save) = button_rects(card);
    paint_button(scene, text, cancel, "Cancel", theme, false);
    paint_button(scene, text, dont_save, "Don't Save", theme, false);
    paint_button(scene, text, save, "Save", theme, true);
}
