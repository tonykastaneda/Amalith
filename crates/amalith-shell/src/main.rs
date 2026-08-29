//! Amalith shell — entry point.
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

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use amalith_commands::{Command, CommandOutcome, Editor, PasteStack};
use amalith_core::{ArtboardId, Document, LayerId, ObjectId};
use amalith_shell::anchors;
use amalith_shell::canvas::{self, AnchorView, CanvasView, DragPreview, PenPreview};
use amalith_shell::dock::{
    Axis, Child, DockModel, DropTarget, Node, NodePath, PanelId, Rail, RailSide, Side,
};
use amalith_shell::handles::{self, Handle};
use amalith_shell::layout::Layout;
use amalith_shell::newdoc;
use amalith_shell::text::TextContext;
use amalith_shell::tool::Tool;
use amalith_shell::{chrome, convert, icons, layout, panels, picker, sample, select, Theme};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke, Vec2};
use vello::peniko::{color::palette, Color, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

/// Height of the top app bar, logical points.
const APP_BAR_H: f64 = 30.0;
/// The document-tab strip, between the app bar and the options bar.
const TAB_BAR_H: f64 = 28.0;
/// The tool options strip, between the app bar and the canvas.
const OPT_BAR_H: f64 = 30.0;
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

#[derive(Clone, Copy)]
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
    /// Panning the canvas; `last` is the previous cursor position.
    Pan { last: Point },
    /// Moving the current selection. Deltas are in document space. Alt
    /// (held at any point) duplicates; Shift locks to 8 directions —
    /// both read live at release / in the preview.
    MoveObjects {
        start_doc: Point,
        last_doc: Point,
        moved: bool,
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
    /// Rubber-banding a new shape with the Rectangle / Ellipse tool.
    DrawShape {
        tool: Tool,
        start_doc: Point,
        cur_doc: Point,
    },
    /// Dragging inside the colour picker (`in_hue` = the hue strip).
    PickColor { in_hue: bool },
    /// Direct Selection: dragging the selected path anchors.
    MoveAnchors {
        start_doc: Point,
        last_doc: Point,
        moved: bool,
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

/// A command reachable from the menu bar (native on macOS, and — later —
/// an in-window bar elsewhere). Keyboard shortcuts still handle these
/// directly; this is the same set routed through one dispatcher.
#[derive(Clone, Copy, Debug)]
enum MenuAction {
    New,
    Open,
    Save,
    SaveAs,
    ImportSvg,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Duplicate,
    SelectAll,
    BringForward,
    BringToFront,
    SendBackward,
    SendToBack,
}

/// An inline panel rename: what's being renamed, the edit buffer, and
/// whether the buffer is still the untouched original (so the first
/// keystroke replaces it, like Illustrator's select-all-on-focus).
struct Rename {
    target: panels::RenameId,
    buf: String,
    fresh: bool,
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
    rename: Option<Rename>,
    stroke_w: f64,
    opacity: f32,
    view: CanvasView,
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
            rename: None,
            stroke_w: 1.0,
            opacity: 1.0,
            view: CanvasView::default(),
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
}

struct App {
    context: RenderContext,
    hosts: HashMap<WindowId, WindowHost>,
    main_id: Option<WindowId>,
    scene: Scene,
    /// Chrome is drawn here in logical units, then appended to `scene`
    /// scaled by the DPI factor.
    content: Scene,
    text: TextContext,
    dock: DockModel,
    /// Parked state for every open document. `tabs[active]` is a
    /// placeholder while that document is the live one on `App`.
    tabs: Vec<Doc>,
    active: usize,
    editor: Editor,
    /// Path this document was last opened from / saved to, for ⌘S.
    file_path: Option<std::path::PathBuf>,
    /// Embedded assets that travel with the `.amalith` container.
    asset_store: amalith_io::AssetStore,
    /// Last file error, shown in the app bar until the next action.
    io_error: Option<String>,
    selection: Vec<ObjectId>,
    /// Selected path anchors, for the Direct Selection tool.
    anchor_sel: Vec<(ObjectId, usize)>,
    /// Group ids shown expanded in the Layers panel.
    expanded_groups: std::collections::HashSet<ObjectId>,
    /// The artboard the Artboard tool is currently editing (shows handles).
    selected_artboard: Option<ArtboardId>,
    /// The layer highlighted in the Layers panel.
    selected_layer: Option<LayerId>,
    /// An in-progress inline rename in a panel.
    rename: Option<Rename>,
    /// The New Document modal, when open.
    newdoc: Option<newdoc::NewDocForm>,
    active_tool: Tool,
    /// The tool that was active when the Artboard tool was entered, so
    /// Escape can drop straight back to it.
    pre_artboard_tool: Tool,
    /// Which paint slot the Swatches panel targets.
    active_slot: panels::PaintSlot,
    /// Current stroke weight / opacity shown in the options bar. The
    /// steppers edit these; new shapes and any selection pick them up.
    stroke_w: f64,
    opacity: f32,
    /// Open colour picker, if any.
    picker: Option<picker::Picker>,
    /// Time + position of the last left press, for double-click detection.
    last_click: Option<(Instant, Point)>,
    /// In-progress Pen path — placed anchors in document space. Empty when
    /// not drawing.
    pen: Vec<Point>,
    /// Anchors popped by ⌘Z while drawing, for ⌘⇧Z to restore.
    pen_redo: Vec<Point>,
    /// The path just committed by the Pen, so ⌘Z can re-open it a point
    /// shorter instead of undoing the whole object. Cleared by any other
    /// action.
    last_pen: Option<(ObjectId, Vec<Point>, bool)>,
    /// Rubber-band rect (screen px) while a marquee drag is live.
    marquee: Option<Rect>,
    view: CanvasView,
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
    /// What the pointer looks like right now (see [`CanvasCursor`]).
    cursor_mode: CanvasCursor,
    /// The macOS application menu bar, once the app has resumed.
    #[cfg(target_os = "macos")]
    native_menu: Option<NativeMenu>,
}

impl App {
    fn new() -> Self {
        Self {
            context: RenderContext::new(),
            hosts: HashMap::new(),
            main_id: None,
            scene: Scene::new(),
            content: Scene::new(),
            text: TextContext::new(),
            dock: {
                let mut d = DockModel::new(demo_right_dock());
                d.left = Rail::with(demo_left_dock());
                d.left.width = 52.0;
                d
            },
            tabs: vec![Doc::placeholder()],
            active: 0,
            editor: Editor::new(sample::document()),
            file_path: None,
            asset_store: amalith_io::AssetStore::new(),
            io_error: None,
            selection: Vec::new(),
            anchor_sel: Vec::new(),
            expanded_groups: std::collections::HashSet::new(),
            selected_artboard: None,
            selected_layer: None,
            rename: None,
            newdoc: None,
            active_tool: Tool::Select,
            pre_artboard_tool: Tool::Select,
            active_slot: panels::PaintSlot::Fill,
            stroke_w: 1.0,
            opacity: 1.0,
            picker: None,
            last_click: None,
            pen: Vec::new(),
            pen_redo: Vec::new(),
            last_pen: None,
            marquee: None,
            view: CanvasView::default(),
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
            cursor_mode: CanvasCursor::Default,
            #[cfg(target_os = "macos")]
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

    fn request_main_redraw(&self) {
        if let Some(w) = self.main_window() {
            w.request_redraw();
        }
    }

    /// Move the live (active) document state off `App` into a [`Doc`].
    fn take_active_doc(&mut self) -> Doc {
        Doc {
            editor: std::mem::replace(
                &mut self.editor,
                Editor::new(amalith_core::Document::new("Untitled")),
            ),
            file_path: self.file_path.take(),
            asset_store: std::mem::replace(&mut self.asset_store, amalith_io::AssetStore::new()),
            io_error: self.io_error.take(),
            selection: std::mem::take(&mut self.selection),
            anchor_sel: std::mem::take(&mut self.anchor_sel),
            expanded_groups: std::mem::take(&mut self.expanded_groups),
            selected_artboard: self.selected_artboard.take(),
            selected_layer: self.selected_layer.take(),
            rename: self.rename.take(),
            stroke_w: self.stroke_w,
            opacity: self.opacity,
            view: self.view,
        }
    }

    /// Make `doc` the live document on `App`.
    fn load_active_doc(&mut self, doc: Doc) {
        self.editor = doc.editor;
        self.file_path = doc.file_path;
        self.asset_store = doc.asset_store;
        self.io_error = doc.io_error;
        self.selection = doc.selection;
        self.anchor_sel = doc.anchor_sel;
        self.expanded_groups = doc.expanded_groups;
        self.selected_artboard = doc.selected_artboard;
        self.selected_layer = doc.selected_layer;
        self.rename = doc.rename;
        self.stroke_w = doc.stroke_w;
        self.opacity = doc.opacity;
        self.view = doc.view;
        // Transient interaction state doesn't cross documents.
        self.drag = Drag::None;
        self.pen.clear();
        self.pen_redo.clear();
        self.last_pen = None;
        self.marquee = None;
        self.picker = None;
    }

    /// Open `doc` in a new tab and make it active.
    fn add_doc(&mut self, doc: Doc) {
        self.tabs[self.active] = self.take_active_doc();
        self.tabs.push(Doc::placeholder());
        self.active = self.tabs.len() - 1;
        self.load_active_doc(doc);
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

    /// Close tab `i`. Closing the last one leaves a fresh Untitled.
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if self.tabs.len() == 1 {
            self.load_active_doc(Doc::placeholder());
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
                &self.editor,
                self.view.zoom,
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
        format!("{name}{dirty} @ {:.0}% ({color}/{preview})", zoom * 100.0)
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
        self.view.to_screen().inverse() * screen
    }

    /// Apply the colour picker's current colour to its slot on the
    /// selection (one undoable command).
    fn apply_picker_color(&mut self) {
        let Some(pk) = self.picker else {
            return;
        };
        if self.selection.is_empty() {
            return;
        }
        let objects = self.selection.clone();
        let paint = amalith_core::Paint::Solid(pk.color());
        let cmd = match pk.slot {
            panels::PaintSlot::Fill => Command::SetFill { objects, paint },
            panels::PaintSlot::Stroke => Command::SetStroke { objects, paint },
        };
        let _ = self.editor.execute(cmd);
        self.request_main_redraw();
    }

    fn apply_panel_action(&mut self, action: panels::Action, double: bool) {
        match action {
            panels::Action::None => {}
            panels::Action::SetTool(t) => self.set_tool(t),
            panels::Action::Select(id) => {
                self.selection = vec![id];
                if double {
                    self.begin_rename(panels::RenameId::Object(id));
                }
            }
            panels::Action::SelectLayer(id) => {
                // Selecting a layer deselects any objects, so the row can
                // show its plain blue highlight.
                self.selection.clear();
                self.anchor_sel.clear();
                self.selected_layer = Some(id);
                if double {
                    self.begin_rename(panels::RenameId::Layer(id));
                }
            }
            panels::Action::SelectArtboard(id) => {
                self.selected_artboard = Some(id);
                if double {
                    self.begin_rename(panels::RenameId::Artboard(id));
                }
            }
            panels::Action::SetActiveSlot(s) => self.active_slot = s,
            // Single click just picks the slot; double click opens the
            // colour picker (Illustrator behaviour).
            panels::Action::OpenPicker(slot) if !double => self.active_slot = slot,
            panels::Action::OpenPicker(slot) => {
                self.active_slot = slot;
                let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                let cur = self.representative().and_then(|a| {
                    match slot {
                        panels::PaintSlot::Fill => a.fill,
                        panels::PaintSlot::Stroke => a.stroke,
                    }
                    .color()
                });
                let origin = Point::new(
                    (self.pointer.x + 10.0).min(w - picker::W - 4.0).max(4.0),
                    (self.pointer.y - picker::H * 0.5)
                        .min(h - picker::H - 4.0)
                        .max(4.0),
                );
                self.picker = Some(picker::Picker::from_color(slot, origin, cur));
            }
            panels::Action::SetPaint(paint) => {
                if !self.selection.is_empty() {
                    let objects = self.selection.clone();
                    let cmd = match self.active_slot {
                        panels::PaintSlot::Fill => Command::SetFill { objects, paint },
                        panels::PaintSlot::Stroke => Command::SetStroke { objects, paint },
                    };
                    let _ = self.editor.execute(cmd);
                }
            }
            panels::Action::SetStrokeWidth(width) => {
                if !self.selection.is_empty() {
                    let _ = self.editor.execute(Command::SetStrokeWidth {
                        objects: self.selection.clone(),
                        width,
                    });
                }
            }
            panels::Action::ToggleVisible(id) => {
                if let Some(cur) = self.editor.document().object(id).map(|o| o.visible) {
                    let _ = self.editor.execute(Command::SetVisible {
                        objects: vec![id],
                        visible: !cur,
                    });
                }
            }
            panels::Action::ToggleLocked(id) => {
                if let Some(cur) = self.editor.document().object(id).map(|o| o.locked) {
                    let _ = self.editor.execute(Command::SetLocked {
                        objects: vec![id],
                        locked: !cur,
                    });
                    if !cur {
                        self.selection.retain(|s| *s != id);
                    }
                }
            }
            panels::Action::ToggleExpand(id) => {
                if !self.expanded_groups.remove(&id) {
                    self.expanded_groups.insert(id);
                }
            }
            panels::Action::NewLayer => {
                let n = self.editor.document().layers().len() + 1;
                let _ = self.editor.execute(Command::CreateLayer {
                    name: format!("Layer {n}"),
                    index: None,
                });
            }
            panels::Action::LayerRestack(dir) => self.restack(dir),
            panels::Action::DeleteObjects => {
                if !self.selection.is_empty() {
                    let _ = self.editor.execute(Command::DeleteObjects {
                        ids: std::mem::take(&mut self.selection),
                    });
                }
            }
            panels::Action::DeleteArtboard => {
                if let Some(id) = self.selected_artboard.take() {
                    let _ = self.editor.execute(Command::DeleteArtboard { id });
                }
            }
            panels::Action::NewArtboard => {
                let boards = self.editor.document().artboards();
                let n = boards.len() + 1;
                // Sit the new board to the right of the rightmost one,
                // same size; default 1200×800 when there are none.
                let rect = boards
                    .iter()
                    .map(|a| a.rect)
                    .reduce(|acc, r| if r.x1 > acc.x1 { r } else { acc })
                    .map(|r| {
                        let (w, h) = (r.width(), r.height());
                        amalith_core::Rect::new(r.x1 + 40.0, r.y0, r.x1 + 40.0 + w, r.y0 + h)
                    })
                    .unwrap_or_else(|| amalith_core::Rect::new(0.0, 0.0, 1200.0, 800.0));
                if let Ok(CommandOutcome::Artboard(id)) = self.editor.execute(Command::CreateArtboard {
                    name: format!("Artboard {n}"),
                    rect,
                    index: None,
                }) {
                    self.selected_artboard = Some(id);
                    self.set_tool(Tool::Artboard);
                }
            }
        }
        self.request_main_redraw();
    }

    /// Start an inline rename, seeding the buffer with the current name.
    fn begin_rename(&mut self, target: panels::RenameId) {
        let doc = self.editor.document();
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
            self.rename = Some(Rename {
                target,
                buf,
                fresh: true,
            });
            self.request_main_redraw();
        }
    }

    /// Commit the inline rename (empty name = cancel).
    fn commit_rename(&mut self) {
        let Some(r) = self.rename.take() else {
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
            let _ = self.editor.execute(cmd);
        }
        self.request_main_redraw();
    }

    /// A key while an inline rename is active. Returns `true` if consumed.
    fn rename_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.rename.is_none() || !event.state.is_pressed() {
            return self.rename.is_some();
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => self.commit_rename(),
            PhysicalKey::Code(KeyCode::Escape) => {
                self.rename = None;
                self.request_main_redraw();
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some(r) = &mut self.rename {
                    r.fresh = false;
                    r.buf.pop();
                }
                self.request_main_redraw();
            }
            _ => {
                if let (Some(r), Some(txt)) = (&mut self.rename, event.text.as_ref()) {
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

    /// Appearance of the first selected object, for the Swatches panel.
    fn representative(&self) -> Option<amalith_core::Appearance> {
        self.selection
            .first()
            .and_then(|id| self.editor.document().object(*id))
            .map(|o| o.appearance)
    }

    /// Drop selection ids / anchors that no longer exist.
    fn prune_selection(&mut self) {
        let doc = self.editor.document();
        self.selection.retain(|id| doc.object(*id).is_some());
        self.anchor_sel
            .retain(|(id, i)| match doc.object(*id).map(|o| &o.kind) {
                Some(amalith_core::ObjectKind::Path(pd)) => {
                    amalith_core::geom::anchor_position(&pd.geometry, *i).is_some()
                }
                _ => false,
            });
        if self
            .selected_layer
            .is_some_and(|id| !doc.layers().iter().any(|l| l.id == id))
        {
            self.selected_layer = None;
        }
        if self
            .selected_artboard
            .is_some_and(|id| doc.artboard(id).is_none())
        {
            self.selected_artboard = None;
        }
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
        let mut out = self.selection.clone();
        for (id, _) in &self.anchor_sel {
            if !out.contains(id) {
                out.push(*id);
            }
        }
        out.retain(|id| {
            matches!(
                self.editor.document().object(*id).map(|o| &o.kind),
                Some(amalith_core::ObjectKind::Path(_))
            )
        });
        out
    }

    /// Arrow-key nudge (Shift = ×10). Moves the selected anchors when the
    /// Direct Selection tool is active, otherwise the object selection.
    fn nudge(&mut self, dx: f64, dy: f64) {
        let step = if self.shift_down { 10.0 } else { 1.0 };
        let delta = amalith_core::Vec2::new(dx * step, dy * step);
        if self.effective_tool() == Tool::DirectSelect && !self.anchor_sel.is_empty() {
            let _ = self.editor.execute(Command::MoveAnchors {
                anchors: self.anchor_sel.clone(),
                delta,
            });
            self.request_main_redraw();
        } else if !self.selection.is_empty() {
            let _ = self.editor.execute(Command::MoveObjects {
                objects: self.selection.clone(),
                delta,
            });
            self.request_main_redraw();
        }
    }

    /// Commit the in-progress Pen path (needs ≥2 anchors). `closed` makes
    /// it a polygon; otherwise an open polyline.
    fn commit_pen(&mut self, closed: bool) {
        self.pen_redo.clear();
        if self.pen.len() < 2 {
            self.pen.clear();
            self.last_pen = None;
            return;
        }
        let anchors: Vec<Point> = std::mem::take(&mut self.pen);
        let pts: Vec<amalith_core::Point> = anchors
            .iter()
            .map(|p| amalith_core::Point::new(p.x, p.y))
            .collect();
        let path = if closed {
            amalith_core::PathData::polygon(&pts)
        } else {
            amalith_core::PathData::polyline(&pts)
        };
        let layer = self.ensure_layer();
        if let Ok(CommandOutcome::Object(id)) = self.editor.execute(Command::CreatePath {
            layer,
            path,
            name: None,
        }) {
            self.selection = vec![id];
            self.last_pen = Some((id, anchors, closed));
            self.apply_new_appearance(id);
        }
        self.request_main_redraw();
    }

    /// Open the New Document modal (⌘N / File ▸ New).
    fn open_new_doc(&mut self) {
        self.newdoc = Some(newdoc::NewDocForm::default());
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

    /// A key while the New Document modal is open.
    fn newdoc_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.newdoc = None;
                self.request_main_redraw();
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                if let Some(f) = self.newdoc.as_mut() {
                    f.focus_next();
                }
                self.request_main_redraw();
                return;
            }
            _ => {}
        }
        let Some(form) = self.newdoc.as_mut() else {
            return;
        };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => form.focus_next(),
            PhysicalKey::Code(KeyCode::Backspace) => form.backspace(),
            _ => {
                if let Some(txt) = &event.text {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        form.push_char(ch);
                    }
                }
            }
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
            self.io_error = Some("Width and height must be greater than zero.".into());
            self.request_main_redraw();
            return;
        }
        let name = {
            let n = form.name.trim();
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
        for i in 0..n_ab {
            let x = i as f64 * (wpx + gap);
            let _ = editor.execute(Command::CreateArtboard {
                name: format!("Artboard {}", i + 1),
                rect: amalith_core::Rect::new(x, 0.0, x + wpx, hpx),
                index: None,
            });
        }
        let _ = editor.execute(Command::CreateLayer {
            name: "Layer 1".into(),
            index: None,
        });

        self.newdoc = None;
        self.add_doc(Doc::new(editor));
    }

    /// Route one [`MenuAction`] to the matching operation. Mirrors the
    /// keyboard shortcuts so the menu bar and the keys stay in step.
    fn run_menu_action(&mut self, action: MenuAction) {
        match action {
            MenuAction::New => self.open_new_doc(),
            MenuAction::Open => self.open_document(),
            MenuAction::Save => self.save_document(false),
            MenuAction::SaveAs => self.save_document(true),
            MenuAction::ImportSvg => self.import_svg(),
            MenuAction::Undo => {
                let _ = self.editor.undo();
                self.prune_selection();
                self.request_main_redraw();
            }
            MenuAction::Redo => {
                let _ = self.editor.redo();
                self.prune_selection();
                self.request_main_redraw();
            }
            MenuAction::Cut => {
                if !self.selection.is_empty() {
                    let _ = self.editor.copy(&self.selection);
                    let _ = self.editor.execute(Command::DeleteObjects {
                        ids: std::mem::take(&mut self.selection),
                    });
                    self.request_main_redraw();
                }
            }
            MenuAction::Copy => {
                let _ = self.editor.copy(&self.selection);
            }
            MenuAction::Paste => {
                if self.editor.has_clipboard() {
                    if let Ok(ids) = self
                        .editor
                        .paste(amalith_core::Vec2::new(16.0, 16.0), PasteStack::Top)
                    {
                        self.selection = ids;
                    }
                    self.request_main_redraw();
                }
            }
            MenuAction::Duplicate => {
                if !self.selection.is_empty() {
                    if let Ok(ids) = self
                        .editor
                        .duplicate_objects(&self.selection, amalith_core::Vec2::new(16.0, 16.0))
                    {
                        self.selection = ids;
                    }
                    self.request_main_redraw();
                }
            }
            MenuAction::SelectAll => self.select_all(),
            MenuAction::BringForward => self.restack(1),
            MenuAction::BringToFront => self.restack_extreme(true),
            MenuAction::SendBackward => self.restack(-1),
            MenuAction::SendToBack => self.restack_extreme(false),
        }
    }

    /// Options-bar Weight stepper. Reads the live selection value (or the
    /// stored current), nudges it, applies to any selection, and keeps it
    /// as the value new shapes will use.
    fn step_weight(&mut self, dir: i32) {
        let base = self
            .selection
            .first()
            .and_then(|id| self.editor.document().object(*id))
            .map(|o| o.appearance.stroke_width)
            .unwrap_or(self.stroke_w);
        let step = if base < 1.0 { 0.25 } else { 1.0 };
        let next = (base + dir as f64 * step).clamp(0.0, 1000.0);
        self.stroke_w = next;
        if !self.selection.is_empty() {
            let _ = self.editor.execute(Command::SetStrokeWidth {
                objects: self.selection.clone(),
                width: next,
            });
        }
        self.request_main_redraw();
    }

    /// Options-bar Opacity stepper (5% steps).
    fn step_opacity(&mut self, dir: i32) {
        let base = self
            .selection
            .first()
            .and_then(|id| self.editor.document().object(*id))
            .map(|o| o.appearance.opacity)
            .unwrap_or(self.opacity);
        let next = (base + dir as f32 * 0.05).clamp(0.0, 1.0);
        self.opacity = next;
        if !self.selection.is_empty() {
            let _ = self.editor.execute(Command::SetOpacity {
                objects: self.selection.clone(),
                opacity: next,
            });
        }
        self.request_main_redraw();
    }

    /// Push the options-bar Weight / Opacity onto a freshly created
    /// object, so those fields mean something with nothing selected.
    fn apply_new_appearance(&mut self, id: ObjectId) {
        if (self.stroke_w - 1.0).abs() > f64::EPSILON {
            let _ = self.editor.execute(Command::SetStrokeWidth {
                objects: vec![id],
                width: self.stroke_w,
            });
        }
        if (self.opacity - 1.0).abs() > f32::EPSILON {
            let _ = self.editor.execute(Command::SetOpacity {
                objects: vec![id],
                opacity: self.opacity,
            });
        }
    }

    /// ⌘] / ⌘[ — move the selection `steps` places forward (+) or back
    /// (−) in its parent's paint order.
    fn restack(&mut self, steps: i32) {
        if self.selection.is_empty() || steps == 0 {
            return;
        }
        let _ = self.editor.execute(Command::NudgeStack {
            ids: self.selection.clone(),
            steps,
        });
        self.request_main_redraw();
    }

    /// ⌘⌥] / ⌘⌥[ — bring the selection to the very front / back. Bounded
    /// by the largest sibling count among the selection's parents, which
    /// is the most swaps `NudgeStack` could ever need.
    fn restack_extreme(&mut self, to_front: bool) {
        let doc = self.editor.document();
        let bound = self
            .selection
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
        match amalith_io::load(&path) {
            Ok((document, assets)) => {
                let mut doc = Doc::new(Editor::new(document));
                doc.asset_store = assets;
                doc.file_path = Some(path);
                self.add_doc(doc);
            }
            Err(err) => {
                self.io_error = Some(format!("Open failed: {err}"));
                self.request_main_redraw();
            }
        }
    }

    /// ⌘S / ⌘⇧S — write the document to its `.amalith` file, prompting for
    /// a path when there isn't one yet or `save_as` forces it.
    fn save_document(&mut self, save_as: bool) {
        let path = if save_as { None } else { self.file_path.clone() }.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Amalith document", &["amalith"])
                .set_file_name("Untitled.amalith")
                .save_file()
        });
        let Some(path) = path else {
            return;
        };
        match amalith_io::save(self.editor.document(), &self.asset_store, &path) {
            Ok(()) => {
                self.file_path = Some(path);
                self.io_error = None;
            }
            Err(err) => self.io_error = Some(format!("Save failed: {err}")),
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
                self.io_error = Some(format!("Import failed: {err}"));
                self.request_main_redraw();
                return;
            }
        };
        match self.editor.copy_from_svg(&svg) {
            Ok(()) => match self.editor.paste(amalith_core::Vec2::ZERO, PasteStack::Top) {
                Ok(ids) => {
                    self.selection = ids;
                    self.anchor_sel.clear();
                    self.io_error = None;
                }
                Err(err) => self.io_error = Some(format!("Import failed: {err}")),
            },
            Err(err) => self.io_error = Some(format!("Import failed: {err}")),
        }
        self.request_main_redraw();
    }

    /// Switch tools, discarding any in-progress Pen path.
    fn set_tool(&mut self, t: Tool) {
        if t != Tool::Pen {
            self.pen.clear();
            self.pen_redo.clear();
        }
        if t != Tool::DirectSelect {
            self.anchor_sel.clear();
        }
        if t == Tool::Artboard && self.active_tool != Tool::Artboard {
            self.pre_artboard_tool = self.active_tool;
        }
        if t != Tool::Artboard {
            self.selected_artboard = None;
        }
        self.last_pen = None;
        self.active_tool = t;
        self.request_main_redraw();
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
            let _ = self.editor.execute(Command::DeleteObject { id });
            anchors.pop();
            if anchors.len() >= 2 {
                self.pen = anchors;
                self.commit_pen(closed && self.pen.len() >= 3);
            } else {
                self.selection.clear();
            }
            self.request_main_redraw();
            return true;
        }
        false
    }

    fn select_all(&mut self) {
        self.selection = self
            .editor
            .document()
            .layers()
            .iter()
            .flat_map(|l| l.children.iter().copied())
            .collect();
        self.request_main_redraw();
    }

    /// The layer new shapes should land in — the topmost, creating one if
    /// the document has none.
    fn ensure_layer(&mut self) -> LayerId {
        if let Some(l) = self.editor.document().layers().last() {
            return l.id;
        }
        match self.editor.execute(Command::CreateLayer {
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
        self.view
            .to_screen()
            .inverse()
            .transform_rect_bbox(Rect::new(left, CHROME_TOP, right, h))
    }

    /// The canvas viewport in screen (logical) px.
    fn canvas_viewport(&self) -> Rect {
        let (_, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let (left, right) = self.canvas_x_span();
        Rect::new(left, CHROME_TOP, right, h)
    }

    /// Recompute the pointer style and, on change, tell the OS.
    fn update_canvas_cursor(&mut self) {
        let over = self.pointer_win == self.main_id
            && self.picker.is_none()
            && self.newdoc.is_none()
            && self.canvas_viewport().contains(self.pointer);
        let mode = if !over {
            CanvasCursor::Default
        } else {
            match self.effective_tool() {
                Tool::Select | Tool::DirectSelect | Tool::Pen => CanvasCursor::Glyph,
                _ => CanvasCursor::Crosshair,
            }
        };
        if mode != self.cursor_mode {
            self.cursor_mode = mode;
            if let Some(w) = self.main_window() {
                w.set_cursor_visible(mode != CanvasCursor::Glyph);
                w.set_cursor(match mode {
                    CanvasCursor::Crosshair => winit::window::CursorIcon::Crosshair,
                    _ => winit::window::CursorIcon::Default,
                });
            }
            self.request_main_redraw();
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

    /// Tear `panel` out of the main rail into a new borderless window that
    /// starts under the cursor, and begin moving it.
    fn tear_off(&mut self, event_loop: &ActiveEventLoop, panel: PanelId, main_local_press: Point) {
        let global = self.main_inner_origin() + main_local_press.to_vec2();
        let pos = global - TEAROFF_GRAB;
        let id = match self.dock.detach(
            panel,
            [pos.x as f32, pos.y as f32, FLOAT_W as f32, FLOAT_H as f32],
        ) {
            Some(id) => id,
            None => return,
        };

        let attrs = Window::default_attributes()
            .with_title(tab_label(panel))
            .with_decorations(false)
            .with_resizable(true)
            .with_inner_size(LogicalSize::new(FLOAT_W, FLOAT_H))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create float window"),
        );
        let wid = window.id();
        let host = self.make_host(window.clone(), Role::Floating(id));
        self.hosts.insert(wid, host);

        self.drag = Drag::MovingFloating {
            id,
            grab: TEAROFF_GRAB,
            pos,
        };
        window.request_redraw();
        self.request_main_redraw();
    }

    fn on_press(&mut self, id: WindowId, double: bool) {
        let Some(role) = self.hosts.get(&id).map(|h| h.role) else {
            return;
        };
        // Any press ends the "⌘Z re-opens the last pen path" window.
        self.last_pen = None;
        // A press anywhere commits an in-progress rename (unless it's the
        // double-click that's about to start one).
        if self.rename.is_some() && !double {
            self.commit_rename();
        }
        match role {
            Role::Main => {
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };

                // The New Document modal is, well, modal.
                if let Some(form) = &self.newdoc {
                    let lay = newdoc::layout(Rect::new(0.0, 0.0, w, h), form.scroll);
                    let hit = newdoc::hit(form, &lay, self.pointer);
                    self.apply_newdoc_hit(hit);
                    return;
                }

                // The app bar swallows clicks (unless the picker is up).
                if self.picker.is_none() && self.pointer.y < APP_BAR_H {
                    return;
                }

                // The options / context bar (full width): chips + steppers.
                if self.picker.is_none()
                    && self.pointer.y >= APP_BAR_H
                    && self.pointer.y < APP_BAR_H + OPT_BAR_H
                {
                    let ob = opt_bar_layout(opt_bar_rect(w));
                    let p = self.pointer;
                    if ob.fill.contains(p) {
                        self.apply_panel_action(
                            panels::Action::OpenPicker(panels::PaintSlot::Fill),
                            double,
                        );
                    } else if ob.stroke.contains(p) {
                        self.apply_panel_action(
                            panels::Action::OpenPicker(panels::PaintSlot::Stroke),
                            double,
                        );
                    } else if ob.weight_up.contains(p) {
                        self.step_weight(1);
                    } else if ob.weight_down.contains(p) {
                        self.step_weight(-1);
                    } else if ob.opacity_up.contains(p) {
                        self.step_opacity(1);
                    } else if ob.opacity_down.contains(p) {
                        self.step_opacity(-1);
                    }
                    return;
                }

                // The document-tab strip: switch tabs / close a tab.
                if self.picker.is_none()
                    && self.pointer.y >= APP_BAR_H + OPT_BAR_H
                    && self.pointer.y < APP_BAR_H + OPT_BAR_H + TAB_BAR_H
                {
                    let (left_x, right_x) = self.canvas_x_span();
                    let strip = tab_bar_rect(left_x, right_x);
                    let labels: Vec<String> =
                        (0..self.tabs.len()).map(|i| self.tab_label(i)).collect();
                    for (i, (whole, close)) in
                        layout_tabs(&mut self.text, &labels, strip).into_iter().enumerate()
                    {
                        if close.contains(self.pointer) {
                            self.close_tab(i);
                            return;
                        }
                        if whole.contains(self.pointer) {
                            self.switch_to(i);
                            return;
                        }
                    }
                    return;
                }

                // The colour picker is modal while open.
                if let Some(pk) = self.picker {
                    match picker::hit(&pk, self.pointer) {
                        picker::Hit::Sv(s, v) => {
                            if let Some(p) = &mut self.picker {
                                p.s = s;
                                p.v = v;
                            }
                            self.drag = Drag::PickColor { in_hue: false };
                        }
                        picker::Hit::Hue(hue) => {
                            if let Some(p) = &mut self.picker {
                                p.h = hue;
                            }
                            self.drag = Drag::PickColor { in_hue: true };
                        }
                        picker::Hit::NoneButton => {
                            if !self.selection.is_empty() {
                                let objects = self.selection.clone();
                                let paint = amalith_core::Paint::None;
                                let _ = self.editor.execute(match pk.slot {
                                    panels::PaintSlot::Fill => Command::SetFill { objects, paint },
                                    panels::PaintSlot::Stroke => {
                                        Command::SetStroke { objects, paint }
                                    }
                                });
                            }
                            self.picker = None;
                        }
                        picker::Hit::Inside => {}
                        picker::Hit::Outside => {
                            self.apply_picker_color();
                            self.picker = None;
                        }
                    }
                    self.request_main_redraw();
                    return;
                }

                for side in [RailSide::Left, RailSide::Right] {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        continue;
                    }
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    // The rail's inner edge widens the whole rail — check it
                    // first, since its grab zone spills onto the canvas.
                    if rail_edge_bar(side, rect)
                        .inflate(GRAB_SLOP + 1.0, 0.0)
                        .contains(self.pointer)
                    {
                        self.drag = Drag::RailWidth { side };
                        return;
                    }
                    if !rect.contains(self.pointer) {
                        continue;
                    }
                    let laid = build_rail_layout(rail, &self.theme, &mut self.text, rect);
                    if let Some(sp) = laid
                        .splitters
                        .iter()
                        .find(|s| s.rect.inflate(GRAB_SLOP, GRAB_SLOP).contains(self.pointer))
                    {
                        self.drag = Drag::Splitter {
                            side,
                            path: sp.path.clone(),
                            gap: sp.index,
                        };
                        return;
                    }
                    for area in &laid.areas {
                        if area.tab_strip.contains(self.pointer) {
                            if let Some(tab) =
                                area.tabs.iter().position(|t| t.rect.contains(self.pointer))
                            {
                                self.drag = Drag::PendingTearoff {
                                    side,
                                    panel: area.tabs[tab].panel,
                                    path: area.path.clone(),
                                    tab,
                                    press: self.pointer,
                                };
                            }
                            return;
                        }
                        if area.body.contains(self.pointer) {
                            if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                                let rep = self.representative();
                                let action = {
                                    let ctx = panels::Ctx {
                                        theme: &self.theme,
                                        doc: self.editor.document(),
                                        selection: &self.selection,
                                        active_tool: self.active_tool,
                                        pointer: self.pointer,
                                        representative: rep,
                                        active_slot: self.active_slot,
                                        expanded: &self.expanded_groups,
                                        renaming: self
                                            .rename
                                            .as_ref()
                                            .map(|r| (r.target, r.buf.as_str())),
                                        selected_layer: self.selected_layer,
                                        selected_artboard: self.selected_artboard,
                                    };
                                    panels::hit(pid, area.body, self.pointer, &ctx)
                                };
                                self.apply_panel_action(action, double);
                            }
                            return;
                        }
                    }
                    return;
                }
                // Not on a rail.
                if self.space_down {
                    self.drag = Drag::Pan { last: self.pointer };
                    return;
                }
                let dp = self.doc_point(self.pointer);

                // Pen: click to place anchors; click the first anchor to
                // close and commit.
                if self.active_tool == Tool::Pen {
                    let close_r = 8.0 / self.view.zoom;
                    if self.pen.len() >= 3
                        && self
                            .pen
                            .first()
                            .is_some_and(|f| (*f - dp).hypot() <= close_r)
                    {
                        self.commit_pen(true);
                    } else {
                        let p = constrained(self.pen.last().copied(), dp, self.shift_down);
                        self.pen.push(p);
                        self.pen_redo.clear();
                        self.request_main_redraw();
                    }
                    return;
                }

                // A shape tool rubber-bands a new object.
                if self.active_tool.is_shape() {
                    self.drag = Drag::DrawShape {
                        tool: self.active_tool,
                        start_doc: dp,
                        cur_doc: dp,
                    };
                    return;
                }

                // Artboard tool: a resize handle of the selected artboard,
                // else drag an existing artboard, else rubber-band a new one.
                if self.active_tool == Tool::Artboard {
                    if let Some(id) = self.selected_artboard {
                        if let Some(ab) = self
                            .editor
                            .document()
                            .artboards()
                            .iter()
                            .find(|a| a.id == id)
                        {
                            let quad = handles::rect_quad(convert::rect(ab.rect))
                                .map(|p| self.view.to_screen() * p);
                            if let Some(handle) = handles::hit_handle(self.pointer, quad) {
                                self.drag = Drag::ResizeArtboard {
                                    id,
                                    handle,
                                    start_rect: ab.rect,
                                    start_doc: dp,
                                    cur_doc: dp,
                                };
                                return;
                            }
                        }
                    }
                    match artboard_at(self.editor.document(), dp) {
                        Some(id) => {
                            self.selected_artboard = Some(id);
                            self.drag = Drag::MoveArtboard {
                                id,
                                start_doc: dp,
                                last_doc: dp,
                            };
                        }
                        None => {
                            self.selected_artboard = None;
                            self.drag = Drag::DrawArtboard {
                                start_doc: dp,
                                cur_doc: dp,
                            };
                        }
                    }
                    self.request_main_redraw();
                    return;
                }

                // Direct Selection (Illustrator white arrow): nodes show
                // only for objects you've already picked. A click grabs a
                // node of such an object; otherwise it starts a marquee
                // that either selects the object under the press (if the
                // pointer never moves) or rubber-bands its nodes.
                if self.effective_tool() == Tool::DirectSelect {
                    let hit_r = 6.0 / self.view.zoom;
                    let shown = self.node_paths();
                    if let Some(a) =
                        anchors::topmost_anchor_among(self.editor.document(), &shown, dp, hit_r)
                    {
                        if self.shift_down {
                            if let Some(i) = self.anchor_sel.iter().position(|x| *x == a) {
                                self.anchor_sel.remove(i);
                            } else {
                                self.anchor_sel.push(a);
                            }
                        } else {
                            if !self.anchor_sel.contains(&a) {
                                self.anchor_sel = vec![a];
                            }
                            self.drag = Drag::MoveAnchors {
                                start_doc: dp,
                                last_doc: dp,
                                moved: false,
                            };
                        }
                        self.request_main_redraw();
                        return;
                    }

                    let visible = self.visible_doc_rect();
                    let candidate =
                        select::topmost_selectable_at(self.editor.document(), dp, visible);
                    self.drag = Drag::AnchorMarquee {
                        start: self.pointer,
                        candidate,
                    };
                    self.request_main_redraw();
                    return;
                }

                // Selection tool (ported from amalith-app's `press`):
                let visible = self.visible_doc_rect();

                // Transform handles / rotation halo win over object hits.
                if !self.selection.is_empty() {
                    if let Some(quad) =
                        select::selection_quad(self.editor.document(), &self.selection)
                    {
                        let to_screen = self.view.to_screen();
                        let scr = quad.map(|p| to_screen * p);
                        let start_xf: HashMap<ObjectId, Affine> = self
                            .selection
                            .iter()
                            .filter_map(|id| {
                                self.editor
                                    .document()
                                    .object(*id)
                                    .map(|o| (*id, convert::affine(o.transform)))
                            })
                            .collect();
                        if !start_xf.is_empty() {
                            if let Some(handle) = handles::hit_handle(self.pointer, scr) {
                                let start_bounds =
                                    select::union_bounds(self.editor.document(), &self.selection)
                                        .unwrap();
                                self.drag = Drag::Scale {
                                    handle,
                                    start_bounds,
                                    preview: start_xf.clone(),
                                    start_xf,
                                };
                                self.request_main_redraw();
                                return;
                            }
                            if handles::hit_rotate_halo(self.pointer, scr) {
                                let center =
                                    select::union_bounds(self.editor.document(), &self.selection)
                                        .unwrap()
                                        .center();
                                self.drag = Drag::Rotate {
                                    center,
                                    start_angle: handles::angle_to(center, dp),
                                    preview: start_xf.clone(),
                                    start_xf,
                                };
                                self.request_main_redraw();
                                return;
                            }
                        }
                    }
                }
                let start_move = |dp: Point| Drag::MoveObjects {
                    start_doc: dp,
                    last_doc: dp,
                    moved: false,
                };
                let doc = self.editor.document();
                if let Some(id) = select::topmost_selectable_at(doc, dp, visible) {
                    if self.shift_down {
                        // Shift-click toggles that object; no drag.
                        if let Some(i) = self.selection.iter().position(|x| *x == id) {
                            self.selection.remove(i);
                        } else {
                            self.selection.push(id);
                        }
                    } else {
                        // Click on an unselected object replaces the
                        // selection before the move; click on one already
                        // selected drags the whole selection.
                        if !self.selection.contains(&id) {
                            self.selection = vec![id];
                        }
                        self.drag = start_move(dp);
                    }
                } else {
                    // Empty space: a press inside the selection box drags
                    // the selection; otherwise it's a marquee.
                    let inside_box = !self.shift_down
                        && select::union_bounds(doc, &self.selection)
                            .is_some_and(|b| b.contains(dp));
                    if inside_box {
                        self.drag = start_move(dp);
                    } else {
                        if !self.shift_down {
                            self.selection.clear();
                        }
                        self.drag = Drag::Marquee {
                            start: self.pointer,
                        };
                    }
                }
                self.request_main_redraw();
            }
            Role::Floating(fid) => {
                let laid = self.floating_layout(fid);
                for area in &laid.areas {
                    if !area.tab_strip.contains(self.pointer) {
                        continue;
                    }
                    let tab = area
                        .tabs
                        .iter()
                        .position(|t| t.rect.contains(self.pointer))
                        .unwrap_or(0);
                    self.drag = Drag::PendingFloatMove {
                        id: fid,
                        tab,
                        press: self.pointer,
                    };
                    return;
                }
            }
        }
    }

    fn on_cursor_move(&mut self) {
        self.update_canvas_cursor();
        // Redraw so the tool glyph tracks the pointer.
        if self.cursor_mode == CanvasCursor::Glyph {
            self.request_main_redraw();
        }
        match &self.drag {
            Drag::RailWidth { side } => {
                let side = *side;
                let Some((w, _)) = self.main_logical_size() else {
                    return;
                };
                let raw = match side {
                    RailSide::Left => self.pointer.x,
                    RailSide::Right => w - self.pointer.x,
                };
                let clamped = raw.clamp(RAIL_MIN_W, (w * 0.7).max(RAIL_MIN_W));
                let gap = self.theme.splitter_thickness as f32;
                self.dock.rail_mut(side).set_width_absorbing(
                    clamped as f32,
                    gap,
                    matches!(side, RailSide::Left),
                );
                self.request_main_redraw();
            }
            Drag::Splitter { side, path, gap } => {
                let (side, path, gap) = (*side, path.clone(), *gap);
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };
                let rect = rail_rect_for(side, self.dock.rail(side).width as f64, w, h);
                let laid =
                    build_rail_layout(self.dock.rail(side), &self.theme, &mut self.text, rect);
                if let Some(sp) = laid
                    .splitters
                    .iter()
                    .find(|s| s.path == path && s.index == gap)
                {
                    let frac = sp.frac_at(self.pointer);
                    self.dock.rail_mut(side).set_boundary(&path, gap, frac);
                    self.request_main_redraw();
                }
            }
            Drag::Pan { last } => {
                let last = *last;
                self.view.pan += self.pointer - last;
                self.drag = Drag::Pan { last: self.pointer };
                self.request_main_redraw();
            }
            Drag::MoveObjects { start_doc, .. } => {
                let start_doc = *start_doc;
                let dp = self.doc_point(self.pointer);
                self.drag = Drag::MoveObjects {
                    start_doc,
                    last_doc: dp,
                    moved: true,
                };
                self.request_main_redraw();
            }
            Drag::Marquee { start } | Drag::AnchorMarquee { start, .. } => {
                self.marquee = Some(Rect::from_points(*start, self.pointer));
                self.request_main_redraw();
            }
            Drag::MoveAnchors { start_doc, .. } => {
                let start_doc = *start_doc;
                self.drag = Drag::MoveAnchors {
                    start_doc,
                    last_doc: self.doc_point(self.pointer),
                    moved: true,
                };
                self.request_main_redraw();
            }
            Drag::PickColor { in_hue } => {
                let in_hue = *in_hue;
                if let Some(pk) = self.picker {
                    let (h, s, v) = picker::drag_value(&pk, self.pointer, in_hue);
                    if let Some(p) = &mut self.picker {
                        p.h = h;
                        p.s = s;
                        p.v = v;
                    }
                    self.request_main_redraw();
                }
            }
            Drag::DrawShape {
                tool, start_doc, ..
            } => {
                let (tool, start_doc) = (*tool, *start_doc);
                self.drag = Drag::DrawShape {
                    tool,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::DrawArtboard { start_doc, .. } => {
                let start_doc = *start_doc;
                self.drag = Drag::DrawArtboard {
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::MoveArtboard { id, start_doc, .. } => {
                let (id, start_doc) = (*id, *start_doc);
                self.drag = Drag::MoveArtboard {
                    id,
                    start_doc,
                    last_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::ResizeArtboard {
                id,
                handle,
                start_rect,
                start_doc,
                ..
            } => {
                let (id, handle, start_rect, start_doc) =
                    (*id, *handle, *start_rect, *start_doc);
                self.drag = Drag::ResizeArtboard {
                    id,
                    handle,
                    start_rect,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::Scale {
                handle,
                start_bounds,
                start_xf,
                ..
            } => {
                let (handle, start_bounds) = (*handle, *start_bounds);
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let m = handles::scaled_transform(
                    start_bounds,
                    handle,
                    dp,
                    self.shift_down,
                    self.alt_down,
                );
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                self.drag = Drag::Scale {
                    handle,
                    start_bounds,
                    start_xf,
                    preview,
                };
                self.request_main_redraw();
            }
            Drag::Rotate {
                center,
                start_angle,
                start_xf,
                ..
            } => {
                let (center, start_angle) = (*center, *start_angle);
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let m = handles::rotate_transform(center, start_angle, dp, self.shift_down);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                self.drag = Drag::Rotate {
                    center,
                    start_angle,
                    start_xf,
                    preview,
                };
                self.request_main_redraw();
            }
            Drag::PendingTearoff { panel, press, .. } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let (panel, press) = (*panel, *press);
                    // event_loop isn't handed to cursor events; defer the
                    // actual spawn to the caller via a queued request.
                    self.pending_tearoff = Some((panel, press));
                }
            }
            Drag::PendingFloatMove { id, press, .. } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let id = *id;
                    let pos = self
                        .dock
                        .floating(id)
                        .map(|f| Point::new(f.rect[0] as f64, f.rect[1] as f64))
                        .unwrap_or(Point::ZERO);
                    self.drag = Drag::MovingFloating {
                        id,
                        grab: press.to_vec2(),
                        pos,
                    };
                }
            }
            _ => {}
        }

        // Move a floating window by locking it to the cursor: the cursor's
        // position *inside* the window, versus where it was grabbed, is the
        // move. Both come from the same `position / scale`, so there is no
        // scale or acceleration mismatch, and nothing reads the OS window
        // rect back — so it can't drift or jitter.
        if let Drag::MovingFloating { id, grab, pos } = self.drag {
            let float_wid = self.floating_window(id).map(|w| w.id());
            let global = if self.pointer_win == float_wid {
                pos + self.pointer.to_vec2()
            } else if self.pointer_win == self.main_id {
                self.main_inner_origin() + self.pointer.to_vec2()
            } else {
                return;
            };
            let new_pos = global - grab;
            self.drag = Drag::MovingFloating {
                id,
                grab,
                pos: new_pos,
            };
            if let Some(w) = self.floating_window(id) {
                w.set_outer_position(LogicalPosition::new(new_pos.x, new_pos.y));
                w.request_redraw();
            }
            self.redock_preview = Some(self.resolve_redock(global));
            self.request_main_redraw();
        }
    }

    fn on_release(&mut self) {
        match std::mem::take(&mut self.drag) {
            Drag::None | Drag::Splitter { .. } | Drag::RailWidth { .. } | Drag::Pan { .. } => {}
            Drag::PickColor { .. } => self.apply_picker_color(),
            Drag::MoveObjects {
                start_doc,
                last_doc,
                moved,
            } => {
                if moved && !self.selection.is_empty() {
                    let mut d = last_doc - start_doc;
                    if self.shift_down {
                        d = snap8(d);
                    }
                    let delta = convert::vec2_to_core(d);
                    if self.alt_down {
                        if let Ok(new_ids) = self
                            .editor
                            .duplicate_objects(&self.selection.clone(), delta)
                        {
                            self.selection = new_ids;
                        }
                    } else {
                        let _ = self.editor.execute(Command::MoveObjects {
                            objects: self.selection.clone(),
                            delta,
                        });
                    }
                    self.request_main_redraw();
                }
            }
            Drag::DrawShape {
                tool,
                start_doc,
                cur_doc,
            } => {
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if r.width() > 0.5 && r.height() > 0.5 {
                    let layer = self.ensure_layer();
                    let cmd = match tool {
                        Tool::Rectangle => Command::CreateRect {
                            layer,
                            rect: r,
                            name: None,
                        },
                        Tool::Ellipse => Command::CreateEllipse {
                            layer,
                            rect: r,
                            name: None,
                        },
                        Tool::RoundedRect | Tool::Polygon | Tool::Star => {
                            match primitive_path(tool, r) {
                                Some(path) => Command::CreatePath {
                                    layer,
                                    path,
                                    name: None,
                                },
                                None => return,
                            }
                        }
                        Tool::Select | Tool::DirectSelect | Tool::Pen | Tool::Artboard => return,
                    };
                    if let Ok(CommandOutcome::Object(id)) = self.editor.execute(cmd) {
                        self.selection = vec![id];
                        self.apply_new_appearance(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::DrawArtboard {
                start_doc,
                cur_doc,
            } => {
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if r.width() > 1.0 && r.height() > 1.0 {
                    let n = self.editor.document().artboards().len() + 1;
                    if let Ok(CommandOutcome::Artboard(id)) =
                        self.editor.execute(Command::CreateArtboard {
                            name: format!("Artboard {n}"),
                            rect: r,
                            index: None,
                        })
                    {
                        self.selected_artboard = Some(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::MoveArtboard {
                id,
                start_doc,
                last_doc,
            } => {
                let mut d = last_doc - start_doc;
                if self.shift_down {
                    d = snap8(d);
                }
                let delta = convert::vec2_to_core(d);
                if delta.x != 0.0 || delta.y != 0.0 {
                    let cmd = if self.alt_down {
                        Command::DuplicateArtboard { id, delta }
                    } else {
                        Command::MoveArtboard { id, delta }
                    };
                    if let Ok(CommandOutcome::Artboard(new_id)) = self.editor.execute(cmd) {
                        self.selected_artboard = Some(new_id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::ResizeArtboard {
                id,
                handle,
                start_rect,
                start_doc,
                cur_doc,
            } => {
                let d = convert::vec2_to_core(cur_doc - start_doc);
                let rect = resize_rect(start_rect, handle, d);
                if (rect.width() - start_rect.width()).abs() > f64::EPSILON
                    || (rect.height() - start_rect.height()).abs() > f64::EPSILON
                    || (rect.x0 - start_rect.x0).abs() > f64::EPSILON
                    || (rect.y0 - start_rect.y0).abs() > f64::EPSILON
                {
                    let _ = self.editor.execute(Command::ResizeArtboard { id, rect });
                    self.request_main_redraw();
                }
            }
            Drag::Scale {
                start_xf, preview, ..
            }
            | Drag::Rotate {
                start_xf, preview, ..
            } => {
                if preview != start_xf {
                    let items = preview
                        .into_iter()
                        .map(|(id, a)| (id, convert::affine_to_core(a)))
                        .collect();
                    let _ = self.editor.execute(Command::SetTransforms { items });
                    self.request_main_redraw();
                }
            }
            Drag::Marquee { start } => {
                let r_screen = Rect::from_points(start, self.pointer);
                let r_doc = self
                    .view
                    .to_screen()
                    .inverse()
                    .transform_rect_bbox(r_screen);
                let hits = select::within(self.editor.document(), r_doc);
                if self.shift_down {
                    for id in hits {
                        if !self.selection.contains(&id) {
                            self.selection.push(id);
                        }
                    }
                } else {
                    self.selection = hits;
                }
                self.marquee = None;
                self.request_main_redraw();
            }
            Drag::MoveAnchors {
                start_doc,
                last_doc,
                moved,
            } => {
                if moved && !self.anchor_sel.is_empty() {
                    let delta = convert::vec2_to_core(last_doc - start_doc);
                    let _ = self.editor.execute(Command::MoveAnchors {
                        anchors: self.anchor_sel.clone(),
                        delta,
                    });
                    self.request_main_redraw();
                }
            }
            Drag::AnchorMarquee { start, candidate } => {
                let moved = (self.pointer - start).hypot() > 3.0;
                if moved {
                    // A real drag: rubber-band every node inside the box,
                    // across all paths — Illustrator's white-arrow marquee
                    // reaches objects that weren't selected first. The
                    // objects it catches then show their contour + nodes.
                    let r_doc = self
                        .view
                        .to_screen()
                        .inverse()
                        .transform_rect_bbox(Rect::from_points(start, self.pointer));
                    let hits = anchors::within(self.editor.document(), r_doc);
                    if self.shift_down {
                        for a in hits {
                            if !self.anchor_sel.contains(&a) {
                                self.anchor_sel.push(a);
                            }
                        }
                    } else {
                        self.anchor_sel = hits;
                    }
                } else if let Some(id) = candidate {
                    // A click on an object: select it, revealing its nodes.
                    if self.shift_down {
                        if !self.selection.contains(&id) {
                            self.selection.push(id);
                        }
                    } else {
                        self.selection = vec![id];
                    }
                    self.anchor_sel.clear();
                } else if !self.shift_down {
                    // A click on empty canvas: clear everything.
                    self.selection.clear();
                    self.anchor_sel.clear();
                }
                self.marquee = None;
                self.request_main_redraw();
            }
            Drag::PendingTearoff {
                side, path, tab, ..
            } => {
                self.dock.rail_mut(side).activate_tab(&path, tab);
                self.request_main_redraw();
            }
            Drag::PendingFloatMove { id, tab, .. } => {
                if let Some(f) = self.dock.floating_mut(id) {
                    if let Node::Tabs { active, panels } = &mut f.node {
                        *active = tab.min(panels.len().saturating_sub(1));
                    }
                }
                if let Some(w) = self.floating_window(id) {
                    w.request_redraw();
                }
            }
            Drag::MovingFloating { id, grab, pos } => {
                let global_cursor = pos + grab;
                let (side, target) = self.resolve_redock(global_cursor);
                if matches!(target, DropTarget::Float) {
                    if let Some(f) = self.dock.floating_mut(id) {
                        f.rect = [pos.x as f32, pos.y as f32, FLOAT_W as f32, FLOAT_H as f32];
                    }
                    if let Some(w) = self.floating_window(id) {
                        w.request_redraw();
                    }
                } else {
                    self.dock.redock(id, side, target);
                    if let Some((wid, _)) = self
                        .hosts
                        .iter()
                        .find(|(_, h)| matches!(h.role, Role::Floating(f) if f == id))
                    {
                        let wid = *wid;
                        self.hosts.remove(&wid); // Arc<Window> drops -> closes
                    }
                }
                self.redock_preview = None;
                self.request_main_redraw();
            }
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
        layout::layout(&node, Rect::new(0.0, 0.0, wl, hl), &theme, &mut |p| {
            self.text.measure(&tab_label(p), 12.0) + theme.tab_pad_x * 2.0
        })
    }

    fn redraw(&mut self, id: WindowId) {
        let Some(host) = self.hosts.get_mut(&id) else {
            return;
        };
        let width = host.surface.config.width;
        let height = host.surface.config.height;
        let scale = self.scale;
        let (wl, hl) = (width as f64 / scale, height as f64 / scale);
        let role = host.role;

        let preview = match &self.drag {
            Drag::MoveObjects {
                start_doc,
                last_doc,
                moved: true,
            } => Some(DragPreview {
                ids: &self.selection,
                delta: if self.shift_down {
                    snap8(*last_doc - *start_doc)
                } else {
                    *last_doc - *start_doc
                },
                dup: self.alt_down,
                xf: None,
                anchors: None,
            }),
            Drag::Scale { preview, .. } | Drag::Rotate { preview, .. } => Some(DragPreview {
                ids: &self.selection,
                delta: Vec2::ZERO,
                dup: false,
                xf: Some(preview),
                anchors: None,
            }),
            Drag::MoveAnchors {
                start_doc,
                last_doc,
                moved: true,
            } => Some(DragPreview {
                ids: &[],
                delta: Vec2::ZERO,
                dup: false,
                xf: None,
                anchors: Some((
                    self.anchor_sel.as_slice(),
                    convert::vec2_to_core(*last_doc - *start_doc),
                )),
            }),
            _ => None,
        };
        let draw_shape = match &self.drag {
            Drag::DrawShape {
                tool,
                start_doc,
                cur_doc,
            } => Some((
                *tool,
                convert::rect(shape_rect(
                    *start_doc,
                    *cur_doc,
                    self.shift_down,
                    self.alt_down,
                )),
            )),
            _ => None,
        };
        // Live outline for the Artboard tool (new-artboard rubber-band, or
        // a ghost of an artboard being dragged). Document-space Rect;
        // canvas applies the view transform.
        let artboard_ghost: Option<Rect> = match &self.drag {
            Drag::DrawArtboard { start_doc, cur_doc } => Some(convert::rect(shape_rect(
                *start_doc,
                *cur_doc,
                self.shift_down,
                self.alt_down,
            ))),
            Drag::MoveArtboard {
                id,
                start_doc,
                last_doc,
            } => {
                let mut d = *last_doc - *start_doc;
                if self.shift_down {
                    d = snap8(d);
                }
                self.editor
                    .document()
                    .artboards()
                    .iter()
                    .find(|a| a.id == *id)
                    .map(|a| convert::rect(a.rect) + d)
            }
            Drag::ResizeArtboard {
                handle,
                start_rect,
                start_doc,
                cur_doc,
                ..
            } => Some(convert::rect(resize_rect(
                *start_rect,
                *handle,
                convert::vec2_to_core(*cur_doc - *start_doc),
            ))),
            _ => None,
        };
        // Resize handles around the selected artboard (or its live rect
        // mid-drag), screen-space. Only while the Artboard tool is active.
        let artboard_handles: Option<[Point; 4]> = self
            .selected_artboard
            .filter(|_| self.active_tool == Tool::Artboard)
            .and_then(|id| {
                let committed = self
                    .editor
                    .document()
                    .artboards()
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| convert::rect(a.rect))?;
                // A Move/Resize drag of this artboard drives the ghost rect.
                let rect = match &self.drag {
                    Drag::MoveArtboard { .. } | Drag::ResizeArtboard { .. } => {
                        artboard_ghost.unwrap_or(committed)
                    }
                    _ => committed,
                };
                let vt = self.view.to_screen();
                Some(handles::rect_quad(rect).map(|p| vt * p))
            });
        let pen_preview = if self.active_tool == Tool::Pen && !self.pen.is_empty() {
            let hover = self.doc_point(self.pointer);
            let near_close = self.pen.len() >= 3
                && self
                    .pen
                    .first()
                    .is_some_and(|f| (*f - hover).hypot() <= 8.0 / self.view.zoom);
            Some(PenPreview {
                anchors: &self.pen,
                hover,
                near_close,
            })
        } else {
            None
        };
        // Direct Selection shows anchors only for objects that have been
        // selected (Illustrator's white arrow), not every path.
        let direct = self.effective_tool() == Tool::DirectSelect;
        let anchor_paths: Vec<ObjectId> = if direct {
            self.node_paths()
        } else {
            Vec::new()
        };
        let anchor_view = direct.then_some(AnchorView {
            selected: &self.anchor_sel,
            paths: &anchor_paths,
        });

        self.content.reset();
        let representative = self.representative();
        // App-bar status: a file error wins, else the current file name.
        let status_text: Option<String> = self.io_error.clone().or_else(|| {
            self.file_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        });
        let tab_labels: Vec<String> = (0..self.tabs.len()).map(|i| self.tab_label(i)).collect();
        let active_tab = self.active;
        let cursor_glyph = (self.cursor_mode == CanvasCursor::Glyph).then(|| {
            let pen_closing = self.active_tool == Tool::Pen && self.pen.len() >= 3 && {
                let hover = self.doc_point(self.pointer);
                self.pen
                    .first()
                    .is_some_and(|f| (*f - hover).hypot() <= 8.0 / self.view.zoom)
            };
            (self.effective_tool(), pen_closing)
        });
        match role {
            Role::Main => paint_main(
                &mut self.content,
                &mut self.text,
                &self.dock,
                self.editor.document(),
                &self.view,
                &self.theme,
                &self.selection,
                self.active_tool,
                self.active_slot,
                representative,
                self.pointer,
                preview,
                draw_shape,
                artboard_ghost,
                artboard_handles,
                pen_preview,
                anchor_view,
                self.marquee,
                wl,
                hl,
                self.redock_preview.as_ref(),
                status_text.as_deref(),
                &self.expanded_groups,
                self.stroke_w,
                self.opacity,
                self.rename.as_ref().map(|r| (r.target, r.buf.as_str())),
                self.selected_layer,
                self.selected_artboard,
                self.newdoc.as_ref(),
                &tab_labels,
                active_tab,
                cursor_glyph,
            ),
            Role::Floating(fid) => {
                if let Some(f) = self.dock.floating(fid) {
                    let node = f.node.clone();
                    paint_floating(
                        &mut self.content,
                        &mut self.text,
                        &node,
                        &self.theme,
                        wl,
                        hl,
                    );
                }
            }
        }
        if matches!(role, Role::Main) {
            if let Some(pk) = self.picker {
                picker::paint(
                    &mut self.content,
                    &pk,
                    self.theme.text,
                    &self.theme,
                    &mut self.text,
                );
            }
        }
        self.scene.reset();
        self.scene.append(&self.content, Some(Affine::scale(scale)));

        let host = self.hosts.get_mut(&id).unwrap();
        let device = &self.context.devices[host.surface.dev_id];
        let surface_texture = match host.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };
        host.renderer
            .render_to_texture(
                &device.device,
                &device.queue,
                &self.scene,
                &host.surface.target_view,
                &vello::RenderParams {
                    base_color: palette::css::BLACK,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("render");
        let mut encoder = device
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surface blit"),
            });
        host.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &host.surface.target_view,
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        surface_texture.present();

        // Keep the window pumping frames. Cheap under AutoVsync and it
        // sidesteps a class of "first RedrawRequested was dropped" bugs
        // where the window opens blank until an event happens to arrive.
        host.window.request_redraw();
    }
}

impl ApplicationHandler for App {
    /// Fires every loop iteration. Requesting redraws here guarantees the
    /// first frame paints even if the post-`resumed` `request_redraw` was
    /// dropped; once running it just tops up the vsync-throttled loop.
    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        #[cfg(target_os = "macos")]
        {
            let actions = self
                .native_menu
                .as_ref()
                .map(NativeMenu::drain)
                .unwrap_or_default();
            for action in actions {
                self.run_menu_action(action);
            }
        }
        // Modal / picker open+close changes whether the OS cursor hides.
        self.update_canvas_cursor();
        for host in self.hosts.values() {
            host.window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main_id.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Amalith Ver. Alpha")
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
        self.scale = window.scale_factor();
        let wid = window.id();
        let host = self.make_host(window, Role::Main);
        self.hosts.insert(wid, host);
        self.main_id = Some(wid);
        #[cfg(target_os = "macos")]
        {
            self.native_menu = Some(NativeMenu::build());
        }
        self.hosts[&wid].window.request_redraw();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
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
                if let Some(host) = self.hosts.get_mut(&id) {
                    self.context.resize_surface(
                        &mut host.surface,
                        size.width.max(1),
                        size.height.max(1),
                    );
                    host.window.request_redraw();
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
                let double = self.last_click.is_some_and(|(t, p)| {
                    now.duration_since(t).as_millis() < 400 && (self.pointer - p).hypot() < 5.0
                });
                self.last_click = Some((now, self.pointer));
                self.on_press(id, double);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.on_release(),
            WindowEvent::ModifiersChanged(m) => {
                if Some(id) == self.main_id {
                    let was_direct = self.effective_tool() == Tool::DirectSelect;
                    self.cmd_down = m.state().super_key();
                    self.shift_down = m.state().shift_key();
                    self.alt_down = m.state().alt_key();
                    // Toggling the temporary white-arrow gesture shows or
                    // hides the node overlay; a live drag reacts to
                    // Shift-lock / Alt-copy changing under it.
                    if (self.effective_tool() == Tool::DirectSelect) != was_direct
                        || !matches!(self.drag, Drag::None)
                    {
                        self.request_main_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } if Some(id) == self.main_id => {
                // The New Document modal, then an inline rename, each
                // swallow all keyboard input while active.
                if self.newdoc.is_some() {
                    self.newdoc_key(&event);
                    return;
                }
                if self.rename.is_some() {
                    self.rename_key(&event);
                    return;
                }
                let pressed = event.state.is_pressed();
                // Any key other than ⌘Z ends the pen re-open window.
                if pressed && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyZ)) {
                    self.last_pen = None;
                }
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Space) => self.space_down = pressed,
                    PhysicalKey::Code(KeyCode::KeyZ) if pressed && self.cmd_down => {
                        let redo = self.shift_down;
                        if redo && self.active_tool == Tool::Pen && !self.pen_redo.is_empty() {
                            if let Some(p) = self.pen_redo.pop() {
                                self.pen.push(p);
                            }
                            self.request_main_redraw();
                        } else if !redo && self.pen_undo_step() {
                            // handled
                        } else {
                            let _ = if redo {
                                self.editor.redo()
                            } else {
                                self.editor.undo()
                            };
                            self.prune_selection();
                            self.request_main_redraw();
                        }
                    }
                    PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                        if pressed && !self.selection.is_empty() =>
                    {
                        let _ = self.editor.execute(Command::DeleteObjects {
                            ids: std::mem::take(&mut self.selection),
                        });
                        self.request_main_redraw();
                    }
                    // ⌘ shortcuts (copy / paste / duplicate / group / all).
                    PhysicalKey::Code(code) if pressed && self.cmd_down => match code {
                        KeyCode::KeyC => {
                            let _ = self.editor.copy(&self.selection);
                        }
                        KeyCode::KeyX if !self.selection.is_empty() => {
                            let _ = self.editor.copy(&self.selection);
                            let _ = self.editor.execute(Command::DeleteObjects {
                                ids: std::mem::take(&mut self.selection),
                            });
                            self.request_main_redraw();
                        }
                        KeyCode::KeyV if self.editor.has_clipboard() => {
                            if let Ok(ids) = self
                                .editor
                                .paste(amalith_core::Vec2::new(16.0, 16.0), PasteStack::Top)
                            {
                                self.selection = ids;
                            }
                            self.request_main_redraw();
                        }
                        // Paste in Front / in Back: same position as the
                        // source, stacked just above / below it.
                        KeyCode::KeyF if self.editor.has_clipboard() => {
                            if let Ok(ids) = self
                                .editor
                                .paste(amalith_core::Vec2::ZERO, PasteStack::InFront)
                            {
                                self.selection = ids;
                            }
                            self.request_main_redraw();
                        }
                        KeyCode::KeyB if self.editor.has_clipboard() => {
                            if let Ok(ids) = self
                                .editor
                                .paste(amalith_core::Vec2::ZERO, PasteStack::Behind)
                            {
                                self.selection = ids;
                            }
                            self.request_main_redraw();
                        }
                        KeyCode::KeyD if !self.selection.is_empty() => {
                            if let Ok(ids) = self.editor.duplicate_objects(
                                &self.selection,
                                amalith_core::Vec2::new(16.0, 16.0),
                            ) {
                                self.selection = ids;
                            }
                            self.request_main_redraw();
                        }
                        KeyCode::KeyG if self.shift_down => {
                            if let Ok(freed) = self.editor.ungroup(&self.selection) {
                                if !freed.is_empty() {
                                    self.selection = freed;
                                }
                            }
                            self.request_main_redraw();
                        }
                        KeyCode::KeyG if self.selection.len() > 1 => {
                            if let Ok(CommandOutcome::Object(g)) =
                                self.editor.execute(Command::Group {
                                    ids: self.selection.clone(),
                                    name: None,
                                })
                            {
                                self.selection = vec![g];
                            }
                            self.request_main_redraw();
                        }
                        KeyCode::KeyA => self.select_all(),
                        // File I/O: open, save, save-as, import SVG.
                        KeyCode::KeyN => self.open_new_doc(),
                        KeyCode::KeyO => self.open_document(),
                        KeyCode::KeyS => self.save_document(self.shift_down),
                        KeyCode::KeyI if self.shift_down => self.import_svg(),
                        // Z-order: ⌘] / ⌘[ step one, ⌘⌥] / ⌘⌥[ to the ends.
                        KeyCode::BracketRight => {
                            if self.alt_down {
                                self.restack_extreme(true);
                            } else {
                                self.restack(1);
                            }
                        }
                        KeyCode::BracketLeft => {
                            if self.alt_down {
                                self.restack_extreme(false);
                            } else {
                                self.restack(-1);
                            }
                        }
                        _ => {}
                    },
                    // Bare-key: arrow nudge, Escape, tool shortcuts.
                    PhysicalKey::Code(code) if pressed && !self.cmd_down && !self.alt_down => {
                        match code {
                            KeyCode::ArrowLeft => self.nudge(-1.0, 0.0),
                            KeyCode::ArrowRight => self.nudge(1.0, 0.0),
                            KeyCode::ArrowUp => self.nudge(0.0, -1.0),
                            KeyCode::ArrowDown => self.nudge(0.0, 1.0),
                            KeyCode::Enter | KeyCode::NumpadEnter if !self.pen.is_empty() => {
                                self.commit_pen(false);
                            }
                            KeyCode::Escape => {
                                if self.picker.take().is_some() {
                                    // just dismissed the picker
                                } else if self.active_tool == Tool::Artboard {
                                    // Exit the Artboard tool back to the
                                    // tool that was active before it.
                                    self.set_tool(self.pre_artboard_tool);
                                } else {
                                    self.pen.clear();
                                    self.pen_redo.clear();
                                    self.anchor_sel.clear();
                                    self.selection.clear();
                                }
                                self.request_main_redraw();
                            }
                            KeyCode::KeyV => self.set_tool(Tool::Select),
                            KeyCode::KeyA => self.set_tool(Tool::DirectSelect),
                            KeyCode::KeyP => self.set_tool(Tool::Pen),
                            KeyCode::KeyM => self.set_tool(Tool::Rectangle),
                            KeyCode::KeyL => self.set_tool(Tool::Ellipse),
                            KeyCode::KeyO if self.shift_down => self.set_tool(Tool::Artboard),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::PinchGesture { delta, .. } if Some(id) == self.main_id => {
                self.view.zoom_at(1.0 + delta, self.pointer);
            }
            WindowEvent::MouseWheel { delta, .. } if Some(id) == self.main_id => {
                let (dx, dy) = match delta {
                    // Line-based (mouse wheel): each notch ≈ 30 logical px.
                    MouseScrollDelta::LineDelta(x, y) => (x as f64 * 30.0, y as f64 * 30.0),
                    // Pixel-based (trackpad): physical px → logical.
                    MouseScrollDelta::PixelDelta(p) => (p.x / self.scale, p.y / self.scale),
                };
                // The New Document modal scrolls its content.
                if let Some(form) = &mut self.newdoc {
                    form.scroll = (form.scroll - dy).max(0.0);
                    self.request_main_redraw();
                    return;
                }
                // Scrolling over a Weight / Opacity field nudges it.
                if self.picker.is_none()
                    && self.pointer.y >= APP_BAR_H
                    && self.pointer.y < APP_BAR_H + OPT_BAR_H
                    && dy.abs() > 0.5
                {
                    let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
                    let ob = opt_bar_layout(opt_bar_rect(w));
                    let dir = if dy > 0.0 { 1 } else { -1 };
                    if ob.weight_field.contains(self.pointer) {
                        self.step_weight(dir);
                        return;
                    }
                    if ob.opacity_field.contains(self.pointer) {
                        self.step_opacity(dir);
                        return;
                    }
                }
                if self.cmd_down {
                    // ⌘ + scroll → zoom at the cursor.
                    let factor = 2f64.powf(dy / 180.0);
                    self.view.zoom_at(factor, self.pointer);
                } else {
                    // Plain scroll → pan.
                    self.view.pan += Vec2::new(dx, dy);
                }
            }
            WindowEvent::RedrawRequested => self.redraw(id),
            _ => {}
        }
    }
}

/// Right rail: Layers over an Artboards/Swatches tab group.
fn demo_right_dock() -> Node {
    Node::Split {
        axis: Axis::Vertical,
        children: vec![
            Child {
                node: Node::Tabs {
                    panels: vec![PanelId("layers")],
                    active: 0,
                },
                weight: 1.6,
            },
            Child {
                node: Node::Tabs {
                    panels: vec![PanelId("artboards"), PanelId("swatches")],
                    active: 0,
                },
                weight: 1.0,
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

/// Fractional (0..1) hotspot of a tool's cursor glyph within its box —
/// where the actual click point sits.
fn cursor_hotspot(t: Tool) -> (f64, f64) {
    match t {
        Tool::Select | Tool::DirectSelect => (0.32, 0.13),
        Tool::Pen => (0.29, 0.08),
        _ => (0.5, 0.5),
    }
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

fn tab_label(panel: PanelId) -> String {
    match panel.0 {
        "tools" => "Tools",
        "layers" => "Layers",
        "artboards" => "Artboards",
        "swatches" => "Swatches",
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
        Some(tree) => layout::layout(tree, rect, theme, &mut |p| {
            text.measure(&tab_label(p), 12.0) + theme.tab_pad_x * 2.0
        }),
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
        let tw = text.measure(label, 12.0);
        let w = tw + 18.0 /* × */ + 22.0 /* padding */;
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

/// Interactive rects along the options bar. `label_*` are where the
/// preceding text label starts, so paint and hit stay in lockstep.
struct OptBar {
    label_fill: f64,
    fill: Rect,
    label_stroke: f64,
    stroke: Rect,
    label_weight: f64,
    weight_field: Rect,
    weight_up: Rect,
    weight_down: Rect,
    label_opacity: f64,
    opacity_field: Rect,
    opacity_up: Rect,
    opacity_down: Rect,
}

fn opt_bar_layout(bar: Rect) -> OptBar {
    let cy = bar.y0 + bar.height() * 0.5;
    let chip = |cx: f64| Rect::from_center_size(Point::new(cx, cy), (18.0, 18.0));
    let field = |x: f64, w: f64| Rect::new(x, cy - 10.0, x + w, cy + 10.0);

    let mut x = bar.x0 + 12.0;
    x += 82.0 + 14.0; // "No Selection" + separator gap

    let label_fill = x;
    x += 28.0;
    let fill = chip(x + 9.0);
    x += 18.0 + 8.0 + 12.0 + 16.0; // chip + gap + indicator square + gap

    let label_stroke = x;
    x += 42.0;
    let stroke = chip(x + 9.0);
    x += 18.0 + 8.0 + 12.0 + 18.0;

    let label_weight = x;
    x += 46.0;
    let weight_field = field(x, 56.0);
    x += 56.0;
    let weight_up = Rect::new(x, cy - 10.0, x + 13.0, cy);
    let weight_down = Rect::new(x, cy, x + 13.0, cy + 10.0);
    x += 13.0 + 20.0;

    let label_opacity = x;
    x += 52.0;
    let opacity_field = field(x, 46.0);
    x += 46.0;
    let opacity_up = Rect::new(x, cy - 10.0, x + 13.0, cy);
    let opacity_down = Rect::new(x, cy, x + 13.0, cy + 10.0);

    OptBar {
        label_fill,
        fill,
        label_stroke,
        stroke,
        label_weight,
        weight_field,
        weight_up,
        weight_down,
        label_opacity,
        opacity_field,
        opacity_up,
        opacity_down,
    }
}

/// A boxed numeric readout plus an up / down stepper column, matching
/// Illustrator's control bar.
fn draw_opt_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    field: Rect,
    up: Rect,
    down: Rect,
    value: &str,
) {
    let border = theme.text_dim.with_alpha(0.5);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &field);
    text.draw(
        scene,
        value,
        11.5,
        theme.text,
        field.x0 + 6.0,
        field.y0 + field.height() * 0.5 + 4.0,
    );
    let stepper = Rect::new(up.x0, up.y0, up.x1, down.y1);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &stepper);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &stepper);
    let cx = stepper.x0 + stepper.width() * 0.5;
    let mut tri = BezPath::new();
    tri.move_to((cx - 3.0, up.y1 - 3.0));
    tri.line_to((cx + 3.0, up.y1 - 3.0));
    tri.line_to((cx, up.y0 + 3.0));
    tri.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &tri);
    let mut tri = BezPath::new();
    tri.move_to((cx - 3.0, down.y0 + 3.0));
    tri.line_to((cx + 3.0, down.y0 + 3.0));
    tri.line_to((cx, down.y1 - 3.0));
    tri.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &tri);
}

#[allow(clippy::too_many_arguments)]
fn paint_options_bar(
    scene: &mut Scene,
    text: &mut TextContext,
    bar: Rect,
    theme: &Theme,
    representative: Option<amalith_core::Appearance>,
    active_slot: panels::PaintSlot,
    selection_count: usize,
    cur_weight: f64,
    cur_opacity: f32,
) {
    scene.fill(Fill::NonZero, ID, theme.strip_active, None, &bar);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(bar.x0, bar.y1 - 1.0, bar.x1, bar.y1),
    );
    let baseline = bar.y0 + bar.height() * 0.5 + 4.0;
    let ob = opt_bar_layout(bar);

    // Selection status.
    let status = match selection_count {
        0 => "No Selection".to_string(),
        1 => "1 Selected".to_string(),
        n => format!("{n} Selected"),
    };
    text.draw(scene, &status, 11.5, theme.text_dim, bar.x0 + 12.0, baseline);

    let sep = |scene: &mut Scene, x: f64| {
        scene.fill(
            Fill::NonZero,
            ID,
            theme.border,
            None,
            &Rect::new(x, bar.y0 + 7.0, x + 1.0, bar.y1 - 7.0),
        );
    };
    sep(scene, ob.label_fill - 12.0);

    // Fill / Stroke.
    let indicator = |scene: &mut Scene, chip: Rect| {
        let s = Rect::from_center_size(
            Point::new(chip.x1 + 12.0, chip.center().y),
            (11.0, 11.0),
        );
        scene.stroke(&Stroke::new(1.0), ID, theme.text_dim, None, &s);
    };
    text.draw(scene, "Fill", 11.5, theme.text_dim, ob.label_fill, baseline);
    panels::draw_paint_swatch(
        scene,
        theme,
        ob.fill,
        representative
            .map(|a| a.fill)
            .unwrap_or(amalith_core::Paint::Solid(amalith_core::Color::rgb(0.87, 0.87, 0.87))),
        active_slot == panels::PaintSlot::Fill,
    );
    indicator(scene, ob.fill);
    text.draw(
        scene,
        "Stroke",
        11.5,
        theme.text_dim,
        ob.label_stroke,
        baseline,
    );
    panels::draw_paint_swatch(
        scene,
        theme,
        ob.stroke,
        representative
            .map(|a| a.stroke)
            .unwrap_or(amalith_core::Paint::None),
        active_slot == panels::PaintSlot::Stroke,
    );
    indicator(scene, ob.stroke);
    sep(scene, ob.label_weight - 12.0);

    // Weight.
    let w = representative.map(|a| a.stroke_width).unwrap_or(cur_weight);
    text.draw(
        scene,
        "Weight",
        11.5,
        theme.text_dim,
        ob.label_weight,
        baseline,
    );
    draw_opt_field(
        scene,
        text,
        theme,
        ob.weight_field,
        ob.weight_up,
        ob.weight_down,
        &format!("{w:.1} px"),
    );
    sep(scene, ob.label_opacity - 12.0);

    // Opacity.
    let op = representative.map(|a| a.opacity).unwrap_or(cur_opacity);
    text.draw(
        scene,
        "Opacity",
        11.5,
        theme.text_dim,
        ob.label_opacity,
        baseline,
    );
    draw_opt_field(
        scene,
        text,
        theme,
        ob.opacity_field,
        ob.opacity_up,
        ob.opacity_down,
        &format!("{:.0}%", op * 100.0),
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_main(
    scene: &mut Scene,
    text: &mut TextContext,
    dock: &DockModel,
    doc: &Document,
    view: &CanvasView,
    theme: &Theme,
    selection: &[ObjectId],
    active_tool: Tool,
    active_slot: panels::PaintSlot,
    representative: Option<amalith_core::Appearance>,
    pointer: Point,
    drag_preview: Option<DragPreview<'_>>,
    draw_shape: Option<(Tool, Rect)>,
    artboard_ghost: Option<Rect>,
    artboard_handles: Option<[Point; 4]>,
    pen_preview: Option<PenPreview<'_>>,
    anchor_view: Option<AnchorView<'_>>,
    marquee: Option<Rect>,
    width: f64,
    height: f64,
    redock_preview: Option<&(RailSide, DropTarget)>,
    status: Option<&str>,
    expanded: &std::collections::HashSet<ObjectId>,
    cur_weight: f64,
    cur_opacity: f32,
    renaming: Option<(panels::RenameId, &str)>,
    selected_layer: Option<LayerId>,
    selected_artboard: Option<ArtboardId>,
    newdoc_form: Option<&newdoc::NewDocForm>,
    tab_labels: &[String],
    active_tab: usize,
    cursor_glyph: Option<(Tool, bool)>,
) {
    scene.fill(
        Fill::NonZero,
        ID,
        theme.bg,
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    // Canvas fills the gap between whatever rails are present.
    let left_x = if dock.left.is_empty() {
        0.0
    } else {
        rail_rect_for(RailSide::Left, dock.left.width as f64, width, height).x1
    };
    let right_x = if dock.right.is_empty() {
        width
    } else {
        rail_rect_for(RailSide::Right, dock.right.width as f64, width, height).x0
    };
    let viewport = Rect::new(left_x, CHROME_TOP, right_x.max(left_x), height);
    canvas::paint(
        scene,
        doc,
        view,
        viewport,
        theme,
        text,
        selection,
        drag_preview,
        draw_shape,
        artboard_ghost,
        artboard_handles,
        active_tool == Tool::Artboard,
        pen_preview,
        anchor_view,
    );

    if let Some(m) = marquee {
        scene.fill(Fill::NonZero, ID, theme.marquee_fill, None, &m);
        scene.stroke(&Stroke::new(1.0), ID, theme.select_blue, None, &m);
    }

    // Document-tab strip (canvas x-span, between options bar and canvas).
    let tab_strip = tab_bar_rect(left_x, right_x);
    scene.fill(Fill::NonZero, ID, theme.app_bar, None, &tab_strip);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(tab_strip.x0, tab_strip.y1 - 1.0, tab_strip.x1, tab_strip.y1),
    );
    for (i, (whole, close)) in layout_tabs(text, tab_labels, tab_strip).into_iter().enumerate() {
        let is_active = i == active_tab;
        if is_active {
            scene.fill(Fill::NonZero, ID, theme.strip_active, None, &whole);
            scene.fill(
                Fill::NonZero,
                ID,
                theme.select_blue,
                None,
                &Rect::new(whole.x0, whole.y1 - 2.0, whole.x1, whole.y1),
            );
        }
        // Close ×.
        let xc = close.center();
        let cc = if is_active { theme.text } else { theme.text_dim };
        let mut xg = BezPath::new();
        xg.move_to((xc.x - 4.0, xc.y - 4.0));
        xg.line_to((xc.x + 4.0, xc.y + 4.0));
        xg.move_to((xc.x + 4.0, xc.y - 4.0));
        xg.line_to((xc.x - 4.0, xc.y + 4.0));
        scene.stroke(&Stroke::new(1.3), ID, cc, None, &xg);
        text.draw(
            scene,
            &tab_labels[i],
            12.0,
            if is_active { theme.text } else { theme.text_dim },
            close.x1 + 6.0,
            tab_strip.y0 + TAB_BAR_H * 0.5 + 4.0,
        );
        // Divider between tabs.
        if i + 1 < tab_labels.len() {
            scene.fill(
                Fill::NonZero,
                ID,
                theme.border,
                None,
                &Rect::new(whole.x1, tab_strip.y0 + 5.0, whole.x1 + 1.0, tab_strip.y1 - 5.0),
            );
        }
    }

    for side in [RailSide::Left, RailSide::Right] {
        let rail = dock.rail(side);
        let is_preview_target = redock_preview.is_some_and(|(s, _)| *s == side);
        if rail.is_empty() && !is_preview_target {
            continue;
        }
        let rect = rail_rect_for(side, rail.width as f64, width, height);
        let laid = build_rail_layout(rail, theme, text, rect);
        if !rail.is_empty() {
            chrome::paint(scene, &laid, theme, text, &tab_label);
            let ctx = panels::Ctx {
                theme,
                doc,
                selection,
                active_tool,
                pointer,
                representative,
                active_slot,
                expanded,
                renaming,
                selected_layer,
                selected_artboard,
            };
            for area in &laid.areas {
                if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                    panels::paint(scene, text, pid, area.body, &ctx);
                }
            }
            // Bar on the canvas-facing edge — the whole-rail resize handle.
            scene.fill(
                Fill::NonZero,
                ID,
                theme.splitter,
                None,
                &rail_edge_bar(side, rect),
            );
        }
        if let Some((_, target)) = redock_preview.filter(|(s, _)| *s == side) {
            chrome::paint_drop(scene, target, &laid, rect, theme);
        }
    }

    // Options / context bar: full width, above the rails.
    paint_options_bar(
        scene,
        text,
        opt_bar_rect(width),
        theme,
        representative,
        active_slot,
        selection.len(),
        cur_weight,
        cur_opacity,
    );

    // The active tool's on-document glyph, standing in for the OS cursor.
    if let Some((tool, pen_closing)) = cursor_glyph {
        let sz = 30.0;
        let (hx, hy) = cursor_hotspot(tool);
        let x0 = pointer.x - sz * hx;
        let y0 = pointer.y - sz * hy;
        let box_ = Rect::new(x0, y0, x0 + sz, y0 + sz);
        let src = match tool {
            Tool::DirectSelect => icons::CURSOR_DIRECT_SELECT_SVG,
            Tool::Pen if pen_closing => icons::CURSOR_PEN_CLOSING_SVG,
            Tool::Pen => icons::CURSOR_PEN_DRAWING_SVG,
            _ => icons::CURSOR_SELECT_SVG,
        };
        icons::draw_cursor(scene, src, box_);
    }

    // Top app bar (drawn last so nothing bleeds over it). macOS keeps the
    // traffic lights floating over its left end.
    let bar = Rect::new(0.0, 0.0, width, APP_BAR_H);
    scene.fill(Fill::NonZero, ID, theme.app_bar, None, &bar);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(0.0, APP_BAR_H - 1.0, width, APP_BAR_H),
    );
    let name = "Amalith Ver. Alpha";
    let tw = text.measure(name, 12.5);
    text.draw(
        scene,
        name,
        12.5,
        Color::from_rgb8(0xcd, 0xcd, 0xcd),
        (width - tw) * 0.5,
        APP_BAR_H * 0.5 + 4.5,
    );
    if let Some(status) = status {
        let sw = text.measure(status, 11.5);
        text.draw(
            scene,
            status,
            11.5,
            Color::from_rgb8(0x9a, 0x9a, 0x9a),
            width - sw - 12.0,
            APP_BAR_H * 0.5 + 4.0,
        );
    }

    // The New Document modal sits over everything.
    if let Some(form) = newdoc_form {
        newdoc::paint(scene, text, theme, Rect::new(0.0, 0.0, width, height), form);
    }
}

/// A torn-off window: dark body, a header strip that always names the
/// panel (so a floating window is never an anonymous square), an accent
/// line, and a 1px frame since the OS chrome is gone.
fn paint_floating(
    scene: &mut Scene,
    text: &mut TextContext,
    node: &Node,
    theme: &Theme,
    width: f64,
    height: f64,
) {
    let full = Rect::new(0.0, 0.0, width, height);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &full);

    let h = theme.tab_strip_h;
    scene.fill(
        Fill::NonZero,
        ID,
        theme.strip_bg,
        None,
        &Rect::new(0.0, 0.0, width, h),
    );
    scene.fill(
        Fill::NonZero,
        ID,
        theme.drop_line,
        None,
        &Rect::new(0.0, h - 2.0, width, h),
    );

    let baseline = h * 0.5 + 12.5 * 0.34;
    text.draw(
        scene,
        &floating_title(node),
        12.5,
        theme.text,
        theme.tab_pad_x,
        baseline,
    );

    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &full);
}

fn floating_title(node: &Node) -> String {
    match node {
        Node::Tabs { panels, active } => panels
            .get(*active)
            .or_else(|| panels.first())
            .map(|p| tab_label(*p))
            .unwrap_or_else(|| "Panel".to_string()),
        Node::Split { .. } => "Panel".to_string(),
    }
}

/// The macOS application menu bar. Items carry the same accelerators as
/// the in-app keyboard shortcuts; clicks arrive on `muda`'s global
/// channel, drained each loop in `about_to_wait`.
#[cfg(target_os = "macos")]
struct NativeMenu {
    items: Vec<(muda::MenuId, MenuAction)>,
    // Kept alive for the process; dropping it tears the menu down.
    _menu: muda::Menu,
}

#[cfg(target_os = "macos")]
impl NativeMenu {
    fn build() -> Self {
        use muda::{
            accelerator::{Accelerator, Code, Modifiers},
            Menu, MenuItem, PredefinedMenuItem, Submenu,
        };
        let sup = Some(Modifiers::SUPER);
        let sup_shift = Some(Modifiers::SUPER | Modifiers::SHIFT);
        let mk = |label: &str, mods, code| MenuItem::new(label, true, Some(Accelerator::new(mods, code)));

        let new_i = mk("New", sup, Code::KeyN);
        let open_i = mk("Open…", sup, Code::KeyO);
        let save_i = mk("Save", sup, Code::KeyS);
        let save_as_i = mk("Save As…", sup_shift, Code::KeyS);
        let import_i = mk("Import SVG…", sup_shift, Code::KeyI);
        let undo_i = mk("Undo", sup, Code::KeyZ);
        let redo_i = mk("Redo", sup_shift, Code::KeyZ);
        let cut_i = mk("Cut", sup, Code::KeyX);
        let copy_i = mk("Copy", sup, Code::KeyC);
        let paste_i = mk("Paste", sup, Code::KeyV);
        let dup_i = mk("Duplicate", sup, Code::KeyD);
        let all_i = mk("Select All", sup, Code::KeyA);
        let forward_i = mk("Bring Forward", sup, Code::BracketRight);
        let front_i = mk("Bring to Front", sup_shift, Code::BracketRight);
        let backward_i = mk("Send Backward", sup, Code::BracketLeft);
        let back_i = mk("Send to Back", sup_shift, Code::BracketLeft);

        let sep = PredefinedMenuItem::separator;
        let app = Submenu::with_items(
            "Amalith",
            true,
            &[&PredefinedMenuItem::quit(Some("Quit Amalith"))],
        )
        .expect("app menu");
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &new_i, &open_i, &sep(), &save_i, &save_as_i, &sep(), &import_i,
            ],
        )
        .expect("file menu");
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &undo_i, &redo_i, &sep(), &cut_i, &copy_i, &paste_i, &dup_i, &sep(), &all_i,
                &sep(), &forward_i, &front_i, &backward_i, &back_i,
            ],
        )
        .expect("edit menu");

        let menu = Menu::new();
        menu.append(&app).expect("append app menu");
        menu.append(&file).expect("append file menu");
        menu.append(&edit).expect("append edit menu");
        menu.init_for_nsapp();

        let items = vec![
            (new_i.id().clone(), MenuAction::New),
            (open_i.id().clone(), MenuAction::Open),
            (save_i.id().clone(), MenuAction::Save),
            (save_as_i.id().clone(), MenuAction::SaveAs),
            (import_i.id().clone(), MenuAction::ImportSvg),
            (undo_i.id().clone(), MenuAction::Undo),
            (redo_i.id().clone(), MenuAction::Redo),
            (cut_i.id().clone(), MenuAction::Cut),
            (copy_i.id().clone(), MenuAction::Copy),
            (paste_i.id().clone(), MenuAction::Paste),
            (dup_i.id().clone(), MenuAction::Duplicate),
            (all_i.id().clone(), MenuAction::SelectAll),
            (forward_i.id().clone(), MenuAction::BringForward),
            (front_i.id().clone(), MenuAction::BringToFront),
            (backward_i.id().clone(), MenuAction::SendBackward),
            (back_i.id().clone(), MenuAction::SendToBack),
        ];
        Self { items, _menu: menu }
    }

    /// Every menu click queued since the last call.
    fn drain(&self) -> Vec<MenuAction> {
        let mut out = Vec::new();
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            if let Some((_, action)) = self.items.iter().find(|(id, _)| *id == event.id) {
                out.push(*action);
            }
        }
        out
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
