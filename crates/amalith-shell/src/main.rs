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
use amalith_core::{Document, LayerId, ObjectId};
use amalith_shell::canvas::{self, CanvasView, DragPreview, PenPreview};
use amalith_shell::dock::{
    Axis, Child, DockModel, DropTarget, Node, NodePath, PanelId, Rail, RailSide, Side,
};
use amalith_shell::handles::{self, Handle};
use amalith_shell::layout::Layout;
use amalith_shell::text::TextContext;
use amalith_shell::tool::Tool;
use amalith_shell::{chrome, convert, layout, panels, picker, sample, select, Theme};
use vello::kurbo::{Affine, Point, Rect, Stroke, Vec2};
use vello::peniko::{color::palette, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

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
    /// Moving the current selection (or, with `dup`, alt-drag-duplicating
    /// it). Deltas are in document space.
    MoveObjects {
        start_doc: Point,
        last_doc: Point,
        moved: bool,
        dup: bool,
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
    editor: Editor,
    selection: Vec<ObjectId>,
    active_tool: Tool,
    /// Which paint slot the Swatches panel targets.
    active_slot: panels::PaintSlot,
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
            editor: Editor::new(sample::document()),
            selection: Vec::new(),
            active_tool: Tool::Select,
            active_slot: panels::PaintSlot::Fill,
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
            panels::Action::Select(id) => self.selection = vec![id],
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
        }
        self.request_main_redraw();
    }

    /// Appearance of the first selected object, for the Swatches panel.
    fn representative(&self) -> Option<amalith_core::Appearance> {
        self.selection
            .first()
            .and_then(|id| self.editor.document().object(*id))
            .map(|o| o.appearance)
    }

    /// Drop selection ids that no longer exist (after undo/redo/delete).
    fn prune_selection(&mut self) {
        let doc = self.editor.document();
        self.selection.retain(|id| doc.object(*id).is_some());
    }

    /// Move the selection by `(dx, dy) * step` document units as one
    /// undoable command (arrow-key nudge; Shift = ×10).
    fn nudge(&mut self, dx: f64, dy: f64) {
        if self.selection.is_empty() {
            return;
        }
        let step = if self.shift_down { 10.0 } else { 1.0 };
        let _ = self.editor.execute(Command::MoveObjects {
            objects: self.selection.clone(),
            delta: amalith_core::Vec2::new(dx * step, dy * step),
        });
        self.request_main_redraw();
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
        }
        self.request_main_redraw();
    }

    /// Switch tools, discarding any in-progress Pen path.
    fn set_tool(&mut self, t: Tool) {
        if t != Tool::Pen {
            self.pen.clear();
            self.pen_redo.clear();
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
    fn visible_doc_rect(&self) -> Rect {
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
        self.view
            .to_screen()
            .inverse()
            .transform_rect_bbox(Rect::new(left, 0.0, right.max(left), h))
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
        match role {
            Role::Main => {
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };

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
                let start_move = |dp: Point, dup: bool| Drag::MoveObjects {
                    start_doc: dp,
                    last_doc: dp,
                    moved: false,
                    dup,
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
                        self.drag = start_move(dp, self.alt_down);
                    }
                } else {
                    // Empty space: a press inside the selection box drags
                    // the selection; otherwise it's a marquee.
                    let inside_box = !self.shift_down
                        && select::union_bounds(doc, &self.selection)
                            .is_some_and(|b| b.contains(dp));
                    if inside_box {
                        self.drag = start_move(dp, self.alt_down);
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
            Drag::MoveObjects { start_doc, dup, .. } => {
                let (start_doc, dup) = (*start_doc, *dup);
                let dp = self.doc_point(self.pointer);
                self.drag = Drag::MoveObjects {
                    start_doc,
                    last_doc: dp,
                    moved: true,
                    dup,
                };
                self.request_main_redraw();
            }
            Drag::Marquee { start } => {
                self.marquee = Some(Rect::from_points(*start, self.pointer));
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
                dup,
            } => {
                if moved && !self.selection.is_empty() {
                    let delta = convert::vec2_to_core(last_doc - start_doc);
                    if dup {
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
                        Tool::Select | Tool::Pen => return,
                    };
                    if let Ok(CommandOutcome::Object(id)) = self.editor.execute(cmd) {
                        self.selection = vec![id];
                    }
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
                dup,
            } => Some(DragPreview {
                ids: &self.selection,
                delta: *last_doc - *start_doc,
                dup: *dup,
                xf: None,
            }),
            Drag::Scale { preview, .. } | Drag::Rotate { preview, .. } => Some(DragPreview {
                ids: &self.selection,
                delta: Vec2::ZERO,
                dup: false,
                xf: Some(preview),
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

        self.content.reset();
        let representative = self.representative();
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
                pen_preview,
                self.marquee,
                wl,
                hl,
                self.redock_preview.as_ref(),
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
        for host in self.hosts.values() {
            host.window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main_id.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Amalith — shell")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        self.scale = window.scale_factor();
        let wid = window.id();
        let host = self.make_host(window, Role::Main);
        self.hosts.insert(wid, host);
        self.main_id = Some(wid);
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
                    self.cmd_down = m.state().super_key();
                    self.shift_down = m.state().shift_key();
                    self.alt_down = m.state().alt_key();
                }
            }
            WindowEvent::KeyboardInput { event, .. } if Some(id) == self.main_id => {
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
                                if self.picker.take().is_none() {
                                    self.pen.clear();
                                    self.pen_redo.clear();
                                    self.selection.clear();
                                }
                                self.request_main_redraw();
                            }
                            KeyCode::KeyV => self.set_tool(Tool::Select),
                            KeyCode::KeyP => self.set_tool(Tool::Pen),
                            KeyCode::KeyM => self.set_tool(Tool::Rectangle),
                            KeyCode::KeyL => self.set_tool(Tool::Ellipse),
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
    match side {
        RailSide::Left => Rect::new(0.0, 0.0, rw, height),
        RailSide::Right => Rect::new(width - rw, 0.0, width, height),
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
    pen_preview: Option<PenPreview<'_>>,
    marquee: Option<Rect>,
    width: f64,
    height: f64,
    redock_preview: Option<&(RailSide, DropTarget)>,
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
    let viewport = Rect::new(left_x, 0.0, right_x.max(left_x), height);
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
        pen_preview,
    );

    if let Some(m) = marquee {
        scene.fill(Fill::NonZero, ID, theme.marquee_fill, None, &m);
        scene.stroke(&Stroke::new(1.0), ID, theme.select_blue, None, &m);
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

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
