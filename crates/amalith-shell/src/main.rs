//! Amalith shell — entry point.
//!
//! Owns the winit event loop and the per-window render state (surface +
//! vello renderer), holds the [`DockModel`], and routes pointer input into
//! it: tab clicks switch the active tab, splitter drags re-weight a split.
//! Panel detach/reattach and real panel content come next.

use std::num::NonZeroUsize;
use std::sync::Arc;

use amalith_shell::dock::{Axis, Child, DockModel, Node, NodePath, PanelId};
use amalith_shell::layout::Layout;
use amalith_shell::text::TextContext;
use amalith_shell::{chrome, layout, Theme};
use vello::kurbo::{Affine, Point, Rect};
use vello::peniko::{color::palette, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// Width of the right-hand dock rail, in logical points.
const RAIL_W: f64 = 320.0;
/// Slack around a splitter's visual gap for grabbing it.
const GRAB_SLOP: f64 = 3.0;

/// One rendered window: the surface, the vello renderer for its device, and
/// the winit handle.
struct WindowState {
    surface: RenderSurface<'static>,
    renderer: Renderer,
    window: Arc<Window>,
}

/// What the pointer is currently doing.
#[derive(Default)]
enum Drag {
    #[default]
    None,
    /// Re-weighting the split at `path`, boundary after child `gap`.
    Splitter { path: NodePath, gap: usize },
}

struct App {
    context: RenderContext,
    state: Option<WindowState>,
    scene: Scene,
    /// Chrome is drawn here in logical units, then appended to `scene`
    /// scaled by the window's DPI factor.
    content: Scene,
    text: TextContext,
    dock: DockModel,
    theme: Theme,
    pointer: Point,
    drag: Drag,
}

impl App {
    fn new() -> Self {
        Self {
            context: RenderContext::new(),
            state: None,
            scene: Scene::new(),
            content: Scene::new(),
            text: TextContext::new(),
            dock: DockModel::new(demo_dock()),
            theme: Theme::default(),
            pointer: Point::ZERO,
            drag: Drag::None,
        }
    }

    /// Logical size of the current window's surface, or `None` if there is
    /// no window yet.
    fn logical_size(&self) -> Option<(f64, f64)> {
        let state = self.state.as_ref()?;
        let scale = state.window.scale_factor();
        Some((
            state.surface.config.width as f64 / scale,
            state.surface.config.height as f64 / scale,
        ))
    }

    fn request_redraw(&self) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }

    fn on_pointer_down(&mut self) {
        let Some((w, h)) = self.logical_size() else {
            return;
        };
        let rail = rail_rect(w, h);
        let laid = build_layout(&self.dock, &self.theme, &mut self.text, rail);

        // A splitter under the pointer wins — start a re-weight drag.
        if let Some(sp) = laid
            .splitters
            .iter()
            .find(|s| s.rect.inflate(GRAB_SLOP, GRAB_SLOP).contains(self.pointer))
        {
            self.drag = Drag::Splitter {
                path: sp.path.clone(),
                gap: sp.index,
            };
            return;
        }

        // Otherwise a tab click switches the active tab of its group.
        for area in &laid.areas {
            if !area.tab_strip.contains(self.pointer) {
                continue;
            }
            if let Some(i) = area.tabs.iter().position(|t| t.rect.contains(self.pointer)) {
                self.dock.activate_tab(&area.path, i);
                self.request_redraw();
            }
            break;
        }
    }

    fn on_pointer_move(&mut self) {
        let (path, gap) = match &self.drag {
            Drag::Splitter { path, gap } => (path.clone(), *gap),
            Drag::None => return,
        };
        let Some((w, h)) = self.logical_size() else {
            return;
        };
        let rail = rail_rect(w, h);
        let laid = build_layout(&self.dock, &self.theme, &mut self.text, rail);
        if let Some(sp) = laid
            .splitters
            .iter()
            .find(|s| s.path == path && s.index == gap)
        {
            let frac = sp.frac_at(self.pointer);
            self.dock.set_boundary(&path, gap, frac);
            self.request_redraw();
        }
    }

    fn on_pointer_up(&mut self) {
        if !matches!(self.drag, Drag::None) {
            self.drag = Drag::None;
            self.request_redraw();
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Amalith — shell")
            .with_inner_size(LogicalSize::new(1280.0, 800.0));
        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));

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

        self.state = Some(WindowState {
            surface,
            renderer,
            window,
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.state.is_none() {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let state = self.state.as_mut().unwrap();
                self.context.resize_surface(
                    &mut state.surface,
                    size.width.max(1),
                    size.height.max(1),
                );
                state.window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self
                    .state
                    .as_ref()
                    .map(|s| s.window.scale_factor())
                    .unwrap_or(1.0);
                self.pointer = Point::new(position.x / scale, position.y / scale);
                self.on_pointer_move();
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.on_pointer_down(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.on_pointer_up(),
            WindowEvent::RedrawRequested => self.redraw(),
            _ => {}
        }
    }
}

impl App {
    fn redraw(&mut self) {
        let state = self.state.as_mut().unwrap();
        let width = state.surface.config.width;
        let height = state.surface.config.height;
        let scale = state.window.scale_factor();
        let (w_logical, h_logical) = (width as f64 / scale, height as f64 / scale);

        self.content.reset();
        paint(
            &mut self.content,
            &mut self.text,
            &self.dock,
            &self.theme,
            w_logical,
            h_logical,
        );
        self.scene.reset();
        self.scene.append(&self.content, Some(Affine::scale(scale)));

        let device = &self.context.devices[state.surface.dev_id];
        let surface_texture = match state.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture)
            | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            // Occluded / timed out / outdated / lost — skip this frame.
            _ => return,
        };

        state
            .renderer
            .render_to_texture(
                &device.device,
                &device.queue,
                &self.scene,
                &state.surface.target_view,
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
        state.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &state.surface.target_view,
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        surface_texture.present();
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

fn rail_rect(width: f64, height: f64) -> Rect {
    Rect::new((width - RAIL_W).max(0.0), 0.0, width, height)
}

fn build_layout(dock: &DockModel, theme: &Theme, text: &mut TextContext, rail: Rect) -> Layout {
    match &dock.root {
        Some(root) => layout::layout(root, rail, theme, &mut |p| {
            text.measure(&tab_label(p), 12.0) + theme.tab_pad_x * 2.0
        }),
        None => Layout::default(),
    }
}

/// The whole frame's drawing, in logical units: a canvas ground and the
/// right-hand dock rail.
fn paint(
    scene: &mut Scene,
    text: &mut TextContext,
    dock: &DockModel,
    theme: &Theme,
    width: f64,
    height: f64,
) {
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.bg,
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    let rail = rail_rect(width, height);
    let laid = build_layout(dock, theme, text, rail);
    chrome::paint(scene, &laid, theme, text, &tab_label);
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
