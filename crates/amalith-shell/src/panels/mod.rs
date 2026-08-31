//! Panel body content: what fills a docked panel below its tab strip.
//!
//! One file per panel — [`tools`], [`layers`], [`artboards`], [`swatches`]
//! — each exposing `paint` and `hit`. This module owns the shared
//! vocabulary ([`Ctx`], [`Action`]) and the widgets they have in common
//! (footer button strip, inline-rename field, paint swatch). Dispatch is
//! still a direct `match` on the panel id string, not the `Panel` trait.

mod artboards;
pub mod character;
mod layers;
mod swatches;
pub mod tools;

use std::collections::HashSet;

use amalith_core::{
    Appearance, ArtboardId, Color as CoreColor, Document, LayerId, ObjectId, Paint,
};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::dock::PanelId;
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

const ID: Affine = Affine::IDENTITY;
const ROW_H: f64 = 26.0;
const PAD: f64 = 10.0;
const SWATCH: f64 = 22.0;
/// Height of a panel's bottom button strip.
const FOOTER_H: f64 = 30.0;

/// Which paint a swatch click targets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaintSlot {
    Fill,
    Stroke,
}

/// A renameable entity — the target of an inline panel edit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameId {
    Layer(LayerId),
    Object(ObjectId),
    Artboard(ArtboardId),
}

/// The preset palette (plus a leading `Paint::None`).
pub fn palette() -> Vec<Paint> {
    let rgb = |r: f32, g: f32, b: f32| Paint::Solid(CoreColor::rgb(r, g, b));
    vec![
        Paint::None,
        rgb(0.0, 0.0, 0.0),
        rgb(0.33, 0.33, 0.33),
        rgb(0.6, 0.6, 0.6),
        rgb(0.85, 0.85, 0.85),
        rgb(1.0, 1.0, 1.0),
        rgb(0.90, 0.20, 0.18),
        rgb(0.96, 0.55, 0.15),
        rgb(0.98, 0.80, 0.18),
        rgb(0.40, 0.75, 0.30),
        rgb(0.18, 0.60, 0.55),
        rgb(0.20, 0.48, 0.90),
        rgb(0.42, 0.32, 0.82),
        rgb(0.80, 0.28, 0.62),
    ]
}

/// Read-only context a panel body draws from.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    pub doc: &'a Document,
    pub selection: &'a [ObjectId],
    pub active_tool: Tool,
    /// Cursor position in screen px, for hover styling.
    pub pointer: Point,
    /// Appearance of the first selected object, if any (for the swatches).
    pub representative: Option<Appearance>,
    pub active_slot: PaintSlot,
    /// Which primitive tool the Tools-panel Shape slot stands in for.
    pub shape_tool: Tool,
    /// Group ids the Layers panel currently shows expanded.
    pub expanded: &'a HashSet<ObjectId>,
    /// The row being inline-renamed, and its current edit buffer.
    pub renaming: Option<(RenameId, &'a str)>,
    /// Panel-row selection highlights.
    pub selected_layer: Option<LayerId>,
    pub selected_artboard: Option<ArtboardId>,
    /// The type style the Character panel edits — the live text edit, else
    /// the selected text object, else the "new text" defaults.
    pub text_style: amalith_core::TextStyle,
    /// True while a text object has the caret (Character panel shows "live").
    pub text_editing: bool,
    /// Installed font family names, sorted (for the family dropdown).
    pub font_families: &'a [String],
    /// Layers panel: the current filter text (empty = show everything).
    pub layer_query: &'a str,
    /// Layers panel: whether the search field holds keyboard focus.
    pub layer_search_focused: bool,
}

/// A character-attribute flag toggled from the Character panel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextFlag {
    Underline,
    Strikethrough,
    SmallCaps,
    Superscript,
    Subscript,
    AllCaps,
}

/// Which Character-panel dropdown to open.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontMenu {
    Family,
    Style,
    Size,
}

/// What a click in a panel body asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    None,
    SetTool(Tool),
    Select(ObjectId),
    /// Layers panel: a layer-header row was clicked.
    SelectLayer(LayerId),
    /// Artboards panel: an artboard row was clicked.
    SelectArtboard(ArtboardId),
    /// Artboards panel: the artboard's number was clicked — double-click
    /// snaps the view back onto it.
    FocusArtboard(ArtboardId),
    SetActiveSlot(PaintSlot),
    /// Open the colour picker for this slot.
    OpenPicker(PaintSlot),
    SetPaint(Paint),
    SetStrokeWidth(f64),
    /// Layers panel: flip an object's `visible` / `locked` flag.
    ToggleVisible(ObjectId),
    ToggleLocked(ObjectId),
    /// Layers panel: expand / collapse a group row.
    ToggleExpand(ObjectId),
    /// Layers panel: the search field was clicked — give it keyboard focus.
    FocusLayerSearch,
    /// Tools panel: the Shape slot was clicked (tap = last shape tool,
    /// hold = flyout).
    ShapeSlot,
    /// Panel footer buttons.
    NewLayer,
    NewArtboard,
    /// Layers footer: restack the selection (+1 up / −1 down).
    LayerRestack(i32),
    /// Layers footer: delete the object selection.
    DeleteObjects,
    /// Artboards footer: delete the selected artboard.
    DeleteArtboard,
    // --- Character panel ---
    SetFontFamily(String),
    SetFontFace { weight: u16, italic: bool },
    SetFontSize(f64),
    /// `None` = auto leading.
    SetLeading(Option<f64>),
    SetTracking(f64),
    ToggleTextFlag(TextFlag),
    /// Open a Character-panel dropdown, anchored at the given screen rect.
    OpenFontMenu(FontMenu, Rect),
}

/// Draw panel `id`'s body into `body`.
pub fn paint(scene: &mut Scene, text: &mut TextContext, id: PanelId, body: Rect, ctx: &Ctx) {
    match id.0 {
        "tools" => tools::paint(scene, text, body, ctx),
        "layers" => layers::paint(scene, text, body, ctx),
        "artboards" => artboards::paint(scene, text, body, ctx),
        "swatches" => swatches::paint(scene, text, body, ctx),
        "character" => character::paint(scene, text, body, ctx),
        _ => {}
    }
}

/// Resolve a click at `local` (panel-body coordinates, same space as
/// `body`) into an [`Action`].
pub fn hit(id: PanelId, body: Rect, local: Point, ctx: &Ctx) -> Action {
    match id.0 {
        "tools" => tools::hit(body, local, ctx),
        "layers" => layers::hit(body, local, ctx),
        "artboards" => artboards::hit(body, local, ctx),
        "swatches" => swatches::hit(body, local, ctx),
        "character" => character::hit(body, local, ctx),
        _ => Action::None,
    }
}

/// A panel's natural body height — the shortest it can be before its
/// content would be clipped. The dock uses this to stop a splitter drag
/// from shrinking a stacked panel down over its own contents. Fixed-layout
/// panels report their full height; list panels report a short floor and
/// clip past it.
pub fn min_body_height(id: PanelId, width: f64) -> f64 {
    match id.0 {
        "character" => character::natural_height(),
        "tools" => tools::natural_height(width),
        "layers" => layers::SEARCH_H + ROW_H * 2.0 + FOOTER_H,
        "artboards" | "swatches" => 132.0,
        _ => 60.0,
    }
}

/// Hover text for the control at `local` in panel `id`'s body, if any.
pub fn tip(id: PanelId, body: Rect, local: Point, ctx: &Ctx) -> Option<String> {
    match id.0 {
        "tools" => tools::tip(body, local, ctx),
        "character" => character::tip(body, local, ctx).map(str::to_string),
        _ => None,
    }
}

// ---- shared widgets --------------------------------------------------

fn row_rect(body: Rect, i: usize) -> Rect {
    let y = body.y0 + i as f64 * ROW_H;
    Rect::new(body.x0, y, body.x1, y + ROW_H)
}

/// The four footer button rects, left→right: move-up, move-down, add,
/// delete — right-aligned in the strip along `body`'s bottom edge.
fn panel_footer_rects(body: Rect) -> [Rect; 4] {
    let sz = 20.0;
    let gap = 10.0;
    let cy = body.y1 - FOOTER_H * 0.5;
    std::array::from_fn(|k| {
        let cx = body.x1 - PAD - (3 - k) as f64 * (sz + gap) - sz * 0.5;
        Rect::from_center_size(Point::new(cx, cy), (sz, sz))
    })
}

fn footer_color(theme: &Theme, enabled: bool, hot: bool) -> Color {
    if !enabled {
        theme.border
    } else if hot {
        theme.text
    } else {
        theme.text_dim
    }
}

/// Draws the footer strip and its four icons. `enabled` gates each of
/// [up, down, add, delete].
fn paint_panel_footer(scene: &mut Scene, body: Rect, theme: &Theme, pointer: Point, enabled: [bool; 4]) {
    let strip = Rect::new(body.x0, body.y1 - FOOTER_H, body.x1, body.y1);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &strip);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(strip.x0, strip.y0, strip.x1, strip.y0 + 1.0),
    );
    let rects = panel_footer_rects(body);
    for (k, r) in rects.iter().enumerate() {
        let c = footer_color(theme, enabled[k], r.contains(pointer));
        match k {
            0 => draw_footer_arrow(scene, *r, true, c),
            1 => draw_footer_arrow(scene, *r, false, c),
            2 => draw_footer_plus(scene, *r, c),
            _ => draw_footer_trash(scene, *r, c),
        }
    }
}

fn draw_footer_arrow(scene: &mut Scene, r: Rect, up: bool, color: Color) {
    let cx = r.center().x;
    let (y_head, y_tail, y_tip) = if up {
        (r.y0 + 6.0, r.y1 - 3.0, r.y0 + 2.0)
    } else {
        (r.y1 - 6.0, r.y0 + 3.0, r.y1 - 2.0)
    };
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((cx, y_tail), (cx, y_tip)),
    );
    let mut head = BezPath::new();
    head.move_to((cx - 4.0, y_head));
    head.line_to((cx, y_tip));
    head.line_to((cx + 4.0, y_head));
    scene.stroke(&Stroke::new(1.6), ID, color, None, &head);
}

fn draw_footer_plus(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 5.0, c.y), (c.x + 5.0, c.y)),
    );
    scene.stroke(
        &Stroke::new(1.6),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x, c.y - 5.0), (c.x, c.y + 5.0)),
    );
}

fn draw_footer_trash(scene: &mut Scene, r: Rect, color: Color) {
    let c = r.center();
    let can = Rect::new(c.x - 4.5, c.y - 2.5, c.x + 4.5, c.y + 6.0);
    scene.stroke(&Stroke::new(1.4), ID, color, None, &can);
    scene.stroke(
        &Stroke::new(1.4),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 7.0, c.y - 2.5), (c.x + 7.0, c.y - 2.5)),
    );
    scene.stroke(
        &Stroke::new(1.4),
        ID,
        color,
        None,
        &vello::kurbo::Line::new((c.x - 2.0, c.y - 5.0), (c.x + 2.0, c.y - 5.0)),
    );
}

/// Draw a row's name: either the plain `label`, or — when `editing` is
/// `Some(buffer)` — an inline text field with the buffer and a caret.
fn draw_name_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    x: f64,
    row: Rect,
    label: &str,
    color: Color,
    editing: Option<&str>,
) {
    let baseline = row.y0 + row.height() * 0.5 + 4.0;
    match editing {
        None => text.draw(scene, label, 12.0, color, x, baseline),
        Some(buf) => {
            let field = Rect::new(x - 4.0, row.y0 + 3.0, row.x1 - PAD, row.y1 - 3.0);
            scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
            scene.stroke(&Stroke::new(1.25), ID, theme.accent, None, &field);
            text.draw(scene, buf, 12.0, theme.text, x, baseline);
            let caret_x = x + text.measure(buf, 12.0) + 1.0;
            scene.stroke(
                &Stroke::new(1.0),
                ID,
                theme.text,
                None,
                &vello::kurbo::Line::new((caret_x, row.y0 + 5.0), (caret_x, row.y1 - 5.0)),
            );
        }
    }
}

/// A single fill / stroke colour chip. `active` gives it the blue border.
pub fn draw_paint_swatch(scene: &mut Scene, theme: &Theme, r: Rect, paint: Paint, active: bool) {
    match paint {
        Paint::None => {
            scene.fill(Fill::NonZero, ID, Color::from_rgb8(0xff, 0xff, 0xff), None, &r);
            let mut slash = BezPath::new();
            slash.move_to((r.x0, r.y1));
            slash.line_to((r.x1, r.y0));
            scene.stroke(&Stroke::new(1.5), ID, Color::from_rgb8(0xd0, 0x30, 0x30), None, &slash);
        }
        Paint::Solid(c) => {
            scene.fill(Fill::NonZero, ID, crate::convert::color(c), None, &r);
        }
    }
    let (w, col) = if active {
        (1.5, theme.accent)
    } else {
        (1.0, theme.border)
    };
    scene.stroke(&Stroke::new(w), ID, col, None, &r);
}
