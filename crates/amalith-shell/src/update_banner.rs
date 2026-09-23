//! A dismissible corner card, bottom-right of the main window — drawn
//! only once `App::update_available` (see `update_check::spawn`) has an
//! answer and the user hasn't already closed it this session. Unlike
//! `confirm_close`/`about`, this is never modal: it doesn't block clicks
//! elsewhere, and there's no auto-download/install — the button just
//! opens the releases page in the system browser.

use crate::metrics::px as ui_px;

use vello::kurbo::{Affine, Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;

fn card_rect(viewport: Rect) -> Rect {
    let w = ui_px(260.0);
    let h = ui_px(108.0);
    let margin = ui_px(20.0);
    Rect::new(viewport.x1 - margin - w, viewport.y1 - margin - h, viewport.x1 - margin, viewport.y1 - margin)
}

fn close_rect(card: Rect) -> Rect {
    Rect::new(card.x1 - ui_px(30.0), card.y0 + ui_px(4.0), card.x1 - ui_px(6.0), card.y0 + ui_px(28.0))
}

fn download_rect(card: Rect) -> Rect {
    let pad = ui_px(16.0);
    Rect::new(card.x0 + pad, card.y0 + ui_px(64.0), card.x1 - pad, card.y0 + ui_px(92.0))
}

pub enum Hit {
    Dismiss,
    Download,
    None,
}

/// `None` for anything outside the card, so callers can fall through to
/// their normal click handling instead of treating this as modal.
pub fn hit(viewport: Rect, p: Point) -> Hit {
    let card = card_rect(viewport);
    if !card.contains(p) {
        return Hit::None;
    }
    if close_rect(card).contains(p) {
        Hit::Dismiss
    } else if download_rect(card).contains(p) {
        Hit::Download
    } else {
        Hit::None
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, viewport: Rect, version: &str, theme: &Theme) {
    let card = card_rect(viewport);
    let rr = card.to_rounded_rect(ui_px(10.0));
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &rr);

    let pad = ui_px(16.0);
    text.draw(scene, "Update available", 13.0, theme.text, card.x0 + pad, card.y0 + ui_px(26.0));
    let subtitle = format!("Version {version} is ready");
    text.draw(scene, &subtitle, 11.5, theme.text_dim, card.x0 + pad, card.y0 + ui_px(46.0));

    let close = close_rect(card);
    let (ccx, ccy) = (close.center().x, close.center().y);
    let s = ui_px(3.6);
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &Line::new((ccx - s, ccy - s), (ccx + s, ccy + s)));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &Line::new((ccx - s, ccy + s), (ccx + s, ccy - s)));

    crate::widgets::button(scene, text, theme, download_rect(card), "Download", true);
}

/// The banner's "Download" button — the releases page, not a file pulled
/// straight into the running app (no self-updater here; see the
/// packaging discussion this shipped with for why that's out of scope).
pub fn open_latest_release() {
    let url = "https://github.com/tonykastaneda/Amalith/releases/latest";
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd").args(["/C", "start", url]).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
