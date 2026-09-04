//! The shell application: the winit event loop, the [`App`] state, input
//! routing, and rendering. [`run`] is the entry point the binary calls.
//!
//! Owns the winit event loop and every window's render state, holds the
//! [`DockModel`], and routes pointer input:
//!
//! - click a tab           → activate it
//! - drag a splitter       → re-weight the split, live
//! - drag a tab off the rail → tear it into a borderless, app-styled OS
//!   window that follows the cursor
//! - release that window over the rail → an Illustrator-style blue line
//!   shows the target; dropping there re-docks it
//!
//! The app tracks each floating window's position itself and moves it with
//! raw mouse-motion deltas. It never reads an OS window rect back into a
//! positioning command — that feedback path is what makes drag jitter.

mod action;
mod command_palette;
mod export;
mod guides;
mod input;
mod isolation;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native_menu;
mod render;
mod shape_dialog;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use native_menu::NativeMenu;

pub(crate) use std::collections::HashMap;
pub(crate) use std::num::NonZeroUsize;
pub(crate) use std::sync::Arc;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use amalith_commands::{Command, CommandOutcome, Editor, PasteStack};
pub(crate) use amalith_core::{ArtboardId, AssetId, Document, LayerId, ObjectId, StrokeStyle};
pub(crate) use crate::anchors;
pub(crate) use crate::canvas::{
    self, AnchorView, CanvasView, DragPreview, PenAnchor, PenPreview, TextBoxPreview,
};
pub(crate) use crate::dock::{
    Axis, Child, DockModel, DropTarget, Node, NodePath, PanelId, Rail, RailSide, Side,
};
pub(crate) use crate::handles::{self, Handle};
pub(crate) use crate::layout::Layout;
pub(crate) use crate::newdoc;
pub(crate) use crate::text::TextContext;
pub(crate) use crate::tool::Tool;
pub(crate) use crate::{
    about, appicon, chrome, context_bar, convert, home, icons, layout, panels, picker, prefs,
    recent, rulers, sample, select, settings, shapedialog, stroke_panel, textedit, workspace, Theme,
};
pub(crate) use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke, Vec2};
pub(crate) use vello::peniko::{color::palette, Color, Fill};
pub(crate) use vello::util::{RenderContext, RenderSurface};
pub(crate) use vello::wgpu;
pub(crate) use vello::{AaConfig, Renderer, RendererOptions, Scene};
pub(crate) use winit::application::ApplicationHandler;
pub(crate) use winit::dpi::{LogicalPosition, LogicalSize};
pub(crate) use winit::event::{ElementState, MouseButton, WindowEvent};
pub(crate) use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
pub(crate) use winit::keyboard::{KeyCode, PhysicalKey};
pub(crate) use winit::window::{Window, WindowId};

/// Height of the top app bar, logical points. It stands in for the hidden
/// OS title bar on macOS. Windows keeps its native title bar *and* gets a
/// native menu bar in that space, so the strip would only be dead air —
/// collapse it there and let the chrome below start at the menu bar.
#[cfg(not(target_os = "windows"))]
const APP_BAR_H: f64 = 30.0;
#[cfg(target_os = "windows")]
const APP_BAR_H: f64 = 0.0;
/// The document-tab strip, between the app bar and the options bar.
const TAB_BAR_H: f64 = 29.4;
/// The tool options strip, between the app bar and the canvas. Sized so
/// its 23px-tall controls clear the edges — see `context_bar`.
const OPT_BAR_H: f64 = 35.0;
/// Total fixed chrome above the canvas / below the top of the window.
const CHROME_TOP: f64 = APP_BAR_H + TAB_BAR_H + OPT_BAR_H;
/// When a rail is empty, the strip of canvas along that edge that still
/// accepts a drop (creating the rail).
const EMPTY_ZONE: f64 = 48.0;
/// Slack around a splitter's visual gap for grabbing it.
const GRAB_SLOP: f64 = 5.0;
/// Visible thickness of the bar on a rail's inner edge.
const RAIL_EDGE: f64 = 4.0;
/// Min rail width, logical points (a narrow icon-only rail is allowed).
const RAIL_MIN_W: f64 = 48.0;
/// Pointer travel before a press becomes a drag.
const DRAG_THRESHOLD: f64 = 5.0;
/// Default size of a torn-off panel window, logical points.
const FLOAT_W: f64 = 264.0;
const FLOAT_H: f64 = 320.0;
/// Where the cursor sits inside a freshly torn-off window (on its strip).
const TEAROFF_GRAB: Vec2 = Vec2::new(58.0, 13.0);

const ID: Affine = Affine::IDENTITY;

/// Placeholder dropped into a freshly created point-text object, selected
/// so the first keystroke overwrites it.
const TEXT_PLACEHOLDER: &str = "Lorem ipsum";

/// Placeholder for a click-dragged area / paragraph text box — a block of
/// filler so the frame reads as a text box on creation. Also selected on
/// open, so the first keystroke clears it.
const TEXT_PLACEHOLDER_PARAGRAPH: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Main,
    Floating(u64),
}

/// One rendered window: surface, its device's vello renderer, the winit
/// handle, and what it shows.
struct WindowHost {
    surface: RenderSurface<'static>,
    renderer: Renderer,
    window: Arc<Window>,
    role: Role,
}

/// What the pointer is currently doing.
#[derive(Default)]
enum Drag {
    #[default]
    None,
    /// Re-weighting the split at `path` in the `side` rail, boundary after
    /// child `gap`.
    Splitter {
        side: RailSide,
        path: NodePath,
        gap: usize,
    },
    /// Pressed a tab in the `side` rail; a click activates it, a drag tears
    /// it off.
    PendingTearoff {
        side: RailSide,
        panel: PanelId,
        path: NodePath,
        tab: usize,
        press: Point,
    },
    /// Pressed a floating window's tab strip; a click activates that tab, a
    /// drag moves the window.
    PendingFloatMove { id: u64, tab: usize, press: Point },
    /// A floating window is following the cursor. `pos` is the app's
    /// authoritative window top-left in virtual-desktop logical points;
    /// `grab` is the constant cursor offset within it.
    MovingFloating { id: u64, grab: Vec2, pos: Point },
    /// Dragging a rail's inner edge to widen / narrow the whole rail.
    RailWidth { side: RailSide },
    /// Dragging a Layers-panel row to restack / reparent the current
    /// selection. `body` is the panel's scrolled body rect (screen px) so
    /// the drop target can be recomputed as the pointer moves; the drag
    /// only "takes" (and `App::layer_drop` populates) once the pointer
    /// leaves a small slop circle around `press`.
    LayerDrag {
        body: Rect,
        press: Point,
        moved: bool,
    },
    /// Panning the canvas; `last` is the previous cursor position.
    Pan { last: Point },
    /// Illustrator scrubby zoom (Space+⌘ drag): zoom anchored at
    /// `anchor`, driven by horizontal motion since `last`.
    ScrubZoom { anchor: Point, last: Point },
    /// Moving the current selection. Deltas are in document space. Alt
    /// (held at any point) duplicates; Shift locks to 8 directions —
    /// both read live at release / in the preview.
    MoveObjects {
        start_doc: Point,
        last_doc: Point,
        moved: bool,
        /// Object under the press, if any — a click (no drag) on an
        /// already-selected object designates it as the Align key object.
        hit: Option<ObjectId>,
    },
    /// Rubber-band selection; `start` is the press point (screen px).
    Marquee { start: Point },
    /// Scaling the selection from `handle`. Transforms are vello affines.
    Scale {
        handle: Handle,
        start_bounds: Rect,
        start_xf: HashMap<ObjectId, Affine>,
        preview: HashMap<ObjectId, Affine>,
    },
    /// Rotating the selection about `center` (document space).
    Rotate {
        center: Point,
        start_angle: f64,
        start_xf: HashMap<ObjectId, Affine>,
        preview: HashMap<ObjectId, Affine>,
    },
    /// Dragging a new ruler guide out of a ruler strip. `pos` is the live
    /// document coordinate; released over the canvas it commits, released
    /// back over a ruler it's discarded.
    NewGuide {
        orient: amalith_core::GuideOrient,
        pos: f64,
    },
    /// Moving an existing ruler guide. `orig` is its coordinate at press;
    /// released over a ruler strip it's deleted instead. `grab` is the
    /// cursor-to-guide offset along the axis at press (doc units) so the
    /// guide doesn't snap to the exact click point, and it holds still
    /// until the pointer leaves a small slop circle around `press`.
    MoveGuide {
        id: amalith_core::GuideId,
        orient: amalith_core::GuideOrient,
        pos: f64,
        orig: f64,
        grab: f64,
        press: Point,
        moved: bool,
    },
    /// Rotate tool: turning the selection about `pivot` (document space).
    /// A release with `moved == false` was a click and re-places the
    /// reference point instead. `copy` (Alt held at press) rotates a
    /// duplicate and leaves the originals put.
    RotateTool {
        pivot: Point,
        start_angle: f64,
        start_xf: HashMap<ObjectId, Affine>,
        preview: HashMap<ObjectId, Affine>,
        copy: bool,
        moved: bool,
    },
    /// Loaded-text cursor: rubber-banding the frame that will receive
    /// `from`'s overflow. Release creates it and threads the two.
    ThreadNewBox {
        from: ObjectId,
        start_doc: Point,
        cur_doc: Point,
    },
    /// Selection tool on area-text box(es): a handle drag resizes the
    /// frame(s) (width / height in document space) and the text re-wraps —
    /// the glyphs are not scaled. One frame follows the pointer edge-for-
    /// edge; several scale proportionally within their union box. Only
    /// while every selected object is an axis-aligned area-text frame.
    ResizeTextBox {
        handle: Handle,
        /// Union box of every frame at press (doc space).
        start_bounds: Rect,
        /// `(id, top-left, width, height)` per frame at press.
        frames: Vec<(ObjectId, Point, f64, f64)>,
        start_doc: Point,
        cur_doc: Point,
    },
    /// Rubber-banding a new shape with the Rectangle / Ellipse tool.
    DrawShape {
        tool: Tool,
        start_doc: Point,
        cur_doc: Point,
    },
    /// Pen tool: dragging a bezier handle out of the anchor just placed.
    /// `from` is that anchor's point (document space), for the drag-slop
    /// test and Shift constraint. While `space_last` is `Some`, Space is
    /// held and the drag is instead sliding the anchor itself (handles
    /// carried rigidly, curvature frozen); it stores the previous cursor
    /// point for the incremental translation.
    PenHandle {
        anchor: usize,
        from: Point,
        space_last: Option<Point>,
    },
    /// Dragging inside the colour picker (`in_hue` = the hue strip).
    PickColor { in_hue: bool },
    /// Color panel: dragging a channel slider.
    ColorScrub { channel: u8, track: Rect },
    /// Color panel: dragging the hue spectrum bar.
    ColorSpectrum { track: Rect },
    /// Moving the colour-picker dialog by its title bar.
    MovePicker { offset: Point },
    /// Direct Selection: dragging the selected path anchors.
    MoveAnchors {
        start_doc: Point,
        last_doc: Point,
        moved: bool,
    },
    /// Direct Selection: dragging one bezier handle of an anchor.
    MoveHandle {
        object: ObjectId,
        anchor: usize,
        side: amalith_core::HandleSide,
        start_doc: Point,
        last_doc: Point,
    },
    /// Direct Selection: rubber-banding to select anchors. `candidate` is
    /// the object under the press — selected on release if the pointer
    /// never moved far enough to count as a marquee.
    AnchorMarquee {
        start: Point,
        candidate: Option<ObjectId>,
    },
    /// Artboard tool: rubber-banding a new artboard.
    DrawArtboard { start_doc: Point, cur_doc: Point },
    /// Type tool: press-drag before deciding point vs area type.
    DrawText { start_doc: Point, cur_doc: Point },
    /// Type tool: dragging to select text inside the open editor.
    TextSelect,
    /// New Document modal: drag-selecting text in a form field.
    NewdocSelect { field: newdoc::Field },
    /// Artboard tool: dragging an existing artboard. Alt (held at any
    /// point) drops a copy; Shift locks to 8 directions — both read live.
    MoveArtboard {
        id: ArtboardId,
        start_doc: Point,
        last_doc: Point,
    },
    /// Artboard tool: dragging a resize handle of the selected artboard.
    ResizeArtboard {
        id: ArtboardId,
        handle: handles::Handle,
        start_rect: amalith_core::Rect,
        start_doc: Point,
        cur_doc: Point,
    },
}

/// A command reachable from the menu bar (native NSMenu on macOS, native
/// HMENU on Windows). Keyboard shortcuts still handle these directly; this
/// is the same set routed through one dispatcher.
#[derive(Clone, Debug)]
enum MenuAction {
    About,
    Preferences,
    /// Quit / Exit. Routed here (not the macOS predefined Quit) so
    /// `about_to_wait` can `event_loop.exit()` and `App::exiting` can
    /// save the layout on the way out.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    Quit,
    New,
    Open,
    Save,
    SaveAs,
    ImportSvg,
    Place,
    ExportForScreens,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    SelectAll,
    /// Select menu.
    SelectAllArtboard,
    Deselect,
    SelectNextAbove,
    SelectNextBelow,
    SelectSame(SameKind),
    BringForward,
    BringToFront,
    SendBackward,
    SendToBack,
    /// View menu.
    ZoomIn,
    ZoomOut,
    FitArtboard,
    FitAll,
    /// Object ▸ Clipping Mask ▸ Make / Release.
    ClipMake,
    ClipRelease,
    /// Type ▸ Convert to Area / Point Type (toggles by selection state).
    ConvertTextKind,
    /// Help ▸ Amalith Help — opens the docs site.
    HelpDocs,
    /// View ▸ Outline (⌘Y).
    ToggleOutline,
    /// View ▸ Guides.
    ToggleGuides,
    ToggleGuideLock,
    ClearGuides,
    /// File ▸ Scripts.
    AddScriptsFolder,
    RevealScriptsFolder,
    RemoveScriptsFolder,
    /// Run the user script at this path.
    RunScript(std::path::PathBuf),
    /// Window menu: show/hide the panel with this id.
    TogglePanel(&'static str),
}

/// The canvas right-click menu: a hand-drawn popover anchored at `origin`
/// (screen px). Rows are plain actions or separators — the vocabulary
/// grows as more of the canvas gets contextual commands.
struct CtxMenu {
    origin: Point,
    items: Vec<CtxItem>,
}

enum CtxItem {
    /// Divider between groups of actions — used once the menu grows.
    #[allow(dead_code)]
    Sep,
    Action {
        label: String,
        action: CtxAction,
        enabled: bool,
    },
}

/// Select ▸ Same ▸ … — which attribute to match against the first
/// selected object.
#[derive(Clone, Copy, PartialEq, Debug)]
enum SameKind {
    FillColor,
    StrokeColor,
    StrokeWeight,
    Opacity,
    FillStroke,
    FontFamily,
    FontSize,
}

#[derive(Clone, Copy, PartialEq)]
enum CtxAction {
    ClipMake,
    ClipRelease,
    ToggleGuides,
    ToggleGuideLock,
    ReleaseGuides,
}

/// What a command-palette row does when chosen.
#[derive(Clone)]
enum PaletteKind {
    Menu(MenuAction),
    Tool(Tool),
    /// Open Preferences to this category index.
    Prefs(usize),
}

/// Where a paste drops. `Plain` recentres on the view; the other two keep
/// exact coordinates and only change stacking.
#[derive(Clone, Copy)]
enum PastePlace {
    Plain,
    InFront,
    Behind,
}

/// An inline panel rename: what's being renamed, the edit buffer, and
/// whether the buffer is still the untouched original (so the first
/// keystroke replaces it, like Illustrator's select-all-on-focus).
struct Rename {
    target: panels::RenameId,
    buf: String,
    fresh: bool,
}

/// A hover tooltip: shown after a short delay, anchored where the pointer
/// was when it appeared.
struct Tooltip {
    text: String,
    anchor: Point,
    since: Instant,
    /// Set once its reveal frame (350ms after `since`) has been drawn, so
    /// `about_to_wait` asks for that one frame and then leaves it alone
    /// instead of repainting on every housekeeping wake.
    shown: bool,
}

/// An open Character-panel dropdown (font family / style / size).
struct FontMenuState {
    kind: panels::FontMenu,
    /// The field the menu drops from, in screen coords.
    anchor: Rect,
    items: Vec<String>,
    /// Type-to-filter text. Empty ⇒ the whole list, no filter header.
    query: String,
    scroll: f64,
}

impl FontMenuState {
    /// Entries after the type-to-filter query (all of them when blank),
    /// case-insensitive substring match.
    fn matches(&self) -> Vec<String> {
        let q = self.query.trim().to_lowercase();
        if q.is_empty() {
            return self.items.clone();
        }
        self.items
            .iter()
            .filter(|it| it.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Height of the filter header row (0 when there's no query).
    fn header_h(&self, row: f64) -> f64 {
        if self.query.trim().is_empty() {
            0.0
        } else {
            row
        }
    }
}

/// An open panel hamburger flyout. Items come from [`panels::menu`].
#[derive(Clone, Copy)]
struct PanelMenu {
    panel: PanelId,
    /// Hamburger button, in the host window's logical coords.
    anchor: Rect,
    win: WindowId,
}

/// One open document's worth of state. The *active* tab's copy lives
/// directly on [`App`] (the fields below `dock`); the inactive tabs are
/// parked here and swapped in on a tab switch via
/// [`App::take_active_doc`] / [`App::load_active_doc`].
struct Doc {
    editor: Editor,
    file_path: Option<std::path::PathBuf>,
    asset_store: amalith_io::AssetStore,
    io_error: Option<String>,
    selection: Vec<ObjectId>,
    anchor_sel: Vec<(ObjectId, usize)>,
    expanded_groups: std::collections::HashSet<ObjectId>,
    selected_artboard: Option<ArtboardId>,
    selected_layer: Option<LayerId>,
    /// The artboard the user last clicked inside (any tool). Targets
    /// artboard-relative Paste in Front / Back.
    current_artboard: Option<ArtboardId>,
    /// The artboard this document's clipboard was copied from, if any.
    clip_artboard: Option<ArtboardId>,
    /// The SVG we last wrote to the OS clipboard — so a following paste
    /// doesn't round-trip our own copy back through the SVG importer.
    last_svg: Option<String>,
    rename: Option<Rename>,
    /// The Fill/Stroke proxy's current colours — what a new object gets,
    /// and what the proxy shows / edits when nothing is selected.
    fill: amalith_core::Paint,
    stroke: amalith_core::Paint,
    stroke_w: f64,
    stroke_style: StrokeStyle,
    opacity: f32,
    view: CanvasView,
    /// Plain (⌘V) pastes since the last copy. Each one nudges further so
    /// repeats don't land on top of the original.
    paste_nudge: u32,
}

impl Doc {
    fn new(editor: Editor) -> Self {
        Self {
            editor,
            file_path: None,
            asset_store: amalith_io::AssetStore::new(),
            io_error: None,
            selection: Vec::new(),
            anchor_sel: Vec::new(),
            expanded_groups: std::collections::HashSet::new(),
            selected_artboard: None,
            selected_layer: None,
            current_artboard: None,
            clip_artboard: None,
            last_svg: None,
            rename: None,
            fill: amalith_core::Paint::Solid(amalith_core::Color::rgb(1.0, 1.0, 1.0)),
            stroke: amalith_core::Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0)),
            stroke_w: amalith_core::Appearance::DEFAULT_STROKE_WIDTH,
            stroke_style: StrokeStyle::default(),
            opacity: 1.0,
            view: CanvasView::default(),
            paste_nudge: 0,
        }
    }

    /// An empty stand-in — used for the active tab's parked slot, and as
    /// the value left behind by `std::mem::replace`.
    fn placeholder() -> Self {
        Self::new(Editor::new(amalith_core::Document::new("Untitled")))
    }
}

/// What the pointer shows over the main window.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CanvasCursor {
    /// Normal OS pointer — over chrome, or a modal is up.
    Default,
    /// OS cursor hidden; the active tool's on-document glyph is drawn.
    Glyph,
    /// OS crosshair — the primitive/shape drawing tools.
    Crosshair,
    /// Open hand — Space held, ready to pan.
    Grab,
    /// Closed hand — Space held and dragging the canvas.
    Grabbing,
    /// Magnifier glyph — Space+⌘ scrubby zoom (＋ or － by direction).
    Zoom,
    /// I-beam — the Type tool.
    IBeam,
    /// Drawn double-arrow scale cursors — hovering a transform grip.
    ScaleNS,
    ScaleEW,
    ScaleNESW,
    ScaleNWSE,
    /// Drawn curved-arrow rotate cursor — hovering just outside a corner.
    /// Carries the corner index (0 = NW, clockwise) so it can face that way.
    Rotate(u8),
    /// Select arrow + link badge — hovering a text frame's out-port.
    ThreadPort,
    /// Loaded-text cursor — an out-port was clicked and the next press
    /// drops the linked frame.
    LoadedText,
    /// "Fit to text" — hovering an area-text box's auto-fit tab (an
    /// up-arrow-to-bar glyph).
    FitUp,
}

impl CanvasCursor {
    /// Modes where the OS cursor is hidden and we paint one instead.
    fn is_drawn(self) -> bool {
        matches!(
            self,
            CanvasCursor::Glyph
                | CanvasCursor::Zoom
                | CanvasCursor::ScaleNS
                | CanvasCursor::ScaleEW
                | CanvasCursor::ScaleNESW
                | CanvasCursor::ScaleNWSE
                | CanvasCursor::Rotate(_)
                | CanvasCursor::ThreadPort
                | CanvasCursor::LoadedText
                | CanvasCursor::FitUp
        )
    }
}

/// What the drawn Pen glyph should say about the next click.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PenHint {
    /// Ordinary "place the next anchor".
    Draw,
    /// Over the first anchor — a click closes the path.
    Closing,
    /// Over a segment of a shown path — a click inserts an anchor there.
    AddPoint,
}

struct App {
    context: RenderContext,
    /// A headless vello renderer, made on first use by Export for Screens.
    export_renderer: Option<Renderer>,
    hosts: HashMap<WindowId, WindowHost>,
    main_id: Option<WindowId>,
    scene: Scene,
    /// Chrome is drawn here in logical units, then appended to `scene`
    /// scaled by the DPI factor.
    content: Scene,
    text: TextContext,
    dock: DockModel,
    /// The live (active-tab) document state. Inactive tabs are parked in
    /// `tabs`; switching swaps this with `tabs[active]`.
    doc: Doc,
    /// Parked state for every open document. `tabs[active]` is a
    /// placeholder while that document is the live one on `doc`.
    tabs: Vec<Doc>,
    active: usize,
    /// The New Document modal, when open.
    newdoc: Option<newdoc::NewDocForm>,
    /// The exact-size shape dialog (Rectangle / Ellipse / Polygon / Star),
    /// opened by a plain click with a primitive tool. Free-floating like
    /// the colour picker; never a dockable panel.
    shape_dialog: Option<shapedialog::ShapeDialog>,
    /// Values the shape dialog reopens with — last committed per tool.
    shape_params: shapedialog::Params,
    /// Set on a plain shape-tool click (release has no `event_loop`); the
    /// dialog window is spawned right after, in `window_event`.
    pending_shape_dialog: Option<(Tool, Point)>,
    /// The Export for Screens dialog (File ▸ Export, ⌘⌥E). Free-floating
    /// like the colour picker; never dockable, never in the Window menu.
    export: Option<crate::export::ExportForScreens>,
    /// Menu / shortcut have no `event_loop`; the window spawns next
    /// `about_to_wait`.
    pending_export: bool,
    /// The Home / Welcome screen. `Some` on launch and after the last tab is
    /// closed; while it's up the canvas takes no input.
    home: Option<home::Home>,
    /// An open text edit (Type tool). `Some` while a text object has the
    /// caret; commits on Esc / click-away / tool switch.
    text_edit: Option<textedit::TextEdit>,
    /// Style the next new text object gets.
    text_defaults: amalith_core::TextStyle,
    /// Alignment + paragraph attributes a new text object starts with.
    text_align_default: amalith_core::TextAlign,
    para_defaults: amalith_core::Paragraph,
    /// "Loaded text" cursor: the user clicked this frame's out-port and the
    /// next press threads its overflow into a new / clicked frame.
    text_load: Option<ObjectId>,
    /// Rotate tool: a custom reference point (document space). `None` falls
    /// back to the selection's bounding-box centre. A plain click with the
    /// Rotate tool re-places it; switching selection clears it.
    transform_pivot: Option<Point>,
    /// Caret blink phase origin.
    text_blink: Instant,
    /// Installed font family names, sorted — built once, for the Character
    /// panel's family dropdown.
    font_families: Vec<String>,
    /// An open Character-panel dropdown.
    font_menu: Option<FontMenuState>,
    /// An open panel hamburger flyout.
    panel_menu: Option<PanelMenu>,
    /// Options-bar Align To dropdown, anchored at the button (screen px).
    align_to_menu: Option<Rect>,
    /// Hover tooltip, if the pointer has been resting on a labelled control.
    tooltip: Option<Tooltip>,
    /// Layers panel search: the current filter text, and whether the field
    /// holds keyboard focus.
    layer_query: String,
    layer_search_focused: bool,
    /// Last resizability pushed to the main window — false while the Home
    /// screen (a fixed-size card) is up.
    main_resizable: bool,
    /// Application settings (Amalith ▸ Preferences).
    settings: prefs::Settings,
    /// User scripts folder + per-script key bindings (File ▸ Scripts,
    /// Preferences ▸ Scripts). Persisted outside the app bundle.
    scripts: crate::scripts::ScriptsConfig,
    /// Named keyboard-shortcut presets (Preferences ▸ Keyboard ▸ Preset).
    keymaps: crate::keymap::Keymaps,
    /// The Preferences modal, when open.
    prefs: Option<prefs::Prefs>,
    active_tool: Tool,
    /// The tool that was active when the Artboard tool was entered, so
    /// Escape can drop straight back to it.
    pre_artboard_tool: Tool,
    /// Which paint slot the Swatches panel targets.
    active_slot: panels::PaintSlot,
    /// The primitive the Tools-panel Shape slot represents / re-activates.
    last_shape_tool: Tool,
    /// Shape-slot press in progress: (when, its screen rect) — a hold
    /// opens the flyout, a quick release re-activates `last_shape_tool`.
    shape_press: Option<(Instant, Rect)>,
    /// The primitive flyout, anchored at the Shape slot's screen rect.
    shape_flyout: Option<Rect>,
    /// Set on boot / new / open — fit the view to the artboards once the
    /// canvas viewport size is known.
    pending_fit: bool,
    /// Direction of the last scrubby-zoom move: `>= 0` = in (＋ cursor),
    /// `< 0` = out (－ cursor).
    zoom_sign: i8,
    /// Whether the Stroke flyout (opened from the options bar) is showing.
    stroke_popover: bool,
    /// Handle to the OS clipboard, for SVG copy/paste with other apps
    /// (Illustrator, browsers). `None` if the platform wouldn't give us one.
    clipboard: Option<arboard::Clipboard>,
    /// Open colour picker, if any.
    picker: Option<picker::Picker>,
    /// Color panel slider space (RGB / HSB / CMYK).
    color_mode: panels::ColorSpace,
    /// Transform panel: 9-point origin, W/H lock, live numeric edit.
    xform_ref: amalith_core::RefPoint,
    xform_constrain: bool,
    xform_edit: Option<(panels::transform::XformField, String, bool)>,
    /// Artboard options-bar segment: live field edit, W/H link, fill menu,
    /// and whether the colour picker is currently retargeted to the
    /// selected artboard's fill.
    artboard_edit: Option<(panels::transform::ABField, String, bool)>,
    artboard_link: bool,
    artboard_fill_menu: bool,
    picker_artboard: bool,
    /// Align panel: what to align to, and the key object (thicker outline).
    align_to: amalith_commands::AlignTo,
    key_object: Option<ObjectId>,
    /// Distribute Spacing value. `None` = Auto.
    align_spacing: Option<f64>,
    /// Live buffer while the Align spacing field is being typed.
    align_spacing_edit: Option<(String, bool)>,
    /// Recently used solid colours for the Color panel, newest first.
    recent_colors: Vec<amalith_core::Color>,
    /// GPU-ready rasters for placed images, keyed by document asset.
    /// Each entry holds every decoded LOD; paint picks by on-screen size.
    image_cache: HashMap<AssetId, crate::lod::ImageLods>,
    /// Same LOD set interned by source path so duplicates share GPU blobs.
    decoded_by_path: HashMap<String, crate::lod::ImageLods>,
    /// Highest LOD received per asset. Intern in `decoded_by_path` means done.
    image_lod: HashMap<AssetId, u8>,
    lod: crate::lod::LodHub,
    lod_inflight: std::collections::HashSet<AssetId>,
    /// Time + position of the last left press, for double-click detection.
    last_click: Option<(Instant, Point)>,
    /// Consecutive left presses in the same spot (1 = single, 2 = double,
    /// 3 = triple). Drives word- vs whole-text selection in the editor.
    click_streak: u32,
    /// In-progress Pen path — placed anchors (with bezier handles) in
    /// document space. Empty when not drawing.
    pen: Vec<PenAnchor>,
    /// Anchors popped by ⌘Z while drawing, for ⌘⇧Z to restore.
    pen_redo: Vec<PenAnchor>,
    /// The path just committed by the Pen, so ⌘Z can re-open it a point
    /// shorter instead of undoing the whole object. Cleared by any other
    /// action.
    last_pen: Option<(ObjectId, Vec<PenAnchor>, bool)>,
    /// Rubber-band rect (screen px) while a marquee drag is live.
    marquee: Option<Rect>,
    theme: Theme,
    scale: f64,
    /// Pointer position within whichever window last reported it, logical.
    pointer: Point,
    pointer_win: Option<WindowId>,
    /// Held modifiers on the main window.
    cmd_down: bool,
    shift_down: bool,
    alt_down: bool,
    space_down: bool,
    drag: Drag,
    /// Live re-dock target (which rail, and where in it) while a floating
    /// window is over the document window.
    redock_preview: Option<(RailSide, DropTarget)>,
    /// Set by cursor-move handling (no `event_loop` in scope) so the
    /// window-event dispatch can spawn the torn-off window.
    pending_tearoff: Option<(PanelId, Point)>,
    /// The "About Amalith" panel: a modal card centred over the main window,
    /// with live text selection. `Some` while it's showing.
    about: Option<about::About>,
    /// What the pointer looks like right now (see [`CanvasCursor`]).
    cursor_mode: CanvasCursor,
    /// Which of our windows currently hold OS focus. Non-empty ⇒ Amalith
    /// is the active app, and floating panels stay above the main window.
    focused: std::collections::HashSet<WindowId>,
    /// Vertical scroll offset per panel, for the fixed-layout panels
    /// (align, transform, pathfinder, character, color, tools) whose
    /// content can be taller than a short docked / floating body. Stored
    /// loosely; `panels::scrolled_body` clamps to the live range on read.
    panel_scroll: std::collections::HashMap<PanelId, f64>,
    /// Canvas rulers along the top / left edges (⌘R). Origin is the
    /// document origin; when on, `canvas_viewport` insets by `rulers::THICK`.
    rulers: bool,
    /// Ruler guides hidden (View ▸ Hide Guides, ⌘;).
    guides_hidden: bool,
    /// Ruler guides locked — can't be grabbed or moved (View ▸ Lock
    /// Guides, ⌘⌥;).
    guides_locked: bool,
    /// Ruler guides currently selected (clicked or marquee'd with a
    /// selection tool); Delete / Backspace removes them.
    selected_guides: Vec<amalith_core::GuideId>,
    /// Outline (wireframe) view — View ▸ Outline, ⌘Y.
    outline_mode: bool,
    /// Isolation-mode breadcrumb: the groups drilled into (outermost
    /// first). Empty = not isolated. Selection, hit-testing and the dim
    /// scrim scope to the last entry.
    isolation: Vec<ObjectId>,
    /// Isolation breadcrumb bar hit rects from the last paint: `(rect,
    /// depth)` — a click truncates the breadcrumb to `depth`.
    iso_bar: Vec<(Rect, usize)>,
    /// Live Layers-panel drag-reorder target: `(parent, insert index,
    /// visible-row index, into-container)`. `Some` only while a
    /// [`Drag::LayerDrag`] is past the slop threshold. The first two drive
    /// the `Reparent` command on release; the last two draw the indicator.
    layer_drop: Option<(amalith_core::ObjectParent, usize, i64, bool)>,
    /// Cached static ruler layer — rebuilt only when the view, canvas
    /// region, ruler origin, or display unit changes.
    ruler_cache: Option<(f64, f64, f64, Rect, f64, f64, amalith_core::Unit, Scene)>,
    /// Right-click unit menu on the rulers, anchored at the click (screen).
    ruler_menu: Option<Point>,
    /// Right-click context menu in the canvas area.
    ctx_menu: Option<CtxMenu>,
    /// Command palette (⌘K), and the action behind each of its rows
    /// (parallel to the palette's display entries).
    palette: Option<crate::palette::Palette>,
    palette_kinds: Vec<PaletteKind>,
    /// Wall-clock instant of the previous `redraw`, and a smoothed
    /// frames-per-second estimate for the debug counter.
    last_frame: Option<Instant>,
    fps: f32,
    /// Set once the first frame has actually presented. Until then
    /// `about_to_wait` keeps nudging a redraw (and holds `ControlFlow::
    /// Poll`) so a dropped initial `RedrawRequested` can't leave the
    /// window blank. After it, rendering is strictly on demand.
    first_frame_done: bool,
    /// Caret blink phase as of the last painted frame. `about_to_wait`
    /// asks for a new frame only when this would flip — edge-triggered, so
    /// an open text edit costs ~2 repaints/sec, not a continuous loop.
    last_caret_drawn: bool,
    /// The native menu bar (macOS NSMenu / Windows HMENU), once the app has
    /// resumed.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    native_menu: Option<NativeMenu>,
}

impl App {
    fn new() -> Self {
        // Default "Essentials Classic" layout, then overlay whatever the
        // last session (or a picked workspace) saved.
        let mut dock = DockModel::new(demo_right_dock());
        dock.left = Rail::with(demo_left_dock());
        dock.left.width = 80.0; // two tool columns
        let mut rulers = false;
        let mut guides_hidden = false;
        let mut guides_locked = false;
        if let Some(saved) = workspace::load() {
            saved.apply_to(&mut dock);
            rulers = saved.rulers;
            guides_hidden = saved.guides_hidden;
            guides_locked = saved.guides_locked;
        }
        Self {
            context: RenderContext::new(),
            export_renderer: None,
            hosts: HashMap::new(),
            main_id: None,
            scene: Scene::new(),
            content: Scene::new(),
            text: TextContext::new(),
            dock,
            doc: Doc::new(Editor::new(sample::document())),
            tabs: vec![Doc::placeholder()],
            active: 0,
            // Boot into the Home screen; New Document opens from there.
            newdoc: None,
            shape_dialog: None,
            shape_params: shapedialog::Params::default(),
            pending_shape_dialog: None,
            export: None,
            pending_export: false,
            home: home::Home::new(recent::load()),
            text_edit: None,
            text_defaults: amalith_core::TextStyle::default(),
            text_align_default: amalith_core::TextAlign::Start,
            para_defaults: amalith_core::Paragraph::default(),
            text_load: None,
            transform_pivot: None,
            text_blink: Instant::now(),
            font_families: Vec::new(),
            font_menu: None,
            panel_menu: None,
            align_to_menu: None,
            tooltip: None,
            layer_query: String::new(),
            layer_search_focused: false,
            main_resizable: true,
            settings: settings::load(),
            scripts: crate::scripts::load(),
            keymaps: crate::keymap::load(),
            prefs: None,
            active_tool: Tool::Select,
            pre_artboard_tool: Tool::Select,
            active_slot: panels::PaintSlot::Fill,
            last_shape_tool: Tool::Rectangle,
            shape_press: None,
            shape_flyout: None,
            pending_fit: true,
            zoom_sign: 1,
            stroke_popover: false,
            clipboard: None,
            picker: None,
            color_mode: panels::ColorSpace::Rgb,
            xform_ref: amalith_core::RefPoint::CENTER,
            xform_constrain: true,
            xform_edit: None,
            artboard_edit: None,
            artboard_link: false,
            artboard_fill_menu: false,
            picker_artboard: false,
            align_to: amalith_commands::AlignTo::Selection,
            key_object: None,
            align_spacing: Some(0.0),
            align_spacing_edit: None,
            recent_colors: Vec::new(),
            image_cache: HashMap::new(),
            decoded_by_path: HashMap::new(),
            image_lod: HashMap::new(),
            lod: crate::lod::LodHub::new(),
            lod_inflight: std::collections::HashSet::new(),
            last_click: None,
            click_streak: 0,
            pen: Vec::new(),
            pen_redo: Vec::new(),
            last_pen: None,
            marquee: None,
            theme: Theme::default(),
            scale: 1.0,
            pointer: Point::ZERO,
            pointer_win: None,
            cmd_down: false,
            shift_down: false,
            alt_down: false,
            space_down: false,
            drag: Drag::None,
            redock_preview: None,
            pending_tearoff: None,
            about: None,
            cursor_mode: CanvasCursor::Default,
            focused: std::collections::HashSet::new(),
            panel_scroll: std::collections::HashMap::new(),
            rulers,
            guides_hidden,
            guides_locked,
            selected_guides: Vec::new(),
            outline_mode: false,
            isolation: Vec::new(),
            iso_bar: Vec::new(),
            layer_drop: None,
            ruler_cache: None,
            ruler_menu: None,
            ctx_menu: None,
            palette: None,
            palette_kinds: Vec::new(),
            last_frame: None,
            fps: 0.0,
            first_frame_done: false,
            last_caret_drawn: false,
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            native_menu: None,
        }
    }

    fn main_window(&self) -> Option<&Arc<Window>> {
        self.hosts.get(&self.main_id?).map(|h| &h.window)
    }

    fn floating_window(&self, id: u64) -> Option<&Arc<Window>> {
        self.hosts
            .values()
            .find(|h| matches!(h.role, Role::Floating(f) if f == id))
            .map(|h| &h.window)
    }

    /// Mark every open window for one repaint. Rendering is on demand — a
    /// frame is produced only after a call like this, or a `WaitUntil` wake
    /// in `about_to_wait` for one of the few things that animate without an
    /// input event. Floating panel windows mirror the main document's
    /// state, so they repaint alongside it; in the common case (nothing
    /// torn off) `self.hosts` is just the main window.
    fn request_main_redraw(&self) {
        for host in self.hosts.values() {
            host.window.request_redraw();
        }
    }

    /// Rebuild the native menu bar — used when the Scripts submenu's
    /// contents change (folder added / removed).
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn rebuild_native_menu(&mut self) {
        let Some(wid) = self.main_id else { return };
        let Some(host) = self.hosts.get(&wid) else {
            return;
        };
        let m = NativeMenu::build(
            &host.window,
            &self.scripts,
            self.guides_hidden,
            self.guides_locked,
            self.outline_mode,
        );
        m.sync_window(&self.dock);
        self.native_menu = Some(m);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn rebuild_native_menu(&mut self) {}

    /// Persist the current dock arrangement + view toggles to `layout.json`.
    fn save_layout(&self) {
        workspace::save(&workspace::Layout::capture(
            &self.dock,
            self.rulers,
            self.guides_hidden,
            self.guides_locked,
        ));
    }

    /// Stored scroll offset for a panel (0 if none). Clamped to the live
    /// range by `panels::scrolled_body` wherever it's consumed.
    fn panel_scroll_of(&self, id: PanelId) -> f64 {
        self.panel_scroll.get(&id).copied().unwrap_or(0.0)
    }

    /// Full content height of panel `id` at `body` — pure-layout panels
    /// from `panels`, the Layers list from the live document.
    fn panel_content_h(&self, id: PanelId, body: Rect) -> f64 {
        if id == PanelId("layers") {
            panels::layers_content_height(
                self.doc.editor.document(),
                &self.doc.expanded_groups,
                &self.layer_query,
            )
        } else {
            panels::max_scroll(id, body.width(), body.height()) + body.height()
        }
    }

    /// The scrollable panel under `p` (rail or floating) and its real body
    /// rect, if that panel's content overflows its body. Used by the wheel
    /// handler to route scroll into the panel instead of the canvas.
    fn scrollable_panel_at(&mut self, p: Point) -> Option<(PanelId, Rect)> {
        let areas: Vec<layout::PanelArea> = if self.pointer_win == self.main_id {
            [RailSide::Left, RailSide::Right]
                .iter()
                .flat_map(|&side| {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        return Vec::new();
                    }
                    let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    build_rail_layout(rail, &self.theme, &mut self.text, rect).areas
                })
                .collect()
        } else if let Some(fid) = self.pointer_win.and_then(|wid| {
            self.hosts.get(&wid).and_then(|h| match h.role {
                Role::Floating(f) => Some(f),
                _ => None,
            })
        }) {
            self.floating_layout(fid).areas
        } else {
            return None;
        };
        for area in &areas {
            if area.body.contains(p) {
                if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                    if self.panel_content_h(pid, area.body) > area.body.height() + 0.5 {
                        return Some((pid, area.body));
                    }
                }
            }
        }
        None
    }

    /// Move the live (active) document state off `App` into a [`Doc`],
    /// leaving a placeholder behind.
    fn take_active_doc(&mut self) -> Doc {
        std::mem::replace(&mut self.doc, Doc::placeholder())
    }

    /// Make `doc` the live document on `App`.
    fn load_active_doc(&mut self, doc: Doc) {
        self.doc = doc;
        // Transient interaction state doesn't cross documents.
        self.drag = Drag::None;
        self.pen.clear();
        self.pen_redo.clear();
        self.last_pen = None;
        self.marquee = None;
        self.picker = None;
        self.pending_shape_dialog = None;
        self.close_shape_dialog(false);
        self.pending_export = false;
        self.close_export(false);
        self.panel_menu = None;
    }

    /// Open `doc` in a new tab and make it active.
    fn add_doc(&mut self, doc: Doc) {
        self.tabs[self.active] = self.take_active_doc();
        self.tabs.push(Doc::placeholder());
        self.active = self.tabs.len() - 1;
        self.load_active_doc(doc);
        self.pending_fit = true;
        self.request_main_redraw();
    }

    /// Switch the live document to tab `i`.
    fn switch_to(&mut self, i: usize) {
        if i == self.active || i >= self.tabs.len() {
            return;
        }
        self.tabs[self.active] = self.take_active_doc();
        let doc = std::mem::replace(&mut self.tabs[i], Doc::placeholder());
        self.active = i;
        self.load_active_doc(doc);
        self.request_main_redraw();
    }

    /// Close tab `i`. Closing the last one drops back to the Home screen.
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.load_active_doc(Doc::placeholder());
            self.doc.selection.clear();
            if self.settings.home_on_last_close {
                self.home = home::Home::new(recent::load());
            }
            self.request_main_redraw();
            return;
        }
        if i == self.active {
            self.tabs.remove(i);
            self.active = i.min(self.tabs.len() - 1);
            let doc = std::mem::replace(&mut self.tabs[self.active], Doc::placeholder());
            self.load_active_doc(doc);
        } else {
            self.tabs.remove(i);
            if i < self.active {
                self.active -= 1;
            }
        }
        self.request_main_redraw();
    }

    /// Display label for tab `i`: `Name* @ zoom% (Color/Preview)`.
    fn tab_label(&self, i: usize) -> String {
        let d = if i == self.active {
            (
                &self.doc.editor,
                self.doc.view.zoom,
            )
        } else {
            (&self.tabs[i].editor, self.tabs[i].view.zoom)
        };
        let (editor, zoom) = d;
        let doc = editor.document();
        let name = doc.metadata.title.as_deref().unwrap_or("Untitled");
        let dirty = if editor.can_undo() { "*" } else { "" };
        let color = match doc.settings.color_mode {
            amalith_core::ColorMode::Cmyk => "CMYK",
            amalith_core::ColorMode::Rgb => "RGB",
        };
        let preview = match doc.settings.preview_mode {
            amalith_core::PreviewMode::Default => "Default",
            amalith_core::PreviewMode::Pixel => "Pixel",
            amalith_core::PreviewMode::Overprint => "Overprint",
        };
        format!(
            "{name}{dirty} @ {} ({color}/{preview})",
            canvas::zoom_percent_label(zoom)
        )
    }

    /// Global (virtual-desktop) logical position of the main window's
    /// client-area origin. Read-only — never fed into a move command.
    fn main_inner_origin(&self) -> Point {
        self.main_window()
            .and_then(|w| w.inner_position().ok())
            .map(|p| Point::new(p.x as f64 / self.scale, p.y as f64 / self.scale))
            .unwrap_or(Point::ZERO)
    }

    /// Screen (logical) point → document point.
    fn doc_point(&self, screen: Point) -> Point {
        self.doc.view.to_screen().inverse() * screen
    }

    /// Set one of the Fill/Stroke proxy paints and, when there is a
    /// selection, apply it to the selected objects as one undoable command.
    fn set_paint(&mut self, slot: panels::PaintSlot, paint: amalith_core::Paint) {
        match slot {
            panels::PaintSlot::Fill => self.doc.fill = paint,
            panels::PaintSlot::Stroke => self.doc.stroke = paint,
        }
        if !self.doc.selection.is_empty() {
            let objects = self.doc.selection.clone();
            let cmd = match slot {
                panels::PaintSlot::Fill => Command::SetFill { objects, paint },
                panels::PaintSlot::Stroke => Command::SetStroke { objects, paint },
            };
            let _ = self.doc.editor.execute(cmd);
        }
        self.request_main_redraw();
    }

    /// Apply the colour picker's current colour to its slot.
    fn apply_picker_color(&mut self) {
        let Some(pk) = self.picker else {
            return;
        };
        self.set_paint(pk.slot, amalith_core::Paint::Solid(pk.color()));
        self.push_recent(pk.color());
    }

    fn active_paint(&self) -> amalith_core::Paint {
        match self.active_slot {
            panels::PaintSlot::Fill => self
                .representative()
                .map(|a| a.fill)
                .unwrap_or(self.doc.fill),
            panels::PaintSlot::Stroke => self
                .representative()
                .map(|a| a.stroke)
                .unwrap_or(self.doc.stroke),
        }
    }

    fn push_recent(&mut self, c: amalith_core::Color) {
        self.recent_colors.retain(|x| *x != c);
        self.recent_colors.insert(0, c);
        self.recent_colors.truncate(12);
    }

    fn apply_solid_rgb(&mut self, r: f32, g: f32, b: f32) {
        let c = amalith_core::Color::rgb(r, g, b);
        self.set_paint(self.active_slot, amalith_core::Paint::Solid(c));
    }

    fn set_color_channel(&mut self, channel: u8, t: f32) {
        let (r, g, b) = self
            .active_paint()
            .color()
            .map(|c| (c.r, c.g, c.b))
            .unwrap_or((0.0, 0.0, 0.0));
        let (r, g, b) = panels::color::apply_channel(self.color_mode, r, g, b, channel, t);
        self.apply_solid_rgb(r, g, b);
    }

    fn set_color_spectrum(&mut self, t: f32) {
        let (r, g, b) = self
            .active_paint()
            .color()
            .map(|c| (c.r, c.g, c.b))
            .unwrap_or((1.0, 0.0, 0.0));
        let (r, g, b) = panels::color::apply_spectrum(r, g, b, t);
        self.apply_solid_rgb(r, g, b);
    }

    /// Close the colour picker overlay. `apply` commits the pending colour
    /// first. Also drops a leftover docked / floating picker panel so OK
    /// and Cancel always take the window with them.
    fn dismiss_picker(&mut self, apply: bool) {
        if apply {
            if self.picker_artboard {
                if let (Some(pk), Some(id)) = (self.picker, self.doc.selected_artboard) {
                    let _ = self.doc.editor.execute(Command::SetArtboardFill {
                        id,
                        fill: Some(pk.color()),
                    });
                }
            } else {
                self.apply_picker_color();
            }
        }
        self.picker_artboard = false;
        self.picker = None;
        self.dock.remove(PanelId("picker"));
        let dead: Vec<WindowId> = self
            .hosts
            .iter()
            .filter_map(|(wid, h)| match h.role {
                Role::Floating(fid) if self.dock.floating(fid).is_none() => Some(*wid),
                _ => None,
            })
            .collect();
        for wid in dead {
            self.hosts.remove(&wid);
            self.focused.remove(&wid);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
        self.request_main_redraw();
    }

    /// Apply `f` to the type style the Character panel targets: the live
    /// text edit, else every selected text object, else the new-text
    /// defaults.
    fn edit_text_style(&mut self, f: impl Fn(&mut amalith_core::TextStyle)) {
        if let Some(te) = &mut self.text_edit {
            let mut s = te.style().clone();
            f(&mut s);
            te.apply_style(&s, &mut self.text);
            self.request_main_redraw();
            return;
        }
        let ids: Vec<ObjectId> = self
            .doc.selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Text(_))
                )
            })
            .collect();
        if ids.is_empty() {
            f(&mut self.text_defaults);
            self.request_main_redraw();
            return;
        }
        for id in ids {
            let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            else {
                continue;
            };
            let mut data = t.clone();
            f(&mut data.style);
            data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
            let _ = self.doc.editor.execute(Command::SetText { object: id, data });
        }
        self.request_main_redraw();
    }

    /// Open a Character-panel dropdown anchored at `anchor` (screen rect).
    fn open_font_menu(&mut self, kind: panels::FontMenu, anchor: Rect) {
        let items: Vec<String> = match kind {
            panels::FontMenu::Family => self.font_families.clone(),
            panels::FontMenu::Style => {
                let fam = self.active_text_style().family;
                let (fc, _) = self.text.parts();
                let mut faces: Vec<(u16, bool, String)> = fc
                    .collection
                    .family_by_name(&fam)
                    .map(|info| {
                        info.fonts()
                            .iter()
                            .map(|font| {
                                let w = font.weight().value().round() as u16;
                                let italic =
                                    !matches!(font.style(), parley::style::FontStyle::Normal);
                                (w, italic, panels::character::face_label(w, italic))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                faces.sort();
                faces.dedup();
                faces.into_iter().map(|(_, _, l)| l).collect()
            }
            panels::FontMenu::Size => panels::character::SIZE_PRESETS
                .iter()
                .map(|v| format!("{}", *v as i64))
                .collect(),
        };
        self.align_to_menu = None;
        self.font_menu = Some(FontMenuState {
            kind,
            anchor,
            items,
            query: String::new(),
            scroll: 0.0,
        });
        self.request_main_redraw();
    }

    const FM_ROW: f64 = 22.0;
    const FM_ROWS: usize = 12;

    fn font_menu_rect(m: &FontMenuState) -> Rect {
        let w = m.anchor.width().max(190.0);
        let rows = m.matches().len().min(Self::FM_ROWS).max(1) as f64;
        let x = m.anchor.x0;
        let y = m.anchor.y1 + 2.0;
        Rect::new(
            x,
            y,
            x + w,
            y + m.header_h(Self::FM_ROW) + rows * Self::FM_ROW + 6.0,
        )
    }

    /// A click while a font dropdown is open. Returns true if consumed.
    fn font_menu_click(&mut self, p: Point) -> bool {
        let Some(m) = &self.font_menu else {
            return false;
        };
        let outer = Self::font_menu_rect(m);
        if !outer.contains(p) {
            self.font_menu = None;
            self.request_main_redraw();
            return true;
        }
        let header = m.header_h(Self::FM_ROW);
        let items = m.matches();
        let idx =
            ((p.y - outer.y0 - 3.0 - header + m.scroll) / Self::FM_ROW).floor() as isize;
        if idx >= 0 && (idx as usize) < items.len() {
            let kind = m.kind;
            let label = items[idx as usize].clone();
            self.font_menu = None;
            self.apply_font_choice(kind, label);
        }
        self.request_main_redraw();
        true
    }

    /// Commit a chosen dropdown label back to the text style.
    fn apply_font_choice(&mut self, kind: panels::FontMenu, label: String) {
        match kind {
            panels::FontMenu::Family => {
                self.apply_panel_action(panels::Action::SetFontFamily(label), false);
            }
            panels::FontMenu::Size => {
                if let Ok(v) = label.parse::<f64>() {
                    self.apply_panel_action(panels::Action::SetFontSize(v), false);
                }
            }
            panels::FontMenu::Style => {
                // Resolve the label back to weight/italic via the family's faces.
                let fam = self.active_text_style().family;
                let (fc, _) = self.text.parts();
                let face = fc.collection.family_by_name(&fam).and_then(|info| {
                    info.fonts().iter().find_map(|font| {
                        let w = font.weight().value().round() as u16;
                        let italic = !matches!(font.style(), parley::style::FontStyle::Normal);
                        (panels::character::face_label(w, italic) == label).then_some((w, italic))
                    })
                });
                if let Some((weight, italic)) = face {
                    self.apply_panel_action(
                        panels::Action::SetFontFace { weight, italic },
                        false,
                    );
                }
            }
        }
    }

    /// Keyboard while a font dropdown is open: type to filter, Backspace to
    /// trim, Enter to take the top match, Escape to dismiss.
    fn font_menu_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.font_menu.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.font_menu = None;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                if let Some(m) = &self.font_menu {
                    let pick = m.matches().into_iter().next();
                    let kind = m.kind;
                    self.font_menu = None;
                    if let Some(label) = pick {
                        self.apply_font_choice(kind, label);
                    }
                }
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some(m) = &mut self.font_menu {
                    m.query.pop();
                    m.scroll = 0.0;
                }
            }
            _ => {
                if let Some(txt) = event.text.as_ref() {
                    let add: String = txt.chars().filter(|c| !c.is_control()).collect();
                    if !add.is_empty() {
                        if let Some(m) = &mut self.font_menu {
                            m.query.push_str(&add);
                            m.scroll = 0.0;
                        }
                    }
                }
            }
        }
        self.request_main_redraw();
        true
    }

    const AT_W: f64 = 188.0;
    const AT_ROW: f64 = 24.0;
    const AT_PAD: f64 = 6.0;

    fn align_to_items() -> [(amalith_commands::AlignTo, &'static str); 3] {
        [
            (amalith_commands::AlignTo::Selection, "Align to Selection"),
            (amalith_commands::AlignTo::KeyObject, "Align to Key Object"),
            (amalith_commands::AlignTo::Artboard, "Align to Artboard"),
        ]
    }

    /// `Some(true)` if a lone area-text object is selected, `Some(false)`
    /// for point text, `None` otherwise — drives the Type ▸ Convert item.
    fn text_convert_menu_state(&self) -> Option<bool> {
        if self.doc.selection.len() != 1 {
            return None;
        }
        match self
            .doc
            .editor
            .document()
            .object(self.doc.selection[0])
            .map(|o| &o.kind)
        {
            Some(amalith_core::ObjectKind::Text(t)) => {
                Some(matches!(t.kind, amalith_core::TextKind::Area { .. }))
            }
            _ => None,
        }
    }

    /// Refresh selection-dependent native-menu items (currently just the
    /// Type ▸ Convert label).
    fn sync_type_menu(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_type(self.text_convert_menu_state());
            m.sync_clip(self.clip_state());
        }
    }
    fn toggle_outline_mode(&mut self) {
        self.outline_mode = !self.outline_mode;
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_outline(self.outline_mode);
        }
        self.request_main_redraw();
    }

    const CM_W: f64 = 200.0;
    const CM_ROW: f64 = 26.0;
    const CM_SEP: f64 = 9.0;
    const CM_PAD: f64 = 6.0;

    fn ctx_menu_height(items: &[CtxItem]) -> f64 {
        Self::CM_PAD * 2.0
            + items
                .iter()
                .map(|it| match it {
                    CtxItem::Sep => Self::CM_SEP,
                    CtxItem::Action { .. } => Self::CM_ROW,
                })
                .sum::<f64>()
    }

    fn ctx_menu_rect(origin: Point, items: &[CtxItem]) -> Rect {
        Rect::new(
            origin.x,
            origin.y,
            origin.x + Self::CM_W,
            origin.y + Self::ctx_menu_height(items),
        )
    }

    /// Build and show the canvas context menu at `at` (screen px).
    fn open_ctx_menu(&mut self, at: Point) {
        let has_guides = !self.doc.editor.document().guides().is_empty();
        let (can_clip, can_release) = self.clip_state();
        let mut items = vec![];
        if can_clip || can_release {
            items.push(CtxItem::Action {
                label: "Make Clipping Mask".into(),
                action: CtxAction::ClipMake,
                enabled: can_clip,
            });
            items.push(CtxItem::Action {
                label: "Release Clipping Mask".into(),
                action: CtxAction::ClipRelease,
                enabled: can_release,
            });
            items.push(CtxItem::Sep);
        }
        items.extend([
            CtxItem::Action {
                label: if self.guides_hidden { "Show Guides" } else { "Hide Guides" }.into(),
                action: CtxAction::ToggleGuides,
                enabled: true,
            },
            CtxItem::Action {
                label: if self.guides_locked { "Unlock Guides" } else { "Lock Guides" }.into(),
                action: CtxAction::ToggleGuideLock,
                enabled: true,
            },
            CtxItem::Action {
                label: "Release Guides".into(),
                action: CtxAction::ReleaseGuides,
                enabled: has_guides,
            },
        ]);
        let h = Self::ctx_menu_height(&items);
        let (w, wh) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let origin = Point::new(
            at.x.min(w - Self::CM_W - 4.0).max(4.0),
            at.y.min(wh - h - 4.0).max(4.0),
        );
        self.ruler_menu = None;
        self.panel_menu = None;
        self.ctx_menu = Some(CtxMenu { origin, items });
        self.request_main_redraw();
    }

    /// A left press while the context menu is open: run the row under `p`
    /// (if any) and close. Returns whether the press was consumed.
    fn ctx_menu_click(&mut self, p: Point) -> bool {
        let Some(menu) = self.ctx_menu.take() else {
            return false;
        };
        self.request_main_redraw();
        let rect = Self::ctx_menu_rect(menu.origin, &menu.items);
        if !rect.contains(p) {
            return true;
        }
        let mut y = rect.y0 + Self::CM_PAD;
        let mut hit = None;
        for it in &menu.items {
            match it {
                CtxItem::Sep => y += Self::CM_SEP,
                CtxItem::Action { action, enabled, .. } => {
                    if *enabled && Rect::new(rect.x0, y, rect.x1, y + Self::CM_ROW).contains(p) {
                        hit = Some(*action);
                    }
                    y += Self::CM_ROW;
                }
            }
        }
        if let Some(a) = hit {
            match a {
                CtxAction::ClipMake => self.clip_make(),
                CtxAction::ClipRelease => self.clip_release(),
                CtxAction::ToggleGuides => self.set_guides_hidden(!self.guides_hidden),
                CtxAction::ToggleGuideLock => self.set_guides_locked(!self.guides_locked),
                CtxAction::ReleaseGuides => self.release_guides(),
            }
        }
        true
    }

    /// Right mouse press: the ruler unit menu over a ruler strip, else the
    /// canvas context menu (or dismisses whatever menu is open).
    fn on_right_press(&mut self) {
        if self.ruler_menu.take().is_some() || self.ctx_menu.take().is_some() {
            self.request_main_redraw();
            return;
        }
        if self.pointer_win != self.main_id {
            return;
        }
        let r = self.canvas_region();
        if self.rulers
            && r.contains(self.pointer)
            && (self.pointer.y < r.y0 + rulers::THICK || self.pointer.x < r.x0 + rulers::THICK)
        {
            self.ruler_menu = Some(self.pointer);
            self.request_main_redraw();
            return;
        }
        if self.canvas_viewport().contains(self.pointer) {
            self.open_ctx_menu(self.pointer);
        }
    }

    fn align_to_menu_rect(anchor: Rect) -> Rect {
        let h = Self::AT_PAD * 2.0 + Self::AT_ROW * 3.0;
        Rect::new(
            anchor.x0,
            anchor.y1 + 2.0,
            anchor.x0 + Self::AT_W,
            anchor.y1 + 2.0 + h,
        )
    }

    /// Click while the Align To dropdown is open. Consumes the press.
    fn align_to_menu_click(&mut self, p: Point) -> bool {
        let Some(anchor) = self.align_to_menu else {
            return false;
        };
        if anchor.contains(p) {
            self.align_to_menu = None;
            self.request_main_redraw();
            return true;
        }
        let fly = Self::align_to_menu_rect(anchor);
        if !fly.contains(p) {
            self.align_to_menu = None;
            self.request_main_redraw();
            return true;
        }
        let mut y = fly.y0 + Self::AT_PAD;
        for (to, _) in Self::align_to_items() {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::AT_ROW);
            if row.contains(p) {
                self.align_to_menu = None;
                self.apply_panel_action(panels::Action::SetAlignTo(to), false);
                return true;
            }
            y += Self::AT_ROW;
        }
        self.align_to_menu = None;
        self.request_main_redraw();
        true
    }

    const PM_W: f64 = 168.0;
    const PM_ROW: f64 = 28.0;
    const PM_SEP: f64 = 9.0;
    const PM_PAD: f64 = 8.0;

    fn panel_menu_height(items: &[panels::MenuEntry]) -> f64 {
        if items.is_empty() {
            return Self::PM_PAD * 2.0 + 8.0;
        }
        let mut h = Self::PM_PAD * 2.0;
        for e in items {
            h += match e {
                panels::MenuEntry::Item { .. } => Self::PM_ROW,
                panels::MenuEntry::Separator => Self::PM_SEP,
            };
        }
        h
    }

    fn panel_menu_flyout(anchor: Rect, items: &[panels::MenuEntry], wl: f64, hl: f64) -> Rect {
        let w = Self::PM_W;
        let h = Self::panel_menu_height(items);
        let mut x = anchor.x1;
        let mut y = anchor.y0;
        if x + w > wl - 4.0 {
            x = (anchor.x0 - w).max(4.0);
        }
        if y + h > hl - 4.0 {
            y = (hl - h - 4.0).max(4.0);
        }
        Rect::new(x, y, x + w, y + h)
    }

    fn toggle_panel_menu(&mut self, panel: PanelId, anchor: Rect, win: WindowId) {
        self.font_menu = None;
        self.align_to_menu = None;
        if self
            .panel_menu
            .as_ref()
            .is_some_and(|m| m.panel == panel && m.win == win)
        {
            self.panel_menu = None;
        } else {
            self.panel_menu = Some(PanelMenu { panel, anchor, win });
        }
        self.request_main_redraw();
        if let Some(h) = self.hosts.get(&win) {
            h.window.request_redraw();
        }
    }

    /// A click while a panel hamburger flyout is open. Returns true if the
    /// press was consumed (inside the flyout, or a toggle on the same
    /// hamburger). Closing on an outside click returns false so the press
    /// can still open another hamburger.
    fn panel_menu_click(&mut self, win: WindowId, p: Point) -> bool {
        let Some(m) = self.panel_menu else {
            return false;
        };
        if m.win != win {
            self.panel_menu = None;
            self.request_main_redraw();
            return false;
        }
        if m.anchor.contains(p) {
            self.panel_menu = None;
            self.request_main_redraw();
            return true;
        }
        let items = panels::menu(m.panel, &self.tip_ctx());
        let (wl, hl) = self
            .hosts
            .get(&win)
            .map(|h| {
                let s = h.window.inner_size();
                (s.width as f64 / self.scale, s.height as f64 / self.scale)
            })
            .unwrap_or((1280.0, 800.0));
        let fly = Self::panel_menu_flyout(m.anchor, &items, wl, hl);
        if !fly.contains(p) {
            self.panel_menu = None;
            self.request_main_redraw();
            return false;
        }
        if let Some(id) = Self::hit_panel_menu_item(fly, &items, p) {
            let panel = m.panel;
            self.panel_menu = None;
            self.apply_panel_action(panels::Action::PanelMenu { panel, id }, false);
        }
        self.request_main_redraw();
        true
    }

    fn hit_panel_menu_item(
        fly: Rect,
        items: &[panels::MenuEntry],
        p: Point,
    ) -> Option<&'static str> {
        let mut y = fly.y0 + Self::PM_PAD;
        for e in items {
            match e {
                panels::MenuEntry::Separator => y += Self::PM_SEP,
                panels::MenuEntry::Item { id, .. } => {
                    let row = Rect::new(fly.x0, y, fly.x1, y + Self::PM_ROW);
                    if row.contains(p) {
                        return Some(*id);
                    }
                    y += Self::PM_ROW;
                }
            }
        }
        None
    }

    /// Start an inline rename, seeding the buffer with the current name.
    fn begin_rename(&mut self, target: panels::RenameId) {
        let doc = self.doc.editor.document();
        let buf = match target {
            panels::RenameId::Layer(id) => doc
                .layers()
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.name.clone()),
            panels::RenameId::Object(id) => {
                Some(doc.object(id).and_then(|o| o.name.clone()).unwrap_or_default())
            }
            panels::RenameId::Artboard(id) => doc.artboard(id).map(|a| a.name.clone()),
        };
        if let Some(buf) = buf {
            self.doc.rename = Some(Rename {
                target,
                buf,
                fresh: true,
            });
            self.request_main_redraw();
        }
    }

    /// Commit the inline rename (empty name = cancel).
    fn commit_rename(&mut self) {
        let Some(r) = self.doc.rename.take() else {
            return;
        };
        let name = r.buf.trim().to_string();
        if !name.is_empty() {
            let cmd = match r.target {
                panels::RenameId::Layer(id) => Command::RenameLayer { id, name },
                panels::RenameId::Object(id) => Command::RenameObject {
                    id,
                    name: Some(name),
                },
                panels::RenameId::Artboard(id) => Command::RenameArtboard { id, name },
            };
            let _ = self.doc.editor.execute(cmd);
        }
        self.request_main_redraw();
    }

    /// A key while an inline rename is active. Returns `true` if consumed.
    fn rename_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.doc.rename.is_none() || !event.state.is_pressed() {
            return self.doc.rename.is_some();
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => self.commit_rename(),
            PhysicalKey::Code(KeyCode::Escape) => {
                self.doc.rename = None;
                self.request_main_redraw();
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some(r) = &mut self.doc.rename {
                    r.fresh = false;
                    r.buf.pop();
                }
                self.request_main_redraw();
            }
            _ => {
                if let (Some(r), Some(txt)) = (&mut self.doc.rename, event.text.as_ref()) {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if r.fresh {
                            r.buf.clear();
                            r.fresh = false;
                        }
                        r.buf.push(ch);
                    }
                    self.request_main_redraw();
                }
            }
        }
        true
    }

    /// Keyboard while the Layers search field has focus. Mirrors
    /// [`Self::rename_key`]: printable chars extend the query, Backspace
    /// trims it, Enter / Escape blur the field (Escape on an empty query
    /// clears the filter). Always consumes the key while focused.
    fn layer_search_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if !self.layer_search_focused {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.layer_search_focused = false;
            }
            PhysicalKey::Code(KeyCode::Escape) => {
                if self.layer_query.is_empty() {
                    self.layer_search_focused = false;
                } else {
                    self.layer_query.clear();
                }
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                self.layer_query.pop();
            }
            _ => {
                if let Some(txt) = event.text.as_ref() {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        self.layer_query.push(ch);
                    }
                }
            }
        }
        self.request_main_redraw();
        true
    }

    /// Appearance of the first selected object, for the Swatches panel.
    fn representative(&self) -> Option<amalith_core::Appearance> {
        self.doc.selection
            .first()
            .and_then(|id| self.doc.editor.document().object(*id))
            .map(|o| o.appearance)
    }

    /// Frontmost selected object (last in layer stacking). Illustrator uses
    /// this as the default key object when Align To Key Object is chosen
    /// without a click.
    fn frontmost_selected(&self) -> Option<ObjectId> {
        let want: std::collections::HashSet<ObjectId> =
            self.doc.selection.iter().copied().collect();
        let doc = self.doc.editor.document();
        let mut last = None;
        for layer in doc.layers() {
            for &id in doc.children_of(amalith_core::ObjectParent::Layer(layer.id)) {
                if want.contains(&id) {
                    last = Some(id);
                }
            }
        }
        last.or_else(|| self.doc.selection.last().copied())
    }

    /// Keep Align To / key object in sync with the current selection.
    /// One object → Artboard (can't align to itself). Key object dropped
    /// when it leaves the selection.
    fn sync_align_mode(&mut self) {
        if self.doc.selection.len() < 2
            || self
                .key_object
                .is_some_and(|k| !self.doc.selection.contains(&k))
        {
            self.key_object = None;
        }
        if self.key_object.is_some() {
            self.align_to = amalith_commands::AlignTo::KeyObject;
            return;
        }
        if self.align_to == amalith_commands::AlignTo::KeyObject {
            self.align_to = amalith_commands::AlignTo::Selection;
        }
        if self.doc.selection.len() <= 1
            && self.align_to == amalith_commands::AlignTo::Selection
        {
            self.align_to = amalith_commands::AlignTo::Artboard;
        }
    }

    /// Drop selection ids / anchors that no longer exist.
    fn prune_selection(&mut self) {
        {
            let doc = self.doc.editor.document();
            self.doc.selection.retain(|id| doc.object(*id).is_some());
            self.doc.anchor_sel
                .retain(|(id, i)| match doc.object(*id).map(|o| &o.kind) {
                    Some(amalith_core::ObjectKind::Path(pd)) => {
                        *i < amalith_core::anchor_count(pd.subpaths())
                    }
                    _ => false,
                });
            if self
                .doc.selected_layer
                .is_some_and(|id| !doc.layers().iter().any(|l| l.id == id))
            {
                self.doc.selected_layer = None;
            }
            if self
                .doc.selected_artboard
                .is_some_and(|id| doc.artboard(id).is_none())
            {
                self.doc.selected_artboard = None;
            }
        }
        self.sync_align_mode();
    }

    /// The tool pointer input actually routes to. Holding ⌘ while the
    /// black arrow is active is Illustrator's temporary white-arrow
    /// gesture (⌘+Space stays reserved for zoom).
    fn effective_tool(&self) -> Tool {
        if self.direct_via_cmd() {
            Tool::DirectSelect
        } else {
            self.active_tool
        }
    }

    /// True while the temporary ⌘ white-arrow gesture is in effect: the
    /// Selection tool is active and ⌘ is held.
    fn direct_via_cmd(&self) -> bool {
        self.active_tool == Tool::Select && self.cmd_down && !self.space_down
    }

    /// Paths whose anchors are currently on screen for Direct Selection:
    /// the object selection plus anything with a live anchor selection.
    /// Nothing shows until you've actually picked something — neither the
    /// `A` tool nor the ⌘ gesture lights up every path on its own. A ⌘
    /// marquee still reaches unselected paths (see `on_release`).
    fn node_paths(&self) -> Vec<ObjectId> {
        let mut out = self.doc.selection.clone();
        for (id, _) in &self.doc.anchor_sel {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out.retain(|id| {
            matches!(
                self.doc.editor.document().object(*id).map(|o| &o.kind),
                Some(amalith_core::ObjectKind::Path(_))
            )
        });
        out
    }

    /// Every descendant `Path` of the current object selection — used by the
    /// hold-Space "show all nodes" peek, which unlike Direct Selection also
    /// reaches into selected groups.
    fn peek_paths(&self) -> Vec<ObjectId> {
        fn walk(doc: &amalith_core::Document, id: ObjectId, out: &mut Vec<ObjectId>) {
            match doc.object(id).map(|o| &o.kind) {
                Some(amalith_core::ObjectKind::Path(_)) => out.push(id),
                Some(amalith_core::ObjectKind::Group(g)) => {
                    for &c in &g.children {
                        walk(doc, c, out);
                    }
                }
                _ => {}
            }
        }
        let doc = self.doc.editor.document();
        let mut out = Vec::new();
        for &id in &self.doc.selection {
            walk(doc, id, &mut out);
        }
        out
    }

    /// Whether the hold-Space anchor peek is active: the Selection tool, a
    /// non-empty object selection, Space held (but not the ⌘ zoom combo).
    fn space_peek(&self) -> bool {
        self.space_down
            && !self.cmd_down
            && self.active_tool == Tool::Select
            && !self.doc.selection.is_empty()
    }

    /// Arrow-key nudge (Shift = ×10). Moves the selected anchors when the
    /// Direct Selection tool is active, otherwise the object selection.
    fn nudge(&mut self, dx: f64, dy: f64) {
        let base = self.settings.nudge_step;
        let step = if self.shift_down { base * 10.0 } else { base };
        let delta = amalith_core::Vec2::new(dx * step, dy * step);
        if self.effective_tool() == Tool::DirectSelect && !self.doc.anchor_sel.is_empty() {
            let _ = self.doc.editor.execute(Command::MoveAnchors {
                anchors: self.doc.anchor_sel.clone(),
                delta,
            });
            self.request_main_redraw();
        } else if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::MoveObjects {
                objects: self.doc.selection.clone(),
                delta,
            });
            self.request_main_redraw();
        }
    }

    /// Recompute the handle being pulled out under `Drag::PenHandle` from
    /// the live pointer plus Shift (lock to 45° / 8 directions from the
    /// anchor) and Alt (break the mirror). Safe to call on a pointer move
    /// or on a modifier change, so Shift snaps even with a still cursor.
    fn drag_pen_handle(&mut self) {
        let Drag::PenHandle {
            anchor,
            from,
            space_last,
        } = &self.drag
        else {
            return;
        };
        let (anchor, from, space_last) = (*anchor, *from, *space_last);
        let dp = self.doc_point(self.pointer);

        // Space held: slide the anchor itself under the cursor, carrying
        // its handles rigidly so the curvature pulled so far is frozen.
        // Releasing Space resumes the handle pull from the new position.
        if self.space_down {
            let Some(a) = self.pen.get_mut(anchor) else {
                return;
            };
            if let Some(prev) = space_last {
                let d = dp - prev;
                a.point += d;
                if let Some(h) = a.handle_in.as_mut() {
                    *h += d;
                }
                if let Some(h) = a.handle_out.as_mut() {
                    *h += d;
                }
            }
            let new_from = a.point;
            self.drag = Drag::PenHandle {
                anchor,
                from: new_from,
                space_last: Some(dp),
            };
            self.request_main_redraw();
            return;
        }
        // Space just released — drop the marker; `from` already tracks the
        // anchor's (possibly moved) point, so the pull resumes from there.
        if space_last.is_some() {
            self.drag = Drag::PenHandle {
                anchor,
                from,
                space_last: None,
            };
        }

        let slop = 3.0 / self.doc.view.zoom;
        let alt = self.alt_down;
        let shift = self.shift_down;
        let Some(a) = self.pen.get_mut(anchor) else {
            return;
        };
        if (dp - from).hypot() > slop {
            let h = if shift {
                constrained(Some(a.point), dp, true)
            } else {
                dp
            };
            a.handle_out = Some(h);
            if alt {
                a.mode = amalith_core::HandleMode::Corner;
                a.handle_in = None;
            } else {
                a.mode = amalith_core::HandleMode::Symmetric;
                a.handle_in = Some(Point::new(a.point.x * 2.0 - h.x, a.point.y * 2.0 - h.y));
            }
        } else {
            a.handle_out = None;
            a.handle_in = None;
            a.mode = amalith_core::HandleMode::Corner;
        }
        self.request_main_redraw();
    }

    /// With the Pen tool active and a path already selected (but no draw
    /// in progress), the segment under the pointer that a click would
    /// insert an anchor into — `(object, flat segment ordinal, t)`.
    fn pen_insert_target(&self) -> Option<(ObjectId, usize, f64)> {
        if self.active_tool != Tool::Pen || !self.pen.is_empty() {
            return None;
        }
        let paths = self.node_paths();
        if paths.is_empty() {
            return None;
        }
        let dp = self.doc_point(self.pointer);
        let r = 6.0 / self.doc.view.zoom;
        // An anchor under the pointer takes priority (a click there would
        // start a new path, not insert) — don't offer "+".
        if anchors::topmost_anchor_among(self.doc.editor.document(), &paths, dp, r).is_some() {
            return None;
        }
        anchors::segment_at(self.doc.editor.document(), &paths, dp, r)
    }

    /// Commit the in-progress Pen path (needs ≥2 anchors). `closed` joins
    /// the last anchor back to the first.
    fn commit_pen(&mut self, closed: bool) {
        self.pen_redo.clear();
        if self.pen.len() < 2 {
            self.pen.clear();
            self.last_pen = None;
            return;
        }
        let anchors: Vec<PenAnchor> = std::mem::take(&mut self.pen);
        let cp = |p: Point| amalith_core::Point::new(p.x, p.y);
        let subpath = amalith_core::Subpath {
            anchors: anchors
                .iter()
                .map(|a| amalith_core::Anchor {
                    point: cp(a.point),
                    handle_in: a.handle_in.map(cp),
                    handle_out: a.handle_out.map(cp),
                    mode: a.mode,
                })
                .collect(),
            closed,
        };
        let path = amalith_core::PathData::from_subpaths(vec![subpath]);
        let layer = self.ensure_layer();
        if let Ok(CommandOutcome::Object(id)) = self.doc.editor.execute(Command::CreatePath {
            layer,
            path,
            name: None,
        }) {
            self.doc.selection = vec![id];
            self.last_pen = Some((id, anchors, closed));
            self.apply_new_appearance(id);
        }
        self.request_main_redraw();
    }

    /// Open the New Document modal (⌘N / File ▸ New).
    fn open_new_doc(&mut self) {
        let mut form = newdoc::NewDocForm::default();
        // From Home there's no open document — Create should fill the parked
        // placeholder tab, not add a second one.
        form.boot = self.home.is_some();
        self.newdoc = Some(form);
        self.request_main_redraw();
    }

    /// Route a click on the New Document modal.
    fn apply_newdoc_hit(&mut self, hit: newdoc::Hit) {
        use newdoc::{Hit, Menu};
        match hit {
            Hit::Create => {
                self.create_from_form();
                return;
            }
            Hit::Close => {
                self.newdoc = None;
                self.request_main_redraw();
                return;
            }
            _ => {}
        }
        let Some(form) = self.newdoc.as_mut() else {
            return;
        };
        // Any click that isn't on the open menu itself dismisses it.
        if !matches!(hit, Hit::MenuItem(..) | Hit::ToggleMenu(_)) {
            form.open_menu = None;
        }
        match hit {
            Hit::Field(f) => {
                form.commit_focus();
                form.focus = Some(f);
            }
            Hit::ToggleMenu(m) => {
                form.open_menu = (form.open_menu != Some(m)).then_some(m);
            }
            Hit::MenuItem(m, i) => {
                match m {
                    Menu::Unit => form.set_unit(newdoc::menu_unit(i)),
                    Menu::Color => form.color_mode = newdoc::menu_color(i),
                    Menu::Raster => form.raster = newdoc::menu_raster(i),
                    Menu::Preview => form.preview = newdoc::menu_preview(i),
                }
                form.open_menu = None;
            }
            Hit::Orientation(portrait) => form.set_orientation(portrait),
            Hit::ArtboardMinus => form.artboards = form.artboards.saturating_sub(1).max(1),
            Hit::ArtboardPlus => form.artboards = (form.artboards + 1).min(100),
            Hit::ToggleLink => {
                let on = !form.bleed_linked;
                form.set_link(on);
            }
            Hit::None | Hit::Backdrop | Hit::Create | Hit::Close => {}
        }
        self.request_main_redraw();
    }

    /// A key while the New Document modal is open. Editing goes through the
    /// focused field's [`TextField`]; Up / Down step the numeric ones.
    fn newdoc_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        let Some(f) = self.newdoc.as_ref().and_then(newdoc::NewDocForm::focused) else {
            // No field focused: Esc closes, Enter creates.
            match event.physical_key {
                PhysicalKey::Code(KeyCode::Escape) => {
                    self.newdoc = None;
                    self.request_main_redraw();
                }
                PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => self.create_from_form(),
                _ => {}
            }
            return;
        };

        // Up / Down nudge a numeric field (TextField doesn't handle them).
        if let PhysicalKey::Code(code @ (KeyCode::ArrowUp | KeyCode::ArrowDown)) = event.physical_key {
            let dir = if code == KeyCode::ArrowUp { 1.0 } else { -1.0 };
            let step = if self.shift_down { 10.0 } else { 1.0 } * dir;
            if let Some(form) = self.newdoc.as_mut() {
                form.step_focused(step, &mut self.text);
            }
            self.request_main_redraw();
            return;
        }

        let mods = textedit::Mods {
            shift: self.shift_down,
            alt: self.alt_down,
            meta: self.cmd_down,
        };
        let logical = event.logical_key.clone();
        let typed = event.text.clone();
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        let resp = self.newdoc.as_mut().unwrap().field(f).key(
            &logical,
            mods,
            typed.as_deref(),
            self.clipboard.as_mut(),
            &mut self.text,
        );
        match resp {
            crate::text_field::Resp::Cancel => self.newdoc = None,
            crate::text_field::Resp::Submit => {
                if let Some(form) = self.newdoc.as_mut() {
                    form.commit_focus();
                }
                self.create_from_form();
            }
            crate::text_field::Resp::Tab(back) => {
                if let Some(form) = self.newdoc.as_mut() {
                    form.focus_next(back, &mut self.text);
                }
            }
            _ => {}
        }
        self.request_main_redraw();
    }

    /// Build a fresh document from the modal's form and swap it in.
    fn create_from_form(&mut self) {
        let Some(form) = self.newdoc.as_mut() else {
            return;
        };
        form.commit_focus();
        let (wpx, hpx) = (form.width_px(), form.height_px());
        if wpx <= 0.0 || hpx <= 0.0 {
            self.doc.io_error = Some("Width and height must be greater than zero.".into());
            self.request_main_redraw();
            return;
        }
        let name = {
            let n = form.name.text();
            let n = n.trim();
            if n.is_empty() { "Untitled".to_string() } else { n.to_string() }
        };
        let n_ab = form.artboards.max(1);
        let [bt, bb, bl, br] = form.bleed_px();
        let (unit, color_mode, raster, preview) =
            (form.unit, form.color_mode, form.raster, form.preview);

        let mut doc = amalith_core::Document::new(&name);
        doc.settings.default_unit = unit;
        doc.settings.color_mode = color_mode;
        doc.settings.raster_effects = raster;
        doc.settings.preview_mode = preview;
        doc.settings.bleed = amalith_core::Bleed {
            top: bt,
            bottom: bb,
            left: bl,
            right: br,
        };
        let mut editor = Editor::new(doc);
        let gap = 48.0;
        // Centre the whole row on the document origin, then tile right.
        let row_w = n_ab as f64 * wpx + (n_ab.saturating_sub(1)) as f64 * gap;
        let x0 = -row_w / 2.0;
        let y0 = -hpx / 2.0;
        for i in 0..n_ab {
            let x = x0 + i as f64 * (wpx + gap);
            let _ = editor.execute(Command::CreateArtboard {
                name: format!("Artboard {}", i + 1),
                rect: amalith_core::Rect::new(x, y0, x + wpx, y0 + hpx),
                index: None,
            });
        }
        let _ = editor.execute(Command::CreateLayer {
            name: "Layer 1".into(),
            index: None,
        });
        // The starter artboards + layer are the baseline, not undo steps —
        // otherwise ⌘Z walks back to a document with no artboards at all.
        editor.clear_history();

        let boot = self.newdoc.as_ref().is_some_and(|f| f.boot);
        self.newdoc = None;
        // Leaving Home for the editor.
        self.home = None;
        if boot {
            // No open document yet: fill the parked placeholder tab.
            self.load_active_doc(Doc::new(editor));
            self.pending_fit = true;
            self.request_main_redraw();
        } else {
            self.add_doc(Doc::new(editor));
        }
    }

    /// Route one [`MenuAction`] to the matching operation. Mirrors the
    /// keyboard shortcuts so the menu bar and the keys stay in step.
    fn run_menu_action(&mut self, action: MenuAction) {
        match action {
            // Quit is dispatched in `about_to_wait`, which holds the event
            // loop; it never reaches here.
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            MenuAction::Quit => {}
            MenuAction::About => {
                if self.about.is_none() {
                    self.about = about::About::load();
                }
                self.request_main_redraw();
            }
            MenuAction::Preferences => {
                self.prefs = Some(prefs::Prefs::new(
                    self.settings,
                    self.scripts.clone(),
                    self.keymaps.clone(),
                ));
                self.request_main_redraw();
            }
            MenuAction::New => self.open_new_doc(),
            MenuAction::Open => self.open_document(),
            MenuAction::Save => self.save_document(false),
            MenuAction::SaveAs => self.save_document(true),
            MenuAction::ImportSvg => self.import_svg(),
            MenuAction::Place => self.place_image_dialog(),
            MenuAction::ExportForScreens => self.request_export_dialog(),
            MenuAction::Undo => {
                // macOS routes ⌘Z through this native item, not the
                // keyboard handler — commit any open text edit first so the
                // typed text is on the undo stack (matches keyboard ⌘Z).
                if self.text_edit.is_some() {
                    self.commit_text_edit();
                }
                let _ = self.doc.editor.undo();
                self.prune_selection();
                self.request_main_redraw();
            }
            MenuAction::Redo => {
                if self.text_edit.is_some() {
                    self.commit_text_edit();
                }
                let _ = self.doc.editor.redo();
                self.prune_selection();
                self.request_main_redraw();
            }
            MenuAction::Cut => {
                if !self.doc.selection.is_empty() {
                    let ids = self.doc.selection.clone();
                    self.copy_selection(&ids);
                    self.purge_threads(&ids);
                    let _ = self.doc.editor.execute(Command::DeleteObjects {
                        ids: std::mem::take(&mut self.doc.selection),
                    });
                    self.request_main_redraw();
                }
            }
            MenuAction::Copy => {
                if !self.doc.selection.is_empty() {
                    let ids = self.doc.selection.clone();
                    self.copy_selection(&ids);
                }
            }
            MenuAction::Paste => self.paste_clipboard(PastePlace::Plain),
            MenuAction::Duplicate => {
                if !self.doc.selection.is_empty() {
                    if let Ok(ids) = self
                        .doc.editor
                        .duplicate_objects(&self.doc.selection, amalith_core::Vec2::new(16.0, 16.0))
                    {
                        self.doc.selection = ids;
                    }
                    self.request_main_redraw();
                }
            }
            MenuAction::SelectAll => self.select_all(),
            MenuAction::SelectAllArtboard => self.select_all_artboard(),
            MenuAction::Deselect => self.deselect(),
            MenuAction::SelectNextAbove => self.select_next_z(1),
            MenuAction::SelectNextBelow => self.select_next_z(-1),
            MenuAction::SelectSame(kind) => self.select_same(kind),
            MenuAction::TogglePanel(id) => self.toggle_panel(id),
            MenuAction::BringForward => self.restack(1),
            MenuAction::BringToFront => self.restack_extreme(true),
            MenuAction::SendBackward => self.restack(-1),
            MenuAction::SendToBack => self.restack_extreme(false),
            MenuAction::ZoomIn => self.zoom_step(1.6),
            MenuAction::ZoomOut => self.zoom_step(1.0 / 1.6),
            MenuAction::FitArtboard => self.zoom_fit(),
            MenuAction::FitAll => self.fit_view(),
            MenuAction::ClipMake => self.clip_make(),
            MenuAction::ClipRelease => self.clip_release(),
            MenuAction::HelpDocs => crate::about::open_url("https://amalith.app/docs"),
            MenuAction::ConvertTextKind => {
                if let Some(&id) = self.doc.selection.first() {
                    if self.doc.selection.len() == 1
                        && matches!(
                            self.doc.editor.document().object(id).map(|o| &o.kind),
                            Some(amalith_core::ObjectKind::Text(_))
                        )
                    {
                        self.toggle_text_kind(id);
                    }
                }
            }
            MenuAction::ToggleOutline => self.toggle_outline_mode(),
            MenuAction::ToggleGuides => self.set_guides_hidden(!self.guides_hidden),
            MenuAction::ToggleGuideLock => self.set_guides_locked(!self.guides_locked),
            MenuAction::ClearGuides => self.clear_guides(),
            MenuAction::AddScriptsFolder => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Choose Scripts Folder")
                    .pick_folder()
                {
                    self.scripts.dir = Some(dir);
                    crate::scripts::save(&self.scripts);
                    self.rebuild_native_menu();
                }
            }
            MenuAction::RevealScriptsFolder => {
                if let Some(dir) = self.scripts.dir.clone() {
                    crate::scripts::reveal(&dir);
                }
            }
            MenuAction::RemoveScriptsFolder => {
                self.scripts.dir = None;
                crate::scripts::save(&self.scripts);
                self.rebuild_native_menu();
            }
            MenuAction::RunScript(path) => crate::scripts::run(&path),
        }
    }

    /// Options-bar Weight stepper. Reads the live selection value (or the
    /// stored current), nudges it, applies to any selection, and keeps it
    /// as the value new shapes will use.
    fn step_weight(&mut self, dir: i32) {
        let base = self
            .doc.selection
            .first()
            .and_then(|id| self.doc.editor.document().object(*id))
            .map(|o| o.appearance.stroke_width)
            .unwrap_or(self.doc.stroke_w);
        let step = if base < 1.0 { 0.25 } else { 1.0 };
        let next = (base + dir as f64 * step).clamp(0.0, 1000.0);
        self.doc.stroke_w = next;
        if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                objects: self.doc.selection.clone(),
                width: next,
            });
        }
        self.request_main_redraw();
    }

    /// Options-bar Opacity stepper (5% steps).
    fn step_opacity(&mut self, dir: i32) {
        let base = self
            .doc.selection
            .first()
            .and_then(|id| self.doc.editor.document().object(*id))
            .map(|o| o.appearance.opacity)
            .unwrap_or(self.doc.opacity);
        let next = (base + dir as f32 * 0.05).clamp(0.0, 1.0);
        self.doc.opacity = next;
        if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::SetOpacity {
                objects: self.doc.selection.clone(),
                opacity: next,
            });
        }
        self.request_main_redraw();
    }

    /// The stroke style the Stroke flyout should show: the first selected
    /// object's, or the stored default when nothing is selected.
    fn stroke_style_repr(&self) -> StrokeStyle {
        self.doc.selection
            .first()
            .and_then(|id| self.doc.editor.document().object(*id))
            .map(|o| o.appearance.stroke_style)
            .unwrap_or(self.doc.stroke_style)
    }

    /// Where the Stroke flyout sits: hanging off the "Stroke" link in the
    /// options bar, clamped to the window.
    fn stroke_flyout_layout(&self, win_w: f64) -> stroke_panel::Layout {
        let cx = self.context_bar_ctx();
        let anchor = context_bar::segment_rect(
            opt_bar_rect(win_w),
            &cx,
            context_bar::SegKind::Stroke,
        )
        .map_or(200.0, |r| r.x0 - 4.0);
        let x = anchor.min(win_w - stroke_panel::W - 6.0).max(6.0);
        stroke_panel::layout(Point::new(x, APP_BAR_H + OPT_BAR_H + 3.0))
    }

    /// Read the current stroke style (from the first selected object, or
    /// the stored default), let `f` mutate it, then write it back onto the
    /// whole selection and remember it for new shapes. Drives the Stroke
    /// flyout.
    fn edit_stroke_style(&mut self, f: impl FnOnce(&mut StrokeStyle)) {
        let mut style = self
            .doc.selection
            .first()
            .and_then(|id| self.doc.editor.document().object(*id))
            .map(|o| o.appearance.stroke_style)
            .unwrap_or(self.doc.stroke_style);
        f(&mut style);
        self.doc.stroke_style = style;
        if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::SetStrokeStyle {
                objects: self.doc.selection.clone(),
                style,
            });
        }
        self.request_main_redraw();
    }

    /// Apply one Stroke-flyout [`stroke_panel::Hit`]. `dir` is the scroll /
    /// stepper direction (already `+1` / `-1`).
    fn apply_stroke_flyout(&mut self, hit: stroke_panel::Hit, dir: i32) {
        use stroke_panel::Hit;
        match hit {
            Hit::Inside => {}
            Hit::Outside => {
                self.stroke_popover = false;
                self.request_main_redraw();
            }
            Hit::WeightStep(_) => self.step_weight(dir),
            Hit::LimitStep(_) => self.edit_stroke_style(|s| {
                s.miter_limit = (s.miter_limit + dir as f64).clamp(1.0, 500.0);
            }),
            Hit::Cap(cap) => self.edit_stroke_style(|s| s.cap = cap),
            Hit::Join(join) => self.edit_stroke_style(|s| s.join = join),
            Hit::Align(align) => self.edit_stroke_style(|s| s.align = align),
            Hit::ToggleDashed => self.edit_stroke_style(|s| {
                s.dashed = !s.dashed;
                if s.dashed && s.dash[0] <= 0.0 && s.dash[1] <= 0.0 {
                    let (d, g) = stroke_panel::dash_gap(s);
                    s.dash[0] = d;
                    s.dash[1] = g;
                }
            }),
            Hit::DashStep(_) => self.edit_stroke_style(|s| {
                let (d, g) = stroke_panel::dash_gap(s);
                s.dash[0] = (d + dir as f64).max(0.0);
                s.dash[1] = g;
            }),
            Hit::GapStep(_) => self.edit_stroke_style(|s| {
                let (d, g) = stroke_panel::dash_gap(s);
                s.dash[0] = d;
                s.dash[1] = (g + dir as f64).max(0.0);
            }),
        }
    }

    /// Push the options-bar Weight / Opacity / Stroke style onto a freshly
    /// created object, so those fields mean something with nothing selected.
    fn apply_new_appearance(&mut self, id: ObjectId) {
        let def = amalith_core::Appearance::default();
        if self.doc.fill != def.fill || self.doc.stroke != def.stroke {
            let _ = self.doc.editor.execute(Command::SetPaints {
                objects: vec![id],
                fill: Some(self.doc.fill),
                stroke: Some(self.doc.stroke),
            });
        }
        if (self.doc.stroke_w - def.stroke_width).abs() > f64::EPSILON {
            let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                objects: vec![id],
                width: self.doc.stroke_w,
            });
        }
        if self.doc.stroke_style != StrokeStyle::default() {
            let _ = self.doc.editor.execute(Command::SetStrokeStyle {
                objects: vec![id],
                style: self.doc.stroke_style,
            });
        }
        if (self.doc.opacity - 1.0).abs() > f32::EPSILON {
            let _ = self.doc.editor.execute(Command::SetOpacity {
                objects: vec![id],
                opacity: self.doc.opacity,
            });
        }
    }

    /// ⌘] / ⌘[ — move the selection `steps` places forward (+) or back
    /// (−) in its parent's paint order.
    fn restack(&mut self, steps: i32) {
        if self.doc.selection.is_empty() || steps == 0 {
            return;
        }
        let _ = self.doc.editor.execute(Command::NudgeStack {
            ids: self.doc.selection.clone(),
            steps,
        });
        self.request_main_redraw();
    }

    /// ⌘⌥] / ⌘⌥[ — bring the selection to the very front / back. Bounded
    /// by the largest sibling count among the selection's parents, which
    /// is the most swaps `NudgeStack` could ever need.
    fn restack_extreme(&mut self, to_front: bool) {
        let doc = self.doc.editor.document();
        let bound = self
            .doc.selection
            .iter()
            .filter_map(|id| doc.object(*id))
            .map(|o| doc.children_of(o.parent).len() as i32)
            .max()
            .unwrap_or(0);
        self.restack(if to_front { bound } else { -bound });
    }

    /// ⌘O — pick a `.amalith` file and load it, replacing the document.
    fn open_document(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Amalith document", &["amalith"])
            .pick_file()
        else {
            return;
        };
        self.open_path(&path);
    }

    /// Load `path` into a tab (filling the Home placeholder if we're on Home,
    /// otherwise a new tab) and record it in the recent list.
    fn open_path(&mut self, path: &std::path::Path) {
        match amalith_io::load(path) {
            Ok((document, assets)) => {
                let mut doc = Doc::new(Editor::new(document));
                doc.asset_store = assets;
                doc.file_path = Some(path.to_path_buf());
                let from_home = self.home.take().is_some();
                if from_home {
                    self.load_active_doc(doc);
                    self.pending_fit = true;
                    self.request_main_redraw();
                } else {
                    self.add_doc(doc);
                }
                recent::push(path);
            }
            Err(err) => {
                self.doc.io_error = Some(format!("Open failed: {err}"));
                self.request_main_redraw();
            }
        }
    }

    /// ⌘S / ⌘⇧S — write the document to its `.amalith` file, prompting for
    /// a path when there isn't one yet or `save_as` forces it.
    fn save_document(&mut self, save_as: bool) {
        let path = if save_as { None } else { self.doc.file_path.clone() }.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Amalith document", &["amalith"])
                .set_file_name("Untitled.amalith")
                .save_file()
        });
        let Some(path) = path else {
            return;
        };
        match amalith_io::save(self.doc.editor.document(), &self.doc.asset_store, &path) {
            Ok(()) => {
                recent::push(&path);
                self.doc.file_path = Some(path);
                self.doc.io_error = None;
            }
            Err(err) => self.doc.io_error = Some(format!("Save failed: {err}")),
        }
        self.request_main_redraw();
    }

    /// ⌘⇧I — pick an `.svg` file and paste its shapes into the document.
    fn import_svg(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG", &["svg"])
            .pick_file()
        else {
            return;
        };
        let svg = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(err) => {
                self.doc.io_error = Some(format!("Import failed: {err}"));
                self.request_main_redraw();
                return;
            }
        };
        match self.doc.editor.copy_from_svg(&svg) {
            Ok(()) => match self.doc.editor.paste(amalith_core::Vec2::ZERO, PasteStack::Top) {
                Ok(ids) => {
                    self.doc.selection = ids;
                    self.doc.anchor_sel.clear();
                    self.doc.io_error = None;
                }
                Err(err) => self.doc.io_error = Some(format!("Import failed: {err}")),
            },
            Err(err) => self.doc.io_error = Some(format!("Import failed: {err}")),
        }
        self.request_main_redraw();
    }

    /// File ▸ Place… / ⌘⇧P — pick a PNG or JPEG and drop it at the view centre.
    pub(in crate::app) fn place_image_dialog(&mut self) {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "heic", "heif", "tif", "tiff", "gif", "webp", "bmp"],
            )
            .pick_file()
        else {
            return;
        };
        let center = self.doc_point(self.canvas_viewport().center());
        self.place_image_at(&path, center);
    }

    /// Insert a linked raster at `center` (document space), 1 image px = 1 doc px.
    /// The object is created from header size immediately; pixels arrive via LOD.
    fn place_image_at(&mut self, path: &std::path::Path, center: Point) {
        let Some((nw, nh)) = canvas::raster_dimensions(path) else {
            self.doc.io_error = Some(format!("Could not read image: {}", path.display()));
            self.request_main_redraw();
            return;
        };
        let (w, h) = (nw as f64, nh as f64);
        let layer = self.ensure_layer();
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
        let src = path.to_string_lossy().into_owned();
        let cmd = Command::CreateImage {
            layer,
            path: src.clone(),
            bounds: amalith_core::Rect::new(0.0, 0.0, w, h),
            transform: amalith_core::Affine::translate((center.x - w * 0.5, center.y - h * 0.5)),
            name,
            embedded: false,
        };
        match self.doc.editor.execute(cmd) {
            Ok(CommandOutcome::Object(id)) => {
                let asset = self.doc.editor.document().object(id).and_then(|o| {
                    match &o.kind {
                        amalith_core::ObjectKind::Image(d) => Some(d.asset),
                        _ => None,
                    }
                });
                if let Some(asset) = asset {
                    self.request_lod(asset, src, Some(path.to_path_buf()), None, nw, nh);
                }
                self.doc.selection = vec![id];
                self.doc.anchor_sel.clear();
                self.doc.io_error = None;
            }
            Err(err) => self.doc.io_error = Some(format!("Place failed: {err}")),
            _ => {}
        }
        self.request_main_redraw();
    }

    /// Drop a raster onto the document at the pointer.
    fn on_drop_file(&mut self, path: std::path::PathBuf) {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return;
        }
        // Try to decode anything we can read (PNG/JPEG, and on macOS HEIC
        // via ImageIO). Skip silently if it isn't an image — a .amalith
        // or random file dropped on the window shouldn't error.
        if !canvas::is_raster_path(&path) && canvas::raster_dimensions(&path).is_none() {
            return;
        }
        let center = if self.pointer_win == self.main_id {
            self.doc_point(self.pointer)
        } else {
            self.doc_point(self.canvas_viewport().center())
        };
        self.place_image_at(&path, center);
    }

    /// Place pixels that never had a file (iMessage pasteboard, etc.).
    /// Bytes go in the document's asset store — nothing is written to disk.
    fn place_image_png_bytes(&mut self, name: &str, bytes: &[u8], center: Point) {
        let Some((nw, nh)) = canvas::raster_dimensions_bytes(bytes) else {
            self.doc.io_error = Some("Could not read dropped image.".into());
            self.request_main_redraw();
            return;
        };
        let (w, h) = (nw as f64, nh as f64);
        let stem = if name.is_empty() { "Image" } else { name };
        let container = format!("images/{stem}-{}.png", amalith_core::AssetId::new());
        self.doc.asset_store.insert(&container, bytes.to_vec());
        let layer = self.ensure_layer();
        let cmd = Command::CreateImage {
            layer,
            path: container.clone(),
            bounds: amalith_core::Rect::new(0.0, 0.0, w, h),
            transform: amalith_core::Affine::translate((center.x - w * 0.5, center.y - h * 0.5)),
            name: Some(stem.to_string()),
            embedded: true,
        };
        match self.doc.editor.execute(cmd) {
            Ok(CommandOutcome::Object(id)) => {
                let asset = self.doc.editor.document().object(id).and_then(|o| {
                    match &o.kind {
                        amalith_core::ObjectKind::Image(d) => Some(d.asset),
                        _ => None,
                    }
                });
                if let Some(asset) = asset {
                    self.request_lod(asset, container, None, Some(bytes.to_vec()), nw, nh);
                }
                self.doc.selection = vec![id];
                self.doc.anchor_sel.clear();
                self.doc.io_error = None;
            }
            Err(err) => self.doc.io_error = Some(format!("Place failed: {err}")),
            _ => {}
        }
        self.request_main_redraw();
    }

    #[cfg(target_os = "macos")]
    fn drain_mac_drops(&mut self) {
        let drops = crate::macdrop::drain();
        if drops.is_empty() {
            return;
        }
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return;
        }
        let fallback = self.doc_point(self.canvas_viewport().center());
        for drop in drops {
            let center = drop.at.map_or(fallback, |(x, y)| self.doc_point(Point::new(x, y)));
            match drop.item {
                crate::macdrop::Incoming::Path(path) => self.place_image_at(&path, center),
                crate::macdrop::Incoming::Png { name, bytes } => {
                    self.place_image_png_bytes(&name, &bytes, center)
                }
            }
        }
    }

    fn request_lod(
        &mut self,
        asset: AssetId,
        key: String,
        path: Option<std::path::PathBuf>,
        bytes: Option<Vec<u8>>,
        native_w: u32,
        native_h: u32,
    ) {
        if let Some(lods) = self.decoded_by_path.get(&key) {
            self.image_cache.insert(asset, lods.clone());
            self.image_lod
                .insert(asset, (crate::lod::LOD_SIDES.len() - 1) as u8);
            return;
        }
        if !self.lod_inflight.insert(asset) {
            return;
        }
        if let Some(p) = path {
            self.lod.enqueue_path(asset, p, native_w, native_h);
        } else if let Some(b) = bytes {
            self.lod.enqueue_bytes(asset, key, b, native_w, native_h);
        } else {
            self.lod_inflight.remove(&asset);
        }
    }

    fn drain_lod(&mut self) {
        let mut dirty = false;
        for ready in self.lod.drain() {
            let done = ready.done;
            let key = ready.key.clone();
            let asset = ready.asset;
            let level = ready.level;
            if let Some(gpu) = ready.gpu {
                let lods = self.image_cache.entry(asset).or_default();
                lods.set(level, gpu);
                let prev = self.image_lod.get(&asset).copied().unwrap_or(0);
                if level >= prev {
                    self.image_lod.insert(asset, level);
                }
            }
            if done {
                if let Some(lods) = self.image_cache.get(&asset) {
                    self.decoded_by_path.insert(key, lods.clone());
                }
                self.lod_inflight.remove(&asset);
            }
            dirty = true;
        }
        if dirty {
            self.request_main_redraw();
        }
    }

    fn visible_image_assets(&self) -> std::collections::HashSet<AssetId> {
        let vis = self
            .doc
            .view
            .to_screen()
            .inverse()
            .transform_rect_bbox(canvas::cull_rect(
                self.canvas_viewport(),
                self.settings.cull_inset,
            ));
        let doc = self.doc.editor.document();
        let mut out = std::collections::HashSet::new();
        fn walk(
            doc: &Document,
            ids: &[ObjectId],
            vis: Rect,
            out: &mut std::collections::HashSet<AssetId>,
        ) {
            for &id in ids {
                let Some(obj) = doc.object(id) else { continue };
                if !obj.visible {
                    continue;
                }
                if let Some(b) = doc.bounds_of(id) {
                    let b = convert::rect(b);
                    if b.x1 <= vis.x0 || b.x0 >= vis.x1 || b.y1 <= vis.y0 || b.y0 >= vis.y1 {
                        continue;
                    }
                }
                match &obj.kind {
                    amalith_core::ObjectKind::Image(i) => {
                        out.insert(i.asset);
                    }
                    amalith_core::ObjectKind::Group(g) => walk(doc, &g.children, vis, out),
                    _ => {}
                }
            }
        }
        for layer in doc.layers() {
            if layer.visible {
                walk(doc, &layer.children, vis, &mut out);
            }
        }
        out
    }

    /// Kick off LOD decode for visible image assets that have no GPU copy yet.
    fn warm_images(&mut self) {
        let needed = self.visible_image_assets();
        let mut linked = Vec::new();
        let mut embedded = Vec::new();
        for a in self.doc.editor.document().assets() {
            if a.kind != amalith_core::AssetKind::Image
                || !needed.contains(&a.id)
                || self.image_cache.contains_key(&a.id)
                || self.lod_inflight.contains(&a.id)
            {
                continue;
            }
            match &a.source {
                amalith_core::AssetSource::Linked { path } => {
                    linked.push((a.id, path.clone()));
                }
                amalith_core::AssetSource::Embedded { container_path } => {
                    embedded.push((a.id, container_path.clone()));
                }
            }
        }
        for (id, path) in linked {
            if let Some(lods) = self.decoded_by_path.get(&path) {
                self.image_cache.insert(id, lods.clone());
                continue;
            }
            let (nw, nh) = canvas::raster_dimensions(std::path::Path::new(&path)).unwrap_or((1, 1));
            self.request_lod(
                id,
                path.clone(),
                Some(std::path::PathBuf::from(&path)),
                None,
                nw,
                nh,
            );
        }
        for (id, key) in embedded {
            if let Some(lods) = self.decoded_by_path.get(&key) {
                self.image_cache.insert(id, lods.clone());
                continue;
            }
            let bytes = self.doc.asset_store.get(&key).map(|b| b.to_vec());
            let Some(bytes) = bytes else { continue };
            let (nw, nh) = canvas::raster_dimensions_bytes(&bytes).unwrap_or((1, 1));
            self.request_lod(id, key, None, Some(bytes), nw, nh);
        }
    }

    /// Push `settings.accent` into the live theme (and its derived tokens).
    fn apply_theme_accent(&mut self) {
        let [r, g, b] = self.settings.accent;
        self.theme
            .set_accent(vello::peniko::Color::from_rgb8(r, g, b));
        self.request_main_redraw();
    }

    fn set_tool(&mut self, t: Tool) {
        // Leaving the Type tool commits whatever's being typed.
        if t != Tool::Text && self.text_edit.is_some() {
            self.commit_text_edit();
        }
        if t != Tool::Pen {
            // Switching tools ends the path in progress: 2+ anchors commit
            // as an open path (matching Esc / Enter), a lone anchor drops.
            if !self.pen.is_empty() {
                self.commit_pen(false);
            }
            self.pen.clear();
            self.pen_redo.clear();
        }
        if t != Tool::DirectSelect {
            self.doc.anchor_sel.clear();
        }
        if t == Tool::Artboard && self.active_tool != Tool::Artboard {
            self.pre_artboard_tool = self.active_tool;
        }
        if t != Tool::Artboard {
            self.doc.selected_artboard = None;
            self.artboard_edit = None;
            self.artboard_fill_menu = false;
        }
        if t.is_shape() {
            self.last_shape_tool = t;
        }
        if t != Tool::Rotate {
            // The Rotate tool's custom reference point is per-session.
            self.transform_pivot = None;
        }
        self.last_pen = None;
        self.active_tool = t;
        self.request_main_redraw();
    }

    // --- Type tool -----------------------------------------------------

    /// The type style the Character panel should show / edit: the live text
    /// edit, else a selected text object, else the new-text defaults.
    fn active_text_style(&self) -> amalith_core::TextStyle {
        if let Some(te) = &self.text_edit {
            return te.style().clone();
        }
        for &id in &self.doc.selection {
            if let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            {
                return t.style.clone();
            }
        }
        self.text_defaults.clone()
    }

    /// Alignment the Paragraph panel shows: live edit, else a selected
    /// text object, else the new-text default.
    fn active_text_align(&self) -> amalith_core::TextAlign {
        if let Some(te) = &self.text_edit {
            return te.align();
        }
        for &id in &self.doc.selection {
            if let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            {
                return t.align;
            }
        }
        self.text_align_default
    }

    /// Paragraph attributes the Paragraph panel shows — same source order.
    fn active_text_paragraph(&self) -> amalith_core::Paragraph {
        if let Some(te) = &self.text_edit {
            return te.paragraph();
        }
        for &id in &self.doc.selection {
            if let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            {
                return t.paragraph;
            }
        }
        self.para_defaults
    }

    /// Apply an alignment to the live edit, else the selected text
    /// objects, else the new-text default.
    fn edit_text_align(&mut self, align: amalith_core::TextAlign) {
        if let Some(te) = &mut self.text_edit {
            te.set_align(align);
            self.request_main_redraw();
        } else if !self.edit_selected_text_data(|d| d.align = align) {
            self.text_align_default = align;
            self.request_main_redraw();
        }
    }

    /// Mutate paragraph attributes the same way.
    fn edit_paragraph(&mut self, f: impl Fn(&mut amalith_core::Paragraph)) {
        if let Some(te) = &mut self.text_edit {
            let mut p = te.paragraph();
            f(&mut p);
            te.set_paragraph(p);
            self.request_main_redraw();
        } else if !self.edit_selected_text_data(|d| f(&mut d.paragraph)) {
            let mut p = self.para_defaults;
            f(&mut p);
            self.para_defaults = p;
            self.request_main_redraw();
        }
    }

    /// Run `f` over every selected text object's `TextData`, re-measure,
    /// and commit each. Returns whether any text object was in the
    /// selection.
    fn edit_selected_text_data(&mut self, f: impl Fn(&mut amalith_core::TextData)) -> bool {
        let ids: Vec<ObjectId> = self
            .doc
            .selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Text(_))
                )
            })
            .collect();
        if ids.is_empty() {
            return false;
        }
        for id in ids {
            let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            else {
                continue;
            };
            let mut data = t.clone();
            f(&mut data);
            data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
            let _ = self.doc.editor.execute(Command::SetText { object: id, data });
        }
        self.request_main_redraw();
        true
    }

    /// Commit a Selection-tool scale (`preview` = new world transforms).
    /// A text object with a (near-)uniform scale folds it into its font
    /// size (and area-box dimensions) and keeps an unscaled transform, so
    /// the point size in the Character panel is real. Everything else —
    /// and any non-uniform text scale — just takes the new transform.
    fn commit_scaled(&mut self, preview: HashMap<ObjectId, Affine>) {
        let mut xforms: Vec<(ObjectId, amalith_core::Affine)> = Vec::new();
        let mut texts: Vec<(ObjectId, amalith_core::TextData)> = Vec::new();

        for (id, xf) in preview {
            let td = match self.doc.editor.document().object(id).map(|o| &o.kind) {
                Some(amalith_core::ObjectKind::Text(t)) => Some(t.clone()),
                _ => None,
            };
            let [a, b, c, d, e, f] = xf.as_coeffs();
            let sx = (a * a + b * b).sqrt();
            let sy = (c * c + d * d).sqrt();
            let uniform = sx > 1e-6 && (sx - sy).abs() / sx.max(sy) < 0.02;
            // Rotation / shear present → not a plain scale, leave it alone.
            let plain = (a * c + b * d).abs() < 1e-3 * sx * sy;

            match td {
                Some(mut data) if uniform && plain && (sx - 1.0).abs() > 1e-4 => {
                    data.style.size *= sx;
                    if let amalith_core::TextKind::Area { width, height } = &mut data.kind {
                        *width *= sx;
                        if let Some(h) = height {
                            *h *= sy;
                        }
                    }
                    data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
                    texts.push((id, data));
                    // Keep the position, drop the scale.
                    xforms.push((id, amalith_core::Affine::translate((e, f))));
                }
                _ => xforms.push((id, convert::affine_to_core(xf))),
            }
        }

        if !texts.is_empty() {
            let _ = self.doc.editor.execute(Command::SetTexts { items: texts });
        }
        if !xforms.is_empty() {
            let _ = self
                .doc.editor
                .execute(Command::SetTransforms { items: xforms });
        }
    }

    /// Eyedropper: copy the appearance of the topmost object under `screen`
    /// (fill, stroke, stroke width) onto the current selection, and adopt
    /// it as the new-object defaults. Nothing under the cursor = no-op.
    fn eyedrop_at(&mut self, screen: Point) {
        let dp = self.doc_point(screen);
        let visible = self.visible_doc_rect();
        let Some(src) =
            select::topmost_selectable_at(self.doc.editor.document(), dp, visible)
        else {
            return;
        };
        let src_obj = self.doc.editor.document().object(src);
        let Some(app) = src_obj.map(|o| o.appearance) else {
            return;
        };
        // A sampled text object also carries its type styling.
        let src_type = src_obj.and_then(|o| match &o.kind {
            amalith_core::ObjectKind::Text(t) => {
                Some((t.style.clone(), t.align, t.paragraph))
            }
            _ => None,
        });
        // Adopt as the tool-palette defaults.
        self.doc.fill = app.fill;
        self.doc.stroke = app.stroke;
        self.doc.stroke_w = app.stroke_width;
        if let Some((style, align, para)) = &src_type {
            self.text_defaults = style.clone();
            self.text_align_default = *align;
            self.para_defaults = *para;
        }
        // Paint it onto everything selected (except the sampled object).
        let targets: Vec<ObjectId> = self
            .doc.selection
            .iter()
            .copied()
            .filter(|id| *id != src)
            .collect();
        if !targets.is_empty() {
            let _ = self.doc.editor.execute(Command::SetPaints {
                objects: targets.clone(),
                fill: Some(app.fill),
                stroke: Some(app.stroke),
            });
            let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                objects: targets,
                width: app.stroke_width,
            });
        }
        // Selected text objects inherit the sampled type styling:
        // character style (family, size, weight, tracking, leading, …),
        // alignment, and paragraph attributes.
        if let Some((style, align, para)) = src_type {
            self.edit_selected_text_data(move |d| {
                d.style = style.clone();
                d.align = align;
                d.paragraph = para;
            });
        }
        self.request_main_redraw();
    }

    /// The head frame of `id`'s text thread (`id` itself if unthreaded or
    /// already the head); `None` if `id` isn't a text object.
    fn thread_head(&self, id: ObjectId) -> Option<ObjectId> {
        crate::thread::head(self.doc.editor.document(), id)
    }

    /// The selected area-text frame whose out-port the pointer is over —
    /// only its tail frame (no `thread_next`) offers a clickable port.
    /// A single selected text object whose right-edge point/area convert
    /// dot the pointer is over. Matches the dot drawn in `canvas.rs`.
    fn text_convert_hit(&self) -> Option<ObjectId> {
        if self.active_tool != Tool::Select || self.doc.selection.len() != 1 {
            return None;
        }
        let id = self.doc.selection[0];
        let obj = self.doc.editor.document().object(id)?;
        if !matches!(obj.kind, amalith_core::ObjectKind::Text(_)) {
            return None;
        }
        let q = select::selection_quad(self.doc.editor.document(), &[id])?;
        let redge = Point::new((q[1].x + q[2].x) * 0.5, (q[1].y + q[2].y) * 0.5);
        let scr = self.doc.view.to_screen() * redge + Vec2::new(16.0, 0.0);
        (scr.distance(self.pointer) <= 8.0).then_some(id)
    }

    /// Toggle a text object between point and area type (double-click the
    /// convert dot). Area → point drops the frame + any thread links;
    /// point → area boxes the text at its current measured size.
    fn toggle_text_kind(&mut self, id: ObjectId) {
        use amalith_core::TextKind;
        let Some(mut td) = self
            .doc
            .editor
            .document()
            .object(id)
            .and_then(|o| match &o.kind {
                amalith_core::ObjectKind::Text(t) => Some(t.clone()),
                _ => None,
            })
        else {
            return;
        };
        match td.kind {
            TextKind::Area { .. } => {
                // Un-thread first so a chain isn't left dangling.
                if td.is_threaded() {
                    self.purge_threads(&[id]);
                    td.thread_prev = None;
                    td.thread_next = None;
                }
                // Bake the frame's soft wraps into hard returns so the
                // point text keeps the same line layout.
                td.content = textedit::hard_wrapped_content(&td, &mut self.text);
                td.kind = TextKind::Point;
            }
            TextKind::Point => {
                let m = textedit::measure_text_data(&td, &mut self.text);
                td.kind = TextKind::Area {
                    width: m.width().max(TEXTBOX_MIN),
                    height: Some(m.height().max(TEXTBOX_MIN)),
                };
            }
        }
        td.local_bounds = textedit::measure_text_data(&td, &mut self.text);
        let _ = self.doc.editor.execute(Command::SetText {
            object: id,
            data: td,
        });
        self.request_main_redraw();
    }

    /// A single selected area-text frame whose bottom-centre auto-fit tab
    /// the pointer is over. Matches the tab drawn in `canvas.rs`.
    fn text_autofit_hit(&self) -> Option<ObjectId> {
        if self.active_tool != Tool::Select || self.doc.selection.len() != 1 {
            return None;
        }
        let id = self.doc.selection[0];
        let obj = self.doc.editor.document().object(id)?;
        let amalith_core::ObjectKind::Text(t) = &obj.kind else {
            return None;
        };
        if !matches!(t.kind, amalith_core::TextKind::Area { .. }) {
            return None;
        }
        let q = select::selection_quad(self.doc.editor.document(), &[id])?;
        let bmid = Point::new((q[2].x + q[3].x) * 0.5, (q[2].y + q[3].y) * 0.5);
        let scr = self.doc.view.to_screen() * bmid + Vec2::new(0.0, 22.0);
        (scr.distance(self.pointer) <= 8.0).then_some(id)
    }

    /// Snap a fixed-height area-text box's height down (or up) to exactly
    /// fit its text — the auto-fit tab's double-click.
    fn fit_text_box_height(&mut self, id: ObjectId) {
        let Some(mut td) = self
            .doc
            .editor
            .document()
            .object(id)
            .and_then(|o| match &o.kind {
                amalith_core::ObjectKind::Text(t) => Some(t.clone()),
                _ => None,
            })
        else {
            return;
        };
        let amalith_core::TextKind::Area { width, .. } = td.kind else {
            return;
        };
        // Measure at the box width with no height constraint.
        let mut probe = td.clone();
        probe.kind = amalith_core::TextKind::Area { width, height: None };
        let content_h = (textedit::td_layout(&mut self.text, &probe).height() as f64)
            .max(TEXTBOX_MIN);
        td.kind = amalith_core::TextKind::Area {
            width,
            height: Some(content_h),
        };
        td.local_bounds = textedit::measure_text_data(&td, &mut self.text);
        let _ = self.doc.editor.execute(Command::SetText {
            object: id,
            data: td,
        });
        self.request_main_redraw();
    }

    fn text_out_port_hit(&self) -> Option<ObjectId> {
        if self.active_tool != Tool::Select || self.doc.selection.len() != 1 {
            return None;
        }
        let id = self.doc.selection[0];
        let obj = self.doc.editor.document().object(id)?;
        let amalith_core::ObjectKind::Text(t) = &obj.kind else {
            return None;
        };
        if !matches!(t.kind, amalith_core::TextKind::Area { .. }) || t.thread_next.is_some() {
            return None;
        }
        let q = select::selection_quad(self.doc.editor.document(), &[id])?;
        // Matches the drawn out-port offset in `canvas.rs` (lifted off the
        // corner scale handle).
        let op = self.doc.view.to_screen() * q[2] + Vec2::new(2.0, -17.0);
        (op.distance(self.pointer) <= 9.0).then_some(id)
    }

    /// Every selected object as `(id, top-left, width, height)` in document
    /// space — but only when *all* of them are axis-aligned area-text
    /// frames. Empty otherwise, so a mixed or rotated selection keeps the
    /// normal `Drag::Scale` behaviour.
    fn area_text_boxes(&self) -> Vec<(ObjectId, Point, f64, f64)> {
        let doc = self.doc.editor.document();
        let mut out = Vec::with_capacity(self.doc.selection.len());
        for &id in &self.doc.selection {
            let Some(obj) = doc.object(id) else {
                return Vec::new();
            };
            let amalith_core::ObjectKind::Text(t) = &obj.kind else {
                return Vec::new();
            };
            let amalith_core::TextKind::Area { width, height } = t.kind else {
                return Vec::new();
            };
            let [a, b, c, d, e, f] = obj.transform.as_coeffs();
            if (a - 1.0).abs() > 1e-9 || b.abs() > 1e-9 || c.abs() > 1e-9 || (d - 1.0).abs() > 1e-9 {
                return Vec::new(); // rotated / scaled — leave it to Drag::Scale
            }
            let h = height.unwrap_or_else(|| t.local_bounds.height());
            out.push((id, Point::new(e, f), width, h));
        }
        out
    }

    /// New frame rect per box for a `ResizeTextBox` drag: a lone box
    /// follows the dragged edge 1:1; several scale proportionally about
    /// the opposite side of their union box.
    fn text_box_resize_rects(
        &self,
        handle: Handle,
        start_bounds: Rect,
        frames: &[(ObjectId, Point, f64, f64)],
        start_doc: Point,
        dp: Point,
    ) -> Vec<(ObjectId, Rect)> {
        if frames.len() == 1 {
            let (id, origin, w, h) = frames[0];
            return vec![(
                id,
                textbox_resized_rect(handle, origin, w, h, start_doc, dp),
            )];
        }
        let m = handles::scaled_transform(
            start_bounds,
            handle,
            dp,
            self.shift_down,
            self.alt_down,
        );
        frames
            .iter()
            .map(|&(id, origin, w, h)| {
                let tl = m * origin;
                let br = m * Point::new(origin.x + w, origin.y + h);
                let r = Rect::new(tl.x.min(br.x), tl.y.min(br.y), tl.x.max(br.x), tl.y.max(br.y));
                let r = Rect::new(
                    r.x0,
                    r.y0,
                    r.x0 + r.width().max(TEXTBOX_MIN),
                    r.y0 + r.height().max(TEXTBOX_MIN),
                );
                (id, r)
            })
            .collect()
    }

    /// Commit a text-box resize: for each `(id, rect)`, swap the object's
    /// `TextKind::Area` dimensions and move its origin onto `rect`. Text
    /// re-wraps; a fixed height clips overflow. One undo step.
    fn resize_text_boxes(&mut self, rects: &[(ObjectId, Rect)]) {
        let mut texts = Vec::new();
        let mut xforms = Vec::new();
        for &(id, rect) in rects {
            let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(id).map(|o| &o.kind)
            else {
                continue;
            };
            let mut data = t.clone();
            data.kind = amalith_core::TextKind::Area {
                width: rect.width(),
                height: Some(rect.height()),
            };
            data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
            texts.push((id, data));
            let cur = self
                .doc.editor
                .document()
                .object(id)
                .map(|o| o.transform.as_coeffs())
                .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
            if (cur[4] - rect.x0).abs() > 1e-6 || (cur[5] - rect.y0).abs() > 1e-6 {
                xforms.push((id, amalith_core::Affine::translate((rect.x0, rect.y0))));
            }
        }
        if !texts.is_empty() {
            let _ = self.doc.editor.execute(Command::SetTexts { items: texts });
        }
        if !xforms.is_empty() {
            let _ = self
                .doc.editor
                .execute(Command::SetTransforms { items: xforms });
        }
        self.request_main_redraw();
    }

    /// True while text is the editing focus — the caret is in a text
    /// object, or the whole selection is text objects. Drives the
    /// options-bar Character cluster.
    fn text_context(&self) -> bool {
        self.text_edit.is_some()
            || (!self.doc.selection.is_empty()
                && self.doc.selection.iter().all(|id| {
                    matches!(
                        self.doc.editor.document().object(*id).map(|o| &o.kind),
                        Some(amalith_core::ObjectKind::Text(_))
                    )
                }))
    }

    /// Options-bar / scroll size step for the Character cluster.
    fn step_font_size(&mut self, delta: f64) {
        let next = (self.active_text_style().size + delta).clamp(1.0, 1296.0);
        self.apply_panel_action(panels::Action::SetFontSize(next), false);
    }

    /// Cheap context-bar Ctx for hover tips — no bounds / xform readout.
    fn context_bar_tip_ctx(&self) -> context_bar::Ctx<'_> {
        context_bar::Ctx {
            theme: &self.theme,
            selection_len: self.doc.selection.len(),
            text_context: self.text_context(),
            representative: None,
            fill_mixed: false,
            stroke_mixed: false,
            active_slot: self.active_slot,
            cur_weight: self.doc.stroke_w,
            cur_opacity: self.doc.opacity,
            stroke_open: self.stroke_popover,
            text_style: amalith_core::TextStyle::default(),
            anchor_sel_len: self.doc.anchor_sel.len(),
            xform: None,
            xform_constrain: self.xform_constrain,
            xform_edit: None,
            pointer: self.pointer,
            align_to: self.align_to,
            align_to_menu: self.align_to_menu.is_some(),
            artboard: self.artboard_bar(),
            artboard_edit: None,
            artboard_link: self.artboard_link,
            artboard_fill_menu: self.artboard_fill_menu,
        }
    }

    /// The selected artboard's data for the options-bar segment, when the
    /// Artboard tool is active.
    fn artboard_bar(&self) -> Option<context_bar::artboard::ArtboardBar> {
        if self.active_tool != Tool::Artboard {
            return None;
        }
        let id = self.doc.selected_artboard?;
        let ab = self.doc.editor.document().artboard(id)?;
        let r = ab.rect;
        Some(context_bar::artboard::ArtboardBar {
            name: ab.name.clone(),
            x: r.x0,
            y: r.y0,
            w: r.x1 - r.x0,
            h: r.y1 - r.y0,
            fill: ab.fill,
            portrait: (r.y1 - r.y0) >= (r.x1 - r.x0),
        })
    }

    /// The read-only slice of state the context bar's segments draw from.
    fn context_bar_ctx(&self) -> context_bar::Ctx<'_> {
        context_bar::Ctx {
            theme: &self.theme,
            selection_len: self.doc.selection.len(),
            text_context: self.text_context(),
            representative: self.representative(),
            fill_mixed: false,
            stroke_mixed: false,
            active_slot: self.active_slot,
            cur_weight: self.doc.stroke_w,
            cur_opacity: self.doc.opacity,
            stroke_open: self.stroke_popover,
            text_style: self.active_text_style(),
            anchor_sel_len: self.doc.anchor_sel.len(),
            xform: selection_xform(self.doc.editor.document(), &self.doc.selection, self.xform_ref),
            xform_constrain: self.xform_constrain,
            xform_edit: self
                .xform_edit
                .as_ref()
                .map(|(f, s, _)| (*f, s.as_str())),
            pointer: self.pointer,
            align_to: self.align_to,
            align_to_menu: self.align_to_menu.is_some(),
            artboard: self.artboard_bar(),
            artboard_edit: self.artboard_edit.as_ref().map(|(f, s, _)| (*f, s.as_str())),
            artboard_link: self.artboard_link,
            artboard_fill_menu: self.artboard_fill_menu,
        }
    }

    /// Whether the caret is in its visible blink phase.
    fn text_blink_on(&self) -> bool {
        self.text_blink.elapsed().as_millis() % 1060 < 530
    }

    /// Screen (logical) point → the open editor's local space.
    fn text_editor_point(&self, screen: Point) -> Option<(f32, f32)> {
        let obj = self.text_edit.as_ref()?.object;
        let world = self.doc.editor.document().world_transform(obj);
        let xf = self.doc.view.to_screen() * convert::affine(world);
        let p = xf.inverse() * screen;
        Some((p.x as f32, p.y as f32))
    }

    /// Create a text object of `kind` anchored at `origin` (document space)
    /// and open it for editing.
    fn create_text(&mut self, kind: amalith_core::TextKind, origin: Point) {
        let layer = self.ensure_layer();
        // Seed with placeholder text, selected on open (see `enter_text_edit`),
        // so the first keystroke replaces it — and so a click-away leaves a
        // visible object behind instead of nothing.
        let placeholder = match kind {
            amalith_core::TextKind::Area { .. } => TEXT_PLACEHOLDER_PARAGRAPH,
            amalith_core::TextKind::Point => TEXT_PLACEHOLDER,
        };
        let mut data = amalith_core::TextData {
            content: placeholder.to_string(),
            kind,
            style: self.text_defaults.clone(),
            align: self.text_align_default,
            paragraph: self.para_defaults,
            local_bounds: amalith_core::Rect::ZERO,
            thread_next: None,
            thread_prev: None,
        };
        // Give the created state real bounds so it's selectable / hit-
        // testable even before the first commit — undoing back to it (⌘Z
        // right after typing) must not leave a zero-size, unclickable box.
        data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
        let cmd = Command::CreateText {
            layer,
            data,
            transform: amalith_core::Affine::translate((origin.x, origin.y)),
            name: None,
        };
        if let Ok(CommandOutcome::Object(id)) = self.doc.editor.execute(cmd) {
            self.doc.selection = vec![id];
            self.enter_text_edit(id, origin, None);
        }
    }

    /// A blank fixed-size area-text frame (no placeholder, not opened for
    /// editing) — the receiving end of a text thread.
    fn create_empty_area_text(&mut self, w: f64, h: f64, origin: Point) -> Option<ObjectId> {
        let layer = self.ensure_layer();
        let mut data = amalith_core::TextData {
            content: String::new(),
            kind: amalith_core::TextKind::Area {
                width: w.max(TEXTBOX_MIN),
                height: Some(h.max(TEXTBOX_MIN)),
            },
            style: self.text_defaults.clone(),
            align: self.text_align_default,
            paragraph: self.para_defaults,
            local_bounds: amalith_core::Rect::ZERO,
            thread_next: None,
            thread_prev: None,
        };
        data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
        match self.doc.editor.execute(Command::CreateText {
            layer,
            data,
            transform: amalith_core::Affine::translate((origin.x, origin.y)),
            name: None,
        }) {
            Ok(CommandOutcome::Object(id)) => Some(id),
            _ => None,
        }
    }

    /// Splice out any threaded text frames among `ids` before they're
    /// deleted, so their neighbours stay linked and no dangling
    /// `thread_next` / `thread_prev` is left behind.
    fn purge_threads(&mut self, ids: &[ObjectId]) {
        let threaded: Vec<ObjectId> = ids
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Text(t)) if t.is_threaded()
                )
            })
            .collect();
        for id in threaded {
            let _ = self
                .doc.editor
                .execute(Command::UnthreadText { object: id });
        }
    }

    /// Link `to` after `from` in a text thread and select the pair.
    fn thread_text(&mut self, from: ObjectId, to: ObjectId) {
        if from == to {
            return;
        }
        // Refuse a cycle: `to` must not already be upstream of `from`.
        if crate::thread::chain(self.doc.editor.document(), to).contains(&from) {
            return;
        }
        let _ = self
            .doc.editor
            .execute(Command::ThreadText { from, to });
        self.doc.selection = vec![to];
        self.text_load = None;
        self.update_canvas_cursor();
        self.request_main_redraw();
    }

    /// Type ▸ Create Outlines (⌘⇧O): replace each selected text object with
    /// an editable path of its glyph contours, keeping the object's place
    /// and fill. Multi-step undo for now — no batch command yet.
    fn create_outlines(&mut self) {
        if self.text_edit.is_some() {
            self.commit_text_edit();
        }
        let targets: Vec<ObjectId> = self
            .doc.selection
            .iter()
            .copied()
            .filter(|id| {
                matches!(
                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Text(_))
                )
            })
            .collect();
        if targets.is_empty() {
            return;
        }
        let mut new_ids = Vec::new();
        for tid in targets {
            let Some((td, transform, appearance, name, layer)) =
                self.doc.editor.document().object(tid).and_then(|o| {
                    let amalith_core::ObjectKind::Text(td) = &o.kind else {
                        return None;
                    };
                    let layer = self.owning_layer(tid)?;
                    Some((td.clone(), o.transform, o.appearance, o.name.clone(), layer))
                })
            else {
                continue;
            };
            let geometry = textedit::outline_text_data(&td, &mut self.text);
            if geometry.elements().is_empty() {
                continue;
            }
            let Ok(CommandOutcome::Object(pid)) = self.doc.editor.execute(Command::CreatePath {
                layer,
                path: amalith_core::PathData::from_bezpath(geometry),
                name,
            }) else {
                continue;
            };
            let _ = self.doc.editor.execute(Command::SetTransform {
                object: pid,
                transform,
            });
            // Carry the text's own paint — don't inherit CreatePath's
            // visible-stroke default.
            let _ = self.doc.editor.execute(Command::SetFill {
                objects: vec![pid],
                paint: appearance.fill,
            });
            let _ = self.doc.editor.execute(Command::SetStroke {
                objects: vec![pid],
                paint: appearance.stroke,
            });
            if appearance.stroke != amalith_core::Paint::None {
                let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                    objects: vec![pid],
                    width: appearance.stroke_width,
                });
                let _ = self.doc.editor.execute(Command::SetStrokeStyle {
                    objects: vec![pid],
                    style: appearance.stroke_style,
                });
            }
            let _ = self.doc.editor.execute(Command::DeleteObject { id: tid });
            new_ids.push(pid);
        }
        if !new_ids.is_empty() {
            self.doc.selection = new_ids;
            self.doc.anchor_sel.clear();
            self.request_main_redraw();
        }
    }

    /// The layer that ultimately owns `id`, walking out through any groups.
    fn owning_layer(&self, mut id: ObjectId) -> Option<LayerId> {
        loop {
            match self.doc.editor.document().object(id)?.parent {
                amalith_core::ObjectParent::Layer(l) => return Some(l),
                amalith_core::ObjectParent::Group(g) => id = g,
            }
        }
    }

    /// Open text object `id` for editing. `click` (screen point) places the
    /// caret; `None` selects all (fresh object).
    fn enter_text_edit(&mut self, mut id: ObjectId, mut origin: Point, click: Option<Point>) {
        // A threaded frame edits the whole story — retarget to the head
        // frame, which owns the text.
        if let Some(head) = self.thread_head(id) {
            if head != id {
                id = head;
                let c = self
                    .doc.editor
                    .document()
                    .object(id)
                    .map(|o| o.transform.as_coeffs())
                    .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                origin = Point::new(c[4], c[5]);
            }
        }
        let Some(td) = self
            .doc.editor
            .document()
            .object(id)
            .and_then(|o| match &o.kind {
                amalith_core::ObjectKind::Text(t) => Some(t.clone()),
                _ => None,
            })
        else {
            return;
        };
        let mut te = textedit::TextEdit::new(
            id,
            amalith_core::Point::new(origin.x, origin.y),
            td.kind,
            td.style,
            td.align,
            td.paragraph,
            &td.content,
            &mut self.text,
        );
        te.set_thread(td.thread_prev, td.thread_next);
        match click {
            Some(p) => {
                let xf =
                    self.doc.view.to_screen() * convert::affine(self.doc.editor.document().world_transform(id));
                let lp = xf.inverse() * p;
                te.pointer_down((lp.x as f32, lp.y as f32), 1, &mut self.text);
            }
            None => te.select_all(&mut self.text),
        }
        self.text_edit = Some(te);
        self.active_tool = Tool::Text;
        self.text_blink = Instant::now();
        if let Some(w) = self.main_window() {
            w.set_ime_allowed(true);
        }
        self.request_main_redraw();
    }

    /// Write the open text edit back to the document (an empty object is
    /// discarded — a placeholder left untouched still has its text) and
    /// leave edit mode.
    fn commit_text_edit(&mut self) {
        let Some(mut te) = self.text_edit.take() else {
            return;
        };
        if te.is_empty() {
            let _ = self.doc.editor.execute(Command::DeleteObject { id: te.object });
            self.doc.selection.retain(|s| *s != te.object);
        } else {
            let data = te.to_text_data(&mut self.text);
            let _ = self.doc.editor.execute(Command::SetText {
                object: te.object,
                data,
            });
        }
        if let Some(w) = self.main_window() {
            w.set_ime_allowed(false);
        }
        self.request_main_redraw();
    }

    /// Route one key event to the open text editor.
    fn text_edit_key(&mut self, event: &winit::event::KeyEvent) -> textedit::KeyResult {
        if !event.state.is_pressed() {
            return textedit::KeyResult::Handled;
        }
        let mods = textedit::Mods {
            shift: self.shift_down,
            alt: self.alt_down,
            meta: self.cmd_down,
        };
        self.text_blink = Instant::now();
        let text = event.text.as_ref().map(|s| s.as_str());
        let Some(te) = &mut self.text_edit else {
            return textedit::KeyResult::PassThrough;
        };
        let r = te.key(&event.logical_key, mods, text, &mut self.text);
        self.request_main_redraw();
        r
    }

    /// ⌘Z with the Pen tool: step back one anchor. While still drawing this
    /// just pops the local anchor; after a commit it deletes the object and
    /// re-commits it one anchor shorter (so the shape shrinks a point at a
    /// time). Returns true if it handled the keystroke.
    fn pen_undo_step(&mut self) -> bool {
        if self.active_tool != Tool::Pen {
            return false;
        }
        if let Some(p) = self.pen.pop() {
            self.pen_redo.push(p);
            self.request_main_redraw();
            return true;
        }
        if let Some((id, mut anchors, closed)) = self.last_pen.take() {
            let _ = self.doc.editor.execute(Command::DeleteObject { id });
            anchors.pop();
            if anchors.len() >= 2 {
                self.pen = anchors;
                self.commit_pen(closed && self.pen.len() >= 3);
            } else {
                self.doc.selection.clear();
            }
            self.request_main_redraw();
            return true;
        }
        false
    }

    fn select_all(&mut self) {
        // ⌘A while editing text selects the text, not every object. This
        // also catches the macOS native "Select All" menu accelerator,
        // which never reaches `text_edit_key`.
        if self.text_edit.is_some() {
            if let Some(te) = self.text_edit.as_mut() {
                te.select_all(&mut self.text);
            }
            self.request_main_redraw();
            return;
        }
        let sel: Vec<ObjectId> = {
            let doc = self.doc.editor.document();
            doc.layers()
                .iter()
                .filter(|l| l.visible)
                .flat_map(|l| l.children.iter().copied())
                .filter(|id| doc.object(*id).is_some_and(|o| o.visible && !o.locked))
                .collect()
        };
        self.doc.selection = sel;
        self.sync_align_mode();
        self.request_main_redraw();
    }

    /// Every selectable top-level object, in stacking order (back → front).
    fn selectable_objects(&self) -> Vec<ObjectId> {
        let doc = self.doc.editor.document();
        doc.layers()
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| l.children.iter().copied())
            .filter(|id| doc.object(*id).is_some_and(|o| o.visible && !o.locked))
            .collect()
    }

    /// Select ▸ Deselect.
    fn deselect(&mut self) {
        self.doc.selection.clear();
        self.doc.anchor_sel.clear();
        self.selected_guides.clear();
        self.key_object = None;
        self.sync_align_mode();
        self.request_main_redraw();
    }

    /// Select ▸ All on Active Artboard.
    fn select_all_artboard(&mut self) {
        if self.text_edit.is_some() {
            self.select_all();
            return;
        }
        let Some(rect) = self
            .current_artboard()
            .and_then(|id| self.doc.editor.document().artboard(id).map(|a| a.rect))
        else {
            self.select_all();
            return;
        };
        let overlap = |a: amalith_core::Rect, b: amalith_core::Rect| {
            a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
        };
        let doc = self.doc.editor.document();
        let sel: Vec<ObjectId> = self
            .selectable_objects()
            .into_iter()
            .filter(|id| doc.bounds_of(*id).is_some_and(|b| overlap(b, rect)))
            .collect();
        self.doc.selection = sel;
        self.sync_align_mode();
        self.request_main_redraw();
    }

    /// Select ▸ Next Object Above / Below — step the selection one place
    /// through the stacking order (`dir` +1 = above, −1 = below).
    fn select_next_z(&mut self, dir: i32) {
        let flat = self.selectable_objects();
        if flat.is_empty() {
            return;
        }
        let n = flat.len() as i32;
        let cur = self
            .doc
            .selection
            .first()
            .and_then(|s| flat.iter().position(|x| x == s));
        let next = match cur {
            Some(i) => (i as i32 + dir).rem_euclid(n) as usize,
            None if dir > 0 => 0,
            None => flat.len() - 1,
        };
        self.doc.selection = vec![flat[next]];
        self.sync_align_mode();
        self.request_main_redraw();
    }

    /// Select ▸ Same ▸ … — select every object sharing the given
    /// attribute with the first currently-selected object.
    fn select_same(&mut self, kind: SameKind) {
        let Some(&r0) = self.doc.selection.first() else {
            return;
        };
        let doc = self.doc.editor.document();
        let Some(ref_obj) = doc.object(r0) else { return };
        let a0 = ref_obj.appearance;
        let t0 = match &ref_obj.kind {
            amalith_core::ObjectKind::Text(t) => Some(t.style.clone()),
            _ => None,
        };
        let hit = |o: &amalith_core::Object| -> bool {
            let same_text_style = |f: &dyn Fn(&amalith_core::TextStyle, &amalith_core::TextStyle) -> bool| {
                match (&o.kind, &t0) {
                    (amalith_core::ObjectKind::Text(t), Some(s0)) => f(&t.style, s0),
                    _ => false,
                }
            };
            match kind {
                SameKind::FillColor => o.appearance.fill == a0.fill,
                SameKind::StrokeColor => o.appearance.stroke == a0.stroke,
                SameKind::StrokeWeight => {
                    (o.appearance.stroke_width - a0.stroke_width).abs() < 1e-6
                }
                SameKind::Opacity => (o.appearance.opacity - a0.opacity).abs() < 1e-4,
                SameKind::FillStroke => {
                    o.appearance.fill == a0.fill && o.appearance.stroke == a0.stroke
                }
                SameKind::FontFamily => same_text_style(&|a, b| a.family == b.family),
                SameKind::FontSize => {
                    same_text_style(&|a, b| (a.size - b.size).abs() < 1e-6)
                }
            }
        };
        let sel: Vec<ObjectId> = self
            .selectable_objects()
            .into_iter()
            .filter(|id| doc.object(*id).is_some_and(hit))
            .collect();
        if !sel.is_empty() {
            self.doc.selection = sel;
            self.sync_align_mode();
            self.request_main_redraw();
        }
    }

    /// `(fill_mixed, stroke_mixed)` — true when the object selection holds
    /// more than one distinct value for that paint. The Fill / Stroke
    /// proxies show a grey "?" swatch instead of a colour when so.
    fn selection_paint_mixed(&self) -> (bool, bool) {
        let doc = self.doc.editor.document();
        let mut paints = self
            .doc
            .selection
            .iter()
            .filter_map(|id| doc.object(*id))
            .map(|o| (o.appearance.fill, o.appearance.stroke));
        let Some((f0, s0)) = paints.next() else {
            return (false, false);
        };
        let (mut fm, mut sm) = (false, false);
        for (f, s) in paints {
            fm |= f != f0;
            sm |= s != s0;
        }
        (fm, sm)
    }

    /// The OS clipboard handle, created on first use (some platforms only
    /// hand one out once the app is fully up).
    fn clipboard(&mut self) -> Option<&mut arboard::Clipboard> {
        if self.clipboard.is_none() {
            self.clipboard = arboard::Clipboard::new().ok();
        }
        self.clipboard.as_mut()
    }

    /// Copy `ids` into the `Editor`'s own clipboard *and* mirror them to
    /// the OS clipboard as a portable `<svg>` document, so the same copy
    /// can be pasted into Illustrator, a browser, Figma, etc.
    fn copy_selection(&mut self, ids: &[ObjectId]) {
        let _ = self.doc.editor.copy(ids);
        self.doc.paste_nudge = 0;
        // Remember which artboard the copy sits in, for artboard-relative
        // Paste in Front / Back.
        self.doc.clip_artboard = self.doc.editor.clipboard_bounds().and_then(|b| {
            let c = b.center();
            artboard_at(self.doc.editor.document(), Point::new(c.x, c.y))
        });
        let svg = amalith_io::export_svg(self.doc.editor.document(), ids);
        self.doc.last_svg = svg.clone();
        if let (Some(svg), Some(cb)) = (svg, self.clipboard()) {
            let _ = cb.set_text(svg);
        }
    }

    /// The artboard a paste should target: the last one clicked inside,
    /// else the one the Artboard tool has selected.
    fn current_artboard(&self) -> Option<ArtboardId> {
        self.doc.current_artboard.or(self.doc.selected_artboard)
    }

    /// If the OS clipboard holds SVG text (Illustrator with "SVG Code"
    /// clipboard handling, an SVG copied from a browser, our own last
    /// copy), load it into the `Editor` clipboard. Non-SVG text is left
    /// alone, so an unrelated clipboard doesn't clobber the last in-app
    /// copy.
    fn pull_svg_from_clipboard(&mut self) {
        let Some(text) = self.clipboard().and_then(|cb| cb.get_text().ok()) else {
            return;
        };
        let head = text.trim_start();
        if head.starts_with("<svg") || head.starts_with("<?xml") {
            // Our own last copy is already in the Editor clipboard (with a
            // known source artboard) — don't round-trip it back in.
            if self.doc.last_svg.as_deref() == Some(text.as_str()) {
                return;
            }
            let _ = self.doc.editor.copy_from_svg(&text);
            // Externally-sourced content has no source artboard.
            self.doc.clip_artboard = None;
        }
    }

    /// Paste the clipboard. `place` decides where it lands: `Plain` drops
    /// the copied bounds' centre on the visible view (Illustrator's
    /// "somewhere new" paste); `InFront` / `Behind` reproduce the copy's
    /// offset within its source artboard inside the current artboard
    /// (falling back to exact coordinates when either artboard is unknown).
    fn paste_clipboard(&mut self, place: PastePlace) {
        self.pull_svg_from_clipboard();
        if !self.doc.editor.has_clipboard() {
            return;
        }
        // Delta between the source artboard and the current one, for the
        // in-front / behind pastes.
        let artboard_delta = {
            let doc = self.doc.editor.document();
            let src = self
                .doc
                .clip_artboard
                .and_then(|id| doc.artboard(id))
                .map(|a| a.rect.origin());
            let dst = self
                .current_artboard()
                .and_then(|id| doc.artboard(id))
                .map(|a| a.rect.origin());
            match (src, dst) {
                (Some(s), Some(d)) => amalith_core::Vec2::new(d.x - s.x, d.y - s.y),
                _ => amalith_core::Vec2::ZERO,
            }
        };
        let (delta, stack) = match place {
            PastePlace::Plain => {
                let vc = self.visible_doc_rect().center();
                let mut delta = self
                    .doc
                    .editor
                    .clipboard_bounds()
                    .map(|b| {
                        let bc = b.center();
                        amalith_core::Vec2::new(vc.x - bc.x, vc.y - bc.y)
                    })
                    .unwrap_or(amalith_core::Vec2::ZERO);
                // Images are placed on the view centre, so a centred paste
                // lands on the original. Nudge each ⌘V further.
                self.doc.paste_nudge = self.doc.paste_nudge.saturating_add(1);
                let step = 16.0 * self.doc.paste_nudge as f64;
                delta.x += step;
                delta.y += step;
                (delta, PasteStack::Top)
            }
            PastePlace::InFront => (artboard_delta, PasteStack::InFront),
            PastePlace::Behind => (artboard_delta, PasteStack::Behind),
        };
        if let Ok(ids) = self.doc.editor.paste(delta, stack) {
            self.doc.selection = ids;
        }
        self.request_main_redraw();
    }

    /// The layer new shapes should land in — the topmost, creating one if
    /// the document has none.
    fn ensure_layer(&mut self) -> LayerId {
        if let Some(l) = self.doc.editor.document().layers().last() {
            return l.id;
        }
        match self.doc.editor.execute(Command::CreateLayer {
            name: "Layer 1".into(),
            index: None,
        }) {
            Ok(CommandOutcome::Layer(id)) => id,
            _ => LayerId::new(),
        }
    }

    /// The document-space rect currently visible in the canvas (between the
    /// rails). Used to cull hit-testing to what the user can see.
    /// Logical x-range of the canvas viewport — window edges minus any
    /// docked rails.
    fn canvas_x_span(&self) -> (f64, f64) {
        let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let left = if self.dock.left.is_empty() {
            0.0
        } else {
            rail_rect_for(RailSide::Left, self.dock.left.width as f64, w, h).x1
        };
        let right = if self.dock.right.is_empty() {
            w
        } else {
            rail_rect_for(RailSide::Right, self.dock.right.width as f64, w, h).x0
        };
        (left, right.max(left))
    }

    fn visible_doc_rect(&self) -> Rect {
        let (_, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let (left, right) = self.canvas_x_span();
        self.doc.view
            .to_screen()
            .inverse()
            .transform_rect_bbox(Rect::new(left, CHROME_TOP, right, h))
    }

    /// The full canvas region between the rails, below the chrome —
    /// before any ruler inset. The ruler strips live in its top / left.
    fn canvas_region(&self) -> Rect {
        let (_, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let (left, right) = self.canvas_x_span();
        Rect::new(left, CHROME_TOP, right, h)
    }

    /// The canvas viewport in screen (logical) px — `canvas_region` inset
    /// by the ruler strips when they're on.
    fn canvas_viewport(&self) -> Rect {
        let r = self.canvas_region();
        let i = if self.rulers { rulers::THICK } else { 0.0 };
        Rect::new(r.x0 + i, r.y0 + i, r.x1, r.y1)
    }

    /// Fit the view so the document's artboards are centred in the canvas.
    fn fit_view(&mut self) {
        let boards = self.doc.editor.document().artboards();
        let Some(first) = boards.first() else { return };
        let mut b = first.rect;
        for a in &boards[1..] {
            b = b.union(a.rect);
        }
        self.fit_view_to(b);
        self.pending_fit = false;
    }

    /// Centre the view on `b` (document space) and zoom so it fills most
    /// of the canvas. Shared by "fit all artboards" and the Artboards
    /// panel's double-click-the-number "snap this one back into view".
    fn fit_view_to(&mut self, b: amalith_core::geom::Rect) {
        let vp = self.canvas_viewport();
        if vp.width() < 10.0 || vp.height() < 10.0 || b.width() < 1.0 || b.height() < 1.0 {
            return;
        }
        let zoom = ((vp.width() / b.width()).min(vp.height() / b.height()) * 0.88)
            .clamp(canvas::ZOOM_MIN, 8.0);
        let (bc, vc) = (b.center(), vp.center());
        self.doc.view.zoom = zoom;
        self.doc.view.pan = Vec2::new(vc.x - zoom * bc.x, vc.y - zoom * bc.y);
        self.request_main_redraw();
    }

    /// Multiply the zoom by `factor`, keeping the canvas centre fixed
    /// (⌘+ / ⌘− and the Zoom-tool click).
    fn zoom_step(&mut self, factor: f64) {
        self.doc.view.zoom_at(factor, self.canvas_viewport().center());
        self.request_main_redraw();
    }

    /// ⌘0 — fit the artboard in play, else every artboard.
    fn zoom_fit(&mut self) {
        let rect = self
            .current_artboard()
            .and_then(|id| self.doc.editor.document().artboard(id).map(|a| a.rect));
        match rect {
            Some(r) => self.fit_view_to(r),
            None => self.fit_view(),
        }
    }

    /// ⌘1 — 100%, keeping the canvas centre fixed.
    fn zoom_actual(&mut self) {
        let z = self.doc.view.zoom;
        if (z - 1.0).abs() > 1e-6 {
            self.doc.view.zoom_at(1.0 / z, self.canvas_viewport().center());
        }
        self.request_main_redraw();
    }

    /// A minimal [`panels::Ctx`] for hit-only / tip-only queries.
    fn tip_ctx(&self) -> panels::Ctx<'_> {
        panels::Ctx {
            theme: &self.theme,
            doc: self.doc.editor.document(),
            selection: &self.doc.selection,
            active_tool: self.active_tool,
            pointer: self.pointer,
            representative: None,
            fill_mixed: false,
            stroke_mixed: false,
            active_slot: self.active_slot,
            picker: self.picker,
            cur_fill: self.doc.fill,
            cur_stroke: self.doc.stroke,
            shape_tool: self.last_shape_tool,
            expanded: &self.doc.expanded_groups,
            renaming: None,
            selected_layer: self.doc.selected_layer,
            selected_artboard: self.doc.selected_artboard,
            text_style: amalith_core::TextStyle::default(),
            text_align: amalith_core::TextAlign::Start,
            text_paragraph: amalith_core::Paragraph::default(),
            text_editing: false,
            font_families: &self.font_families,
            layer_query: &self.layer_query,
            layer_search_focused: self.layer_search_focused,
            layer_scroll: self.panel_scroll_of(PanelId("layers")),
            layer_drop: None,
            color_mode: self.color_mode,
            recent: &self.recent_colors,
            xform_ref: self.xform_ref,
            xform_constrain: self.xform_constrain,
            xform_edit: self
                .xform_edit
                .as_ref()
                .map(|(f, s, _)| (*f, s.as_str())),
            align_to: self.align_to,
            align_spacing: self.align_spacing,
            align_spacing_edit: self.align_spacing_edit.as_ref().map(|(s, _)| s.as_str()),
            key_object: self.key_object,
            shape_dialog: None,
            export: None,
        }
    }

    /// Text for a hover tooltip over the pointer's current position, if it's
    /// resting on a labelled panel control or a tab close button.
    fn hover_tooltip(&mut self) -> Option<String> {
        if !self.settings.show_tooltips
            || !matches!(self.drag, Drag::None)
            || self.font_menu.is_some()
            || self.align_to_menu.is_some()
            || self.ruler_menu.is_some()
            || self.ctx_menu.is_some()
            || self.prefs.is_some()
            || self.palette.is_some()
        {
            return None;
        }
        if self.pointer_win == self.main_id
            && self.pointer.y >= APP_BAR_H
            && self.pointer.y < APP_BAR_H + OPT_BAR_H
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            // Tip layout only needs selection_len / text_context / pointer —
            // skip selection_xform (recursive group bounds on every move).
            let cx = self.context_bar_tip_ctx();
            return context_bar::tip(opt_bar_rect(w), self.pointer, &cx);
        }
        let areas: Vec<layout::PanelArea> = if self.pointer_win == self.main_id {
            [RailSide::Left, RailSide::Right]
                .iter()
                .flat_map(|&side| {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        return Vec::new();
                    }
                    let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    build_rail_layout(rail, &self.theme, &mut self.text, rect).areas
                })
                .collect()
        } else if let Some(fid) = self.pointer_win.and_then(|wid| {
            self.hosts.get(&wid).and_then(|h| match h.role {
                Role::Floating(f) => Some(f),
                _ => None,
            })
        }) {
            self.floating_layout(fid).areas
        } else {
            return None;
        };

        for area in &areas {
            if area.tab_strip.contains(self.pointer) {
                if let Some(t) = area.tabs.iter().find(|t| t.rect.contains(self.pointer)) {
                    if chrome::panel_tab_close_rect(t.rect).contains(self.pointer) {
                        return Some("Close panel".into());
                    }
                    return Some(tab_label(t.panel));
                }
                return None;
            }
            if area.body.contains(self.pointer) {
                if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                    let pbody = panels::scrolled_body(pid, area.body, self.panel_scroll_of(pid)).0;
                    let ctx = self.tip_ctx();
                    return panels::tip(pid, pbody, self.pointer, &ctx);
                }
            }
        }
        None
    }

    /// Recompute the hover tooltip after a pointer move.
    fn refresh_tooltip(&mut self) {
        let new = self.hover_tooltip();
        match (&self.tooltip, &new) {
            (Some(t), Some(n)) if &t.text == n => {}
            (_, Some(n)) => {
                self.tooltip = Some(Tooltip {
                    text: n.clone(),
                    anchor: self.pointer,
                    since: Instant::now(),
                    shown: false,
                });
                self.request_main_redraw();
            }
            (Some(_), None) => {
                self.tooltip = None;
                self.request_main_redraw();
            }
            (None, None) => {}
        }
    }

    /// The × on a panel tab. In a rail: remove the panel. In a floating
    /// window: close that window (drop its panels — no redock).
    fn close_panel_tab(&mut self, pid: PanelId, floating: Option<u64>) {
        if pid.0 == "picker" {
            self.picker = None;
        }
        if panels::shape_dialog_tool(pid).is_some() {
            self.shape_dialog = None;
        }
        if pid == export::EXPORT_PID {
            self.export = None;
        }
        if self.panel_menu.as_ref().is_some_and(|m| m.panel == pid) {
            self.panel_menu = None;
        }
        if let Some(fid) = floating {
            let wid = self
                .hosts
                .iter()
                .find(|(_, h)| matches!(h.role, Role::Floating(f) if f == fid))
                .map(|(k, _)| *k);
            if let Some(wid) = wid {
                self.hosts.remove(&wid);
                self.focused.remove(&wid);
            }
            self.dock.remove_floating(fid);
        } else {
            self.dock.remove(pid);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
        self.request_main_redraw();
    }

    /// Window menu: show the panel (docked into the right rail) if hidden,
    /// or remove it if shown.
    fn toggle_panel(&mut self, id: &str) {
        let pid = PanelId(match WINDOW_PANELS.iter().find(|(p, _)| *p == id) {
            Some((p, _)) => *p,
            None => return,
        });
        if self.dock.contains(pid) {
            self.dock.remove(pid);
        } else {
            let path = self.dock.right.any_tab_path().unwrap_or_default();
            self.dock.rail_mut(RailSide::Right).dock(
                pid,
                DropTarget::Tab {
                    path,
                    index: usize::MAX,
                },
            );
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
        self.request_main_redraw();
    }

    /// Snap the view onto one artboard (Artboards panel, double-click its
    /// number).
    fn focus_artboard(&mut self, id: ArtboardId) {
        if let Some(ab) = self
            .doc.editor
            .document()
            .artboards()
            .iter()
            .find(|a| a.id == id)
        {
            let rect = ab.rect;
            self.fit_view_to(rect);
        }
    }

    /// Recompute the pointer style and, on change, tell the OS.
    fn update_canvas_cursor(&mut self) {
        let over_stroke_flyout = self.stroke_popover && {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            self.stroke_flyout_layout(w).panel.contains(self.pointer)
        };
        let over = self.pointer_win == self.main_id
            && (self.picker.is_none() || self.dock.contains(PanelId("picker")))
            && self.newdoc.is_none()
            && self.about.is_none()
            && self.home.is_none()
            && self.prefs.is_none()
            && self.font_menu.is_none()
            && self.panel_menu.is_none()
            && self.ruler_menu.is_none()
            && self.ctx_menu.is_none()
            && self.palette.is_none()
            && !over_stroke_flyout
            && self.canvas_viewport().contains(self.pointer);
        let mode = if !over {
            CanvasCursor::Default
        } else if matches!(self.drag, Drag::ScrubZoom { .. })
            || (self.space_down && self.cmd_down)
        {
            // Mid-scrub the +/− follows drag direction (set in the drag
            // handler); otherwise it follows Alt.
            if !matches!(self.drag, Drag::ScrubZoom { .. }) {
                self.zoom_sign = if self.alt_down { -1 } else { 1 };
            }
            CanvasCursor::Zoom
        } else if (self.space_down && !matches!(self.drag, Drag::PenHandle { .. }))
            || matches!(self.drag, Drag::Pan { .. })
        {
            if matches!(self.drag, Drag::Pan { .. }) {
                CanvasCursor::Grabbing
            } else {
                CanvasCursor::Grab
            }
        } else {
            match self.effective_tool() {
                Tool::Text => CanvasCursor::IBeam,
                Tool::Select | Tool::DirectSelect | Tool::Pen => CanvasCursor::Glyph,
                Tool::Hand => CanvasCursor::Grab,
                Tool::Zoom => {
                    self.zoom_sign = if self.alt_down { -1 } else { 1 };
                    CanvasCursor::Zoom
                }
                _ => CanvasCursor::Crosshair,
            }
        };
        // Hovering a transform grip / rotation halo wins over the tool's
        // default cursor — unless the pointer is over an area-text
        // auto-fit tab, which sits in the rotation-halo zone below the box.
        let over_text_widget =
            self.text_autofit_hit().is_some() || self.text_convert_hit().is_some();
        let mode = if over
            && matches!(self.drag, Drag::None)
            && !self.space_down
            && !over_text_widget
        {
            self.handle_hover_cursor().unwrap_or(mode)
        } else if self.text_autofit_hit().is_some() {
            CanvasCursor::FitUp
        } else {
            mode
        };
        // Text threading: a loaded out-port, or hovering one.
        let mode = if self.text_load.is_some() {
            if over {
                CanvasCursor::LoadedText
            } else {
                mode
            }
        } else if over
            && matches!(self.drag, Drag::None)
            && self.text_out_port_hit().is_some()
        {
            CanvasCursor::ThreadPort
        } else {
            mode
        };
        // Mid-rotation with the Rotate tool: the curved-arrow cursor.
        let mode = if matches!(self.drag, Drag::RotateTool { moved: true, .. }) {
            CanvasCursor::Rotate(2)
        } else {
            mode
        };
        if mode != self.cursor_mode {
            self.cursor_mode = mode;
            if let Some(w) = self.main_window() {
                use winit::window::CursorIcon;
                let drawn = mode.is_drawn();
                w.set_cursor_visible(!drawn);
                w.set_cursor(match mode {
                    CanvasCursor::Crosshair => CursorIcon::Crosshair,
                    CanvasCursor::Grab => CursorIcon::Grab,
                    CanvasCursor::Grabbing => CursorIcon::Grabbing,
                    CanvasCursor::IBeam => CursorIcon::Text,
                    _ => CursorIcon::Default,
                });
            }
            self.request_main_redraw();
        }
    }

    /// The Rotate tool's reference point (document space): the custom
    /// `transform_pivot` if placed, else the selection's bbox centre.
    /// `None` when nothing is selected.
    fn rotate_pivot(&self) -> Option<Point> {
        let c = select::union_bounds(self.doc.editor.document(), &self.doc.selection)?.center();
        Some(self.transform_pivot.unwrap_or(c))
    }

    /// If the pointer is over a transform grip (Selection or Artboard
    /// tool) or a rotation halo, the cursor that fits.
    fn handle_hover_cursor(&self) -> Option<CanvasCursor> {
        let to_screen = self.doc.view.to_screen();
        let scale_for = |h: handles::Handle| match h {
            handles::Handle::Nw | handles::Handle::Se => CanvasCursor::ScaleNWSE,
            handles::Handle::Ne | handles::Handle::Sw => CanvasCursor::ScaleNESW,
            handles::Handle::N | handles::Handle::S => CanvasCursor::ScaleNS,
            handles::Handle::E | handles::Handle::W => CanvasCursor::ScaleEW,
        };
        match self.effective_tool() {
            Tool::Select if !self.doc.selection.is_empty() => {
                let quad =
                    select::selection_quad(self.doc.editor.document(), &self.doc.selection)?;
                let scr = quad.map(|p| to_screen * p);
                if let Some(h) = handles::hit_handle(self.pointer, scr) {
                    Some(scale_for(h))
                } else {
                    handles::rotate_halo_handle(self.pointer, scr)
                        .map(|c| CanvasCursor::Rotate(c as u8))
                }
            }
            Tool::Artboard => {
                let ab = self
                    .doc
                    .editor
                    .document()
                    .artboard(self.doc.selected_artboard?)?;
                let scr = handles::rect_quad(convert::rect(ab.rect)).map(|p| to_screen * p);
                handles::hit_handle(self.pointer, scr).map(scale_for)
            }
            _ => None,
        }
    }

    /// Logical size of the main window's client area.
    fn main_logical_size(&self) -> Option<(f64, f64)> {
        let w = self.main_window()?;
        let sz = w.inner_size();
        Some((sz.width as f64 / self.scale, sz.height as f64 / self.scale))
    }

    /// Resolve where the cursor (in global logical coords) would re-dock:
    /// which rail, and where within it. Checks the left rail/edge, then the
    /// right.
    fn resolve_redock(&mut self, global_cursor: Point) -> (RailSide, DropTarget) {
        let Some((w, h)) = self.main_logical_size() else {
            return (RailSide::Right, DropTarget::Float);
        };
        let local = global_cursor - self.main_inner_origin().to_vec2();
        if !Rect::new(0.0, 0.0, w, h).contains(local) {
            return (RailSide::Right, DropTarget::Float);
        }
        for side in [RailSide::Left, RailSide::Right] {
            let rail = self.dock.rail(side);
            let rect = rail_rect_for(side, rail.width as f64, w, h);
            if rail.is_empty() {
                let in_zone = match side {
                    RailSide::Left => local.x <= EMPTY_ZONE,
                    RailSide::Right => local.x >= w - EMPTY_ZONE,
                };
                if in_zone {
                    let edge = match side {
                        RailSide::Left => Side::Left,
                        RailSide::Right => Side::Right,
                    };
                    return (
                        side,
                        DropTarget::Split {
                            path: NodePath(Vec::new()),
                            side: edge,
                        },
                    );
                }
            } else if rect.contains(local) {
                let laid = build_rail_layout(rail, &self.theme, &mut self.text, rect);
                return (side, layout::hit_test(&laid, rect, local, &self.theme));
            }
        }
        (RailSide::Right, DropTarget::Float)
    }

    fn make_host(&mut self, window: Arc<Window>, role: Role) -> WindowHost {
        let size = window.inner_size();
        let surface = pollster::block_on(self.context.create_surface(
            window.clone(),
            size.width.max(1),
            size.height.max(1),
            wgpu::PresentMode::AutoVsync,
        ))
        .expect("create surface");
        let renderer = Renderer::new(
            &self.context.devices[surface.dev_id].device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: NonZeroUsize::new(1),
                pipeline_cache: None,
            },
        )
        .expect("create renderer");
        WindowHost {
            surface,
            renderer,
            window,
            role,
        }
    }

    /// The on-screen size of `panel` while it's docked in a rail, if it is.
    fn panel_dock_size(&mut self, panel: PanelId) -> Option<(f64, f64)> {
        let (w, h) = self.main_logical_size()?;
        for side in [RailSide::Left, RailSide::Right] {
            let rail = self.dock.rail(side);
            if rail.is_empty() {
                continue;
            }
            let rect = rail_rect_for(side, rail.width as f64, w, h);
            let laid = build_rail_layout(rail, &self.theme, &mut self.text, rect);
            if let Some(area) = laid
                .areas
                .iter()
                .find(|a| a.tabs.iter().any(|t| t.panel == panel))
            {
                return Some((area.bounds.width(), area.bounds.height()));
            }
        }
        None
    }

    /// Tear `panel` out of the main rail into a new borderless window that
    /// starts under the cursor, and begin moving it. The window keeps the
    /// size the panel had while docked.
    fn tear_off(&mut self, event_loop: &ActiveEventLoop, panel: PanelId, main_local_press: Point) {
        let global = self.main_inner_origin() + main_local_press.to_vec2();
        let (fw, fh) = if panel.0 == "picker" {
            (picker::W, picker::H + self.theme.tab_strip_h)
        } else {
            self.panel_dock_size(panel)
                .map(|(w, h)| (w.max(RAIL_MIN_W), h.clamp(160.0, 1200.0)))
                .unwrap_or((FLOAT_W, FLOAT_H))
        };
        // Keep the cursor grip inside the (possibly narrow) torn-off window.
        let grab = Vec2::new(TEAROFF_GRAB.x.min(fw - 12.0).max(12.0), TEAROFF_GRAB.y);
        let pos = global - grab;
        let id = match self
            .dock
            .detach(panel, [pos.x as f32, pos.y as f32, fw as f32, fh as f32])
        {
            Some(id) => id,
            None => return,
        };

        let attrs = Window::default_attributes()
            .with_title(tab_label(panel))
            .with_decorations(false)
            .with_resizable(panel.0 != "picker")
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create float window"),
        );
        let wid = window.id();
        let host = self.make_host(window.clone(), Role::Floating(id));
        self.hosts.insert(wid, host);

        self.drag = Drag::MovingFloating { id, grab, pos };
        window.request_redraw();
        self.request_main_redraw();
    }

    /// Open the colour picker as its own non-resizable floating window,
    /// centred over the main window. Dragging the tab strip moves it
    /// anywhere on the desktop — same path as a torn-off panel.
    fn spawn_picker_window(&mut self, event_loop: &ActiveEventLoop) {
        let pid = PanelId("picker");
        let (mw, mh) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let fw = picker::W;
        let fh = picker::H + self.theme.tab_strip_h;
        let origin = self.main_inner_origin();
        let pos = Point::new(
            origin.x + ((mw - fw) * 0.5).max(4.0),
            origin.y + ((mh - fh) * 0.5).max(4.0),
        );
        let rect = [pos.x as f32, pos.y as f32, fw as f32, fh as f32];

        if let Some(fid) = self.dock.floating_id_of(pid) {
            if let Some(f) = self.dock.floating_mut(fid) {
                f.rect = rect;
            }
            if let Some(w) = self.floating_window(fid) {
                w.set_outer_position(LogicalPosition::new(pos.x, pos.y));
                w.request_redraw();
            }
            return;
        }

        let id = self.dock.float_alone(pid, rect);
        if self.floating_window(id).is_some() {
            if let Some(w) = self.floating_window(id) {
                w.set_outer_position(LogicalPosition::new(pos.x, pos.y));
                w.request_redraw();
            }
            return;
        }

        let attrs = Window::default_attributes()
            .with_title(tab_label(pid))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create picker window"),
        );
        let wid = window.id();
        let host = self.make_host(window.clone(), Role::Floating(id));
        self.hosts.insert(wid, host);
        window.request_redraw();
        self.request_main_redraw();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
    }

    /// Dismiss the "About Amalith" panel.
    fn close_about(&mut self) {
        if self.about.take().is_some() {
            self.request_main_redraw();
        }
    }

    fn floating_layout(&mut self, id: u64) -> Layout {
        let Some(f) = self.dock.floating(id) else {
            return Layout::default();
        };
        let node = f.node.clone();
        let sz = self
            .floating_window(id)
            .map(|w| w.inner_size())
            .unwrap_or_default();
        let (wl, hl) = (
            (sz.width as f64 / self.scale).max(1.0),
            (sz.height as f64 / self.scale).max(1.0),
        );
        let theme = self.theme.clone();
        layout::layout(
            &node,
            Rect::new(0.0, 0.0, wl, hl),
            &theme,
            &mut |p| {
                self.text.measure(&tab_label(p), 12.0)
                    + theme.tab_pad_x * chrome::PANEL_TAB_PAD_MUL * 2.0
                    + chrome::PANEL_TAB_CLOSE_W
            },
            &panels::has_menu,
        )
    }

}

impl ApplicationHandler for App {
    /// Fires once per loop iteration, right before the loop would sleep.
    /// Does the housekeeping that has no event behind it (native-menu
    /// clicks, macOS drops, finished image decodes, view-fit), then sets
    /// `ControlFlow`: `Wait` when the app is idle, `WaitUntil` when
    /// something is mid-animation, `Poll` only until the first frame lands.
    /// Rendering itself is on demand — see [`App::request_main_redraw`].
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let actions = self
                .native_menu
                .as_ref()
                .map(NativeMenu::drain)
                .unwrap_or_default();
            for action in actions {
                if matches!(action, MenuAction::Quit) {
                    // `exiting` saves the layout on the way out.
                    event_loop.exit();
                    continue;
                }
                self.run_menu_action(action);
            }
            // Refresh selection-dependent menu items before the loop
            // parks — the user's next click on the menu bar sees them.
            self.sync_type_menu();
        }
        #[cfg(target_os = "macos")]
        self.drain_mac_drops();
        self.drain_lod();
        if std::mem::take(&mut self.pending_export) {
            self.spawn_export_dialog(event_loop);
        }
        if self.pending_fit {
            self.fit_view();
        }
        // The Home screen is a fixed card — lock the main window's size
        // while it's up, restore resizing once a document is open.
        let want_resizable = self.home.is_none();
        if want_resizable != self.main_resizable {
            self.main_resizable = want_resizable;
            if let Some(h) = self.main_id.and_then(|id| self.hosts.get(&id)) {
                h.window.set_resizable(want_resizable);
            }
        }
        // Modal / picker open+close changes whether the OS cursor hides.
        self.update_canvas_cursor();
        // A held Shape-slot press opens the primitive flyout.
        if let Some((t, anchor)) = self.shape_press {
            if self.shape_flyout.is_none() && t.elapsed().as_millis() >= 300 {
                self.shape_flyout = Some(anchor);
                self.shape_press = None;
                self.request_main_redraw();
            }
        }

        // --- Frame scheduling --------------------------------------------
        //
        // Until the first frame has presented, keep pumping: a dropped
        // initial `RedrawRequested` otherwise leaves the window blank.
        if !self.first_frame_done {
            for host in self.hosts.values() {
                host.window.request_redraw();
            }
            event_loop.set_control_flow(ControlFlow::Poll);
            return;
        }

        // Soonest moment we need to wake for an animation, if any. `merge`
        // keeps the nearest deadline.
        let mut wake: Option<Duration> = None;
        fn merge(cur: Option<Duration>, d: Duration) -> Option<Duration> {
            Some(cur.map_or(d, |c| c.min(d)))
        }

        // Caret blink while a text object holds the caret. Toggles every
        // 530ms; ask for a frame only when the phase actually flips, then
        // sleep until the next flip.
        if self.text_edit.is_some() || self.shape_dialog.is_some() || self.export.is_some() {
            if self.text_blink_on() != self.last_caret_drawn {
                self.request_main_redraw();
            }
            let phase = self.text_blink.elapsed().as_millis() % 1060;
            let to_flip = if phase < 530 { 530 - phase } else { 1060 - phase };
            wake = merge(wake, Duration::from_millis(to_flip as u64 + 8));
        }

        // Hover tooltip: revealed 350ms after it is set, with no event in
        // between. Draw that one reveal frame, then leave it be.
        let reveal = Duration::from_millis(350);
        let reveal_now = matches!(&self.tooltip, Some(tt)
            if !tt.shown && self.pointer_win.is_some() && tt.since.elapsed() >= reveal);
        if reveal_now {
            if let Some(tt) = &mut self.tooltip {
                tt.shown = true;
            }
            self.request_main_redraw();
        } else if let Some(tt) = &self.tooltip {
            if !tt.shown && self.pointer_win.is_some() {
                wake = merge(wake, reveal.saturating_sub(tt.since.elapsed()) + Duration::from_millis(8));
            }
        }

        // A held Shape-slot press opens its flyout after 300ms (handled
        // above); wake in time to notice.
        if let Some((t, _)) = self.shape_press {
            if self.shape_flyout.is_none() {
                wake = merge(
                    wake,
                    Duration::from_millis(300).saturating_sub(t.elapsed()) + Duration::from_millis(8),
                );
            }
        }

        // A finished background image decode has no wake channel of its
        // own — poll while jobs are outstanding, then fall back to sleep.
        if !self.lod_inflight.is_empty() {
            wake = merge(wake, Duration::from_millis(30));
        }

        event_loop.set_control_flow(match wake {
            Some(d) => ControlFlow::WaitUntil(Instant::now() + d),
            None => ControlFlow::Wait,
        });
    }

    /// Fires once as the event loop is about to terminate (main window
    /// closed, ⌘Q / Exit, ⌘W of the last tab). Save the dock layout so
    /// the next launch comes back the way it was left.
    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.save_layout();
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main_id.is_some() {
            return;
        }
        self.apply_theme_accent();
        if self.font_families.is_empty() {
            let (fc, _) = self.text.parts();
            let mut names: Vec<String> =
                fc.collection.family_names().map(str::to_string).collect();
            names.sort_by_key(|s| s.to_lowercase());
            names.dedup();
            self.font_families = names;
        }
        let attrs = Window::default_attributes()
            .with_title("Amalith Ver. Alpha")
            .with_window_icon(appicon::window_icon())
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            // Paint behind the title bar; keep the traffic lights, drop the
            // title text.
            attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true)
        };
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        // No `.app` bundle yet, so set the Dock / Cmd-Tab icon at runtime.
        appicon::set_dock_icon();
        self.scale = window.scale_factor();
        let wid = window.id();
        let host = self.make_host(window, Role::Main);
        self.hosts.insert(wid, host);
        self.main_id = Some(wid);
        #[cfg(target_os = "macos")]
        crate::macdrop::install(&self.hosts[&wid].window);
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let m = NativeMenu::build(
                &self.hosts[&wid].window,
                &self.scripts,
                self.guides_hidden,
                self.guides_locked,
                self.outline_mode,
            );
            m.sync_window(&self.dock);
            self.native_menu = Some(m);
        }
        self.hosts[&wid].window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::Focused(focused) => {
                let was_active = !self.focused.is_empty();
                if focused {
                    self.focused.insert(id);
                } else {
                    self.focused.remove(&id);
                }
                let now_active = !self.focused.is_empty();
                if now_active != was_active {
                    // Panels ride above the main window only while Amalith
                    // is the frontmost app.
                    let level = if now_active {
                        winit::window::WindowLevel::AlwaysOnTop
                    } else {
                        winit::window::WindowLevel::Normal
                    };
                    for host in self.hosts.values() {
                        if matches!(host.role, Role::Floating(_)) {
                            host.window.set_window_level(level);
                        }
                    }
                }
            }
            WindowEvent::CloseRequested => {
                self.focused.remove(&id);
                if Some(id) == self.main_id {
                    event_loop.exit();
                } else if let Some(host) = self.hosts.remove(&id) {
                    if let Role::Floating(fid) = host.role {
                        // Closing a floating window folds its panels back
                        // into the right rail so nothing is lost.
                        let path = self.dock.right.any_tab_path().unwrap_or_default();
                        self.dock.redock(
                            fid,
                            RailSide::Right,
                            DropTarget::Tab {
                                path,
                                index: usize::MAX,
                            },
                        );
                    }
                    self.request_main_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                let role = self.hosts.get(&id).map(|h| h.role);
                if let Some(host) = self.hosts.get_mut(&id) {
                    self.context.resize_surface(
                        &mut host.surface,
                        size.width.max(1),
                        size.height.max(1),
                    );
                    host.window.request_redraw();
                }
                // Track a floating group's size so it re-docks at the
                // width the user left it.
                if let Some(Role::Floating(fid)) = role {
                    let (w, h) = (
                        size.width as f32 / self.scale as f32,
                        size.height as f32 / self.scale as f32,
                    );
                    if let Some(f) = self.dock.floating_mut(fid) {
                        f.rect[2] = w;
                        f.rect[3] = h;
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if Some(id) == self.main_id {
                    self.scale = scale_factor;
                }
                if let Some(host) = self.hosts.get(&id) {
                    host.window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = Point::new(position.x / self.scale, position.y / self.scale);
                self.pointer_win = Some(id);
                self.on_cursor_move();
                if let Some((panel, press)) = self.pending_tearoff.take() {
                    self.tear_off(event_loop, panel, press);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if Some(id) == self.pointer_win {
                    self.pointer_win = None;
                }
                self.update_canvas_cursor();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                let now = Instant::now();
                let near = self.last_click.is_some_and(|(t, p)| {
                    now.duration_since(t).as_millis() < 400 && (self.pointer - p).hypot() < 5.0
                });
                self.click_streak = if near { self.click_streak + 1 } else { 1 };
                self.last_click = Some((now, self.pointer));
                self.on_press(event_loop, id, self.click_streak >= 2);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.on_release();
                if let Some((tool, anchor)) = self.pending_shape_dialog.take() {
                    self.spawn_shape_dialog(event_loop, tool, anchor);
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } if Some(id) == self.main_id => self.on_right_press(),
            WindowEvent::Ime(ime) if Some(id) == self.main_id => {
                if let Some(te) = &mut self.text_edit {
                    te.ime(&ime, &mut self.text);
                    self.text_blink = Instant::now();
                    self.request_main_redraw();
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                let was_direct = self.effective_tool() == Tool::DirectSelect;
                // Keep the modifier flags live for the free-floating dialog
                // windows too (Shift-Tab in a shape dialog, etc.).
                if Some(id) == self.main_id
                    || self.shape_dialog.is_some()
                    || self.export.is_some()
                    || self.picker.is_some()
                {
                    self.cmd_down = m.state().super_key();
                    self.shift_down = m.state().shift_key();
                    self.alt_down = m.state().alt_key();
                }
                if Some(id) == self.main_id {
                    // Toggling the temporary white-arrow gesture shows or
                    // hides the node overlay; a live drag reacts to
                    // Shift-lock / Alt-copy changing under it.
                    let now_direct = self.effective_tool() == Tool::DirectSelect;
                    if !now_direct && was_direct && matches!(self.drag, Drag::None) {
                        // The ⌘ gesture ended: drop the node selection so
                        // the plain arrow's bounding box comes back.
                        self.doc.anchor_sel.clear();
                    }
                    // A pen handle mid-pull re-snaps to / releases the
                    // 45° lock the instant Shift changes, without needing
                    // a cursor nudge.
                    if matches!(self.drag, Drag::PenHandle { .. }) {
                        self.drag_pen_handle();
                    }
                    // Zoom tool / ⌘Space: Alt flips the magnifier + ↔ −.
                    if self.effective_tool() == Tool::Zoom
                        || (self.space_down && self.cmd_down)
                    {
                        self.update_canvas_cursor();
                        self.request_main_redraw();
                    }
                    if now_direct != was_direct || !matches!(self.drag, Drag::None) {
                        self.request_main_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. }
                if Some(id) == self.main_id
                    || self.picker.is_some()
                    || self.shape_dialog.is_some()
                    || self.export.is_some() =>
            {
                self.on_key(event);
            }
            WindowEvent::PinchGesture { delta, .. } if Some(id) == self.main_id => {
                self.on_pinch(delta);
            }
            WindowEvent::MouseWheel { delta, .. } if Some(id) == self.main_id => {
                self.on_wheel(delta);
            }
            WindowEvent::RedrawRequested => self.redraw(id),
            WindowEvent::DroppedFile(path) if Some(id) == self.main_id => {
                self.on_drop_file(path);
            }
            _ => {}
        }
    }
}

/// Right rail: Color|Transform|Pathfinder|Align on top (Swatches starts
/// closed), Character in the middle, Layers|Artboards at the bottom.
fn demo_right_dock() -> Node {
    Node::Split {
        axis: Axis::Vertical,
        children: vec![
            Child {
                node: Node::Tabs {
                    panels: vec![
                        PanelId("color"),
                        PanelId("transform"),
                        PanelId("pathfinder"),
                        PanelId("align"),
                    ],
                    active: 0,
                },
                weight: 1.5,
            },
            Child {
                node: Node::Tabs {
                    panels: vec![PanelId("character"), PanelId("paragraph")],
                    active: 0,
                },
                weight: 1.1,
            },
            Child {
                node: Node::Tabs {
                    panels: vec![PanelId("layers"), PanelId("artboards")],
                    active: 0,
                },
                weight: 1.6,
            },
        ],
    }
}

/// Left rail: the Tools panel, like amalith-app.
fn demo_left_dock() -> Node {
    Node::Tabs {
        panels: vec![PanelId("tools")],
        active: 0,
    }
}

/// Snap `p` to a 45°-stepped direction from `prev` when `snap` (Shift).
fn constrained(prev: Option<Point>, p: Point, snap: bool) -> Point {
    let Some(prev) = prev.filter(|_| snap) else {
        return p;
    };
    let d = p - prev;
    let len = d.hypot();
    if len == 0.0 {
        return p;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let a = (d.y.atan2(d.x) / step).round() * step;
    Point::new(prev.x + len * a.cos(), prev.y + len * a.sin())
}

/// Normalized rect between two document-space points; `square` locks it
/// to the larger dimension. Returns core kurbo for the create commands.
/// `PathData` for the primitive shape tools, from a document-space box.
fn primitive_path(tool: Tool, r: amalith_core::Rect) -> Option<amalith_core::PathData> {
    use amalith_core::{PathData, Point as CP};
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let c = r.center();
    let (rx, ry) = (r.width() * 0.5, r.height() * 0.5);
    match tool {
        Tool::RoundedRect => Some(PathData::rounded_rectangle(
            r,
            r.width().min(r.height()) * 0.18,
        )),
        Tool::Polygon => {
            let pts: Vec<CP> = (0..6)
                .map(|i| {
                    let a = -FRAC_PI_2 + i as f64 * TAU / 6.0;
                    CP::new(c.x + rx * a.cos(), c.y + ry * a.sin())
                })
                .collect();
            Some(PathData::polygon(&pts))
        }
        Tool::Star => {
            let pts: Vec<CP> = (0..10)
                .map(|i| {
                    let a = -FRAC_PI_2 + i as f64 * PI / 5.0;
                    let k = if i % 2 == 0 { 1.0 } else { 0.45 };
                    CP::new(c.x + rx * k * a.cos(), c.y + ry * k * a.sin())
                })
                .collect();
            Some(PathData::polygon(&pts))
        }
        _ => None,
    }
}

/// `start` with the edge(s) belonging to `h` shifted by `d`, normalised
/// and kept at least 8 units on a side.
fn resize_rect(start: amalith_core::Rect, h: Handle, d: amalith_core::Vec2) -> amalith_core::Rect {
    let (mut x0, mut y0, mut x1, mut y1) = (start.x0, start.y0, start.x1, start.y1);
    if matches!(h, Handle::Nw | Handle::W | Handle::Sw) {
        x0 += d.x;
    }
    if matches!(h, Handle::Ne | Handle::E | Handle::Se) {
        x1 += d.x;
    }
    if matches!(h, Handle::Nw | Handle::N | Handle::Ne) {
        y0 += d.y;
    }
    if matches!(h, Handle::Sw | Handle::S | Handle::Se) {
        y1 += d.y;
    }
    let n = amalith_core::Rect::new(x0, y0, x1, y1).abs();
    amalith_core::Rect::new(
        n.x0,
        n.y0,
        n.x0 + n.width().max(8.0),
        n.y0 + n.height().max(8.0),
    )
}

/// Topmost artboard whose rect contains the document-space point `dp`.
fn artboard_at(doc: &Document, dp: Point) -> Option<ArtboardId> {
    let p = amalith_core::Point::new(dp.x, dp.y);
    doc.artboards()
        .iter()
        .rev()
        .find(|ab| ab.rect.contains(p))
        .map(|ab| ab.id)
}

/// Rect of primitive flyout cell `i`, a horizontal row right of `anchor`.
fn shape_flyout_cell(anchor: Rect, i: usize) -> Rect {
    let sz = 34.0;
    let x = anchor.x1 + 8.0 + i as f64 * sz;
    Rect::new(x, anchor.y0, x + sz, anchor.y0 + sz)
}

/// Fractional (0..1) hotspot of a tool's cursor glyph within its box —
/// where the actual click point sits.
fn cursor_hotspot(t: Tool) -> (f64, f64) {
    match t {
        Tool::Select | Tool::DirectSelect => (0.32, 0.13),
        Tool::Pen => (0.29, 0.08),
        _ => (0.5, 0.5),
    }
}

fn selection_xform(
    doc: &Document,
    selection: &[ObjectId],
    rp: amalith_core::RefPoint,
) -> Option<amalith_core::TransformValues> {
    let id = *selection.first()?;
    let b = doc.local_bounds_of(id)?;
    Some(amalith_core::xform::values(doc.world_transform(id), b, rp))
}

/// Snap a delta to the nearest of the 8 cardinal / diagonal directions,
/// keeping only the component along that axis (Shift-lock for drags).
fn snap8(d: Vec2) -> Vec2 {
    if d.hypot() < 1e-6 {
        return d;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (d.y.atan2(d.x) / step).round() * step;
    let dir = Vec2::new(angle.cos(), angle.sin());
    dir * d.dot(dir)
}

/// Box between two document-space points. `square` (Shift) locks it to
/// the larger dimension; `from_center` (Alt) treats `a` as the centre.
fn shape_rect(a: Point, b: Point, square: bool, from_center: bool) -> amalith_core::Rect {
    let (mut ex, mut ey) = (b.x, b.y);
    if square {
        let s = (b.x - a.x).abs().max((b.y - a.y).abs());
        ex = a.x + s.copysign(b.x - a.x);
        ey = a.y + s.copysign(b.y - a.y);
    }
    if from_center {
        let (hx, hy) = ((ex - a.x).abs(), (ey - a.y).abs());
        amalith_core::Rect::new(a.x - hx, a.y - hy, a.x + hx, a.y + hy)
    } else {
        amalith_core::Rect::new(a.x.min(ex), a.y.min(ey), a.x.max(ex), a.y.max(ey))
    }
}

/// Smallest an area-text box may be dragged in document px.
const TEXTBOX_MIN: f64 = 8.0;

/// The new frame rect for a text-box handle drag: `handle` moves the
/// edge(s) it touches by the pointer delta `(dp - start_doc)`; opposite
/// edges stay put. Result is normalised and clamped to [`TEXTBOX_MIN`].
fn textbox_resized_rect(
    handle: Handle,
    origin: Point,
    w: f64,
    h: f64,
    start_doc: Point,
    dp: Point,
) -> Rect {
    let (dx, dy) = (dp.x - start_doc.x, dp.y - start_doc.y);
    let (mut x0, mut y0) = (origin.x, origin.y);
    let (mut x1, mut y1) = (origin.x + w, origin.y + h);
    if matches!(handle, Handle::W | Handle::Nw | Handle::Sw) {
        x0 += dx;
    }
    if matches!(handle, Handle::E | Handle::Ne | Handle::Se) {
        x1 += dx;
    }
    if matches!(handle, Handle::N | Handle::Nw | Handle::Ne) {
        y0 += dy;
    }
    if matches!(handle, Handle::S | Handle::Sw | Handle::Se) {
        y1 += dy;
    }
    if x1 - x0 < TEXTBOX_MIN {
        if matches!(handle, Handle::W | Handle::Nw | Handle::Sw) {
            x0 = x1 - TEXTBOX_MIN;
        } else {
            x1 = x0 + TEXTBOX_MIN;
        }
    }
    if y1 - y0 < TEXTBOX_MIN {
        if matches!(handle, Handle::N | Handle::Nw | Handle::Ne) {
            y0 = y1 - TEXTBOX_MIN;
        } else {
            y1 = y0 + TEXTBOX_MIN;
        }
    }
    Rect::new(x0, y0, x1, y1)
}

fn tab_label(panel: PanelId) -> String {
    match panel.0 {
        "tools" => "Tools",
        "layers" => "Layers",
        "artboards" => "Artboards",
        "swatches" => "Swatches",
        "character" => "Character",
        "paragraph" => "Paragraph",
        "color" => "Color",
        "transform" => "Transform",
        "pathfinder" => "Pathfinder",
        "align" => "Align",
        "picker" => "Color Picker",
        "shapedlg.rect" => "Rectangle",
        "shapedlg.round" => "Rounded Rectangle",
        "shapedlg.ellipse" => "Ellipse",
        "shapedlg.polygon" => "Polygon",
        "shapedlg.star" => "Star",
        "export-screens" => "Export for Screens",
        other => other,
    }
    .to_string()
}

fn rail_rect_for(side: RailSide, rail_w: f64, width: f64, height: f64) -> Rect {
    let rw = rail_w.clamp(RAIL_MIN_W, (width * 0.7).max(RAIL_MIN_W));
    // Rails sit below the full-width app bar + options bar.
    let top = APP_BAR_H + OPT_BAR_H;
    match side {
        RailSide::Left => Rect::new(0.0, top, rw, height),
        RailSide::Right => Rect::new(width - rw, top, width, height),
    }
}

/// The draggable bar on a rail's canvas-facing edge.
fn rail_edge_bar(side: RailSide, rect: Rect) -> Rect {
    match side {
        RailSide::Left => Rect::new(rect.x1 - RAIL_EDGE, rect.y0, rect.x1, rect.y1),
        RailSide::Right => Rect::new(rect.x0, rect.y0, rect.x0 + RAIL_EDGE, rect.y1),
    }
}

fn build_rail_layout(rail: &Rail, theme: &Theme, text: &mut TextContext, rect: Rect) -> Layout {
    match &rail.tree {
        Some(tree) => layout::layout(
            tree,
            rect,
            theme,
            &mut |p| {
                text.measure(&tab_label(p), 12.0)
                    + theme.tab_pad_x * chrome::PANEL_TAB_PAD_MUL * 2.0
                    + chrome::PANEL_TAB_CLOSE_W
            },
            &panels::has_menu,
        ),
        None => Layout::default(),
    }
}

/// The options / context bar: full window width, directly under the app
/// bar and above both the tools rail and the tab strip.
fn opt_bar_rect(width: f64) -> Rect {
    Rect::new(0.0, APP_BAR_H, width, APP_BAR_H + OPT_BAR_H)
}

/// The document-tab strip: the canvas x-span, between the options bar and
/// the canvas.
fn tab_bar_rect(left_x: f64, right_x: f64) -> Rect {
    Rect::new(
        left_x,
        APP_BAR_H + OPT_BAR_H,
        right_x.max(left_x),
        APP_BAR_H + OPT_BAR_H + TAB_BAR_H,
    )
}

/// Lay `labels` out as document tabs across `strip`, left to right.
/// Returns `(whole tab rect, close-× rect)` per tab.
fn layout_tabs(text: &mut TextContext, labels: &[String], strip: Rect) -> Vec<(Rect, Rect)> {
    let mut out = Vec::with_capacity(labels.len());
    let mut x = strip.x0 + 4.0;
    for label in labels {
        let tw = text.measure(label, 12.6);
        let w = tw + 18.9 /* × */ + 23.1 /* padding */;
        let whole = Rect::new(x, strip.y0, (x + w).min(strip.x1), strip.y1);
        let close = Rect::new(
            whole.x0 + 6.0,
            strip.y0 + 4.0,
            whole.x0 + 20.0,
            strip.y1 - 4.0,
        );
        out.push((whole, close));
        x += w + 2.0;
        if x >= strip.x1 {
            break;
        }
    }
    out
}

/// The panels the Panels menu lists, alphabetical like Illustrator.
const WINDOW_PANELS: [(&str, &str); 10] = [
    ("align", "Align"),
    ("artboards", "Artboards"),
    ("character", "Character"),
    ("color", "Color"),
    ("layers", "Layers"),
    ("paragraph", "Paragraph"),
    ("pathfinder", "Pathfinder"),
    ("swatches", "Swatches"),
    ("tools", "Tools"),
    ("transform", "Transform"),
];

/// Start the shell: create the winit event loop and run [`App`] on it.
pub fn run() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
