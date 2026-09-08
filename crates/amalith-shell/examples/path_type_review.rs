//! Render production path glyphs (top) and exported outlines (bottom).
//! cargo run -p amalith-shell --example path_type_review -- /tmp/path-type.png
use amalith_core::{
    ArcLengthPath, ObjectId, PathData, PathTextAlign, PathTextData, Rect, TextData, TextKind,
};
use amalith_shell::{pathtext, text::TextContext};
use vello::{
    kurbo::{Affine, Line, Stroke},
    peniko::{Color, Fill},
    wgpu, Scene,
};

fn main() {
    let output = std::env::args().nth(1).expect("output PNG path");
    let mut scene = Scene::new();
    let mut tcx = TextContext::new();
    let path = PathData::ellipse(Rect::new(70.0, 80.0, 310.0, 320.0));
    let arc = ArcLengthPath::new(&path.flattened_points(0.05)[0], true);
    for column in 0..3 {
        let pt = PathTextData {
            path: ObjectId::new(),
            start: arc.total_length() * 0.56,
            end: arc.total_length() * 1.46,
            align: if column == 2 {
                PathTextAlign::Center
            } else {
                PathTextAlign::Baseline
            },
            flip: column == 1,
        };
        let mut td = TextData {
            content: "Type on a path · café".into(),
            kind: TextKind::Path(pt),
            path_geometry: Some(path.clone()),
            ..TextData::default()
        };
        td.style.size = 29.0;
        td.style.family = "Arial".into();
        let xf = Affine::translate((column as f64 * 400.0, 0.0));
        assert!(!pathtext::paint_path_text(
            &mut scene,
            &mut tcx,
            &td,
            &pt,
            &arc,
            amalith_core::Affine::IDENTITY,
            xf,
            Color::BLACK
        ));
        let outline =
            pathtext::outline_path_text(&mut tcx, &td, &pt, &arc, amalith_core::Affine::IDENTITY);
        assert!(!outline.is_empty());
        let converted = amalith_shell::textedit::outline_text_data(&td, &mut tcx);
        assert_eq!(outline, converted, "export must retain curved placement");
        let outline = vello::kurbo::BezPath::from_svg(&outline.to_svg()).unwrap();
        scene.fill(
            Fill::NonZero,
            Affine::translate((column as f64 * 400.0, 400.0)),
            Color::BLACK,
            None,
            &outline,
        );
        for row in 0..2 {
            let xf = Affine::translate((column as f64 * 400.0, row as f64 * 400.0));
            for h in pathtext::screen_brackets(&arc, &pt, xf) {
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    Color::from_rgb8(45, 145, 255),
                    None,
                    &Line::new(h.base, h.tip),
                );
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    Color::from_rgb8(45, 145, 255),
                    None,
                    &Line::new(h.tip - h.tangent * 5.0, h.tip + h.tangent * 5.0),
                );
            }
        }
    }
    let mut context = vello::util::RenderContext::new();
    let index = pollster::block_on(context.device(None)).expect("GPU adapter");
    let dev = &context.devices[index];
    let mut renderer =
        vello::Renderer::new(&dev.device, vello::RendererOptions::default()).unwrap();
    let (width, height) = (1200, 800);
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = dev.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("path type review"),
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
    let row_bytes = width as usize * 400 * 4;
    let mean_error = pixels[..row_bytes]
        .iter()
        .zip(&pixels[row_bytes..])
        .map(|(&a, &b)| a.abs_diff(b) as u64)
        .sum::<u64>() as f64
        / row_bytes as f64;
    println!(
        "Glyph rendering versus exported outlines: mean channel difference {mean_error:.4}/255"
    );
    assert!(mean_error < 1.0, "screen and export diverged");
}
