//! Pane focus routes the existing editor and PTY controllers; document IDs
//! resolve to the same live Editor regardless of how many tabs show them.
//! Every pane owns a real list of tabs (`multiplexer::Tab`) — there is no
//! "one content per pane" anymore; see the module doc comment on
//! `crate::multiplexer` for the tree-level invariants (a pane always has
//! at least one tab; closing the last one is the only way a pane closes).
use super::*;
use crate::multiplexer::{Multiplexer, PaneId, Splitter, TabContent};

#[derive(Default)]
pub(super) struct State {
    pub model: Multiplexer,
    /// Parked (not currently on-screen) terminals, keyed by the *tab*
    /// that owns them — not the pane, since a pane can hold more than
    /// one terminal tab. The one currently on-screen lives in
    /// `App::terminal` instead, same "active lives on App" pattern as
    /// documents (`App::doc` vs `App::tabs`).
    pub terminals: std::collections::HashMap<crate::multiplexer::TabId, terminal::TerminalPane>,
    switching: bool,
    origin: Option<Point>,
}
/// Every pane's own tab strip — same height/role for every content kind
/// (Document/Terminal/Chooser alike), holding one chip per tab plus a
/// "+" to add another (Cmd+N, scoped to whichever pane it's clicked in).
/// Same metric and visual language as the old global document tab strip
/// it replaces (`app/mod.rs`'s retired `layout_tabs`/`tab_bar_rect`) —
/// same height, font size, × placement, and inter-tab dividers — so
/// panes don't read as a smaller, second-class version of it.
fn header(r: Rect) -> Rect {
    Rect::new(r.x0, r.y0, r.x1, r.y0 + metric_tab_bar_h())
}
fn body(r: Rect) -> Rect {
    let h = header(r);
    Rect::new(r.x0, h.y1, r.x1, r.y1)
}

const TAB_ADD_W: f64 = 26.0;

/// Lays every tab in `labels` out left to right in `strip`, stopping
/// before the trailing "+" button — `(whole chip rect, close-× rect)`
/// per tab. Identical geometry to `app/mod.rs`'s `layout_tabs` (the now-
/// retired global document tab strip): × sits at the chip's own left
/// edge, the label follows it.
fn pane_tab_chips(text: &mut TextContext, labels: &[String], strip: Rect) -> Vec<(Rect, Rect)> {
    let mut out = Vec::with_capacity(labels.len());
    let mut x = strip.x0 + ui_px(4.0);
    let limit = strip.x1 - ui_px(TAB_ADD_W);
    for label in labels {
        if x >= limit {
            break;
        }
        let tw = text.measure(label, 12.6);
        let w = tw + ui_px(18.9) /* × */ + ui_px(23.1) /* padding */;
        let whole = Rect::new(x, strip.y0, (x + w).min(limit), strip.y1);
        let close = Rect::new(whole.x0 + ui_px(6.0), strip.y0 + ui_px(4.0), whole.x0 + ui_px(20.0), strip.y1 - ui_px(4.0));
        out.push((whole, close));
        x += w + ui_px(2.0);
    }
    out
}
fn pane_tab_add_rect(strip: Rect) -> Rect {
    Rect::new(strip.x1 - ui_px(TAB_ADD_W), strip.y0, strip.x1, strip.y1)
}

// --- Chooser layout -------------------------------------------------
// A tab with nothing in it offers a single narrow, centered column —
// "GET STARTED" actions (New Document / Terminal / Import), then
// "RECENT DOCUMENTS" below, both sections sharing the same left edge —
// a launcher-style list (redesigned to match, of all things, Zed's own
// welcome screen), not a card grid. Every paint site and every hit-test
// site (`mux_scenes`, `mux_press`, `mux_scroll`) shares these functions
// so they can't drift apart.

/// One entry in the chooser's "RECENT DOCUMENTS" list — see
/// `App::mux_recent_entries`.
enum RecentPick {
    /// Index into `App::tabs` — already open this session.
    Open(usize),
    /// A file from disk history, not currently open.
    File(std::path::PathBuf),
}

const CHOOSER_ACTIONS: usize = 3;
const CHOOSER_COL_W: f64 = 340.0;
const CHOOSER_PAD: f64 = 16.0;
const CHOOSER_SECTION_GAP: f64 = 18.0;
const CHOOSER_HEADER_H: f64 = 20.0;
const CHOOSER_ACTION_ROW_H: f64 = 34.0;
const CHOOSER_ROW_H: f64 = 30.0;
/// The Amalith mark, top-center above "GET STARTED" — same idea as
/// Zed's own welcome screen, redrawn small since this appears on every
/// fresh pane, not just once at boot.
const CHOOSER_MARK_SIZE: f64 = 47.6;
const CHOOSER_MARK_GAP: f64 = 18.0;
/// Vertical space reserved for the "Amalith v.x" line directly under the
/// mark, before `CHOOSER_MARK_GAP` starts — same version string as the
/// macOS title-bar label and the About panel.
const CHOOSER_VERSION_BLOCK: f64 = 22.0;
const CHOOSER_VERSION: &str = crate::version::TITLE;

/// The app mark, drawn as themed vector artwork rather than a raster —
/// path data straight from `branding/Logos/emptytab-icon.svg` (the mark
/// made specifically for this "empty/new tab" chooser context), filled
/// with the theme's own ink instead of that file's own solid white
/// (`fill: #fff`, meant for a permanently-dark background) so it still
/// reads in a light theme. Square 297.58×297.58 view box; 9 sub-paths,
/// one `scene.fill` per (same convention as `panel_icon::draw`'s
/// themed, `BezPath::from_svg`-parsed glyphs, just a fill instead of a
/// stroke and a much larger source).
const CHOOSER_MARK_VIEWBOX: f64 = 297.58;
const CHOOSER_MARK_PATHS: &[&str] = &[
    "M32.69,99.1c11.09-1.91,17.5-6.24,25.98-13.34l2.94-2.77-2.25-1.21-9.53,3.81c-5.89,2.25-11.95,4.33-18.36,3.81-13.54-1.18-19.72-8.84-31.47-11.44v10.69c1.94.53,3.85,1.29,5.66,2.31,8.66,4.85,15.59,10.05,27.02,8.14Z",
    "M.64,220.7c-.2.19-.43.37-.64.56v7.62c3.47-3.01,6.27-6.68,8.09-10.95.69-1.56.87-3.29.35-4.85l-7.79,7.62Z",
    "M6.7,273.35l-3.64,7.1c-.9,1.78-1.93,3.45-3.07,5.03v12.09h.02c10.97-13.2,8.89-28.5,6.68-24.22Z",
    "M190.49,278.72l9.35-2.6c-.87-3.64-3.46-5.89-6.24-7.79-5.89-3.98-12.82-4.68-19.23-1.56-13.16,6.58-20.44-1.73-21.48,1.04-.69,2.08,9.18,11.95,22.35,7.62,9.01-3.12,12.99,3.81,15.24,3.29Z",
    "M198.44,0c.53,1.87.89,3.5,1.05,4.35l1.04,5.72c.35,2.25,1.04,3.98.87,6.41-.69,7.62-4.16,14.38-9.18,20.09-1.56,1.91-1.73,4.85-2.6,6.93-2.25,5.37,3.12,7.97,1.21,14.2l-6.06-1.39c-.52,2.77,1.21,5.54,3.64,7.1l8.66,5.89c7.97,5.54,4.85,11.95,7.27,13.16,1.73.87,7.28-3.46,8.14-5.2,4.68-9.7,11.26-16.98,22.52-18.19l11.26-1.38,1.91.87c1.21,1.91-4.16,8.83-5.72,10.91-5.02,7.27-12.99,13.86-22.34,13.16-3.81-.17-6.58,1.21-9.7,3.12l-6.76,3.98-2.94,6.76.35,2.77c2.08,2.43,3.98,5.02,6.76,6.41l4.16,2.08,5.02,2.42c6.24,6.93,10.05,3.98,17.5,7.79,7.97,3.81,12.82,11.26,14.72,19.75l1.39,6.06-1.04,2.25c-1.91,1.21-5.89-1.91-9.53-2.77-6.58-1.73-12.47-4.68-16.11-10.57-5.2-8.66-2.42-14.2-10.22-18.36-5.2-2.6.17,7.62-7.45,18.02-1.91,2.6-4.5,4.16-6.93,6.41-2.08,1.91-3.29,5.54-3.29,8.49.35,6.93,4.85,12.47,10.74,15.42,4.85,2.43,8.32,5.72,11.09,10.22,5.54,9.35,4.5,20.96-2.43,29.45-2.6,3.29-4.33,7.45-3.98,11.61.52,7.97,6.76,14.03,14.38,14.72,12.82,1.04,15.24-10.57,32.04-4.68,10.05,3.46,11.61,14.55,9.01,15.94-2.43,1.21-2.6-5.54-11.09-6.24-4.16-.35-8.14,1.73-11.61,4.33-5.37,3.98-11.95,6.76-18.71,7.45-8.14.87-15.94-1.21-22.87-5.72-4.33-2.77-8.66-6.06-10.74-10.91-2.08-4.33-1.9-8.49-1.9-13.16,0-1.56-.17-3.29-1.21-4.5-5.72,5.37-7.62,13.34-5.54,20.61,2.94,10.57,10.22,10.39,15.94,13.86,3.29,2.08,4.85,5.72,7.8,8.14,3.64,3.12,9.35,3.29,14.03,2.6,8.14-1.21,16.11.35,22.69,5.37,5.72,4.68,9.53,11.61,9.35,19.05v8.31l-2.25,2.94c-2.6-.35-4.68-1.56-7.1-2.6-5.72-2.43-11.78-4.33-18.01-5.54-5.54-1.21-10.74-1.21-16.28-.69-8.49.87-16.28,3.29-23.56,7.62-3.41,1.95-6.55,4.08-9.39,6.73h35.93c5.99-1.08,12-1.42,19.36-.84,2.1.2,4.14.49,6.15.84h59.71V0h-99.14ZM218.21,30.97l-3.54,16.21c-.83,3.78-4.72,9.96-7.84,12.63-2.41,2.06-5.34,3.02-8.41,4.45-6.85-5.22-3.94-6.82-3.37-13.93.37-4.57,2.32-8.2,4.48-12.04,1.92-3.4,4.89-5.47,8.54-7.03,2.6-1.11,5.13-3.12,7.94-3.87,1.62-.43,2.47,2.28,2.19,3.58ZM269.17,97.79c-6.81,3.43-13.8,5.87-21.21,7.81-12.79,3.35-18.32.51-28.19-6.01-2.28-1.51-9.79-1.12-9.96-4.39,3.79-3.11,7.03-.21,13.17-3.44,7.6-3.99,15.81-5.6,24.45-4.44,7.75,1.04,14.86,3.29,21.96,6.5l1.24,1.91-1.47,2.07Z",
    "M3.77,23.72c3.76-1.16,5.9-4.1,7.73-7.13l2.97-4.93c2.6-4.31,5.63-8.23,9.11-11.66H4.94c.08,1.03.19,2.06.36,3.09.41,2.51.82,4.93.43,7.54l-1.96,13.09Z",
    "M33.9,57.35c3.81.87,7.45,1.73,11.09,3.64,3.64,1.73,7.45,2.94,11.43,3.29l6.41.87c6.24.87,26.33-1.73,31.87-7.27l-13.51-2.77c-3.29-.69-6.24-1.56-9.53-2.08-1.56-.17-7.45-.87-7.1-2.77l.87-1.39,5.54-3.81,2.94-2.94,6.06-5.54c8.66-8.14,21.13-4.85,29.62-10.05,3.46-1.91,6.58-4.16,10.39-5.37,3.81-1.39,7.62-.87,11.95-1.73-3.46-2.43-7.27-3.64-11.09-4.33-5.72-1.04-11.43.35-17.15,1.73-5.02,1.21-11.78,3.29-16.28,5.37-14.55,6.58-18.01,15.42-27.02,20.09l-7.27,3.98c-2.43,1.21-4.5.87-6.93.35l-12.65-2.6c-6.58-1.38-11.95.17-18.53,1.21l-3.29-.52c.17-3.64,10.39-8.32,13.51-9.7,6.41-3.12,12.47-6.93,17.84-11.61,8.14-7.45,17.32-12.99,28.23-15.42,2.95-.69,6.06-1.56,9.35-1.73,6.41-.17,12.82.52,19.23,1.21,3.12.17,5.89.17,8.66-.69-1.56-1.73-3.29-2.6-5.2-3.64-5.25-2.82-8.03-2.43-11.39-3.13h-41.18c-2.74,5-6.26,9.75-10.18,13.83-6.18,6.44-13.86,10.54-22.04,13.87-5.93,2.41-12.76,7.05-18.57,12.18v19.58c5.17-1.82,10.56-3.09,16.23-3.66,6.24-.69,11.78,0,17.67,1.56Z",
    "M165.54,123c3.12,0,7.62-1.91,13.51-1.04l-1.56-2.94c-7.62-4.16-13.86,3.98-11.95,3.98Z",
    "M145.97,283.23c-8.49-5.72-11.09-16.11-12.64-25.81l-2.25-14.38c-.52-2.95-5.02,1.04-5.89,1.91-4.33,4.33-8.14,8.83-11.26,14.03l-2.77,4.68c-2.25,3.64-4.68,11.95-7.97,11.26-6.06-1.39.35-25.12-22.86-42.96-8.14-6.41-15.24-13.68-20.09-22.86l-.17-2.08,2.08-.35c18.19,12.82,38.97,21.31,60.97,24.77,9.35,1.39,20.79.52,26.33-8.14l10.74-17.15c7.28-11.78,18.71-33.26,20.96-46.77,1.21-6.41.52-12.64.17-19.05-.17-2.42.69-4.68-.87-6.93l-2.77,5.02c-2.08,1.39-4.85,0-6.41-1.56l-5.02-2.77-.52,1.91,1.73,11.26v.52c.35,1.56.52,3.12.87,4.68,0,.87.17,1.56.35,2.43,0,.35,0,.52.17.87,0,.52-.17.69-.35.87-.35.52-.87.52-1.39.35-1.21-.17-2.43-1.56-2.77-2.77-2.42-7.62-4.85-21.13-5.54-28.93-.52-8.66,3.12-17.49,10.05-22.86,1.56-1.38,4.85-2.25,6.93-2.25,3.81-.17,10.05,3.29,9.87,5.89-.17,1.73-5.02.17-10.22,3.29l-.87,1.04.52,1.21,1.91,2.25c6.24,1.56,7.27,9.53,9.53,5.54,2.77-5.2,2.6-11.26,1.73-17.15l-2.25-12.3c-.69-3.98-1.91-7.45-4.33-10.57-3.64-5.02-3.64-9.7-8.31-16.63-.69-.87-7.45-10.74-5.54-11.09l1.39.17,7.62,3.81,1.04-.17-3.46-6.58c-1.9-1.73-3.81-3.46-6.06-4.68-1.56-1.04-3.12-1.91-4.68-3.12-2.25-1.56-4.5-3.46-6.06-5.89-1.21-1.91-2.25-4.16-3.29-6.06-1.04-1.73-2.25-3.46-3.64-5.02-1.21-1.56-2.6-3.12-3.46-5.02-.04-.04-.16-.17-.29-.34-.4-.49-1-1.35-.74-1.74.35-.52,1.9-.17,2.77,0,2.6,0,5.54.35,7.97,1.73,1.04.69,2.08,1.56,3.29,2.25,1.04.69,2.25,1.21,3.46,1.73,5.2,2.6,10.05,5.72,14.03,9.87l4.68,4.85c3.46-2.6,0-8.14-1.21-13.16-1.56-6.76-.87-13.51,2.25-19.57l2.43-4.69h-24.16c-.35,1.54-.91,3.05-1.72,4.47-2.06,3.58-.75,6.6-2.31,6.95-1.87.42-2.05-4.05-6.42-6.03-2.47-1.12-3.95-3.21-5.3-5.39h-19.89c-.05,1.29-.02,2.57.21,3.83.52,2.77,2.77,5.02,5.54,5.54,2.95.52,4.33,2.25,5.72,4.68l1.76,3.03,2.57,4.41c2.77,4.68,5.54,9.01,7.62,14.2,1.21,2.94,0,6.41-3.46,7.27l-6.06,1.73c-3.81,1.04-7.45,1.56-11.26,2.94-18.36,6.41-16.11,10.74-25.64,20.27-4.68,4.68-10.05,8.32-15.76,11.43l-10.74,5.72c-2.08,1.04-6.24,11.95-10.91,17.15-4.33,5.2-10.74,9.7-16.8,12.47-.36.14-.69.28-1.03.43-1.33.55-2.62,1.1-3.99,1.65l-.62.23c-18.19,5.72-38.18,1.68-38.18,1.68l-4.16-1.38c-.27-.11-.54-.21-.81-.31v12.72c1.61-.53,3.18-1.17,4.8-1.85l1.04-.52.87-.35,1.04-.35c2.43-.69,6.58-1.39,13.34-1.73l1.91-.17h1.56c6.83-.24,11.98-1.51,15.34-2.69,1.58-.55,2.77-1.08,3.54-1.47l.87-.35c.35-.17,1.56-.52,1.56-.52l1.39.35c1.73,1.91-6.41,7.45.35,16.98,2.43,3.64,3.12,7.62,3.12,11.95s3.64,7.79,1.39,9.87l-2.94.69-4.85-1.04c-4.33-1.04-7.27-4.68-9.18-3.46-1.56,1.04-1.04,3.12-1.91,4.16-2.45,2.2-5.3.12-7.64-1.2-1.5-.85-3.03-1.72-4.26-2.95-1.19-1.18-1.89-2.75-3-4-1.38-1.56,0-3.64.87-5.02,2.43-3.81,4.68-7.45,5.37-12.13-2.6,1.21-5.02,2.77-6.76,4.85-1.42,1.56-2.7,2.98-3.92,4.3-4.28,4.64-7.82,7.94-13.87,10.93v9.89c.92-.62,1.88-1.2,2.89-1.73,4.68-2.6,5.02-4.85,9.7-7.27l2.25,3.12c1.73,6.41,6.58,15.24,14.38,15.24,2.25,0,3.98-1.73,5.89-2.77,3.64.87,3.12,6.41.87,9.87-2.94,3.98-6.24,2.25-6.76,7.1-.52,5.2.17,9.87.52,14.9,1.73,29.45,6.24,58.03,12.47,86.78l1.79,8.46h121.41l2.73-3.61-1.04-.52c-8.31-3.29-13.68-5.2-21.13-10.22ZM164.13,10.83c.66-2.61,2.94-4.64,5.87-4.44,3.07.2,5.25,3.11,5.12,6.09-.11,2.53-2.49,5.61-5.65,5.36l-2.54-.53c-2.52-1.05-3.38-4.2-2.8-6.48ZM156.71,18.38v.17l-.17-.17h.17ZM148.39,201.73c-.11.22-.26.41-.42.61-1.34,1.74-2.73,4.3-5.23,4.4-.52.02-1.03-.08-1.54-.18-2.12-.43-4.38-.68-6.37-1.57-.77-.34-1.51-.79-2.08-1.41-.54-.58-1.13-1.29-1.02-2.08.64-1.64,1.31-2.86,2.1-4.01.1-.14.21-.29.36-.37.17-.08.37-.07.56-.05,3.36.34,6.59,1.56,9.96,1.73,1.83.09,5.06.25,3.67,2.93ZM155.28,192.81c-1.2.49-2.04.56-2.85.42-1.19-.21-1.79-1.04-2.81-1.53-.67-.32-1.41-.49-2.16-.48-1.48,0-2.76.58-4.22,0-.88-.35-1.66-.9-2.5-1.35-3.05-1.65-6.67-1.87-10.13-1.76-1.86.06-3.74.2-5.59-.05-1.64-.22-3.22-.78-4.51-1.84-1.17-.96-.63-1.95.36-2.89,1.2-1.15,2.31-.85,3.83-.71,4.78.43,9.4-1.56,14.09-2.61,2.79-.62,4.98-.13,7.37,1.43,3.77,2.47,5.91-.78,8.02,1.22.95.9,1.16,2.18,1.32,3.41.15,1.1.55,2.18.56,3.3.01,1.17-.3,2.39-.8,3.45ZM148.03,160.8c2.74.09,5.2,1.06,7.8,1.78,1.19.33,2.4.56,3.63.54.45,0,4.26-.66,4.17-.86.64,1.4,1.08,3.2.71,4.73-.41,1.71-2.49,2.85-3.81,3.87-.2.16-.41.31-.64.42-.34.15-.68.2-1.01.27-1.75.4-2.98,1.64-3.78,3.19-.48.92-1.03,2.62-2.37,2.32-2.86-.63-4.64-3.07-5.09-5.74-1.1-6.44-7.42-1.14-10.03-4.86-2.14-3.06,6.76-5.8,10.43-5.68ZM113.69,83.66c9.51-.97,24.2,1.22,30.1,9.81l.54,5.05-4.39.09c-1.61-.64-2.42.36-3.56.95-3.79,1.97-7.68-.92-10.18-3.91-3.98-4.78-16.61-7.08-22.32-6.15l-10.39,1.7-2.58-.31c2.49-3.89,18.36-6.78,22.79-7.24ZM89.24,110.46c6.53-5.68,14.41-8.87,22.91-10.64,4.24-.88,8.35-.56,12.69-.12,1.72.18,3.37.27,4.58,1.63,5.93,6.67,1.39,8.49,4.57,11.18l2.9,4.08c-1.61,2.63-4.53,2.44-7.42,1.62l.64,4.43-2.43,3.68c-3.51,2.63-3.41-2.52-7.85-2.52-3.03,0-6.66-.49-8.52-3.2-.98-1.42-1.35-3.77-2.12-5.34-2.41-1.55-5.56-.41-7.08,1.96l-3.72-1.74-8.54-1.96-1.43-1.23.82-1.85Z",
    "M104.87,108.25c6.32-1.69,12-2.07,18.51-1.28.96-3.74-7.99-4.48-10.17-4.29-6.42.58-12.93,3.07-18.05,6.43l-.63,1.17,1.51.33,8.83-2.36Z",
];
/// Draws the mark centered on `center`, scaled to fit within a
/// `max_size` square, filled with `color`.
fn draw_mark(scene: &mut Scene, center: Point, max_size: f64, color: Color) {
    let scale = max_size / CHOOSER_MARK_VIEWBOX;
    let half = CHOOSER_MARK_VIEWBOX * scale * 0.5;
    let xf = Affine::translate((center.x - half, center.y - half)) * Affine::scale(scale);
    for d in CHOOSER_MARK_PATHS {
        if let Ok(path) = BezPath::from_svg(d) {
            scene.fill(Fill::NonZero, xf, color, None, &path);
        }
    }
}

/// The chooser's single centered column's x-span — narrow and fixed,
/// not stretched to the pane's own width, same as a launcher's content
/// block.
fn chooser_col_x(r: Rect) -> (f64, f64) {
    let b = body(r);
    let w = ui_px(CHOOSER_COL_W).min((b.width() - ui_px(CHOOSER_PAD) * 2.0).max(1.0));
    let x0 = b.x0 + (b.width() - w) * 0.5;
    (x0, x0 + w)
}
/// Total content height — independent of pane width now that the
/// column itself has a fixed width (only wraps to something narrower
/// when the pane is too small for it).
/// Height of the "OPEN DOCUMENTS" section — 0 (not shown at all) when
/// nothing's open, unlike "RECENT DOCUMENTS" below it, which always
/// shows. Showing an "OPEN DOCUMENTS" header with nothing under it on
/// every fresh, still-empty pane would be pure noise; "RECENT DOCUMENTS"
/// stays always-visible because it's a real complaint fix (see its own
/// call sites) — this one's new, no such history to preserve.
fn chooser_open_section_h(n_open: usize) -> f64 {
    if n_open == 0 {
        0.0
    } else {
        ui_px(CHOOSER_SECTION_GAP) + ui_px(CHOOSER_HEADER_H) + n_open as f64 * ui_px(CHOOSER_ROW_H)
    }
}
fn chooser_content_h(n_open: usize, n_recent: usize) -> f64 {
    ui_px(CHOOSER_PAD)
        + ui_px(CHOOSER_MARK_SIZE) + ui_px(CHOOSER_VERSION_BLOCK) + ui_px(CHOOSER_MARK_GAP)
        + ui_px(CHOOSER_HEADER_H)
        + CHOOSER_ACTIONS as f64 * ui_px(CHOOSER_ACTION_ROW_H)
        + chooser_open_section_h(n_open)
        + ui_px(CHOOSER_SECTION_GAP)
        + ui_px(CHOOSER_HEADER_H)
        + n_recent.max(1) as f64 * ui_px(CHOOSER_ROW_H)
        + ui_px(CHOOSER_PAD)
}
/// Where the chooser's whole content block starts, vertically — centered
/// in the pane when it's shorter than the available height (the common
/// case), or pinned to the top and scrolled once it overflows.
fn chooser_content_top(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    let b = body(r);
    let content_h = chooser_content_h(n_open, n_recent);
    if content_h <= b.height() {
        b.y0 + (b.height() - content_h) * 0.5
    } else {
        b.y0 - scroll
    }
}
/// Center point of the mark image, top-center above "GET STARTED".
fn chooser_mark_center(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> Point {
    let (x0, x1) = chooser_col_x(r);
    let y = chooser_content_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_PAD) + ui_px(CHOOSER_MARK_SIZE) * 0.5;
    Point::new((x0 + x1) * 0.5, y)
}
/// Baseline of the "Amalith v.x" line directly under the mark.
fn chooser_version_baseline_y(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_content_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_PAD) + ui_px(CHOOSER_MARK_SIZE) + ui_px(14.0)
}
/// Top of the "GET STARTED" section, below the mark and version line.
fn chooser_actions_top(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_content_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_PAD) + ui_px(CHOOSER_MARK_SIZE) + ui_px(CHOOSER_VERSION_BLOCK) + ui_px(CHOOSER_MARK_GAP)
}
/// Baseline of the "GET STARTED" section header.
fn chooser_actions_header_y(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_actions_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H) * 0.5 + ui_px(4.0)
}
/// Action row `i`'s rect (`i` < `CHOOSER_ACTIONS`).
fn chooser_action_row(r: Rect, i: usize, n_open: usize, n_recent: usize, scroll: f64) -> Rect {
    let (x0, x1) = chooser_col_x(r);
    let top = chooser_actions_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H);
    let y = top + i as f64 * ui_px(CHOOSER_ACTION_ROW_H);
    Rect::new(x0, y, x1, y + ui_px(CHOOSER_ACTION_ROW_H))
}
/// Top of the "OPEN DOCUMENTS" section (meaningless when `n_open == 0`
/// — callers must check that first).
fn chooser_open_section_top(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_actions_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H)
        + CHOOSER_ACTIONS as f64 * ui_px(CHOOSER_ACTION_ROW_H)
        + ui_px(CHOOSER_SECTION_GAP)
}
/// Baseline of the "OPEN DOCUMENTS" header.
fn chooser_open_header_y(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_open_section_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H) * 0.5 + ui_px(4.0)
}
/// Open-document row `i`'s rect, below the "OPEN DOCUMENTS" header.
fn chooser_open_row(r: Rect, i: usize, n_open: usize, n_recent: usize, scroll: f64) -> Rect {
    let (x0, x1) = chooser_col_x(r);
    let top = chooser_open_section_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H);
    let y = top + i as f64 * ui_px(CHOOSER_ROW_H);
    Rect::new(x0, y, x1, y + ui_px(CHOOSER_ROW_H) - ui_px(4.0))
}
/// Top of the "RECENT DOCUMENTS" section, right below "OPEN DOCUMENTS"
/// (or right below the action rows, when there's no open-documents
/// section to begin with) — shared by its header and its own rows so
/// they can't drift apart.
fn chooser_recent_section_top(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_open_section_top(r, n_open, n_recent, scroll) + chooser_open_section_h(n_open)
}
/// Baseline of the "RECENT DOCUMENTS" header.
fn chooser_recent_header_y(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_recent_section_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H) * 0.5 + ui_px(4.0)
}
/// Recent-document row `i`'s rect, below the header.
fn chooser_recent_row(r: Rect, i: usize, n_open: usize, n_recent: usize, scroll: f64) -> Rect {
    let (x0, x1) = chooser_col_x(r);
    let top = chooser_recent_section_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_HEADER_H);
    let y = top + i as f64 * ui_px(CHOOSER_ROW_H);
    Rect::new(x0, y, x1, y + ui_px(CHOOSER_ROW_H) - ui_px(4.0))
}
/// A section header: small-caps dim label, then a rule filling the rest
/// of the column's width — same style for "GET STARTED" and "RECENT
/// DOCUMENTS" so the two sections read as one unified list.
fn paint_chooser_section_header(scene: &mut Scene, text: &mut TextContext, theme: &Theme, x0: f64, x1: f64, baseline_y: f64, label: &str) {
    let w = text.measure(label, 10.5);
    text.draw(scene, label, 10.5, theme.text_dim, x0, baseline_y);
    let rule_x0 = x0 + w + ui_px(10.0);
    scene.stroke(
        &Stroke::new(1.0), Affine::IDENTITY, theme.border, None,
        &vello::kurbo::Line::new((rule_x0, baseline_y - ui_px(4.0)), (x1, baseline_y - ui_px(4.0))),
    );
}

/// The chooser uses a deliberately spare, unboxed glyph set. These actions
/// sit in a launcher list, so enclosing every mark in a document or terminal
/// outline made the left edge visually heavy.
const CHOOSER_ICON_STROKE: f64 = 1.75;

fn stroke_icon(scene: &mut Scene, color: Color, width: f64, shape: &impl vello::kurbo::Shape) {
    scene.stroke(
        &Stroke::new(ui_px(width))
            .with_caps(vello::kurbo::Cap::Round)
            .with_join(vello::kurbo::Join::Round),
        Affine::IDENTITY,
        color,
        None,
        shape,
    );
}

/// Compact bare plus used by the pane tab-strip and the new-document action.
fn icon_plus(scene: &mut Scene, c: Point, radius: f64, color: Color) {
    stroke_icon(
        scene,
        color,
        CHOOSER_ICON_STROKE,
        &vello::kurbo::Line::new((c.x - radius, c.y), (c.x + radius, c.y)),
    );
    stroke_icon(
        scene,
        color,
        CHOOSER_ICON_STROKE,
        &vello::kurbo::Line::new((c.x, c.y - radius), (c.x, c.y + radius)),
    );
}

fn icon_terminal(scene: &mut Scene, box_: Rect, color: Color) {
    let frame = Rect::new(
        box_.x0 + box_.width() * 0.06,
        box_.y0 + box_.height() * 0.11,
        box_.x1 - box_.width() * 0.06,
        box_.y1 - box_.height() * 0.11,
    );
    stroke_icon(scene, color, CHOOSER_ICON_STROKE, &frame.to_rounded_rect(ui_px(2.5)));
    let divider_y = frame.y0 + frame.height() * 0.29;
    stroke_icon(
        scene,
        color,
        CHOOSER_ICON_STROKE,
        &vello::kurbo::Line::new((frame.x0 + ui_px(1.9), divider_y), (frame.x1 - ui_px(1.9), divider_y)),
    );
    let (cx, cy) = (frame.x0 + frame.width() * 0.34, frame.y0 + frame.height() * 0.65);
    let a = frame.height() * 0.14;
    let mut chevron = BezPath::new();
    chevron.move_to((cx - a, cy - a));
    chevron.line_to((cx + a, cy));
    chevron.line_to((cx - a, cy + a));
    stroke_icon(scene, color, CHOOSER_ICON_STROKE, &chevron);
    let uy = cy + a * 1.15;
    stroke_icon(
        scene,
        color,
        CHOOSER_ICON_STROKE,
        &vello::kurbo::Line::new((cx + a * 0.85, uy), (cx + a * 2.75, uy)),
    );
}
fn icon_import(scene: &mut Scene, box_: Rect, color: Color) {
    let cx = box_.center().x;
    let (ay0, ay1) = (box_.y0 + box_.height() * 0.13, box_.y0 + box_.height() * 0.58);
    let arm = box_.width() * 0.17;
    let mut arrow = BezPath::new();
    arrow.move_to((cx, ay0));
    arrow.line_to((cx, ay1));
    arrow.move_to((cx - arm, ay1 - arm));
    arrow.line_to((cx, ay1));
    arrow.line_to((cx + arm, ay1 - arm));
    stroke_icon(scene, color, CHOOSER_ICON_STROKE, &arrow);
    let tray_y = box_.y1 - box_.height() * 0.14;
    let tray_half = box_.width() * 0.36;
    let tray_rise = box_.height() * 0.16;
    let mut tray = BezPath::new();
    tray.move_to((cx - tray_half, tray_y - tray_rise));
    tray.line_to((cx - tray_half, tray_y));
    tray.line_to((cx + tray_half, tray_y));
    tray.line_to((cx + tray_half, tray_y - tray_rise));
    stroke_icon(scene, color, CHOOSER_ICON_STROKE, &tray);
}
/// Paints one "GET STARTED" action row: icon, label, hover highlight —
/// a plain list row, not a bordered box (see the module doc comment for
/// why). `kind` selects the icon (0 = New Document, 1 = Terminal,
/// 2 = Import).
fn paint_chooser_action_row(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, kind: usize, hover: bool) {
    if hover {
        // A translucent light overlay rather than `theme.strip_active`
        // (an opaque near-match for the pane's own background, so it
        // barely showed at all) — reads as a clear full-width bar
        // regardless of exactly what's painted underneath.
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.text.with_alpha(0.09), None, &r.to_rounded_rect(ui_px(6.0)));
    }
    let ink = if hover { theme.text } else { theme.text_dim };
    let icon_c = Point::new(r.x0 + ui_px(14.0), r.center().y);
    let icon_box = Rect::new(icon_c.x - ui_px(9.0), icon_c.y - ui_px(9.0), icon_c.x + ui_px(9.0), icon_c.y + ui_px(9.0));
    match kind {
        0 => icon_plus(scene, icon_c, ui_px(7.0), ink),
        1 => icon_terminal(scene, icon_box, ink),
        _ => icon_import(scene, icon_box, ink),
    }
    text.draw(scene, label, 13.0, theme.text, r.x0 + ui_px(32.0), r.center().y + ui_px(4.5));
}

// --- Compact "New Document" overlay ---------------------------------
// A command-palette-styled card (Name / Width / Height / Color Mode
// only — no bleed/raster/preview/art gallery, see `docs/canvas-panes.md`)
// drawn centered on whichever pane spawned it, replacing the old
// full-page dialog (`newdoc.rs`, kept but no longer the default entry
// point) as the chooser's "Create a new Document" action.
pub(super) struct QuickNewDoc {
    pub form: newdoc::NewDocForm,
    /// The pane this overlay is centered on — looked up fresh from the
    /// live layout every frame/hit-test, so a resize or further split
    /// while it's open keeps it correctly positioned.
    pane: PaneId,
}

const QND_W: f64 = 320.0;
const QND_PAD: f64 = 16.0;
const QND_LABEL_H: f64 = 16.0;
const QND_FIELD_H: f64 = 28.0;
const QND_ROW_GAP: f64 = 10.0;
const QND_BTN_H: f64 = 30.0;
const QND_BTN_W: f64 = 84.0;
const QND_TITLE_H: f64 = 34.0;
/// Name, Width/Unit, Height/Orientation, Artboards, Bleed, Color Mode,
/// Raster Effects, Start With. Preview Mode isn't offered here — it's
/// rarely touched after a document's created, and stays reachable
/// afterward through its own menu.
const QND_ROWS: usize = 8;

/// The overlay's own card rect, centered on `pane_r`.
fn quick_newdoc_rect(pane_r: Rect) -> Rect {
    let w = ui_px(QND_W);
    let h = ui_px(
        QND_PAD * 2.0
            + QND_TITLE_H
            + (QND_LABEL_H + QND_FIELD_H + QND_ROW_GAP) * QND_ROWS as f64
            + QND_BTN_H,
    );
    let c = pane_r.center();
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, c.x + w / 2.0, c.y + h / 2.0)
}
fn qnd_row(card: Rect, i: usize) -> Rect {
    let y = card.y0 + ui_px(QND_PAD + QND_TITLE_H)
        + i as f64 * ui_px(QND_LABEL_H + QND_FIELD_H + QND_ROW_GAP)
        + ui_px(QND_LABEL_H);
    Rect::new(card.x0 + ui_px(QND_PAD), y, card.x1 - ui_px(QND_PAD), y + ui_px(QND_FIELD_H))
}
fn qnd_name_rect(card: Rect) -> Rect { qnd_row(card, 0) }
fn qnd_width_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 1);
    Rect::new(r.x0, r.y0, r.x0 + r.width() * 0.6 - ui_px(6.0), r.y1)
}
fn qnd_unit_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 1);
    Rect::new(r.x0 + r.width() * 0.6 + ui_px(6.0), r.y0, r.x1, r.y1)
}
fn qnd_height_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 2);
    Rect::new(r.x0, r.y0, r.x0 + r.width() * 0.6 - ui_px(6.0), r.y1)
}
fn qnd_orient_rects(card: Rect) -> (Rect, Rect) {
    let r = qnd_row(card, 2);
    let x0 = r.x0 + r.width() * 0.6 + ui_px(6.0);
    let half = (r.x1 - x0 - ui_px(4.0)) * 0.5;
    let portrait = Rect::new(x0, r.y0, x0 + half, r.y1);
    let landscape = Rect::new(x0 + half + ui_px(4.0), r.y0, r.x1, r.y1);
    (portrait, landscape)
}
fn qnd_ab_minus_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 3);
    Rect::new(r.x0, r.y0, r.x0 + ui_px(28.0), r.y1)
}
fn qnd_ab_plus_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 3);
    Rect::new(r.x1 - ui_px(28.0), r.y0, r.x1, r.y1)
}
fn qnd_ab_count_rect(card: Rect) -> Rect {
    let r = qnd_row(card, 3);
    Rect::new(r.x0 + ui_px(32.0), r.y0, r.x1 - ui_px(32.0), r.y1)
}
fn qnd_bleed_rect(card: Rect) -> Rect { qnd_row(card, 4) }
fn qnd_color_rect(card: Rect) -> Rect { qnd_row(card, 5) }
fn qnd_raster_rect(card: Rect) -> Rect { qnd_row(card, 6) }
fn qnd_start_layer_rect(card: Rect) -> Rect { qnd_row(card, 7) }
/// The field a dropdown (`newdoc::Menu`) opens from.
fn qnd_menu_trigger_rect(card: Rect, menu: newdoc::Menu) -> Rect {
    match menu {
        newdoc::Menu::Unit => qnd_unit_rect(card),
        newdoc::Menu::Color => qnd_color_rect(card),
        newdoc::Menu::Raster => qnd_raster_rect(card),
        newdoc::Menu::StartLayer => qnd_start_layer_rect(card),
    }
}
/// One rect per option in `menu`'s dropdown, stacked below its trigger —
/// same popup shape as `prefs.rs`'s own preset dropdown.
fn qnd_menu_item_rects(card: Rect, menu: newdoc::Menu) -> Vec<Rect> {
    let trigger = qnd_menu_trigger_rect(card, menu);
    let n = match menu {
        newdoc::Menu::Unit => newdoc::UNITS.len(),
        newdoc::Menu::Color => newdoc::COLORS.len(),
        newdoc::Menu::Raster => newdoc::RASTERS.len(),
        newdoc::Menu::StartLayer => newdoc::START_LAYERS.len(),
    };
    (0..n)
        .map(|i| {
            let y = trigger.y1 + ui_px(2.0) + i as f64 * ui_px(24.0);
            Rect::new(trigger.x0, y, trigger.x1, y + ui_px(24.0))
        })
        .collect()
}
fn qnd_menu_labels(menu: newdoc::Menu) -> Vec<&'static str> {
    match menu {
        newdoc::Menu::Unit => newdoc::UNITS.iter().map(|u| newdoc::unit_label(*u)).collect(),
        newdoc::Menu::Color => newdoc::COLORS.iter().map(|c| newdoc::color_label(*c)).collect(),
        newdoc::Menu::Raster => newdoc::RASTERS.iter().map(|r| newdoc::raster_label(*r)).collect(),
        newdoc::Menu::StartLayer => newdoc::START_LAYERS.iter().map(|k| newdoc::start_layer_label(*k)).collect(),
    }
}
/// Small filled down-chevron — a dropdown's own affordance, same idea as
/// `prefs.rs`'s private `tri()` (not shared, so redrawn locally here).
fn qnd_chevron(scene: &mut Scene, c: Point, color: Color) {
    let d = ui_px(3.0);
    let mut p = BezPath::new();
    p.move_to((c.x - d, c.y - d * 0.6));
    p.line_to((c.x + d, c.y - d * 0.6));
    p.line_to((c.x, c.y + d * 0.6));
    p.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &p);
}
fn qnd_buttons_row(card: Rect) -> Rect {
    let y = card.y1 - ui_px(QND_PAD) - ui_px(QND_BTN_H);
    Rect::new(card.x0 + ui_px(QND_PAD), y, card.x1 - ui_px(QND_PAD), y + ui_px(QND_BTN_H))
}
fn qnd_create_rect(card: Rect) -> Rect {
    let r = qnd_buttons_row(card);
    Rect::new(r.x1 - ui_px(QND_BTN_W), r.y0, r.x1, r.y1)
}
fn qnd_cancel_rect(card: Rect) -> Rect {
    let r = qnd_buttons_row(card);
    let create = qnd_create_rect(card);
    Rect::new(create.x0 - ui_px(8.0) - ui_px(QND_BTN_W), r.y0, create.x0 - ui_px(8.0), r.y1)
}

/// Small line-art portrait/landscape glyph for the Orientation toggle.
fn icon_orientation(scene: &mut Scene, r: Rect, portrait: bool, color: Color) {
    let (w, h) = if portrait { (r.width() * 0.36, r.height() * 0.64) } else { (r.width() * 0.64, r.height() * 0.36) };
    let c = r.center();
    let rr = Rect::new(c.x - w * 0.5, c.y - h * 0.5, c.x + w * 0.5, c.y + h * 0.5);
    scene.stroke(&Stroke::new(ui_px(1.4)), Affine::IDENTITY, color, None, &rr);
}

fn paint_quick_newdoc(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    card: Rect,
    form: &mut newdoc::NewDocForm,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_bg, None, &card.to_rounded_rect(ui_px(6.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.accent, None, &card.to_rounded_rect(ui_px(6.0)));
    text.draw(
        scene, "New Document", 13.5, theme.text,
        card.x0 + ui_px(QND_PAD), card.y0 + ui_px(QND_PAD) + ui_px(14.0),
    );

    let field_bg = |scene: &mut Scene, r: Rect, on: bool| {
        scene.fill(Fill::NonZero, Affine::IDENTITY, if on { theme.strip_active } else { theme.bg }, None, &r.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, if on { theme.accent } else { theme.border }, None, &r.to_rounded_rect(ui_px(4.0)));
    };
    let field_label = |scene: &mut Scene, text: &mut TextContext, r: Rect, label: &str| {
        text.draw(scene, label, 10.5, theme.text_dim, r.x0 + ui_px(2.0), r.y0 - ui_px(4.0));
    };
    // A real *button* (Orientation, Artboard −/+) rather than a text
    // field or dropdown trigger — sharp corners, same fill/border
    // convention as `widgets::button`, distinct from `field_bg`'s
    // rounded, subtler field look.
    let button_bg = |scene: &mut Scene, r: Rect, selected: bool| {
        scene.fill(Fill::NonZero, Affine::IDENTITY, if selected { theme.accent } else { theme.strip_active }, None, &r);
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.text_dim.with_alpha(0.6), None, &r);
    };

    let focused = form.focused();
    let name_r = qnd_name_rect(card);
    field_label(scene, text, name_r, "Name");
    field_bg(scene, name_r, false);
    form.name.paint(scene, text, theme, name_r, "Untitled-1", caret_on && focused == Some(newdoc::Field::Name));

    let width_r = qnd_width_rect(card);
    field_label(scene, text, width_r, "Width");
    field_bg(scene, width_r, false);
    form.width.paint(scene, text, theme, width_r, "", caret_on && focused == Some(newdoc::Field::Width));

    let unit_r = qnd_unit_rect(card);
    field_bg(scene, unit_r, form.open_menu == Some(newdoc::Menu::Unit));
    let ul = newdoc::unit_label(form.unit);
    text.draw(scene, ul, 12.0, theme.text, unit_r.x0 + ui_px(8.0), unit_r.y0 + unit_r.height() * 0.5 + ui_px(4.0));
    qnd_chevron(scene, Point::new(unit_r.x1 - ui_px(12.0), unit_r.center().y), theme.text_dim);

    let height_r = qnd_height_rect(card);
    field_label(scene, text, height_r, "Height");
    field_bg(scene, height_r, false);
    form.height.paint(scene, text, theme, height_r, "", caret_on && focused == Some(newdoc::Field::Height));

    let (portrait_r, landscape_r) = qnd_orient_rects(card);
    let is_portrait = form.portrait();
    field_label(scene, text, portrait_r, "Orientation");
    button_bg(scene, portrait_r, is_portrait);
    icon_orientation(scene, portrait_r, true, if is_portrait { theme.on_accent } else { theme.text_dim });
    button_bg(scene, landscape_r, !is_portrait);
    icon_orientation(scene, landscape_r, false, if !is_portrait { theme.on_accent } else { theme.text_dim });

    let ab_minus = qnd_ab_minus_rect(card);
    let ab_plus = qnd_ab_plus_rect(card);
    let ab_count = qnd_ab_count_rect(card);
    field_label(scene, text, ab_count, "Artboards");
    button_bg(scene, ab_minus, false);
    field_bg(scene, ab_count, false);
    button_bg(scene, ab_plus, false);
    text.draw(scene, "−", 13.0, theme.text, ab_minus.center().x - ui_px(3.5), ab_minus.center().y + ui_px(4.5));
    text.draw(scene, "+", 13.0, theme.text, ab_plus.center().x - ui_px(4.0), ab_plus.center().y + ui_px(4.5));
    let ab_str = form.artboards.to_string();
    let abw = text.measure(&ab_str, 12.0);
    text.draw(scene, &ab_str, 12.0, theme.text, ab_count.center().x - abw * 0.5, ab_count.center().y + ui_px(4.0));

    let bleed_r = qnd_bleed_rect(card);
    field_label(scene, text, bleed_r, "Bleed (all sides)");
    field_bg(scene, bleed_r, false);
    form.bleed[0].paint(scene, text, theme, bleed_r, "0", caret_on && matches!(focused, Some(newdoc::Field::BleedTop | newdoc::Field::BleedBottom | newdoc::Field::BleedLeft | newdoc::Field::BleedRight)));

    let color_r = qnd_color_rect(card);
    field_label(scene, text, color_r, "Color Mode");
    field_bg(scene, color_r, form.open_menu == Some(newdoc::Menu::Color));
    let cl = newdoc::color_label(form.color_mode);
    text.draw(scene, cl, 12.0, theme.text, color_r.x0 + ui_px(8.0), color_r.y0 + color_r.height() * 0.5 + ui_px(4.0));
    qnd_chevron(scene, Point::new(color_r.x1 - ui_px(12.0), color_r.center().y), theme.text_dim);

    let raster_r = qnd_raster_rect(card);
    field_label(scene, text, raster_r, "Raster Effects");
    field_bg(scene, raster_r, form.open_menu == Some(newdoc::Menu::Raster));
    let rl = newdoc::raster_label(form.raster);
    text.draw(scene, rl, 12.0, theme.text, raster_r.x0 + ui_px(8.0), raster_r.y0 + raster_r.height() * 0.5 + ui_px(4.0));
    qnd_chevron(scene, Point::new(raster_r.x1 - ui_px(12.0), raster_r.center().y), theme.text_dim);

    let start_layer_r = qnd_start_layer_rect(card);
    field_label(scene, text, start_layer_r, "Start With");
    field_bg(scene, start_layer_r, form.open_menu == Some(newdoc::Menu::StartLayer));
    let sl = newdoc::start_layer_label(form.start_layer);
    text.draw(scene, sl, 12.0, theme.text, start_layer_r.x0 + ui_px(8.0), start_layer_r.y0 + start_layer_r.height() * 0.5 + ui_px(4.0));
    qnd_chevron(scene, Point::new(start_layer_r.x1 - ui_px(12.0), start_layer_r.center().y), theme.text_dim);

    crate::widgets::button(scene, text, theme, qnd_cancel_rect(card), "Cancel", false);
    crate::widgets::button(scene, text, theme, qnd_create_rect(card), "Create", true);

    // The open dropdown (if any) paints last, on top of everything else
    // in the card — same popup style as `prefs.rs`'s own preset dropdown.
    if let Some(menu) = form.open_menu {
        let items = qnd_menu_item_rects(card, menu);
        let labels = qnd_menu_labels(menu);
        if let (Some(first), Some(last)) = (items.first(), items.last()) {
            let box_ = Rect::new(first.x0, first.y0, first.x1, last.y1);
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_bg, None, &box_.to_rounded_rect(ui_px(4.0)));
            scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.accent, None, &box_.to_rounded_rect(ui_px(4.0)));
            let current = match menu {
                newdoc::Menu::Unit => newdoc::UNITS.iter().position(|u| *u == form.unit),
                newdoc::Menu::Color => newdoc::COLORS.iter().position(|c| *c == form.color_mode),
                newdoc::Menu::Raster => newdoc::RASTERS.iter().position(|r| *r == form.raster),
                newdoc::Menu::StartLayer => newdoc::START_LAYERS.iter().position(|k| *k == form.start_layer),
            };
            for (i, (r, label)) in items.iter().zip(labels.iter()).enumerate() {
                if current == Some(i) {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent.multiply_alpha(0.18), None, r);
                }
                text.draw(scene, label, 12.0, theme.text, r.x0 + ui_px(10.0), r.y0 + r.height() * 0.5 + ui_px(4.0));
            }
        }
    }
}

impl App {
    pub(super) fn mux_area(&self) -> Rect {
        let (left, right) = self.canvas_full_x_span();
        let (_, height) = self.main_logical_size().unwrap_or((1280., 800.));
        // `metric_chrome_top()` includes the now-retired global document
        // tab strip's own height — skip straight past the options bar
        // instead, or every pane's real tab strip sits below a second,
        // empty bar with nothing in it.
        let top = metric_app_bar_h() + metric_opt_bar_h();
        Rect::new(left, top, right, height - ui_px(28.))
    }
    pub(super) fn mux_focused_rect(&self) -> Option<Rect> {
        self.mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .find(|(p, _)| p.id == self.mux.model.focused)
            .map(|(_, r)| r)
    }
    pub(super) fn mux_document_rect(&self) -> Option<Rect> {
        if !self.mux.model.enabled() {
            return None;
        }
        let (p, r) = self
            .mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .find(|(p, _)| p.id == self.mux.model.focused)?;
        Some(if matches!(p.active_tab().content, TabContent::Document(_)) {
            body(r)
        } else {
            Rect::new(r.x0, r.y0, r.x0, r.y0)
        })
    }
    /// A tab's display label. For a Document: `Name* @ zoom% (CMYK)` —
    /// the same information the old (now-retired) global tab strip's own
    /// `App::tab_label` showed, minus its preview-mode suffix. `Terminal`,
    /// or "New Tab" for a chooser.
    fn mux_tab_label(&self, tab: &crate::multiplexer::Tab) -> String {
        match tab.content {
            TabContent::Document(id) => {
                let Some(i) = self.mux_doc_index(id) else {
                    return "Untitled".to_string();
                };
                // The doc-index bookkeeping (`self.active`/`self.doc`)
                // tracks whichever tab is currently live on screen, which
                // is always the focused pane's active tab — everything
                // else's up-to-date zoom lives on the tab itself instead
                // (`mux_save_view` parks it there on switch-away).
                let live = self
                    .mux
                    .model
                    .pane(self.mux.model.focused)
                    .is_some_and(|p| p.active_tab().id == tab.id);
                let (editor, zoom) = if live {
                    (&self.doc.editor, self.doc.view.zoom)
                } else {
                    (&self.tabs[i].editor, tab.view.zoom)
                };
                let doc = editor.document();
                let name = doc.metadata.title.as_deref().unwrap_or("Untitled");
                let dirty = if editor.is_dirty() { "*" } else { "" };
                let color = match doc.settings.color_mode {
                    amalith_core::ColorMode::Cmyk => "CMYK",
                    amalith_core::ColorMode::Rgb => "RGB",
                };
                format!("{name}{dirty} @ {} ({color})", canvas::zoom_percent_label(zoom))
            }
            TabContent::Terminal => "Terminal".to_string(),
            TabContent::Chooser => "New Tab".to_string(),
        }
    }
    /// Where a tab being dragged over `pane` (occupying screen rect `r`)
    /// would land if dropped right now — the insertion index into that
    /// pane's tab list. Hovering directly over the tab strip snaps to
    /// whichever chip-gap the pointer is nearest (left half of a chip ->
    /// before it); hovering the pane's body instead (not the strip
    /// itself) falls back to "append at the end", same as dropping on an
    /// empty pane.
    pub(super) fn mux_tab_drop_index(&mut self, pane_id: PaneId, r: Rect) -> usize {
        let Some(labels) = self
            .mux
            .model
            .pane(pane_id)
            .map(|p| p.tabs.iter().map(|t| self.mux_tab_label(t)).collect::<Vec<_>>())
        else {
            return 0;
        };
        let n = labels.len();
        let strip = header(r);
        if !strip.contains(self.pointer) {
            return n;
        }
        let chips = pane_tab_chips(&mut self.text, &labels, strip);
        for (i, (whole, _)) in chips.iter().enumerate() {
            if self.pointer.x < whole.x0 + whole.width() * 0.5 {
                return i;
            }
        }
        n
    }
    pub(super) fn mux_save_view(&mut self) {
        if self.mux.switching {
            return;
        }
        let Some(origin) = self.mux.origin else {
            return;
        };
        let focused = self.mux.model.focused;
        if let Some(tab) = self.mux.model.active_tab_mut(focused) {
            if tab.content == TabContent::Document(self.doc.id) {
                tab.view = self.doc.view;
                tab.view.pan -= origin.to_vec2();
            }
        }
    }
    /// Parks the focused pane's active tab's live terminal (if it has
    /// one) under that tab's own id, so switching away from it doesn't
    /// kill the shell — it picks back up exactly where it left off next
    /// time that tab becomes active again.
    fn mux_park_terminal(&mut self) {
        let focused = self.mux.model.focused;
        let Some(tab) = self.mux.model.active_tab_mut(focused) else {
            return;
        };
        if tab.content != TabContent::Terminal {
            return;
        }
        let tab_id = tab.id;
        if let Some(mut t) = self.terminal.take() {
            t.focused = false;
            self.mux.terminals.insert(tab_id, t);
        }
    }
    /// Loads `pane`'s active tab's content into `App::doc`/`App::terminal`
    /// — the live state a Document/Terminal tab's content actually lives
    /// in while it's the one on screen (see the module doc comment).
    /// Called whenever the focused pane changes, the active tab within it
    /// changes, or a tab closes out from under it.
    fn mux_rebind(&mut self, pane: PaneId) {
        self.mux.switching = true;
        let Some((p, r)) = self
            .mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .find(|(p, _)| p.id == pane)
        else {
            self.mux.switching = false;
            return;
        };
        let tab = p.active_tab().clone();
        match tab.content {
            TabContent::Document(doc) => {
                if let Some(i) = self.mux_doc_index(doc) {
                    self.switch_to(i);
                }
                self.doc.view = tab.view;
                self.doc.view.pan += body(r).origin().to_vec2();
                self.mux.origin = Some(body(r).origin());
            }
            TabContent::Terminal => {
                self.terminal = self.mux.terminals.remove(&tab.id);
                if let Some(t) = &mut self.terminal {
                    t.focused = true;
                }
                self.mux.origin = None;
                self.layers_new_menu = false;
                self.layers_blend_menu = false;
            }
            TabContent::Chooser => {
                self.mux.origin = None;
                self.layers_new_menu = false;
                self.layers_blend_menu = false;
            }
        }
        self.drag = Drag::None;
        self.mux.switching = false;
    }
    /// Converts the focused pane's active tab into a reference to the
    /// current `App::doc` — called whenever a new/opened document
    /// becomes the live one (`add_doc`/`switch_to`) while mux is
    /// enabled, replacing whatever that tab held (typically a chooser,
    /// picking "Create a new Document"/"Import"/a recent file).
    pub(super) fn mux_bind_document(&mut self) {
        if self.mux.switching || !self.mux.model.enabled() {
            return;
        }
        self.mux_park_terminal();
        let focused = self.mux.model.focused;
        let origin = self
            .mux_focused_rect()
            .map(body)
            .map(|r| r.origin())
            .unwrap_or_default();
        if let Some(tab) = self.mux.model.active_tab_mut(focused) {
            tab.content = TabContent::Document(self.doc.id);
            tab.view = self.doc.view;
            tab.view.pan -= origin.to_vec2();
        }
        self.mux.origin = Some(origin);
    }
    fn mux_doc_index(&self, id: ObjectId) -> Option<usize> {
        (0..self.tabs.len()).find(|&i| {
            if i == self.active {
                self.doc.id == id
            } else {
                self.tabs[i].id == id
            }
        })
    }
    /// How many tabs, across every pane, currently reference document
    /// `id` — used to decide whether closing one of them should also
    /// close the underlying document (last reference) or just that
    /// pane's own view of it (shared with at least one other tab).
    fn mux_doc_ref_count(&self, id: ObjectId) -> usize {
        self.mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .flat_map(|(p, _)| p.tabs)
            .filter(|t| t.content == TabContent::Document(id))
            .count()
    }
    /// The chooser's "OPEN DOCUMENTS" section: every document already
    /// open *this session*, in any pane — so switching panes to see
    /// something you already have open doesn't mean digging for it.
    /// Empty (and the whole section hidden — see `chooser_open_section_h`)
    /// while nothing real has been created/opened yet.
    fn mux_open_docs(&self) -> Vec<(String, RecentPick)> {
        if self.boot_empty {
            return Vec::new();
        }
        (0..self.tabs.len()).map(|i| (self.tab_title(i), RecentPick::Open(i))).collect()
    }
    /// Focused pane's active tab is a document the user opened or created.
    /// False at boot (chooser over the construction-time sample), after
    /// closing the last document, and while the focused pane is a chooser
    /// or terminal — even if another pane still has a document.
    pub(super) fn document_open(&self) -> bool {
        if self.boot_empty {
            return false;
        }
        if !self.mux.model.enabled() {
            return true;
        }
        self.mux
            .model
            .pane(self.mux.model.focused)
            .is_some_and(|p| matches!(p.active_tab().content, TabContent::Document(_)))
    }
    /// The chooser's "RECENT DOCUMENTS" section: on-disk file history,
    /// skipping anything already represented in "OPEN DOCUMENTS" so a
    /// saved, still-open document doesn't appear twice.
    fn mux_recent_files(&self) -> Vec<(String, RecentPick)> {
        let mut open_paths: Vec<std::path::PathBuf> = Vec::new();
        if !self.boot_empty {
            for i in 0..self.tabs.len() {
                let path = if i == self.active { self.doc.file_path.clone() } else { self.tabs[i].file_path.clone() };
                if let Some(p) = path {
                    open_paths.push(p);
                }
            }
        }
        crate::recent::load()
            .into_iter()
            .filter(|p| !open_paths.contains(p))
            .map(|p| (home::display_name(&p), RecentPick::File(p)))
            .collect()
    }
    /// Acts on a "RECENT DOCUMENTS" click/⌘-digit pick — switches to an
    /// already-open document (binding it into the focused pane's active
    /// tab, replacing its chooser) or opens a file fresh from disk.
    fn mux_open_recent(&mut self, pick: &RecentPick) {
        match pick {
            RecentPick::Open(i) => {
                self.switch_to(*i);
                self.mux_bind_document();
                self.pending_fit = true;
            }
            RecentPick::File(path) => self.open_path(path),
        }
    }
    fn mux_focus(&mut self, id: PaneId) {
        if id == self.mux.model.focused {
            return;
        }
        self.pending_fit = false;
        self.mux_save_view();
        self.mux_park_terminal();
        self.mux.model.focused = id;
        self.mux_rebind(id);
        self.request_main_redraw();
    }
    /// Switches to a different tab within the same (already-focused)
    /// pane — clicking a tab chip that isn't the active one, or the
    /// resolution of a tab-chip press released before the drag
    /// threshold (a plain click; see `Drag::PendingTabDrag`).
    pub(super) fn mux_switch_tab(&mut self, pane: PaneId, tab_idx: usize) {
        let Some(p) = self.mux.model.pane(pane) else {
            return;
        };
        if tab_idx >= p.tabs.len() || tab_idx == p.active {
            return;
        }
        self.mux_save_view();
        self.mux_park_terminal();
        if let Some(p) = self.mux.model.pane_mut(pane) {
            p.active = tab_idx;
        }
        self.mux_rebind(pane);
        self.request_main_redraw();
    }
    /// Moves tab `tab_id` from `from_pane` to sit at `to_idx` within
    /// `target_pane` — the drop resolution of a tab drag
    /// (`Drag::DraggingTab`). Same pane in and out is a reorder (just
    /// reshuffles the pane's own tab list, via `reorder_tab`); a
    /// different `target_pane` is a real cross-pane move landing at that
    /// index (via `insert_tab_at`) instead of always appending. If the
    /// moved tab was the one on screen, focus follows it to
    /// `target_pane`; otherwise nothing else about either pane's current
    /// view changes.
    pub(super) fn mux_move_tab(
        &mut self,
        from_pane: PaneId,
        tab_id: crate::multiplexer::TabId,
        target_pane: PaneId,
        to_idx: usize,
    ) {
        let Some(from_idx) = self
            .mux
            .model
            .pane(from_pane)
            .and_then(|p| p.tabs.iter().position(|t| t.id == tab_id))
        else {
            return;
        };
        if from_pane == target_pane {
            self.mux.model.reorder_tab(from_pane, from_idx, to_idx);
            self.request_main_redraw();
            return;
        }
        let was_focused_active = from_pane == self.mux.model.focused
            && self.mux.model.pane(from_pane).is_some_and(|p| p.active == from_idx);
        if was_focused_active {
            // About to stop being the tab on screen (it's moving to a
            // different pane entirely) — park its terminal, if any, same
            // as every other such transition, so the move doesn't kill
            // the shell.
            self.mux_park_terminal();
        }
        let Some((tab, _)) = self.mux.model.take_tab(from_pane, from_idx) else {
            return;
        };
        self.mux.model.insert_tab_at(target_pane, to_idx, tab);
        if was_focused_active {
            self.mux.model.focused = target_pane;
            self.mux_rebind(target_pane);
        }
        self.request_main_redraw();
    }
    /// Adds a fresh chooser tab to the focused pane and switches to it —
    /// ⌘T / File ▸ New Tab, and the pane tab strip's own "+" button.
    /// Always a *new* tab; it never jumps straight to the New Document
    /// overlay — the user picks what goes in it from the chooser, same
    /// as any other pane.
    pub(super) fn mux_new_tab(&mut self) {
        let pane = self.mux.model.focused;
        self.mux_save_view();
        self.mux_park_terminal();
        if self.mux.model.add_tab(pane, TabContent::Chooser).is_some() {
            self.mux_rebind(pane);
            self.request_main_redraw();
        }
    }
    /// ⌘N / File ▸ New — jumps straight into creating a new document
    /// instead of making the user land on the chooser and pick "Create a
    /// new Document" themselves. Still opens a fresh tab first
    /// (`mux_new_tab`) so `create_from_quick_form`'s in-place `add_doc`
    /// (via `mux_bind_document`, which always overwrites the focused
    /// pane's *active* tab) has a disposable chooser tab to land on
    /// rather than clobbering whatever the pane was already showing.
    pub(super) fn mux_new_document(&mut self) {
        self.mux_new_tab();
        self.open_quick_new_doc();
    }
    /// Starts a terminal in the focused pane — the chooser's "Start a
    /// shell in this pane" card, and File ▸ Scripts ▸ Terminal /
    /// `toggle_terminal`. Replaces the active tab in place if it's a
    /// chooser (same "pick something for this tab" flow as New
    /// Document/Import); otherwise adds a new terminal tab alongside
    /// whatever else the pane already holds.
    pub(super) fn open_mux_terminal(&mut self) {
        let pane = self.mux.model.focused;
        let replace_active = self
            .mux
            .model
            .pane(pane)
            .is_some_and(|p| p.active_tab().content == TabContent::Chooser);
        self.mux_save_view();
        self.mux_park_terminal();
        if !replace_active {
            self.mux.model.add_tab(pane, TabContent::Chooser);
        }
        if let Some(tab) = self.mux.model.active_tab_mut(pane) {
            tab.content = TabContent::Terminal;
        }
        if self.terminal.is_none() {
            self.open_terminal();
        }
        if let Some(t) = &mut self.terminal {
            t.visible = true;
            t.focused = true;
        }
        self.mux.origin = None;
        self.request_main_redraw();
    }
    /// File ▸ Create New Split ▸ Split Right — same action as prefix+D.
    pub(super) fn mux_split_right(&mut self) {
        self.mux_split(crate::multiplexer::Axis::Horizontal);
    }
    /// File ▸ Create New Split ▸ Split Down — same action as prefix+⇧D.
    pub(super) fn mux_split_down(&mut self) {
        self.mux_split(crate::multiplexer::Axis::Vertical);
    }
    /// Splits the focused pane along `axis` — the new pane gets one
    /// fresh chooser tab and becomes focused. No-ops below a sane
    /// minimum size instead of creating an unusably small pane.
    fn mux_split(&mut self, axis: crate::multiplexer::Axis) {
        self.mux_save_view();
        let big_enough = self.mux_focused_rect().is_some_and(|r| match axis {
            crate::multiplexer::Axis::Horizontal => r.width() >= ui_px(320.),
            crate::multiplexer::Axis::Vertical => r.height() >= ui_px(240.),
        });
        if big_enough {
            if let Some(id) = self.mux.model.split(axis) {
                self.mux_focus(id);
            }
        }
    }
    /// Actually removes tab `tab_idx` from `pane` and rebinds
    /// `App::doc`/`App::terminal` if that was the one on screen. Doesn't
    /// touch the underlying global document list — `mux_close_tab` (the
    /// real entry point) decides whether this tab closing should also
    /// close the document itself first.
    fn mux_close_tab_now(&mut self, pane: PaneId, tab_idx: usize) {
        let Some(p) = self.mux.model.pane(pane) else {
            return;
        };
        let Some(tab) = p.tabs.get(tab_idx) else {
            return;
        };
        let (tab_id, content) = (tab.id, tab.content);
        let was_focused_active = pane == self.mux.model.focused && tab_idx == p.active;

        // A closed terminal tab really closes the shell (unlike the old
        // standalone terminal pane's "..." button, which only hid it) —
        // there's no "hide but keep alive" concept anymore now that
        // switching to a different tab already does exactly that.
        match content {
            TabContent::Terminal if was_focused_active => {
                if let Some(mut t) = self.terminal.take() {
                    t.terminate();
                }
            }
            TabContent::Terminal => {
                if let Some(mut t) = self.mux.terminals.remove(&tab_id) {
                    t.terminate();
                }
            }
            _ => {}
        }

        match self.mux.model.close_tab(pane, tab_idx) {
            crate::multiplexer::CloseOutcome::TabRemoved { active_changed: true } => {
                self.mux_rebind(pane);
            }
            crate::multiplexer::CloseOutcome::PaneRemoved { next_focus } => {
                if was_focused_active {
                    self.mux_rebind(next_focus);
                }
            }
            crate::multiplexer::CloseOutcome::RootReset => {
                if was_focused_active {
                    self.mux_rebind(pane);
                }
            }
            _ => {}
        }
        self.request_main_redraw();
    }
    /// Closes tab `tab_idx` in `pane` — the shared entry point for a
    /// tab's × button, ⌘W, and prefix+X (all "close a tab", scoped to
    /// whichever pane/tab is targeted). A document tab this is the last
    /// reference to prompts for unsaved changes exactly like the old
    /// global tab strip's × did; the pane-tree removal itself then
    /// happens reactively once that resolves (see `mux_tick`) — a
    /// cancelled prompt leaves the tab exactly as it was.
    pub(super) fn mux_close_tab(&mut self, pane: PaneId, tab_idx: usize) {
        let Some(p) = self.mux.model.pane(pane) else {
            return;
        };
        let Some(tab) = p.tabs.get(tab_idx) else {
            return;
        };
        if let TabContent::Document(id) = tab.content {
            if self.mux_doc_ref_count(id) <= 1 {
                if let Some(i) = self.mux_doc_index(id) {
                    self.request_close_tab(i);
                }
                return;
            }
        }
        self.mux_close_tab_now(pane, tab_idx);
    }
    /// ⌘W / prefix+X — always closes the active pane's active tab.
    pub(super) fn mux_close_active_tab(&mut self) {
        let pane = self.mux.model.focused;
        let Some(idx) = self.mux.model.pane(pane).map(|p| p.active) else {
            return;
        };
        self.mux_close_tab(pane, idx);
    }
    pub(super) fn mux_begin(&mut self) {
        if self.mux.model.enabled() {
            return;
        }
        let mut view = self.doc.view;
        let origin = body(self.mux_area()).origin();
        view.pan -= origin.to_vec2();
        self.mux.model.start(self.doc.id, view);
        // Boot always starts on the chooser, not the construction-time
        // placeholder document (see `App::boot_empty`) — the same
        // absorption `Home` used to need on its way out, now
        // unconditional since Home itself is hidden (see `App::new`'s
        // own doc comment). `self.terminal` is always `None` here — this
        // only ever runs once, from `App::new`, before any terminal
        // could exist.
        self.mux.model.pane_mut(0).unwrap().tabs[0].content = TabContent::Chooser;
        self.mux.origin = None;
        self.home = None;
    }
    /// Looks `event`'s physical key + held modifiers up against each of
    /// `wanted`'s own configured chord (Preferences ▸ Keyboard ▸ Pane
    /// Multiplexer), returning whichever one matches. Deliberately checks
    /// each wanted action's own binding directly rather than reverse-
    /// scanning the whole `action_keys` table for "whoever owns this
    /// chord" — `MuxSplitRight`'s default (plain D) collides with
    /// `DefaultPaints`'s, and a global first-match scan would resolve a
    /// prefix-mode "D" to the wrong one.
    fn mux_action(&self, event: &winit::event::KeyEvent, wanted: &[prefs::PrefAction]) -> Option<prefs::PrefAction> {
        let PhysicalKey::Code(code) = event.physical_key else {
            return None;
        };
        let chord = prefs::KeyChord {
            code,
            shift: self.shift_down,
            cmd: self.cmd_down,
            alt: self.alt_down,
        };
        wanted.iter().copied().find(|&act| {
            prefs::PrefAction::ALL
                .iter()
                .position(|a| *a == act)
                .and_then(|i| self.settings.action_keys[i])
                == Some(chord)
        })
    }
    pub(super) fn mux_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.mux.model.prefix {
            if !event.state.is_pressed() {
                return true;
            }
            if event.repeat {
                return true;
            }
            // A bare modifier key-down (pressing Shift to reach for
            // Prefix+Shift+D, say) is its own `KeyEvent` — physical key
            // Shift/Control/Alt/Super — not the chord's actual letter.
            // Letting it fall through to "no action matched" below would
            // cancel prefix mode before the real key ever arrives, which
            // is exactly what made holding Shift after the prefix look
            // like it cleared it.
            if matches!(
                event.physical_key,
                PhysicalKey::Code(
                    KeyCode::ShiftLeft
                        | KeyCode::ShiftRight
                        | KeyCode::ControlLeft
                        | KeyCode::ControlRight
                        | KeyCode::AltLeft
                        | KeyCode::AltRight
                        | KeyCode::SuperLeft
                        | KeyCode::SuperRight
                )
            ) {
                return true;
            }
            // Escape always cancels prefix mode — not user-remappable,
            // same as every other modal's Escape-to-cancel in this app.
            if event.physical_key != PhysicalKey::Code(KeyCode::Escape) {
                use prefs::PrefAction::{MuxCloseTab, MuxFocusNext, MuxFocusPrev, MuxSplitDown, MuxSplitRight};
                match self.mux_action(event, &[MuxSplitRight, MuxSplitDown, MuxFocusNext, MuxFocusPrev, MuxCloseTab]) {
                    Some(MuxSplitRight) => self.mux_split(crate::multiplexer::Axis::Horizontal),
                    Some(MuxSplitDown) => self.mux_split(crate::multiplexer::Axis::Vertical),
                    act @ (Some(MuxFocusNext) | Some(MuxFocusPrev)) => {
                        let panes = self.mux.model.layout(self.mux_area());
                        if let Some(i) = panes
                            .iter()
                            .position(|(p, _)| p.id == self.mux.model.focused)
                        {
                            let back = act == Some(MuxFocusPrev);
                            let next = (i + if back { panes.len() - 1 } else { 1 }) % panes.len();
                            self.mux_focus(panes[next].0.id);
                        }
                    }
                    Some(MuxCloseTab) => self.mux_close_active_tab(),
                    _ => {}
                }
            }
            self.mux.model.prefix = false;
            self.request_main_redraw();
            return true;
        }
        if event.state.is_pressed()
            && !event.repeat
            && self
                .mux_action(event, &[prefs::PrefAction::MuxPrefix])
                .is_some()
        {
            self.mux.model.prefix = true;
            self.request_main_redraw();
            return true;
        }
        let on_chooser = self
            .mux
            .model
            .pane(self.mux.model.focused)
            .is_some_and(|p| p.active_tab().content == TabContent::Chooser);
        // ⌘1-9 pick a recent document straight from the chooser — scoped
        // to a focused Chooser tab specifically (this whole branch only
        // runs while one is active; see the catch-all below), so it
        // never shadows ⌘1's real global meaning (Zoom to Actual Size)
        // etc. once a document tab is active instead.
        if on_chooser && event.state.is_pressed() && !event.repeat && self.cmd_down {
            if let PhysicalKey::Code(code) = event.physical_key {
                let digit = match code {
                    KeyCode::Digit1 => Some(0), KeyCode::Digit2 => Some(1),
                    KeyCode::Digit3 => Some(2), KeyCode::Digit4 => Some(3),
                    KeyCode::Digit5 => Some(4), KeyCode::Digit6 => Some(5),
                    KeyCode::Digit7 => Some(6), KeyCode::Digit8 => Some(7),
                    KeyCode::Digit9 => Some(8),
                    _ => None,
                };
                // ⌘-digit numbering runs continuously top to bottom
                // across both sections — "OPEN DOCUMENTS" first, then
                // "RECENT DOCUMENTS" — matching the ⌘N hints painted
                // next to each row (see `mux_scenes`).
                let entries: Vec<_> = self.mux_open_docs().into_iter().chain(self.mux_recent_files()).collect();
                if let Some(i) = digit {
                    if let Some((_, pick)) = entries.into_iter().nth(i) {
                        self.mux_open_recent(&pick);
                        self.request_main_redraw();
                        return true;
                    }
                }
            }
        }
        self.mux.model.enabled() && on_chooser
    }
    /// The splitter (if any) under the pointer right now — checked ahead
    /// of pane content on press, and every frame for the resize-cursor
    /// hover in `App::update_canvas_cursor`.
    pub(in crate::app) fn mux_splitter_at(&self, p: Point) -> Option<Splitter> {
        if !self.mux.model.enabled() {
            return None;
        }
        self.mux
            .model
            .splitters(self.mux_area())
            .into_iter()
            .find(|s| s.rect.contains(p))
    }
    pub(super) fn mux_press(&mut self) -> bool {
        if !self.mux.model.enabled() {
            return false;
        }
        if let Some(s) = self.mux_splitter_at(self.pointer) {
            self.drag = Drag::MuxSplitDrag {
                left_leaf: s.left_leaf,
                right_leaf: s.right_leaf,
                axis: s.axis,
                bounding: s.bounding,
            };
            return true;
        }
        let Some((pane, r)) = self
            .mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .find(|(_, r)| r.contains(self.pointer))
        else {
            let area = self.mux_area();
            return self.pointer.x >= area.x0
                && self.pointer.x <= area.x1
                && self.pointer.y >= area.y0;
        };
        if pane.id != self.mux.model.focused {
            self.mux_focus(pane.id);
            if !matches!(pane.active_tab().content, TabContent::Document(_)) {
                return true;
            }
        }
        // Every pane's own tab strip — chip click switches tabs, ×
        // closes one, "+" adds a fresh chooser tab.
        let strip = header(r);
        if strip.contains(self.pointer) {
            if pane_tab_add_rect(strip).contains(self.pointer) {
                self.mux_new_tab();
                return true;
            }
            let labels: Vec<String> = pane.tabs.iter().map(|t| self.mux_tab_label(t)).collect();
            for (i, (whole, close)) in pane_tab_chips(&mut self.text, &labels, strip).into_iter().enumerate() {
                if close.contains(self.pointer) {
                    self.mux_close_tab(pane.id, i);
                    return true;
                }
                if whole.contains(self.pointer) {
                    // Deferred: a plain click (released before the drag
                    // threshold) switches to it — resolved in
                    // `pointer.rs`'s release handler, same click-vs-drag
                    // pattern as `Drag::PendingPanelDrag`. Past the
                    // threshold it escalates into dragging the tab to
                    // another pane instead (`App::mux_move_tab`).
                    self.drag = Drag::PendingTabDrag { pane: pane.id, tab_id: pane.tabs[i].id, press: self.pointer };
                    return true;
                }
            }
            return true;
        }
        match pane.active_tab().content {
            TabContent::Chooser => {
                let scroll = pane.active_tab().scroll;
                let open_docs = self.mux_open_docs();
                let recent_files = self.mux_recent_files();
                let (n_open, n_recent) = (open_docs.len(), recent_files.len());
                if chooser_action_row(r, 0, n_open, n_recent, scroll).contains(self.pointer) {
                    self.open_quick_new_doc();
                } else if chooser_action_row(r, 1, n_open, n_recent, scroll).contains(self.pointer) {
                    self.open_mux_terminal();
                } else if chooser_action_row(r, 2, n_open, n_recent, scroll).contains(self.pointer) {
                    self.open_or_import_as_new_doc();
                } else {
                    let mut hit = None;
                    for (i, (_, pick)) in open_docs.iter().enumerate() {
                        if chooser_open_row(r, i, n_open, n_recent, scroll).contains(self.pointer) {
                            hit = Some(pick);
                            break;
                        }
                    }
                    if hit.is_none() {
                        for (i, (_, pick)) in recent_files.iter().enumerate() {
                            if chooser_recent_row(r, i, n_open, n_recent, scroll).contains(self.pointer) {
                                hit = Some(pick);
                                break;
                            }
                        }
                    }
                    if let Some(pick) = hit {
                        self.mux_open_recent(pick);
                    }
                }
                self.request_main_redraw();
                true
            }
            TabContent::Terminal => {
                if let Some(t) = &mut self.terminal {
                    t.focused = true;
                }
                true
            }
            TabContent::Document(_) => false,
        }
    }
    pub(super) fn mux_scroll(&mut self, dy: f64) -> bool {
        let Some((pane, r)) = self
            .mux
            .model
            .layout(self.mux_area())
            .into_iter()
            .find(|(p, r)| p.id == self.mux.model.focused && r.contains(self.pointer))
        else {
            return false;
        };
        if pane.active_tab().content != TabContent::Chooser {
            return false;
        }
        let n_open = self.mux_open_docs().len();
        let n_recent = self.mux_recent_files().len();
        let max = (chooser_content_h(n_open, n_recent) - body(r).height()).max(0.);
        let scroll = pane.active_tab().scroll;
        if let Some(tab) = self.mux.model.active_tab_mut(pane.id) {
            tab.scroll = (scroll - dy).clamp(0., max);
        }
        self.request_main_redraw();
        true
    }
    pub(super) fn mux_tick(&mut self) {
        if !self.mux.model.enabled() {
            return;
        }
        // Reap any pane tab whose document no longer resolves globally —
        // e.g. its last reference just closed for real via the unsaved-
        // changes confirm flow, which has no pane awareness of its own
        // (see `mux_close_tab`). One at a time, re-scanning after each
        // removal since indices can shift and a removal can cascade
        // (a pane's own last tab going away removes the pane too).
        loop {
            let stale = self.mux.model.layout(self.mux_area()).into_iter().find_map(|(p, _)| {
                p.tabs
                    .iter()
                    .enumerate()
                    .find(|(_, t)| matches!(t.content, TabContent::Document(id) if self.mux_doc_index(id).is_none()))
                    .map(|(i, _)| (p.id, i))
            });
            let Some((pane_id, tab_idx)) = stale else { break };
            self.mux_close_tab_now(pane_id, tab_idx);
        }

        let layout = self.mux.model.layout(self.mux_area());
        for (p, r) in &layout {
            let active = p.active_tab();
            if let Some(t) = self.mux.terminals.get_mut(&active.id) {
                t.tick_and_resize(*r);
            }
            if p.id == self.mux.model.focused {
                if let Some(t) = &mut self.terminal {
                    t.tick_and_resize(*r);
                }
                if matches!(active.content, TabContent::Document(_)) {
                    let origin = body(*r).origin();
                    if let Some(old) = self.mux.origin {
                        self.doc.view.pan += origin - old;
                    }
                    self.mux.origin = Some(origin);
                }
            }
        }
        if self.terminal.is_some() || !self.mux.terminals.is_empty() {
            self.request_main_redraw();
        }
    }
    pub(super) fn mux_exit_terminals(&mut self) {
        for (_, mut t) in self.mux.terminals.drain() {
            t.terminate();
        }
    }
    pub(super) fn mux_scenes(&mut self) -> (Scene, Scene) {
        let mut back = Scene::new();
        let mut front = Scene::new();
        let area = self.mux_area();
        if self.mux.model.enabled() {
            let layout = self.mux.model.layout(area);
            // With only one pane there's nothing to distinguish it
            // *from* — the focus border exists to answer "which pane are
            // my keystrokes going to", a question that doesn't arise
            // until there's a second pane to confuse it with.
            let multi_pane = layout.len() > 1;
            for (pane, r) in layout {
                let focused = pane.id == self.mux.model.focused;
                back.fill(Fill::NonZero, Affine::IDENTITY, self.theme.panel_bg, None, &r);

                let active = pane.active_tab().clone();
                match active.content {
                    TabContent::Document(id) => {
                        if !focused {
                            if let Some(i) = self.mux_doc_index(id) {
                                let doc = if i == self.active { &self.doc } else { &self.tabs[i] };
                                let mut view = active.view;
                                view.pan += body(r).origin().to_vec2();
                                // Each pane keeps its own placeholders.
                                self.adjust.set_pane(pane.id as u64 + 1);
                                canvas::paint(
                                    &mut back,
                                    self.recolor_dialog.as_ref().filter(|d| d.preview && d.document == doc.id && d.revision == doc.editor.revision()).map_or(doc.editor.document(), |d| &d.rendered),
                                    &view,
                                    body(r),
                                    &self.theme,
                                    &mut self.text,
                                    &[],
                                    Point::new(-1.0, -1.0),
                                    None,
                                    None,
                                    None,
                                    None,
                                    false,
                                    None,
                                    None,
                                    None,
                                    &self.image_cache,
                                    None,
                                    self.settings.cull_inset,
                                    false,
                                    None,
                                    false,
                                    None,
                                    self.transparency_grid,
                                    self.settings.show_grid,
                                    self.settings.grid_spacing,
                                    None,
                                    0.0,
                                    &mut self.adjust,
                                );
                            }
                        }
                    }
                    TabContent::Terminal => {
                        let terminal = if focused {
                            self.terminal.as_ref()
                        } else {
                            self.mux.terminals.get(&active.id)
                        };
                        if let Some(t) = terminal {
                            crate::terminal_paint::paint(
                                &mut back,
                                &crate::terminal_paint::TerminalPaintArgs {
                                    rect: body(r),
                                    header: false,
                                    focused,
                                    term: &t.term,
                                    font: &t.font,
                                    cell_w: t.cell_w,
                                    cell_h: t.cell_h,
                                    ascent: t.ascent,
                                },
                                &self.theme,
                                &mut self.text,
                            );
                        }
                    }
                    TabContent::Chooser => {
                        let scroll = active.scroll;
                        // "OPEN DOCUMENTS" (this session, any pane) above
                        // "RECENT DOCUMENTS" (on-disk history, excluding
                        // anything already open) — see `App::mux_open_docs`
                        // / `App::mux_recent_files`. Their lengths also
                        // decide whether the whole content block is short
                        // enough to center in the pane.
                        let open_docs = self.mux_open_docs();
                        let recent_files = self.mux_recent_files();
                        let (n_open, n_recent) = (open_docs.len(), recent_files.len());
                        back.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &body(r));

                        let mark_c = chooser_mark_center(r, n_open, n_recent, scroll);
                        draw_mark(&mut back, mark_c, ui_px(CHOOSER_MARK_SIZE), self.theme.text);
                        let (col_x0, col_x1) = chooser_col_x(r);
                        let vw = self.text.measure(CHOOSER_VERSION, 11.5);
                        self.text.draw(
                            &mut back,
                            CHOOSER_VERSION,
                            11.5,
                            self.theme.text_dim,
                            (col_x0 + col_x1) * 0.5 - vw * 0.5,
                            chooser_version_baseline_y(r, n_open, n_recent, scroll),
                        );
                        let ahy = chooser_actions_header_y(r, n_open, n_recent, scroll);
                        paint_chooser_section_header(&mut back, &mut self.text, &self.theme, col_x0, col_x1, ahy, "GET STARTED");
                        // Legends, not remappable bindings — each is the
                        // existing global shortcut for the same action
                        // (⌘N/⌘O), or the new one just added for it (⌘J,
                        // Terminal previously had none).
                        const ACTIONS: [(&str, usize, &str); CHOOSER_ACTIONS] = [
                            ("New Document", 0, "⌘N"),
                            ("New Shell", 1, "⌘J"),
                            ("Open / Import", 2, "⌘O"),
                        ];
                        for (i, (label, kind, hint)) in ACTIONS.iter().enumerate() {
                            let ar = chooser_action_row(r, i, n_open, n_recent, scroll);
                            let hover = ar.contains(self.pointer);
                            paint_chooser_action_row(&mut back, &mut self.text, &self.theme, ar, label, *kind, hover);
                            let hw = self.text.measure(hint, 11.5);
                            self.text.draw(
                                &mut back,
                                hint,
                                11.5,
                                self.theme.text_dim,
                                ar.x1 - hw - ui_px(10.0),
                                ar.center().y + ui_px(4.0),
                            );
                        }

                        // ⌘-digit hints run continuously across both
                        // sections (see `App::mux_key`'s own chained list).
                        let mut hint_i = 0usize;

                        // Hidden entirely (not even its header) while
                        // nothing's open — a "GET STARTED"-adjacent
                        // section that's empty on every fresh pane would
                        // be pure noise. Unlike "RECENT DOCUMENTS" below,
                        // which always shows (a deliberate, separate fix).
                        if n_open > 0 {
                            let ohy = chooser_open_header_y(r, n_open, n_recent, scroll);
                            paint_chooser_section_header(&mut back, &mut self.text, &self.theme, col_x0, col_x1, ohy, "OPEN DOCUMENTS");
                            for (i, (name, _)) in open_docs.iter().enumerate() {
                                let row = chooser_open_row(r, i, n_open, n_recent, scroll);
                                if row.contains(self.pointer) {
                                    back.fill(Fill::NonZero, Affine::IDENTITY, self.theme.strip_active, None, &row.to_rounded_rect(ui_px(3.0)));
                                }
                                self.text.draw(&mut back, name, 12.5, self.theme.text, row.x0 + ui_px(4.0), row.y0 + ui_px(19.));
                                if hint_i < 9 {
                                    let hint = format!("⌘{}", hint_i + 1);
                                    let hw = self.text.measure(&hint, 11.5);
                                    self.text.draw(
                                        &mut back, &hint, 11.5, self.theme.text_dim,
                                        row.x1 - hw - ui_px(4.0), row.y0 + ui_px(18.),
                                    );
                                }
                                hint_i += 1;
                            }
                        }

                        // Always shown — even with nothing under it yet,
                        // so its absence never reads as "this is broken"
                        // (⇐ explicit user complaint: show the section
                        // regardless of whether there's anything in it).
                        // Same left edge as "GET STARTED" above it, not
                        // the raw pane edge — the whole thing reads as
                        // one unified list.
                        let rhy = chooser_recent_header_y(r, n_open, n_recent, scroll);
                        paint_chooser_section_header(&mut back, &mut self.text, &self.theme, col_x0, col_x1, rhy, "RECENT DOCUMENTS");
                        if recent_files.is_empty() {
                            let row = chooser_recent_row(r, 0, n_open, n_recent, scroll);
                            self.text.draw(&mut back, "Nothing yet", 12.0, self.theme.text_dim, row.x0 + ui_px(4.0), row.y0 + ui_px(19.));
                        }
                        for (i, (name, _)) in recent_files.iter().enumerate() {
                            let row = chooser_recent_row(r, i, n_open, n_recent, scroll);
                            if row.contains(self.pointer) {
                                // Same translucent bar as "GET STARTED"'s rows
                                // (paint_chooser_action_row) — one consistent
                                // hover style across the whole chooser list.
                                back.fill(Fill::NonZero, Affine::IDENTITY, self.theme.text.with_alpha(0.09), None, &row.to_rounded_rect(ui_px(6.0)));
                            }
                            self.text.draw(&mut back, name, 12.5, self.theme.text, row.x0 + ui_px(4.0), row.y0 + ui_px(19.));
                            if hint_i < 9 {
                                let hint = format!("⌘{}", hint_i + 1);
                                let hw = self.text.measure(&hint, 11.5);
                                self.text.draw(
                                    &mut back, &hint, 11.5, self.theme.text_dim,
                                    row.x1 - hw - ui_px(4.0), row.y0 + ui_px(18.),
                                );
                            }
                            hint_i += 1;
                        }
                        back.pop_layer();
                    }
                }

                // Every pane gets the same tab strip, regardless of what
                // its active tab holds — see `docs/canvas-panes.md`.
                let strip = header(r);
                front.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &strip);
                front.fill(Fill::NonZero, Affine::IDENTITY, self.theme.app_bar, None, &strip);
                front.fill(
                    Fill::NonZero, Affine::IDENTITY, self.theme.border, None,
                    &Rect::new(strip.x0, strip.y1 - 1.0, strip.x1, strip.y1),
                );
                let labels: Vec<String> = pane.tabs.iter().map(|t| self.mux_tab_label(t)).collect();
                let chips = pane_tab_chips(&mut self.text, &labels, strip);
                let n_chips = chips.len();
                for (i, (whole, close)) in chips.into_iter().enumerate() {
                    let active_chip = i == pane.active;
                    if active_chip {
                        front.fill(Fill::NonZero, Affine::IDENTITY, self.theme.strip_active, None, &whole);
                        front.fill(
                            Fill::NonZero, Affine::IDENTITY, self.theme.accent, None,
                            &Rect::new(whole.x0, whole.y1 - ui_px(2.0), whole.x1, whole.y1),
                        );
                    }
                    let ink = if active_chip { self.theme.text } else { self.theme.text_dim };
                    let xc = close.center();
                    let mut xg = BezPath::new();
                    xg.move_to((xc.x - ui_px(4.0), xc.y - ui_px(4.0)));
                    xg.line_to((xc.x + ui_px(4.0), xc.y + ui_px(4.0)));
                    xg.move_to((xc.x + ui_px(4.0), xc.y - ui_px(4.0)));
                    xg.line_to((xc.x - ui_px(4.0), xc.y + ui_px(4.0)));
                    front.stroke(&Stroke::new(ui_px(1.3)), Affine::IDENTITY, ink, None, &xg);
                    let label_x = close.x1 + ui_px(6.0);
                    let label_y = strip.y0 + metric_tab_bar_h() * 0.5 + ui_px(4.0);
                    self.text.draw(&mut front, &labels[i], 12.6, ink, label_x, label_y);
                    // Divider between tabs.
                    if i + 1 < n_chips {
                        front.fill(
                            Fill::NonZero, Affine::IDENTITY, self.theme.border, None,
                            &Rect::new(whole.x1, strip.y0 + ui_px(5.0), whole.x1 + 1.0, strip.y1 - ui_px(5.0)),
                        );
                    }
                }
                let add_r = pane_tab_add_rect(strip);
                let add_ink = if add_r.contains(self.pointer) { self.theme.text } else { self.theme.text_dim };
                icon_plus(&mut front, add_r.center(), ui_px(5.0), add_ink);
                front.pop_layer();

                // Vector/Raster mode pill — fixed at the pane's own top
                // left, independent of which tab is active or where it
                // sits in the tab order, sized tightly around its own
                // bold label. Drawn *after* the tab strip's clip pops (it
                // sits below `strip`, so clipping to `strip` would cut it
                // off entirely) but still on top of this pane's canvas
                // content, painted earlier in this same pass. Only the
                // focused pane's active tab has a meaningful "current
                // layer" to report (`App::current_layer_kind` reads
                // `self.doc`, which always tracks the focused tab), and
                // only when that tab is actually a document (a Terminal/
                // Chooser tab has no layers at all).
                if focused && matches!(pane.active_tab().content, TabContent::Document(_)) {
                    if let Some(kind) = self.current_layer_kind() {
                        let (label, color) = match kind {
                            amalith_core::LayerKind::Vector => ("VECTOR", self.theme.accent),
                            amalith_core::LayerKind::Raster => ("RASTER", self.theme.raster_accent),
                        };
                        let label_size = 11.5;
                        let pad_x = ui_px(8.0);
                        let pill_h = ui_px(19.0);
                        let text_w = self.text.measure_bold(label, label_size);
                        let pill = Rect::new(strip.x0, strip.y1, strip.x0 + text_w + pad_x * 2.0, strip.y1 + pill_h);
                        front.fill(Fill::NonZero, Affine::IDENTITY, color, None, &pill);
                        self.text.draw_bold(&mut front, label, label_size, Color::WHITE, pill.x0 + pad_x, pill.y0 + pill_h * 0.5 + ui_px(4.0));
                    }
                }

                // Only meaningful once there's more than one pane to
                // tell apart — with just one, "which pane are my
                // keystrokes going to" has an obvious answer and the
                // border is pure noise. Thinner than it used to be
                // (1.4 vs. 2.0) now that it's only ever shown when it's
                // actually answering that question.
                if multi_pane {
                    front.stroke(
                        &vello::kurbo::Stroke::new(if focused { 1.4 } else { 1.0 }),
                        Affine::IDENTITY,
                        if focused { self.theme.accent } else { self.theme.border },
                        None,
                        &r.inset(-1.),
                    );
                }
            }
            // A thin line in each split's gap — otherwise the resize
            // drag (`App::mux_press`/`mux_splitter_at`) has no visible
            // affordance at all.
            for s in self.mux.model.splitters(area) {
                let line = match s.axis {
                    crate::multiplexer::Axis::Horizontal => {
                        let cx = s.rect.center().x;
                        Rect::new(cx - 0.5, s.rect.y0, cx + 0.5, s.rect.y1)
                    }
                    crate::multiplexer::Axis::Vertical => {
                        let cy = s.rect.center().y;
                        Rect::new(s.rect.x0, cy - 0.5, s.rect.x1, cy + 0.5)
                    }
                };
                front.fill(Fill::NonZero, Affine::IDENTITY, self.theme.splitter, None, &line);
            }
        }
        if self.mux.model.enabled() || self.mux.model.prefix {
            let (_, h) = self.main_logical_size().unwrap_or((1280., 800.));
            let bar = Rect::new(area.x0, h - ui_px(28.), area.x1, h);
            front.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                self.theme.strip_bg,
                None,
                &bar,
            );
            // Built from the live (possibly user-rebound) chords, not a
            // hardcoded string, so it can't go stale against Preferences
            // ▸ Keyboard ▸ Pane Multiplexer.
            let key_of = |act: prefs::PrefAction| {
                prefs::PrefAction::ALL
                    .iter()
                    .position(|a| *a == act)
                    .and_then(|i| self.settings.action_keys[i])
            };
            // Lowercase, spelled-out form ("shift+d", not "Shift+D") to
            // match the rest of the bar's lowercase key labels.
            let label = |act: prefs::PrefAction| -> String {
                let Some(c) = key_of(act) else {
                    return "—".to_string();
                };
                let mut s = String::new();
                if c.cmd {
                    s.push_str("cmd+");
                }
                if c.alt {
                    s.push_str("opt+");
                }
                if c.shift {
                    s.push_str("shift+");
                }
                s.push(prefs::key_char(c.code).unwrap_or('?').to_ascii_lowercase());
                s
            };
            let text_size: f32 = 12.0;
            let cy = bar.center().y + ui_px(4.0);
            if self.mux.model.prefix {
                // A pointed "flag" badge, not a plain rect — reads as a
                // mode indicator (this is a whole extra input mode, not
                // just a hint) rather than another label in the row.
                let badge_h = ui_px(20.0);
                let badge_y0 = bar.center().y - badge_h / 2.0;
                let pad = ui_px(10.0);
                let point = ui_px(8.0);
                let word = "PREFIX";
                let tw = self.text.measure(word, text_size);
                let badge_x0 = bar.x0 + ui_px(10.0);
                let badge_x1 = badge_x0 + pad + tw + pad + point;
                let mut flag = BezPath::new();
                flag.move_to((badge_x0, badge_y0));
                flag.line_to((badge_x1 - point, badge_y0));
                flag.line_to((badge_x1, badge_y0 + badge_h / 2.0));
                flag.line_to((badge_x1 - point, badge_y0 + badge_h));
                flag.line_to((badge_x0, badge_y0 + badge_h));
                flag.close_path();
                front.fill(Fill::NonZero, Affine::IDENTITY, self.theme.accent, None, &flag);
                self.text.draw(&mut front, word, text_size, self.theme.on_accent, badge_x0 + pad, cy);

                // Resolved up front (and out of `label`'s borrow of
                // `self.settings`) so the draw loop below can borrow
                // `self.text` mutably.
                let items = [
                    (label(prefs::PrefAction::MuxSplitRight), "split right"),
                    (label(prefs::PrefAction::MuxSplitDown), "split down"),
                    (label(prefs::PrefAction::MuxFocusPrev), "focus pane left"),
                    (label(prefs::PrefAction::MuxFocusNext), "focus pane right"),
                    (label(prefs::PrefAction::MuxCloseTab), "close tab"),
                    ("esc".to_string(), "cancel"),
                ];
                let mut x = badge_x1 - point + ui_px(24.0);
                for (key, desc) in items {
                    self.text.draw(&mut front, &key, text_size, self.theme.accent, x, cy);
                    x += self.text.measure(&key, text_size) + ui_px(6.0);
                    self.text.draw(&mut front, desc, text_size, self.theme.text_dim, x, cy);
                    x += self.text.measure(desc, text_size) + ui_px(24.0);
                }
            } else {
                let text = format!(
                    "{}  pane commands     click a pane to focus",
                    label(prefs::PrefAction::MuxPrefix),
                );
                self.text.draw(&mut front, &text, text_size, self.theme.text_dim, bar.x0 + ui_px(10.), cy);
            }
        }
        if self.quick_newdoc.is_some() {
            let caret = self.text_blink_on();
            let qnd = self.quick_newdoc.as_mut().unwrap();
            let pane_r = self
                .mux
                .model
                .layout(area)
                .into_iter()
                .find(|(p, _)| p.id == qnd.pane)
                .map(|(_, r)| r)
                .unwrap_or(area);
            let card = quick_newdoc_rect(pane_r);
            paint_quick_newdoc(&mut front, &mut self.text, &self.theme, card, &mut qnd.form, caret);
        }
        // A tab mid-drag: a small ghost chip riding the cursor, and the
        // pane it would land in (if any — no floating tabs, so nothing
        // to show while over empty space) highlighted. No OS window
        // involved, unlike the panel system's own cross-window drag.
        if let Drag::DraggingTab { from_pane, tab_id } = &self.drag {
            let (from_pane, tab_id) = (*from_pane, *tab_id);
            if let Some((target, idx)) = self.mux_tab_drop_preview {
                if let Some((_, r)) = self.mux.model.layout(area).into_iter().find(|(p, _)| p.id == target) {
                    front.stroke(&Stroke::new(3.0), Affine::IDENTITY, self.theme.accent, None, &r.inset(-1.5));
                    // Also mark exactly where among the target pane's own
                    // tabs it would land — a thin caret in the strip — so
                    // a same-pane reorder (whole-pane outline alone would
                    // be uninformative there, since the dragged-from pane
                    // is already the one it's hovering) still reads.
                    let strip = header(r);
                    // Excludes the dragged tab's own chip when previewing
                    // a same-pane reorder, so `idx` (already computed in
                    // "as if it's already gone" terms — see `on_cursor_move`)
                    // lines up with the chip it's actually meant to land
                    // beside instead of drifting by one.
                    let labels: Vec<String> = self
                        .mux
                        .model
                        .pane(target)
                        .map(|p| {
                            p.tabs
                                .iter()
                                .filter(|t| !(target == from_pane && t.id == tab_id))
                                .map(|t| self.mux_tab_label(t))
                                .collect()
                        })
                        .unwrap_or_default();
                    let chips = pane_tab_chips(&mut self.text, &labels, strip);
                    let x = match chips.get(idx) {
                        Some((whole, _)) => whole.x0,
                        None => chips.last().map(|(whole, _)| whole.x1).unwrap_or(strip.x0 + ui_px(4.0)),
                    };
                    front.stroke(
                        &Stroke::new(ui_px(2.0)),
                        Affine::IDENTITY,
                        self.theme.accent,
                        None,
                        &vello::kurbo::Line::new((x, strip.y0 + ui_px(3.0)), (x, strip.y1 - ui_px(3.0))),
                    );
                }
            }
            let tab = self
                .mux
                .model
                .pane(from_pane)
                .and_then(|p| p.tabs.iter().find(|t| t.id == tab_id))
                .cloned();
            let label = tab.map(|t| self.mux_tab_label(&t)).unwrap_or_default();
            let gw = self.text.measure(&label, 12.6) + ui_px(18.9) + ui_px(23.1);
            let gh = metric_tab_bar_h();
            let ghost = Rect::new(
                self.pointer.x - gw * 0.5, self.pointer.y - gh * 0.5,
                self.pointer.x + gw * 0.5, self.pointer.y + gh * 0.5,
            );
            front.fill(Fill::NonZero, Affine::IDENTITY, self.theme.strip_active.multiply_alpha(0.92), None, &ghost.to_rounded_rect(ui_px(4.0)));
            front.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, self.theme.accent, None, &ghost.to_rounded_rect(ui_px(4.0)));
            self.text.draw(&mut front, &label, 12.6, self.theme.text, ghost.x0 + ui_px(20.0), ghost.y0 + ghost.height() * 0.5 + ui_px(4.0));
        }
        (back, front)
    }
    /// Open the compact "New Document" overlay, centered on the pane
    /// this was invoked from (or the focused pane if invoked from
    /// outside a pane context, e.g. ⌘N / File ▸ New). Superseding
    /// `open_new_doc` as the default entry point.
    pub(super) fn open_quick_new_doc(&mut self) {
        if !self.mux.model.enabled() {
            self.mux_begin();
        }
        self.quick_newdoc = Some(multiplexer::QuickNewDoc {
            form: newdoc::NewDocForm::default(),
            pane: self.mux.model.focused,
        });
        self.request_main_redraw();
    }
    pub(super) fn quick_newdoc_card_rect(&self) -> Option<Rect> {
        let qnd = self.quick_newdoc.as_ref()?;
        let area = self.mux_area();
        let pane_r = self
            .mux
            .model
            .layout(area)
            .into_iter()
            .find(|(p, _)| p.id == qnd.pane)
            .map(|(_, r)| r)
            .unwrap_or(area);
        Some(quick_newdoc_rect(pane_r))
    }
    /// The overlay's full clickable extent — the card itself, plus (while
    /// a dropdown is open) its popup's own bounds, which can run past the
    /// card's bottom edge (e.g. Raster Effects' 3 options). A click has
    /// to miss *this*, not just the bare card, to count as "outside" and
    /// close the whole overlay — otherwise picking a low dropdown item
    /// would close the overlay instead of picking it.
    pub(super) fn quick_newdoc_hit_bounds(&self) -> Option<Rect> {
        let card = self.quick_newdoc_card_rect()?;
        let Some(menu) = self.quick_newdoc.as_ref()?.form.open_menu else {
            return Some(card);
        };
        Some(qnd_menu_item_rects(card, menu).iter().fold(card, |acc, r| acc.union(*r)))
    }
    /// Routes a press while the compact New Document overlay is open.
    /// Returns whether the press was consumed (it swallows everything —
    /// modal, like every other dialog in this app).
    pub(super) fn quick_newdoc_press(&mut self) -> bool {
        let Some(card) = self.quick_newdoc_card_rect() else {
            return false;
        };
        // A dropdown eats every click while it's open: a click on one of
        // its items picks that value and closes it; any other click
        // (even one that would otherwise hit Create/a field) just closes
        // the dropdown instead of also acting on whatever's under it.
        // That's what stopped the accidental double-action clicking
        // through to the pane behind the overlay before this existed —
        // every click while a menu is up now resolves to *only* the menu.
        if let Some(menu) = self.quick_newdoc.as_ref().and_then(|q| q.form.open_menu) {
            let items = qnd_menu_item_rects(card, menu);
            if let Some(i) = items.iter().position(|r| r.contains(self.pointer)) {
                if let Some(qnd) = self.quick_newdoc.as_mut() {
                    match menu {
                        newdoc::Menu::Unit => qnd.form.set_unit(newdoc::UNITS[i]),
                        newdoc::Menu::Color => qnd.form.color_mode = newdoc::COLORS[i],
                        newdoc::Menu::Raster => qnd.form.raster = newdoc::RASTERS[i],
                        newdoc::Menu::StartLayer => qnd.form.start_layer = newdoc::START_LAYERS[i],
                    }
                    qnd.form.open_menu = None;
                }
            } else if let Some(qnd) = self.quick_newdoc.as_mut() {
                qnd.form.open_menu = None;
            }
            self.request_main_redraw();
            return true;
        }
        if qnd_create_rect(card).contains(self.pointer) {
            self.create_from_quick_form();
            return true;
        }
        if qnd_cancel_rect(card).contains(self.pointer) {
            self.quick_newdoc = None;
            self.request_main_redraw();
            return true;
        }
        let Some(qnd) = self.quick_newdoc.as_mut() else {
            return true;
        };
        if qnd_name_rect(card).contains(self.pointer) {
            qnd.form.commit_focus();
            qnd.form.focus = Some(newdoc::Field::Name);
        } else if qnd_width_rect(card).contains(self.pointer) {
            qnd.form.commit_focus();
            qnd.form.focus = Some(newdoc::Field::Width);
        } else if qnd_height_rect(card).contains(self.pointer) {
            qnd.form.commit_focus();
            qnd.form.focus = Some(newdoc::Field::Height);
        } else if qnd_unit_rect(card).contains(self.pointer) {
            qnd.form.open_menu = Some(newdoc::Menu::Unit);
        } else if qnd_orient_rects(card).0.contains(self.pointer) {
            qnd.form.set_orientation(true);
        } else if qnd_orient_rects(card).1.contains(self.pointer) {
            qnd.form.set_orientation(false);
        } else if qnd_ab_minus_rect(card).contains(self.pointer) {
            qnd.form.artboards = qnd.form.artboards.saturating_sub(1).max(1);
        } else if qnd_ab_plus_rect(card).contains(self.pointer) {
            qnd.form.artboards = (qnd.form.artboards + 1).min(100);
        } else if qnd_bleed_rect(card).contains(self.pointer) {
            qnd.form.commit_focus();
            qnd.form.focus = Some(newdoc::Field::BleedTop);
        } else if qnd_color_rect(card).contains(self.pointer) {
            qnd.form.open_menu = Some(newdoc::Menu::Color);
        } else if qnd_raster_rect(card).contains(self.pointer) {
            qnd.form.open_menu = Some(newdoc::Menu::Raster);
        } else if qnd_start_layer_rect(card).contains(self.pointer) {
            qnd.form.open_menu = Some(newdoc::Menu::StartLayer);
        }
        // A click anywhere else on/around the card (including the card
        // background itself) is absorbed here without action — only a
        // click truly outside the card, handled by the caller before
        // this is even reached, should close it.
        self.request_main_redraw();
        true
    }
    /// A key while the compact New Document overlay is open.
    pub(super) fn quick_newdoc_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        const QND_FIELDS: [newdoc::Field; 4] =
            [newdoc::Field::Name, newdoc::Field::Width, newdoc::Field::Height, newdoc::Field::BleedTop];
        let Some(qnd) = self.quick_newdoc.as_mut() else {
            return;
        };
        let Some(f) = qnd.form.focused() else {
            match event.physical_key {
                PhysicalKey::Code(KeyCode::Escape) => {
                    if qnd.form.open_menu.take().is_none() {
                        self.quick_newdoc = None;
                    }
                    self.request_main_redraw();
                }
                PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => self.create_from_quick_form(),
                _ => {}
            }
            return;
        };
        if let PhysicalKey::Code(code @ (KeyCode::ArrowUp | KeyCode::ArrowDown)) = event.physical_key {
            let dir = if code == KeyCode::ArrowUp { 1.0 } else { -1.0 };
            let step = (if self.cmd_down { 0.1 } else if self.shift_down { 10.0 } else { 1.0 }) * dir;
            self.quick_newdoc.as_mut().unwrap().form.step_focused(step, &mut self.text);
            self.request_main_redraw();
            return;
        }
        let mods = textedit::Mods { shift: self.shift_down, alt: self.alt_down, meta: self.cmd_down };
        let logical = event.logical_key.clone();
        let typed = event.text.clone();
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let resp = self
            .quick_newdoc
            .as_mut()
            .unwrap()
            .form
            .field(f)
            .key(&logical, mods, typed.as_deref(), self.clipboard.as_mut(), &mut self.text);
        match resp {
            crate::text_field::Resp::Cancel => self.quick_newdoc = None,
            crate::text_field::Resp::Submit => {
                self.quick_newdoc.as_mut().unwrap().form.commit_focus();
                self.create_from_quick_form();
            }
            crate::text_field::Resp::Tab(back) => {
                let qnd = self.quick_newdoc.as_mut().unwrap();
                qnd.form.commit_focus();
                let cur = QND_FIELDS.iter().position(|x| *x == f).unwrap_or(0);
                let n = QND_FIELDS.len();
                let next = if back { (cur + n - 1) % n } else { (cur + 1) % n };
                qnd.form.focus = Some(QND_FIELDS[next]);
                qnd.form.field(QND_FIELDS[next]).select_all(&mut self.text);
            }
            _ => {}
        }
        self.request_main_redraw();
    }
    /// Build a fresh document from the compact overlay's form
    /// (`App::build_editor_from_form`) and hand it to `App::add_doc`,
    /// which itself knows whether to replace the boot placeholder or
    /// push a new tab.
    fn create_from_quick_form(&mut self) {
        let Some(qnd) = self.quick_newdoc.as_mut() else {
            return;
        };
        qnd.form.commit_focus();
        let color_mode = qnd.form.color_mode;
        let editor = match Self::build_editor_from_form(&qnd.form) {
            Ok(e) => e,
            Err(msg) => {
                self.doc.io_error = Some(msg);
                self.request_main_redraw();
                return;
            }
        };
        self.sync_color_panel_to(color_mode);
        self.quick_newdoc = None;
        self.add_doc(Doc::new(editor));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::Axis;

    #[test]
    fn shared_document_focus_keeps_views_separate_and_undo_shared() {
        // `App::new()` already boots straight into a single mux pane
        // showing the chooser (see `App::new`'s own doc comment) — rebind
        // it to the construction-time document instead of re-running
        // `mux_begin` (a no-op once mux is already enabled).
        let mut app = App::new();
        let first = app.mux.model.focused;
        let id = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(id);
        // A pane now genuinely shows a document — no longer the
        // untouched boot placeholder (see `App::boot_empty`'s doc
        // comment; only ever happens for real via `add_doc`, bypassed
        // here by the direct `pane_mut` rebind above).
        app.boot_empty = false;
        let before = app.doc.view;
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        {
            let pane = app.mux.model.pane_mut(second).unwrap();
            pane.tabs[0].content = TabContent::Document(id);
            pane.tabs[0].view = CanvasView {
                pan: Vec2::new(30., 40.),
                zoom: 3.,
            };
        }
        app.mux_focus(second);
        assert_eq!(app.doc.id, id);
        assert_eq!(app.doc.view.zoom, 3.);
        app.doc
            .editor
            .execute(Command::CreateLayer {
                name: "Shared edit".into(),
                index: None,
            })
            .unwrap();
        app.doc.view.zoom = 5.;
        app.mux_focus(first);
        assert_eq!(app.doc.view.zoom, before.zoom);
        assert_eq!(app.doc.view.pan, before.pan);
        assert!(app
            .doc
            .editor
            .document()
            .layers()
            .iter()
            .any(|l| l.name == "Shared edit"));
        app.doc.editor.undo().unwrap();
        app.mux_focus(second);
        assert_eq!(app.doc.view.zoom, 5.);
        assert!(!app
            .doc
            .editor
            .document()
            .layers()
            .iter()
            .any(|l| l.name == "Shared edit"));
    }
    #[test]
    fn different_documents_resolve_to_original_editors_after_tab_switches() {
        // See the comment in the test above — `App::new()` already begins
        // mux'd, so rebind pane 0 to a document instead of calling
        // `mux_begin` (a no-op once mux is already enabled).
        let mut app = App::new();
        let first = app.mux.model.focused;
        let original = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(original);
        app.boot_empty = false;
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux_focus(second);
        app.add_doc(Doc::new(Editor::new(Document::new("Second"))));
        let other = app.doc.id;
        assert_ne!(original, other);
        app.mux_focus(first);
        assert_eq!(app.doc.id, original);
        app.mux_focus(second);
        assert_eq!(app.doc.id, other);
        assert_eq!(app.tabs.len(), 2);
    }
    #[test]
    fn closing_a_tab_that_shares_a_document_with_another_pane_leaves_the_document_open() {
        let mut app = App::new();
        let first = app.mux.model.focused;
        let id = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(id);
        app.boot_empty = false;
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux.model.pane_mut(second).unwrap().tabs[0].content = TabContent::Document(id);
        // Close the *first* pane's tab (a second view of the same doc,
        // not the last reference) — should not prompt, and the document
        // stays open in the global tab list.
        app.mux_close_tab(first, 0);
        assert_eq!(app.tabs.len(), 1);
        assert!(app.confirm_close.is_none());
    }
    #[test]
    fn closing_a_panes_last_terminal_tab_removes_the_pane() {
        let mut app = App::new();
        app.boot_empty = false;
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux.model.pane_mut(second).unwrap().tabs[0].content = TabContent::Terminal;
        assert_eq!(app.mux.model.layout(Rect::new(0., 0., 1000., 600.)).len(), 2);
        app.mux_close_tab(second, 0);
        assert_eq!(app.mux.model.layout(Rect::new(0., 0., 1000., 600.)).len(), 1);
    }
    #[test]
    fn moving_the_focused_active_tab_follows_it_with_focus() {
        let mut app = App::new();
        app.boot_empty = false;
        let first = app.mux.model.focused;
        let doc_id = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(doc_id);
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux_focus(first);
        let tab_id = app.mux.model.pane(first).unwrap().tabs[0].id;
        app.mux_move_tab(first, tab_id, second, 1);
        // The moved tab was the one on screen — focus followed it.
        assert_eq!(app.mux.model.focused, second);
        assert_eq!(app.doc.id, doc_id);
        let dst = app.mux.model.pane(second).unwrap();
        assert_eq!(dst.tabs.len(), 2);
        assert_eq!(dst.active, 1);
        assert_eq!(dst.tabs[1].content, TabContent::Document(doc_id));
        // The source pane's own last tab was taken, so it was removed
        // from the tree entirely (only the destination remains).
        assert_eq!(app.mux.model.layout(Rect::new(0., 0., 1000., 600.)).len(), 1);
    }
    #[test]
    fn moving_a_background_tab_does_not_touch_focus_or_the_live_document() {
        let mut app = App::new();
        app.boot_empty = false;
        let first = app.mux.model.focused;
        let original_doc = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(original_doc);
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        // A second, background tab in the first pane — not the active one.
        let bg_id = app.mux.model.add_tab(first, TabContent::Chooser).unwrap();
        app.mux.model.pane_mut(first).unwrap().active = 0; // back to the document tab
        app.mux_focus(first);
        app.mux_move_tab(first, bg_id, second, 1);
        // Nothing about the live view changed — the moved tab wasn't
        // the one on screen.
        assert_eq!(app.mux.model.focused, first);
        assert_eq!(app.doc.id, original_doc);
        assert_eq!(app.mux.model.pane(first).unwrap().tabs.len(), 1);
        let dst = app.mux.model.pane(second).unwrap();
        assert_eq!(dst.tabs.len(), 2);
        assert_eq!(dst.tabs[1].id, bg_id);
    }
    #[test]
    fn mux_move_tab_within_the_same_pane_reorders_instead_of_moving_panes() {
        let mut app = App::new();
        app.boot_empty = false;
        let first = app.mux.model.focused;
        let doc_id = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(doc_id);
        let a_id = app.mux.model.pane(first).unwrap().tabs[0].id;
        let b_id = app.mux.model.add_tab(first, TabContent::Chooser).unwrap();
        let c_id = app.mux.model.add_tab(first, TabContent::Terminal).unwrap();
        app.mux_focus(first);
        // Drag tab A (currently first) to land at the very end.
        app.mux_move_tab(first, a_id, first, 2);
        let p = app.mux.model.pane(first).unwrap();
        assert_eq!(p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(), vec![b_id, c_id, a_id]);
        // Only one pane the whole time — this never touched the tree.
        assert_eq!(app.mux.model.layout(Rect::new(0., 0., 1000., 600.)).len(), 1);
    }
    /// Manual GPU review: cargo test -p amalith-shell render_pane_review -- --ignored
    #[test]
    #[ignore = "writes a GPU-rendered visual review to /tmp"]
    fn render_pane_review() {
        use vello::{peniko::Color, wgpu};
        let mut app = App::new();
        app.dock = DockModel::new();
        let mut doc = Document::new("Shared artwork");
        let layer = LayerId::new();
        doc.insert_layer(amalith_core::Layer::new(layer, "Art"), 0);
        doc.insert_artboard(
            amalith_core::Artboard::new(
                ArtboardId::new(),
                "Page",
                amalith_core::Rect::new(0., 0., 400., 400.),
            ),
            0,
        );
        for (i, color) in [
            amalith_core::Color::rgb(0., 0.65, 0.65),
            amalith_core::Color::rgb(1., 0.65, 0.1),
        ]
        .into_iter()
        .enumerate()
        {
            let mut object = amalith_core::Object::rectangle(
                ObjectId::new(),
                amalith_core::ObjectParent::Layer(layer),
                amalith_core::Rect::new(
                    30. + i as f64 * 120.,
                    40. + i as f64 * 100.,
                    200. + i as f64 * 100.,
                    190. + i as f64 * 100.,
                ),
            );
            object
                .appearance
                .set_fill(amalith_core::Paint::Solid(color));
            doc.insert_object(object, i).unwrap();
        }
        app.doc = Doc::new(Editor::new(doc));
        // `App::new()` already begins mux'd — rebind pane 0 to this test's
        // own document instead of calling `mux_begin` (a no-op once mux
        // is already enabled).
        let focused = app.mux.model.focused;
        app.mux.model.pane_mut(focused).unwrap().tabs[0].content = TabContent::Document(app.doc.id);
        app.boot_empty = false;
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        let pane = app.mux.model.pane_mut(second).unwrap();
        pane.tabs[0].content = TabContent::Document(app.doc.id);
        pane.tabs[0].view = CanvasView {
            pan: Vec2::new(-60., 30.),
            zoom: 1.5,
        };
        app.mux.model.focused = second;
        let third = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux.model.focused = third;
        app.mux.model.prefix = true;
        let (mut scene, front) = app.mux_scenes();
        scene.append(&front, None);
        let output = "/tmp/amalith-multiplexer-review.png";
        let mut context = vello::util::RenderContext::new();
        let index = pollster::block_on(context.device(None)).expect("GPU adapter");
        let dev = &context.devices[index];
        let mut renderer =
            vello::Renderer::new(&dev.device, vello::RendererOptions::default()).unwrap();
        let (width, height) = (1280, 800);
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = dev.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("UI scale review"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                &scene,
                &texture.create_view(&Default::default()),
                &vello::RenderParams {
                    base_color: Color::WHITE,
                    width,
                    height,
                    antialiasing_method: vello::AaConfig::Area,
                },
            )
            .unwrap();
        let padded = (width * 4).div_ceil(256) * 256;
        let buffer = dev.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = dev.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            size,
        );
        dev.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
        dev.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        let pixels: Vec<u8> = mapped
            .chunks(padded as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect();
        image::save_buffer(output, &pixels, width, height, image::ColorType::Rgba8).unwrap();
    }

    #[test]
    fn boot_chooser_is_not_an_open_document() {
        let app = App::new();
        assert!(app.boot_empty);
        assert!(!app.document_open());
        assert!(!app.doc.editor.document().layers().is_empty());
    }

    #[test]
    fn focused_chooser_pane_is_not_an_open_document() {
        let mut app = App::new();
        let first = app.mux.model.focused;
        let id = app.doc.id;
        app.mux.model.pane_mut(first).unwrap().tabs[0].content = TabContent::Document(id);
        app.boot_empty = false;
        assert!(app.document_open());
        let second = app.mux.model.split(Axis::Horizontal).unwrap();
        app.mux_focus(second);
        assert!(!app.document_open());
        app.mux_focus(first);
        assert!(app.document_open());
    }
}
