//! A dismissible corner card, bottom-right of the main window. Unlike
//! `confirm_close`/`about` this is never modal: it doesn't block clicks
//! elsewhere, and it only consumes a press that actually lands on it.
//!
//! Two notices share the card, and at most one is drawn at a time (an app
//! update wins — see `App::redraw`):
//!
//! - **An available app update**, once `App::update_available` (see
//!   `update_check::spawn`) has an answer. There's no auto-download here;
//!   the button opens the releases page in the system browser.
//! - **An integration that has drifted** from the installed build
//!   (`crate::integrations::needs_update`) — the `ama` shell function or an
//!   agent's copy of the skill. Its button opens Preferences ▸ Integrations,
//!   where the user reinstalls. Amalith doesn't rewrite those files unasked:
//!   they live inside another tool's configuration.
//!
//! Either way the card is dismissible for the session: the ✕ in its top-right
//! corner closes it, and it clears itself after [`TIMEOUT`] so a card nobody
//! acted on doesn't sit in the corner for the rest of the session.

use crate::metrics::px as ui_px;

use vello::kurbo::{Affine, Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;

/// How long a notice stays up before clearing itself. Long enough to read and
/// act on, short enough that an ignored card doesn't become furniture.
pub const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn card_rect(viewport: Rect) -> Rect {
    let w = ui_px(260.0);
    let h = ui_px(108.0);
    let margin = ui_px(20.0);
    Rect::new(viewport.x1 - margin - w, viewport.y1 - margin - h, viewport.x1 - margin, viewport.y1 - margin)
}

fn close_rect(card: Rect) -> Rect {
    Rect::new(card.x1 - ui_px(30.0), card.y0 + ui_px(4.0), card.x1 - ui_px(6.0), card.y0 + ui_px(28.0))
}

fn action_rect(card: Rect) -> Rect {
    let pad = ui_px(16.0);
    Rect::new(card.x0 + pad, card.y0 + ui_px(64.0), card.x1 - pad, card.y0 + ui_px(92.0))
}

/// One notice's words. The card's shape is fixed; only these change.
pub struct Notice<'a> {
    pub title: &'a str,
    pub body: &'a str,
    /// Label on the action button.
    pub action: &'a str,
}

pub enum Hit {
    Dismiss,
    /// The action button — what that means depends on which notice is up.
    Action,
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
    } else if action_rect(card).contains(p) {
        Hit::Action
    } else {
        Hit::None
    }
}

pub fn paint(
    scene: &mut Scene,
    text: &mut TextContext,
    viewport: Rect,
    notice: &Notice,
    theme: &Theme,
) {
    let card = card_rect(viewport);
    let rr = card.to_rounded_rect(ui_px(10.0));
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &rr);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &rr);

    let pad = ui_px(16.0);
    text.draw(scene, notice.title, 13.0, theme.text, card.x0 + pad, card.y0 + ui_px(26.0));
    text.draw(scene, notice.body, 11.5, theme.text_dim, card.x0 + pad, card.y0 + ui_px(46.0));

    let close = close_rect(card);
    let (ccx, ccy) = (close.center().x, close.center().y);
    let s = ui_px(3.6);
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &Line::new((ccx - s, ccy - s), (ccx + s, ccy + s)));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &Line::new((ccx - s, ccy + s), (ccx + s, ccy - s)));

    crate::widgets::button(scene, text, theme, action_rect(card), notice.action, true);
}

/// The update notice's "Download" button — the releases page, not a file pulled
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
