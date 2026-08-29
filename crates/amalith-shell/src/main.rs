//! Amalith shell — proof of life.
//!
//! One winit window, one wgpu surface, one vello scene. Draws a dark
//! ground and a couple of shapes and stays responsive to resize/close.
//! This exists to prove the winit + wgpu + vello stack builds and runs on
//! the target machine before the toolkit is built on top of it.

use std::num::NonZeroUsize;
use std::sync::Arc;

use vello::kurbo::{Affine, Rect, RoundedRect, Stroke};
use vello::peniko::{color::palette, Color, Fill};
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
}

impl App {
    fn new() -> Self {
        Self {
            context: RenderContext::new(),
            state: None,
            scene: Scene::new(),
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
                paint(&mut self.scene, width as f64, height as f64);

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

/// The whole frame's drawing. This is the seam where the real UI + dock +
/// canvas rendering will plug in.
fn paint(scene: &mut Scene, width: f64, height: f64) {
    // Ground.
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(0x2b, 0x2b, 0x2b),
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    // A panel-ish rounded rect on the right, to prove fills + rounding.
    let panel = RoundedRect::new(width - 260.0, 20.0, width - 20.0, height - 20.0, 6.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(0x33, 0x33, 0x33),
        None,
        &panel,
    );
    scene.stroke(
        &Stroke::new(1.0),
        Affine::IDENTITY,
        Color::from_rgb8(0x1f, 0x1f, 0x1f),
        None,
        &panel,
    );

    // A blue accent bar — the future dock-drop indicator.
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        Color::from_rgb8(0x1d, 0x7a, 0xf0),
        None,
        &Rect::new(width - 260.0, 60.0, width - 20.0, 63.0),
    );
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.run_app(&mut App::new()).expect("run app");
}
