//! Amalith shell — entry point.
//!
//! Owns the winit event loop and the per-window render state (surface +
//! vello renderer). The frame's drawing delegates to `amalith_shell`'s
//! `layout` + `chrome` over a demo dock tree; input wiring and real panels
//! come next.

use std::num::NonZeroUsize;
use std::sync::Arc;

use amalith_shell::dock::{Axis, Child, Node, PanelId};
use amalith_shell::text::TextContext;
use amalith_shell::{chrome, layout, Theme};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{color::palette, Fill};
use vello::util::{RenderContext, RenderSurface};
use vello::wgpu;
use vello::{AaConfig, Renderer, RendererOptions, Scene};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

/// One rendered window: the surface, the vello renderer for its device, and
/// the winit handle.
struct WindowState {
    surface: RenderSurface<'static>,
    renderer: Renderer,
    window: Arc<Window>,
}

struct App {
    context: RenderContext,
    state: Option<WindowState>,
    scene: Scene,
    text: TextContext,
}

impl App {
    fn new() -> Self {
        Self {
            context: RenderContext::new(),
            state: None,
            scene: Scene::new(),
            text: TextContext::new(),
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
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.context.resize_surface(
                    &mut state.surface,
                    size.width.max(1),
                    size.height.max(1),
                );
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let width = state.surface.config.width;
                let height = state.surface.config.height;

                self.scene.reset();
                paint(&mut self.scene, &mut self.text, width as f64, height as f64);

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

                let mut encoder =
                    device
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

/// The whole frame's drawing: a canvas ground and a right-hand dock rail
/// laid out and rendered by `amalith_shell`.
fn paint(scene: &mut Scene, text: &mut TextContext, width: f64, height: f64) {
    let theme = Theme::default();

    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.bg,
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    let rail = Rect::new((width - 300.0).max(0.0), 0.0, width, height);
    let dock = demo_dock();
    let laid = layout::layout(&dock, rail, &theme, &mut |p| {
        text.measure(&tab_label(p), 12.0) + theme.tab_pad_x * 2.0
    });
    chrome::paint(scene, &laid, &theme, text, &tab_label);
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
