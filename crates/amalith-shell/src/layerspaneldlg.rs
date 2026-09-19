//! Layers Panel Options — Thumbnail Size / Thumbnail Contents. Opened
//! from the Layers panel's own hamburger menu ("Panel Options…"),
//! deliberately *not* Preferences (this is a per-panel display setting,
//! same reasoning as the Symbols panel's own List/Thumbnails toggle) —
//! see [`crate::prefs::Settings::layer_thumbnail_size`]/
//! `layer_thumbnail_contents`, which this dialog edits a working copy of
//! and commits to on OK. Layout/paint/hit live here; `app/
//! layer_panel_options_dialog.rs` is the `App`-side glue (spawning the
//! floating window, closing it), mirroring `layerdlg.rs`/`app/
//! layer_dialog.rs`.

use crate::metrics::px as ui_px;
use crate::prefs::{LayerThumbnailContents, LayerThumbnailSize, Settings};
use crate::text::TextContext;
use crate::theme::Theme;
use vello::kurbo::{Affine, Circle, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

const ID: Affine = Affine::IDENTITY;

fn metric_pad() -> f64 { crate::metrics::with(|m| m.layerdlg_pad) }
fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.layerdlg_btn_h) }
/// Shared with `xformdlg`'s own radio buttons — a generic "radio ring
/// radius" constant, not something semantically tied to that one dialog.
fn metric_radio_r() -> f64 { crate::metrics::with(|m| m.xformdlg_radio_r) }

pub fn metric_w() -> f64 { ui_px(240.0) }

const SIZE_OPTIONS: [(&str, LayerThumbnailSize); 4] = [
    ("None", LayerThumbnailSize::None),
    ("Small", LayerThumbnailSize::Small),
    ("Medium", LayerThumbnailSize::Medium),
    ("Large", LayerThumbnailSize::Large),
];
const CONTENTS_OPTIONS: [(&str, LayerThumbnailContents); 2] = [
    ("Layer Bounds", LayerThumbnailContents::LayerBounds),
    ("Entire Document", LayerThumbnailContents::EntireDocument),
];

const ROW_H: f64 = 24.0;
const HEADER_H: f64 = 20.0;
const SECTION_GAP: f64 = 16.0;

pub struct LayersPanelOptionsDialog {
    pub size: LayerThumbnailSize,
    pub contents: LayerThumbnailContents,
}

impl LayersPanelOptionsDialog {
    /// Seeds a working copy from the real, currently-saved settings —
    /// nothing here touches `Settings` itself until OK.
    pub fn open(settings: &Settings) -> Self {
        Self { size: settings.layer_thumbnail_size, contents: settings.layer_thumbnail_contents }
    }
}

struct Layout {
    /// Baseline for the "Thumbnail Size" label.
    size_header_y: f64,
    size_rows: Vec<Rect>,
    /// Baseline for the "Thumbnail Contents" label.
    contents_header_y: f64,
    contents_rows: Vec<Rect>,
    ok: Rect,
    cancel: Rect,
}

fn layout(body: Rect) -> Layout {
    let x0 = body.x0 + metric_pad();
    let x1 = body.x1 - metric_pad();
    let mut y = body.y0 + metric_pad();
    let size_header_y = y + ui_px(HEADER_H) - ui_px(8.0);
    y += ui_px(HEADER_H);
    let size_rows: Vec<Rect> = (0..SIZE_OPTIONS.len())
        .map(|_| {
            let r = Rect::new(x0, y, x1, y + ui_px(ROW_H));
            y += ui_px(ROW_H);
            r
        })
        .collect();
    y += ui_px(SECTION_GAP);
    let contents_header_y = y + ui_px(HEADER_H) - ui_px(8.0);
    y += ui_px(HEADER_H);
    let contents_rows: Vec<Rect> = (0..CONTENTS_OPTIONS.len())
        .map(|_| {
            let r = Rect::new(x0, y, x1, y + ui_px(ROW_H));
            y += ui_px(ROW_H);
            r
        })
        .collect();
    y += ui_px(SECTION_GAP);
    let btn_w = ui_px(72.0);
    let ok = Rect::new(x1 - btn_w, y, x1, y + metric_btn_h());
    let cancel = Rect::new(ok.x0 - ui_px(8.0) - btn_w, y, ok.x0 - ui_px(8.0), y + metric_btn_h());
    Layout { size_header_y, size_rows, contents_header_y, contents_rows, ok, cancel }
}

/// Body height (window-local, excluding the tab strip) — fixed, since
/// neither radio group changes how much space the other needs.
pub fn body_height() -> f64 {
    metric_pad()
        + ui_px(HEADER_H)
        + SIZE_OPTIONS.len() as f64 * ui_px(ROW_H)
        + ui_px(SECTION_GAP)
        + ui_px(HEADER_H)
        + CONTENTS_OPTIONS.len() as f64 * ui_px(ROW_H)
        + ui_px(SECTION_GAP)
        + metric_btn_h()
        + metric_pad()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Size(LayerThumbnailSize),
    Contents(LayerThumbnailContents),
    Ok,
    Cancel,
    None,
}

/// Pure geometry — neither radio group's *hit-testing* depends on which
/// entry is currently selected, only its *paint* does (to highlight it).
pub fn hit(body: Rect, p: Point) -> Hit {
    let lay = layout(body);
    for (i, r) in lay.size_rows.iter().enumerate() {
        if r.contains(p) {
            return Hit::Size(SIZE_OPTIONS[i].1);
        }
    }
    for (i, r) in lay.contents_rows.iter().enumerate() {
        if r.contains(p) {
            return Hit::Contents(CONTENTS_OPTIONS[i].1);
        }
    }
    if lay.ok.contains(p) {
        return Hit::Ok;
    }
    if lay.cancel.contains(p) {
        return Hit::Cancel;
    }
    Hit::None
}

/// A section label with a thin rule filling the rest of the row —
/// Photoshop's own "Thumbnail Size ────" framing, simplified to a label
/// + line instead of a full bordered group box.
fn draw_section_header(scene: &mut Scene, text: &mut TextContext, theme: &Theme, x0: f64, x1: f64, y: f64, label: &str) {
    let w = text.measure(label, 12.5);
    text.draw(scene, label, 12.5, theme.text_dim, x0, y);
    let line_x0 = x0 + w + ui_px(8.0);
    if line_x0 < x1 {
        scene.fill(Fill::NonZero, ID, theme.border, None, &Rect::new(line_x0, y - 1.0, x1, y));
    }
}

fn draw_radio(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, selected: bool) {
    let c = Point::new(r.x0 + metric_radio_r(), r.center().y);
    let ring = Circle::new(c, metric_radio_r());
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &ring);
    if selected {
        scene.fill(Fill::NonZero, ID, theme.accent, None, &Circle::new(c, metric_radio_r() * 0.5));
    }
    text.draw(scene, label, 12.5, theme.text, r.x0 + metric_radio_r() * 2.0 + ui_px(10.0), r.center().y + ui_px(4.5));
}

pub fn paint(scene: &mut Scene, dlg: &LayersPanelOptionsDialog, body: Rect, theme: &Theme, text: &mut TextContext) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(body);
    let x0 = body.x0 + metric_pad();
    let x1 = body.x1 - metric_pad();

    draw_section_header(scene, text, theme, x0, x1, lay.size_header_y, "Thumbnail Size");
    for (i, (label, kind)) in SIZE_OPTIONS.iter().enumerate() {
        draw_radio(scene, text, theme, lay.size_rows[i], label, dlg.size == *kind);
    }

    draw_section_header(scene, text, theme, x0, x1, lay.contents_header_y, "Thumbnail Contents");
    for (i, (label, kind)) in CONTENTS_OPTIONS.iter().enumerate() {
        draw_radio(scene, text, theme, lay.contents_rows[i], label, dlg.contents == *kind);
    }

    crate::widgets::button(scene, text, theme, lay.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, lay.ok, "OK", true);
}
