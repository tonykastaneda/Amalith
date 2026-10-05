//! Offscreen sheet of every tool icon (the Pen-flyout anchor tools,
//! Curvature, Scissors, Group Selection, Reshape, the grids and the liquify brushes outlined), at toolbar
//! size and enlarged, light and dark — no window, nothing on screen.
//! cargo run -p amalith-shell --example path_tools_review -- /tmp/tools.png
use amalith_shell::{icons, text::TextContext, tool::Tool};
use vello::{
    kurbo::{Affine, Rect, Stroke},
    peniko::{Color, Fill},
    wgpu, Scene,
};

fn main() {
    let output = std::env::args().nth(1).expect("output PNG path");
    let mut scene = Scene::new();
    let mut tcx = TextContext::new();
    let new = [
        Tool::AddAnchor,
        Tool::DeleteAnchor,
        Tool::AnchorPoint,
        Tool::Curvature,
        Tool::Scissors,
        Tool::GroupSelect,
        Tool::Reshape,
        Tool::RectangularGrid,
        Tool::PolarGrid,
        Tool::Warp,
        Tool::Twirl,
        Tool::Pucker,
        Tool::Bloat,
        Tool::Scallop,
        Tool::Crystallize,
        Tool::Wrinkle,
    ];
    let (light, dark) = (Color::from_rgb8(0xf2, 0xf2, 0xf2), Color::from_rgb8(0x2b, 0x2b, 0x2b));
    let (ink_light, ink_dark) = (Color::from_rgb8(0x30, 0x30, 0x30), Color::from_rgb8(0xd8, 0xd8, 0xd8));
    scene.fill(Fill::NonZero, Affine::IDENTITY, light, None, &Rect::new(0., 0., 550., 760.));
    scene.fill(Fill::NonZero, Affine::IDENTITY, dark, None, &Rect::new(550., 0., 1100., 760.));
    // Big: the new tools, 64 px, with labels.
    for (half, (bg_ink, x0)) in [(ink_light, 0.0), (ink_dark, 550.0)].into_iter().enumerate() {
        let label = if half == 0 { Color::BLACK } else { Color::WHITE };
        for (i, t) in new.iter().enumerate() {
            let x = x0 + 20.0 + (i % 4) as f64 * 130.0;
            let y = 20.0 + (i / 4) as f64 * 105.0;
            icons::draw(&mut scene, t.icon(), Rect::new(x + 20.0, y, x + 84.0, y + 64.0), bg_ink);
            tcx.draw(&mut scene, t.label(), 11.0, label, x, y + 88.0);
        }
        // Small: every tool at toolbar size, new ones boxed.
        for (i, t) in Tool::ALL.iter().enumerate() {
            let x = x0 + 20.0 + (i % 12) as f64 * 42.0;
            let y = 470.0 + (i / 12) as f64 * 42.0;
            let r = Rect::new(x, y, x + 22.0, y + 22.0);
            icons::draw(&mut scene, t.icon(), r, bg_ink);
            if new.contains(t) {
                scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, Color::from_rgb8(0xe0, 0x9a, 0x1a), None, &r.inflate(6.0, 6.0));
            }
        }
        // Cursor base glyphs at canvas size.
        for (i, src) in [icons::CURSOR_PEN_DRAWING_SVG, icons::CURSOR_DIRECT_SELECT_SVG].into_iter().enumerate() {
            let x = x0 + 20.0 + i as f64 * 60.0;
            icons::draw_cursor(&mut scene, src, Rect::new(x, 700.0, x + 30.0, 730.0));
        }
    }
    let mut context = vello::util::RenderContext::new();
    let index = pollster::block_on(context.device(None)).expect("GPU adapter");
    let dev = &context.devices[index];
    let mut renderer =
        vello::Renderer::new(&dev.device, vello::RendererOptions::default()).unwrap();
    let (width, height) = (1100u32, 760u32);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = dev.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("UI scale review"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    renderer
        .render_to_texture(
            &dev.device,
            &dev.queue,
            &scene,
            &texture.create_view(&Default::default()),
            &vello::RenderParams {
                base_color: Color::WHITE,
                width,
                height,
                antialiasing_method: vello::AaConfig::Area,
            },
        )
        .unwrap();
    let padded = (width * 4).div_ceil(256) * 256;
    let buffer = dev.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (padded * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = dev.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(height),
            },
        },
        size,
    );
    dev.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    dev.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let pixels: Vec<u8> = mapped
        .chunks(padded as usize)
        .flat_map(|row| row[..width as usize * 4].iter().copied())
        .collect();
    image::save_buffer(output, &pixels, width, height, image::ColorType::Rgba8).unwrap();
}
