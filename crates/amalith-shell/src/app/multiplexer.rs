//! Pane focus routes the existing editor and PTY controllers; document IDs
//! resolve to the same live Editor regardless of how many tabs show them.
//! Every pane owns a real list of tabs (`multiplexer::Tab`) — there is no
//! "one content per pane" anymore; see the module doc comment on
//! `crate::multiplexer` for the tree-level invariants (a pane always has
//! at least one tab; closing the last one is the only way a pane closes).
use super::*;
use crate::multiplexer::{Multiplexer, PaneId, TabContent};

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
const CHOOSER_MARK_SIZE: f64 = 34.0;
const CHOOSER_MARK_GAP: f64 = 18.0;

/// The app mark, decoded once from the same asset `home.rs`'s welcome
/// screen uses (`assets/home/mark.png`) and cached — cheap to reuse a
/// few times a frame, not cheap to re-decode a PNG every frame.
fn chooser_mark() -> Option<&'static vello::peniko::ImageData> {
    static CACHE: std::sync::OnceLock<Option<vello::peniko::ImageData>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            const MARK_PNG: &[u8] = include_bytes!("../../assets/home/mark.png");
            let (rgba, w, h) = crate::appicon::decode_png(MARK_PNG)?;
            Some(vello::peniko::ImageData {
                data: vello::peniko::Blob::from(rgba),
                format: vello::peniko::ImageFormat::Rgba8,
                alpha_type: vello::peniko::ImageAlphaType::Alpha,
                width: w,
                height: h,
            })
        })
        .as_ref()
}
/// Draws `img` centered on `center`, scaled (preserving aspect) to fit
/// within a `max_size` square.
fn draw_mark(scene: &mut Scene, img: &vello::peniko::ImageData, center: Point, max_size: f64) {
    let (iw, ih) = (img.width as f64, img.height as f64);
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let scale = max_size / iw.max(ih);
    let dst = Rect::new(center.x - iw * scale * 0.5, center.y - ih * scale * 0.5, center.x + iw * scale * 0.5, center.y + ih * scale * 0.5);
    let xf = Affine::translate((dst.x0, dst.y0)) * Affine::scale(scale);
    scene.draw_image(img, xf);
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
        + ui_px(CHOOSER_MARK_SIZE) + ui_px(CHOOSER_MARK_GAP)
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
/// Top of the "GET STARTED" section, below the mark.
fn chooser_actions_top(r: Rect, n_open: usize, n_recent: usize, scroll: f64) -> f64 {
    chooser_content_top(r, n_open, n_recent, scroll) + ui_px(CHOOSER_PAD) + ui_px(CHOOSER_MARK_SIZE) + ui_px(CHOOSER_MARK_GAP)
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

fn icon_plus(scene: &mut Scene, c: Point, radius: f64, color: Color) {
    let s = Stroke::new(ui_px(2.0));
    scene.stroke(&s, Affine::IDENTITY, color, None, &vello::kurbo::Line::new((c.x - radius, c.y), (c.x + radius, c.y)));
    scene.stroke(&s, Affine::IDENTITY, color, None, &vello::kurbo::Line::new((c.x, c.y - radius), (c.x, c.y + radius)));
}
fn icon_terminal(scene: &mut Scene, box_: Rect, color: Color) {
    scene.stroke(&Stroke::new(ui_px(1.6)), Affine::IDENTITY, color, None, &box_.to_rounded_rect(ui_px(3.0)));
    let (cx, cy) = (box_.x0 + box_.width() * 0.3, box_.center().y);
    let a = box_.height() * 0.17;
    let mut chevron = BezPath::new();
    chevron.move_to((cx - a, cy - a));
    chevron.line_to((cx + a, cy));
    chevron.line_to((cx - a, cy + a));
    scene.stroke(&Stroke::new(ui_px(1.8)), Affine::IDENTITY, color, None, &chevron);
    let uy = cy + a * 0.9;
    scene.stroke(
        &Stroke::new(ui_px(1.8)), Affine::IDENTITY, color, None,
        &vello::kurbo::Line::new((cx + a * 0.5, uy), (cx + a * 1.8, uy)),
    );
}
fn icon_import(scene: &mut Scene, box_: Rect, color: Color) {
    let fold = box_.width().min(box_.height()) * 0.3;
    let mut page = BezPath::new();
    page.move_to((box_.x0, box_.y0));
    page.line_to((box_.x1 - fold, box_.y0));
    page.line_to((box_.x1, box_.y0 + fold));
    page.line_to((box_.x1, box_.y1));
    page.line_to((box_.x0, box_.y1));
    page.close_path();
    scene.stroke(&Stroke::new(ui_px(1.6)), Affine::IDENTITY, color, None, &page);
    let mut corner = BezPath::new();
    corner.move_to((box_.x1 - fold, box_.y0));
    corner.line_to((box_.x1 - fold, box_.y0 + fold));
    corner.line_to((box_.x1, box_.y0 + fold));
    scene.stroke(&Stroke::new(ui_px(1.2)), Affine::IDENTITY, color, None, &corner);
    let cx = box_.x0 + box_.width() * 0.38;
    let (ay0, ay1) = (box_.y0 + box_.height() * 0.38, box_.y1 - box_.height() * 0.2);
    let arm = box_.width() * 0.12;
    let mut arrow = BezPath::new();
    arrow.move_to((cx, ay0));
    arrow.line_to((cx, ay1));
    arrow.move_to((cx - arm, ay1 - arm));
    arrow.line_to((cx, ay1));
    arrow.line_to((cx + arm, ay1 - arm));
    scene.stroke(&Stroke::new(ui_px(1.6)), Affine::IDENTITY, color, None, &arrow);
}
/// Paints one action card: border, icon, label. `kind` selects the icon
/// (0 = New Document, 1 = Terminal, 2 = Import).
/// Paints one "GET STARTED" action row: icon, label, hover highlight —
/// a plain list row, not a bordered box (see the module doc comment for
/// why). `kind` selects the icon (0 = New Document, 1 = Terminal,
/// 2 = Import).
fn paint_chooser_action_row(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, kind: usize, hover: bool) {
    if hover {
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_active, None, &r.to_rounded_rect(ui_px(4.0)));
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
/// Raster Effects — everything the old full-page dialog offered except
/// Preview Mode (the one control rarely touched after a document's
/// created; still there via `newdoc.rs`'s own dialog if ever wanted).
const QND_ROWS: usize = 7;

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
/// The field a dropdown (`newdoc::Menu`) opens from.
fn qnd_menu_trigger_rect(card: Rect, menu: newdoc::Menu) -> Rect {
    match menu {
        newdoc::Menu::Unit => qnd_unit_rect(card),
        newdoc::Menu::Color => qnd_color_rect(card),
        newdoc::Menu::Raster | newdoc::Menu::Preview => qnd_raster_rect(card),
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
        newdoc::Menu::Preview => 0,
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
        newdoc::Menu::Preview => Vec::new(),
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

/// Small line-art portrait/landscape glyph — the same visual idea as the
/// old full-page dialog's own orientation icons, just redrawn locally
/// (that dialog's are baked into its own fixed layout, not reusable here).
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
    // convention as `widgets::button` and the old full-page dialog's own
    // `draw_orient`/`draw_stepper` (which this card's controls otherwise
    // drifted from by routing through `field_bg`'s rounded, subtler
    // field look instead).
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
                newdoc::Menu::Preview => None,
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
            }
            TabContent::Chooser => {
                self.mux.origin = None;
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
    /// ⌘N / File ▸ New, and the pane tab strip's own "+" button. Always
    /// a *new* tab; it never jumps straight to the New Document overlay
    /// — the user picks what goes in it from the chooser, same as any
    /// other pane.
    pub(super) fn mux_new_tab(&mut self) {
        let pane = self.mux.model.focused;
        self.mux_save_view();
        self.mux_park_terminal();
        if self.mux.model.add_tab(pane, TabContent::Chooser).is_some() {
            self.mux_rebind(pane);
            self.request_main_redraw();
        }
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
    pub(super) fn mux_press(&mut self) -> bool {
        if !self.mux.model.enabled() {
            return false;
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
                    self.import_svg_as_new_doc();
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
                                canvas::paint(
                                    &mut back,
                                    doc.editor.document(),
                                    &view,
                                    body(r),
                                    &self.theme,
                                    &mut self.text,
                                    &[],
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

                        if let Some(mark) = chooser_mark() {
                            let c = chooser_mark_center(r, n_open, n_recent, scroll);
                            draw_mark(&mut back, mark, c, ui_px(CHOOSER_MARK_SIZE));
                        }
                        let (col_x0, col_x1) = chooser_col_x(r);
                        let ahy = chooser_actions_header_y(r, n_open, n_recent, scroll);
                        paint_chooser_section_header(&mut back, &mut self.text, &self.theme, col_x0, col_x1, ahy, "GET STARTED");
                        const ACTIONS: [(&str, usize); CHOOSER_ACTIONS] = [
                            ("Create a new Document", 0),
                            ("Start a shell in this pane", 1),
                            ("Import", 2),
                        ];
                        for (i, (label, kind)) in ACTIONS.iter().enumerate() {
                            let ar = chooser_action_row(r, i, n_open, n_recent, scroll);
                            let hover = ar.contains(self.pointer);
                            paint_chooser_action_row(&mut back, &mut self.text, &self.theme, ar, label, *kind, hover);
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
                    self.text.draw(
                        &mut front, &labels[i], 12.6, ink,
                        close.x1 + ui_px(6.0), strip.y0 + metric_tab_bar_h() * 0.5 + ui_px(4.0),
                    );
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
            let chord = |act: prefs::PrefAction| {
                prefs::PrefAction::ALL
                    .iter()
                    .position(|a| *a == act)
                    .and_then(|i| self.settings.action_keys[i])
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "—".to_string())
            };
            let text = if self.mux.model.prefix {
                format!(
                    "PREFIX   {}  Split Right   {}  Split Down   {} / {}  Focus pane   {}  Close tab   Esc  Cancel",
                    chord(prefs::PrefAction::MuxSplitRight),
                    chord(prefs::PrefAction::MuxSplitDown),
                    chord(prefs::PrefAction::MuxFocusPrev),
                    chord(prefs::PrefAction::MuxFocusNext),
                    chord(prefs::PrefAction::MuxCloseTab),
                )
            } else {
                format!(
                    "{}  Pane commands     Click a pane to focus",
                    chord(prefs::PrefAction::MuxPrefix),
                )
            };
            self.text.draw(
                &mut front,
                &text,
                12.,
                if self.mux.model.prefix {
                    self.theme.accent
                } else {
                    self.theme.text_dim
                },
                bar.x0 + ui_px(10.),
                bar.y0 + ui_px(19.),
            );
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
        // A dropdown eats every click while it's open — same priority
        // order as the old full-page dialog's own `newdoc::hit`: a click
        // on one of its items picks that value and closes it; any other
        // click (even one that would otherwise hit Create/a field) just
        // closes the dropdown instead of also acting on whatever's under
        // it. That's what stopped the accidental double-action clicking
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
                        newdoc::Menu::Preview => {}
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
    /// Build a fresh document from the compact overlay's form and swap it
    /// in — the same document-building path as the old full-page dialog
    /// (`App::build_editor_from_form`), always adding a new tab (never
    /// the boot in-place replace `create_from_form` does — the overlay
    /// is reachable from a live session, not just an empty boot).
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
}
