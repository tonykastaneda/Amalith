//! Compare per-dialog renderer construction with reuse of the main renderer.
//! cargo run -p amalith-shell --example dialog_renderer_review
use std::{num::NonZeroUsize, time::Instant};
use vello::{peniko::Color, wgpu, AaConfig, RenderParams, Renderer, RendererOptions, Scene};

fn main() {
    let mut context = vello::util::RenderContext::new();
    let index = pollster::block_on(context.device(None)).expect("GPU adapter");
    let dev = &context.devices[index];
    let create = || Renderer::new(&dev.device, RendererOptions {
        use_cpu: false,
        antialiasing_support: vello::AaSupport::area_only(),
        num_init_threads: NonZeroUsize::new(1),
        pipeline_cache: None,
    }).unwrap();
    let render = |renderer: &mut Renderer, size: u32| {
        let texture = dev.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("dialog review"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING,
            view_formats: &[],
        });
        let mut scene = Scene::new();
        scene.fill(vello::peniko::Fill::NonZero, vello::kurbo::Affine::IDENTITY,
            Color::from_rgb8(40, 120, 220), None,
            &vello::kurbo::Rect::new(10., 10., size as f64 - 10., size as f64 - 10.));
        renderer.render_to_texture(&dev.device, &dev.queue, &scene,
            &texture.create_view(&Default::default()),
            &RenderParams { base_color: Color::BLACK, width: size, height: size, antialiasing_method: AaConfig::Area }).unwrap();
        dev.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    };
    let start = Instant::now();
    let mut shared = create();
    render(&mut shared, 1024);
    println!("Main renderer initialization + first frame: {:?}", start.elapsed());
    for size in [320, 520, 320] {
        let start = Instant::now();
        render(&mut shared, size);
        println!("Shared renderer, {size}px dialog: {:?}", start.elapsed());
        let start = Instant::now();
        let mut fresh = create();
        render(&mut fresh, size);
        println!("Fresh renderer, {size}px dialog: {:?}", start.elapsed());
    }
}
