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

use amalith_shell::dock::{
    Axis, Child, DockModel, DropTarget, Node, NodePath, PanelId, Rail, RailSide, Side,
};
use amalith_shell::layout::Layout;
use amalith_shell::text::TextContext;
use amalith_shell::{chrome, layout, Theme};
use vello::kurbo::{Affine, Point, Rect, Stroke, Vec2};
use vello::peniko::{color::palette, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// When a rail is empty, the strip of canvas along that edge that still
/// accepts a drop (creating the rail).
const EMPTY_ZONE: f64 = 48.0;
/// Slack around a splitter's visual gap for grabbing it.
const GRAB_SLOP: f64 = 5.0;
/// Visible thickness of the bar on a rail's inner edge.
const RAIL_EDGE: f64 = 4.0;
/// Min / max rail width as a fraction of the window, logical points.
const RAIL_MIN_W: f64 = 160.0;
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
    theme: Theme,
    scale: f64,
    /// Pointer position within whichever window last reported it, logical.
    pointer: Point,
    pointer_win: Option<WindowId>,
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
            dock: DockModel::new(demo_dock()),
            theme: Theme::default(),
            scale: 1.0,
            pointer: Point::ZERO,
            pointer_win: None,
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

    fn on_press(&mut self, id: WindowId) {
        let Some(role) = self.hosts.get(&id).map(|h| h.role) else {
            return;
        };
        match role {
            Role::Main => {
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };
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
                        if !area.tab_strip.contains(self.pointer) {
                            continue;
                        }
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
                    return;
                }
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
                self.dock.rail_mut(side).width = clamped as f32;
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
            Drag::None | Drag::Splitter { .. } | Drag::RailWidth { .. } => {}
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

        self.content.reset();
        match role {
            Role::Main => paint_main(
                &mut self.content,
                &mut self.text,
                &self.dock,
                &self.theme,
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
    }
}

impl ApplicationHandler for App {
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
        window.request_redraw();
        let host = self.make_host(window, Role::Main);
        self.hosts.insert(wid, host);
        self.main_id = Some(wid);
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
            } => self.on_press(id),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.on_release(),
            WindowEvent::RedrawRequested => self.redraw(id),
            _ => {}
        }
    }
}

/// A stand-in dock tree until real workspace state exists: Layers on top,
/// an Artboards/Swatches tab group below it.
fn demo_dock() -> Node {
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

fn tab_label(panel: PanelId) -> String {
    match panel.0 {
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

fn paint_main(
    scene: &mut Scene,
    text: &mut TextContext,
    dock: &DockModel,
    theme: &Theme,
    width: f64,
    height: f64,
    preview: Option<&(RailSide, DropTarget)>,
) {
    scene.fill(
        Fill::NonZero,
        ID,
        theme.bg,
        None,
        &Rect::new(0.0, 0.0, width, height),
    );
    for side in [RailSide::Left, RailSide::Right] {
        let rail = dock.rail(side);
        let is_preview_target = preview.is_some_and(|(s, _)| *s == side);
        if rail.is_empty() && !is_preview_target {
            continue;
        }
        let rect = rail_rect_for(side, rail.width as f64, width, height);
        let laid = build_rail_layout(rail, theme, text, rect);
        if !rail.is_empty() {
            chrome::paint(scene, &laid, theme, text, &tab_label);
            // Bar on the canvas-facing edge — the whole-rail resize handle.
            scene.fill(
                Fill::NonZero,
                ID,
                theme.splitter,
                None,
                &rail_edge_bar(side, rect),
            );
        }
        if let Some((_, target)) = preview.filter(|(s, _)| *s == side) {
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
