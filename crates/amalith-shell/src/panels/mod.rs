//! Panel body content: what fills a docked panel below its tab strip.
//!
//! One file per panel — [`tools`], [`layers`], [`artboards`], [`swatches`]
//! — each exposing `paint` and `hit`. This module owns the shared
//! vocabulary ([`Ctx`], [`Action`]) and the widgets they have in common
//! (footer button strip, inline-rename field, paint swatch). Dispatch is
//! still a direct `match` on the panel id string, not the `Panel` trait.

mod artboards;
pub mod character;
pub mod color;
pub(crate) mod layers;
mod swatches;
pub mod tools;
pub mod transform;
pub mod pathfinder;
pub mod align;
pub mod paragraph;

use std::collections::HashSet;

use amalith_core::{
    Appearance, ArtboardId, Color as CoreColor, Document, LayerId, ObjectId, Paint, RefPoint,
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
    /// The selection has more than one distinct fill / stroke — the
    /// proxies show a "?" swatch.
    pub fill_mixed: bool,
    pub stroke_mixed: bool,
    pub active_slot: PaintSlot,
    /// Current state for the Color Picker panel, when it is open.
    pub picker: Option<crate::picker::Picker>,
    /// The Fill/Stroke proxy's current colours — shown when nothing is
    /// selected (there's no `representative` to read).
    pub cur_fill: amalith_core::Paint,
    pub cur_stroke: amalith_core::Paint,
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
    /// Paragraph alignment + attributes the Paragraph panel edits, from
    /// the same source as `text_style`.
    pub text_align: amalith_core::TextAlign,
    pub text_paragraph: amalith_core::Paragraph,
    /// True while a text object has the caret (Character panel shows "live").
    pub text_editing: bool,
    /// Installed font family names, sorted (for the family dropdown).
    pub font_families: &'a [String],
    /// Layers panel: the current filter text (empty = show everything).
    pub layer_query: &'a str,
    /// Layers panel: whether the search field holds keyboard focus.
    pub layer_search_focused: bool,
    /// Layers panel: wheel-scroll offset of the row list, px.
    pub layer_scroll: f64,
    /// Layers panel: live drag-reorder indicator — `(visible-row index, into)`.
    /// The drop line sits at the top of that row; when `into` is set the row
    /// itself (a group / layer) is outlined as the drop container instead.
    pub layer_drop: Option<(i64, bool)>,
    /// Color panel: RGB / HSB / CMYK slider set.
    pub color_mode: ColorSpace,
    /// Color panel: recently used solid colours, newest first.
    pub recent: &'a [CoreColor],
    /// Transform panel 9-point origin and W/H lock.
    pub xform_ref: RefPoint,
    pub xform_constrain: bool,
    /// Live numeric edit buffer, if a Transform field is being typed.
    pub xform_edit: Option<(transform::XformField, &'a str)>,
    pub align_to: amalith_commands::AlignTo,
    pub align_spacing: Option<f64>,
    /// Live buffer while the Align spacing field is being typed.
    pub align_spacing_edit: Option<&'a str>,
    /// Object that stays put for Align To Key Object (thicker outline).
    pub key_object: Option<ObjectId>,
    /// The exact-size shape dialog and its caret-blink phase, when one of
    /// the `shapedlg.*` float-only panels is being drawn / hit-tested.
    pub shape_dialog: Option<(&'a crate::shapedialog::ShapeDialog, bool)>,
    /// The Export for Screens dialog + caret-blink phase, when the
    /// `export-screens` float-only panel is being drawn / hit-tested.
    pub export: Option<(&'a crate::export::ExportForScreens, bool)>,
}

/// The primitive tool a `shapedlg.*` panel id stands for.
pub(crate) fn shape_dialog_tool(id: PanelId) -> Option<Tool> {
    Some(match id.0 {
        "shapedlg.rect" => Tool::Rectangle,
        "shapedlg.round" => Tool::RoundedRect,
        "shapedlg.ellipse" => Tool::Ellipse,
        "shapedlg.polygon" => Tool::Polygon,
        "shapedlg.star" => Tool::Star,
        _ => return None,
    })
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

/// A numeric field of the Paragraph panel (all in px / pt).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ParaField {
    IndentStart,
    IndentEnd,
    IndentFirst,
    SpaceBefore,
    SpaceAfter,
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
    PickerSv(f32, f32),
    PickerHue(f32),
    PickerCancel,
    PickerOk,
    /// Exact-size shape dialog.
    ShapeField(usize),
    ShapeStep(usize, i32),
    ShapeLink,
    ShapeOption(u32),
    ShapeCancel,
    ShapeOk,
    /// Export for Screens — the whole hit enum passes through; the App
    /// owns the state machine (`ExportForScreens::apply`).
    ExportHit(crate::export::Hit),
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
    // --- Paragraph panel ---
    SetTextAlign(amalith_core::TextAlign),
    /// Set one paragraph metric (px). `ParaField` picks which.
    SetParagraphMetric(ParaField, f64),
    ToggleHyphenate,
    // --- context bar ---
    /// Nudge the options-bar stroke Weight (`+1` / `-1`).
    StepWeight(i32),
    /// Nudge the options-bar Opacity (`+1` / `-1`).
    StepOpacity(i32),
    /// Nudge the options-bar font size by this many points.
    StepFontSize(f64),
    /// Open / close the Stroke flyout from its "Stroke" link.
    ToggleStrokeFlyout,
    /// Context bar "Anchor Point ▸ Convert": set every selected anchor to
    /// a smooth point (`true`) or a sharp corner (`false`).
    ConvertAnchor { smooth: bool },
    /// Fill/Stroke proxy: exchange the two paints.
    SwapPaints,
    /// Fill/Stroke proxy: reset to white fill / black stroke.
    DefaultPaints,
    /// An item from a panel's hamburger flyout (`id` is panel-defined).
    PanelMenu {
        panel: PanelId,
        id: &'static str,
    },
    /// Color panel: scrub slider `channel` to `t` (0..1). `track` is the
    /// screen rect so a drag can keep mapping the pointer.
    ColorScrub { channel: u8, t: f32, track: Rect },
    /// Color panel: pick a hue from the spectrum bar.
    ColorSpectrum { t: f32, track: Rect },
    // --- Transform panel ---
    SetXformRef(RefPoint),
    ToggleXformConstrain,
    BeginXformEdit(transform::XformField),
    NudgeXform {
        field: transform::XformField,
        delta: f64,
    },
    Pathfinder(amalith_commands::PathfinderOp),
    ExpandStroke,
    Align(amalith_commands::AlignKind),
    SetAlignTo(amalith_commands::AlignTo),
    BeginAlignSpacingEdit,
    /// Options-bar Align To dropdown, anchored at the button rect.
    OpenAlignToMenu(Rect),
}

/// One row in a panel hamburger flyout. Panels return these from [`menu`];
/// the shell draws and hit-tests them. Empty for now — the flyout still
/// opens so the chrome is in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        id: &'static str,
        label: &'static str,
        checked: bool,
    },
    Separator,
}

/// Flyout items for the hamburger on panel `id`.
pub fn menu(id: PanelId, ctx: &Ctx) -> Vec<MenuEntry> {
    match id.0 {
        "color" => color::menu(ctx),
        "transform" => transform::menu(ctx),
        "align" => align::menu(ctx),
        _ => Vec::new(),
    }
}

/// Whether the hamburger should show on `id`'s tab strip. Hidden when
/// [`menu`] is empty so unused chrome stays out of the way.
pub fn has_menu(id: PanelId) -> bool {
    matches!(id.0, "color" | "transform" | "align")
}

pub use color::ColorSpace;

/// Full content height of the Layers panel for the given document state —
/// the shell's wheel handler uses it to size the scroll range.
pub fn layers_content_height(
    doc: &Document,
    expanded: &std::collections::HashSet<ObjectId>,
    query: &str,
) -> f64 {
    layers::content_height(doc, expanded, query)
}

/// Draw panel `id`'s body into `body`.
pub fn paint(scene: &mut Scene, text: &mut TextContext, id: PanelId, body: Rect, ctx: &Ctx) {
    match id.0 {
        "tools" => tools::paint(scene, text, body, ctx),
        "layers" => layers::paint(scene, text, body, ctx),
        "artboards" => artboards::paint(scene, text, body, ctx),
        "swatches" => swatches::paint(scene, text, body, ctx),
        "character" => character::paint(scene, text, body, ctx),
        "color" => color::paint(scene, text, body, ctx),
        "transform" => transform::paint(scene, text, body, ctx),
        "pathfinder" => pathfinder::paint(scene, text, body, ctx),
        "align" => align::paint(scene, text, body, ctx),
        "paragraph" => paragraph::paint(scene, text, body, ctx),
        "picker" => {
            if let Some(pk) = ctx.picker {
                let mut local = pk;
                local.origin = Point::new(body.x0, body.y0);
                crate::picker::paint(scene, &local, ctx.theme.text, ctx.theme, text);
            }
        }
        s if shape_dialog_tool(PanelId(s)).is_some() => {
            if let Some((dlg, caret)) = ctx.shape_dialog {
                crate::shapedialog::paint(scene, dlg, body, ctx.theme, text, caret);
            }
        }
        "export-screens" => {
            if let Some((dlg, caret)) = ctx.export {
                crate::export::paint(scene, dlg, body, ctx.theme, text, caret, ctx.doc);
            }
        }
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
        "color" => color::hit(body, local, ctx),
        "transform" => transform::hit(body, local, ctx),
        "pathfinder" => pathfinder::hit(body, local, ctx),
        "align" => align::hit(body, local, ctx),
        "paragraph" => paragraph::hit(body, local, ctx),
        "picker" => {
            ctx.picker.map_or(Action::None, |mut pk| {
                pk.origin = Point::new(body.x0, body.y0);
                match crate::picker::hit(&pk, local) {
                crate::picker::Hit::Sv(s, v) => Action::PickerSv(s, v),
                crate::picker::Hit::Hue(h) => Action::PickerHue(h),
                crate::picker::Hit::Cancel => Action::PickerCancel,
                crate::picker::Hit::Ok => Action::PickerOk,
                _ => Action::None,
                }
            })
        }
        s if shape_dialog_tool(PanelId(s)).is_some() => {
            match ctx.shape_dialog.map(|(d, _)| d.hit(body, local)) {
                Some(crate::shapedialog::Hit::Field(i)) => Action::ShapeField(i),
                Some(crate::shapedialog::Hit::Step(i, d)) => Action::ShapeStep(i, d),
                Some(crate::shapedialog::Hit::Link) => Action::ShapeLink,
                Some(crate::shapedialog::Hit::Option(tag)) => Action::ShapeOption(tag),
                Some(crate::shapedialog::Hit::Cancel) => Action::ShapeCancel,
                Some(crate::shapedialog::Hit::Ok) => Action::ShapeOk,
                _ => Action::None,
            }
        }
        "export-screens" => match ctx.export {
            Some((dlg, _)) => Action::ExportHit(crate::export::hit(dlg, body, local)),
            None => Action::None,
        },
        _ => Action::None,
    }
}

/// Smallest body a splitter drag may leave a panel with, in a rail. The
/// fixed-layout panels (align, transform, …) can be dragged well below
/// their content height now that they scroll — they only need room for a
/// couple of rows plus the scrollbar. The list panels keep their real
/// functional minimum ([`min_body_height`]).
pub fn rail_floor(id: PanelId, width: f64) -> f64 {
    if fixed_content_height(id, width).is_some() {
        52.0
    } else {
        min_body_height(id, width)
    }
}

/// A panel's natural body height — the shortest it can be before its
/// content would be clipped. Fixed-layout panels report their full
/// height; list panels report a short floor and clip past it.
pub fn min_body_height(id: PanelId, width: f64) -> f64 {
    match id.0 {
        "character" => character::natural_height(),
        "tools" => tools::natural_height(width),
        "layers" => layers::SEARCH_H + ROW_H * 2.0 + FOOTER_H,
        "artboards" | "swatches" => 132.0,
        "color" => color::NATURAL_H,
        "transform" => transform::natural_height(),
        "pathfinder" => pathfinder::natural_height(),
        "align" => align::natural_height(),
        "paragraph" => paragraph::natural_height(),
        "picker" => crate::picker::H,
        "export-screens" => crate::export::H,
        s if shape_dialog_tool(PanelId(s)).is_some() => {
            crate::shapedialog::body_height(shape_dialog_tool(PanelId(s)).unwrap())
        }
        _ => 60.0,
    }
}

/// Full content height of a panel whose layout is a pure function of its
/// width (independent of document state), so a scroll range is well
/// defined. `None` for the dynamic list panels (layers / artboards /
/// swatches / picker), which own their overflow behaviour.
fn fixed_content_height(id: PanelId, width: f64) -> Option<f64> {
    Some(match id.0 {
        "character" => character::natural_height(),
        "tools" => tools::natural_height(width),
        "color" => color::NATURAL_H,
        "transform" => transform::natural_height(),
        "pathfinder" => pathfinder::natural_height(),
        "align" => align::natural_height(),
        "paragraph" => paragraph::natural_height(),
        _ => return None,
    })
}

/// Largest useful scroll offset for panel `id` in an on-screen body of
/// `body_h` logical px — `content - body_h`, or `0` if it fits or isn't a
/// scrollable panel.
pub fn max_scroll(id: PanelId, width: f64, body_h: f64) -> f64 {
    fixed_content_height(id, width)
        .map(|c| (c - body_h).max(0.0))
        .unwrap_or(0.0)
}

/// The rect to hand a panel's [`paint`] / [`hit`] / [`tip`], given its
/// real on-screen `body` and a scroll offset. When the panel's content
/// overflows `body`, this is `body` slid up by the clamped scroll and
/// stretched to the full content height (`x0` / `x1` unchanged); callers
/// keep their clip layer at the real `body`, and test `body.contains`
/// with the real rect. Returns the clamped scroll actually applied, for
/// the scrollbar.
pub fn scrolled_body(id: PanelId, body: Rect, scroll: f64) -> (Rect, f64) {
    let Some(content) = fixed_content_height(id, body.width()) else {
        return (body, 0.0);
    };
    let overflow = (content - body.height()).max(0.0);
    if overflow <= 0.0 {
        return (body, 0.0);
    }
    let s = scroll.clamp(0.0, overflow);
    (
        Rect::new(body.x0, body.y0 - s, body.x1, body.y0 - s + content),
        s,
    )
}

/// Thin scroll indicator down the right edge of `body`, drawn only when
/// the panel actually overflows. Call inside the panel's clip layer,
/// after its `paint`.
pub fn paint_scrollbar(scene: &mut Scene, body: Rect, id: PanelId, scroll: f64, theme: &Theme) {
    let Some(content) = fixed_content_height(id, body.width()) else {
        return;
    };
    if content <= body.height() + 0.5 {
        return;
    }
    let track_w = 3.0;
    let x1 = body.x1 - 2.0;
    let x0 = x1 - track_w;
    let frac_vis = (body.height() / content).clamp(0.0, 1.0);
    let thumb_h = (body.height() * frac_vis).max(24.0);
    let travel = body.height() - thumb_h;
    let frac_scr = (scroll / (content - body.height())).clamp(0.0, 1.0);
    let y0 = body.y0 + travel * frac_scr;
    scene.fill(
        Fill::NonZero,
        ID,
        theme.splitter,
        None,
        &Rect::new(x0, y0, x1, y0 + thumb_h).to_rounded_rect(track_w * 0.5),
    );
}

/// Hover text for the control at `local` in panel `id`'s body, if any.
pub fn tip(id: PanelId, body: Rect, local: Point, ctx: &Ctx) -> Option<String> {
    match id.0 {
        "tools" => tools::tip(body, local, ctx),
        "color" => color::tip(body, local, ctx).map(str::to_string),
        "picker" => Some("Color Picker".into()),
        "character" => character::tip(body, local, ctx).map(str::to_string),
        "transform" => transform::tip(body, local, ctx).map(str::to_string),
        "pathfinder" => pathfinder::tip(body, local, ctx).map(str::to_string),
        "align" => align::tip(body, local, ctx).map(str::to_string),
        "paragraph" => paragraph::tip(body, local, ctx).map(str::to_string),
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
#[allow(clippy::too_many_arguments)]
pub fn draw_paint_swatch(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    paint: Paint,
    active: bool,
    mixed: bool,
) {
    if mixed {
        scene.fill(Fill::NonZero, ID, Color::from_rgb8(0x3c, 0x3c, 0x3c), None, &r);
        mixed_marks(scene, text, r);
    } else {
        match paint {
            Paint::None => {
                scene.fill(Fill::NonZero, ID, Color::from_rgb8(0xff, 0xff, 0xff), None, &r);
                let mut slash = BezPath::new();
                slash.move_to((r.x0, r.y1));
                slash.line_to((r.x1, r.y0));
                scene.stroke(
                    &Stroke::new(1.5),
                    ID,
                    Color::from_rgb8(0xd0, 0x30, 0x30),
                    None,
                    &slash,
                );
            }
            Paint::Solid(c) => {
                scene.fill(Fill::NonZero, ID, crate::convert::color(c), None, &r);
            }
        }
    }
    let (w, col) = if active {
        (1.5, theme.accent)
    } else {
        (1.0, theme.border)
    };
    scene.stroke(&Stroke::new(w), ID, col, None, &r);
}

/// A grey "?" pattern for a swatch whose value isn't single-valued: one
/// question mark at each corner and one in the centre (Illustrator's
/// mixed-appearance cue), or just a centred one on a small swatch.
pub(crate) fn mixed_marks(scene: &mut Scene, text: &mut TextContext, r: Rect) {
    let ink = Color::from_rgb8(0xdc, 0xdc, 0xdc);
    let big = r.height() >= 28.0;
    let sz = (r.height() * if big { 0.30 } else { 0.62 }).clamp(7.0, 15.0) as f32;
    let w = text.measure("?", sz);
    let put = |scene: &mut Scene, text: &mut TextContext, cx: f64, cy: f64| {
        text.draw(scene, "?", sz, ink, cx - w * 0.5, cy + sz as f64 * 0.36);
    };
    put(scene, text, r.center().x, r.center().y);
    if big {
        let ix = r.width() * 0.25;
        let iy = r.height() * 0.27;
        for (cx, cy) in [
            (r.x0 + ix, r.y0 + iy),
            (r.x1 - ix, r.y0 + iy),
            (r.x0 + ix, r.y1 - iy),
            (r.x1 - ix, r.y1 - iy),
        ] {
            put(scene, text, cx, cy);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hamburger_starts_with_no_items() {
        // Every panel shares the empty menu until it fills `menu()` in.
        // The flyout still opens; it just has nothing to list.
        assert!(matches!(
            MenuEntry::Separator,
            MenuEntry::Separator
        ));
    }
}
