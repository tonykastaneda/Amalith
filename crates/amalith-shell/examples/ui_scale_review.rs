//! Visual regression review of real chrome at 100%, 125%, and 150%.
//! cargo run -p amalith-shell --example ui_scale_review -- /tmp/ui-scale 1.5
use amalith_shell::{
    metrics, panels, picker,
    prefs::{Prefs, Settings},
    text::TextContext,
    theme::Theme,
};
use vello::{kurbo::Point, peniko::Color, wgpu, Scene};

fn main() {
    let output = std::env::args().nth(1).expect("output PNG path");
    let scale: f64 = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "1.0".into())
        .parse()
        .unwrap();
    metrics::apply(scale);
    let mut scene = Scene::new();
    let mut tcx = TextContext::new();
    tcx.set_ui_scale(scale);
    let mut theme = Theme::default();
    theme.set_ui_scale(scale);
    let panel_gallery = std::env::args().nth(3).as_deref() == Some("panels");
    if panel_gallery {
        let doc = amalith_shell::sample::document();
        let expanded = std::collections::HashSet::new();
        let ctx = panels::Ctx {
            theme: &theme,
            doc: &doc,
            selection: &[],
            active_tool: amalith_shell::tool::Tool::Select,
            pointer: Point::new(-1.0, -1.0),
            representative: None,
            fill_mixed: false,
            stroke_mixed: false,
            active_slot: panels::PaintSlot::Fill,
            picker: None,
            cur_fill: amalith_core::Paint::Solid(amalith_core::Color::rgb(0.3, 0.5, 0.9)),
            cur_stroke: amalith_core::Paint::None,
            shape_tool: amalith_shell::tool::Tool::Rectangle,
            rotate_group_tool: amalith_shell::tool::Tool::Rotate,
            scale_group_tool: amalith_shell::tool::Tool::Scale,
            type_group_tool: amalith_shell::tool::Tool::Text,
            expanded: &expanded,
            renaming: None,
            selected_layer: None,
            selected_artboard: None,
            text_style: Default::default(),
            text_align: amalith_core::TextAlign::Start,
            text_paragraph: Default::default(),
            text_editing: false,
            text_vertical: false,
            text_kind_is_area: false,
            text_cross_align: amalith_core::TextAlign::Start,
            font_families: &[],
            layer_query: "",
            layer_search_focused: false,
            layer_scroll: 0.0,
            layer_drop: None,
            links_scroll: 0.0,
            selected_asset: None,
            color_mode: panels::ColorSpace::Rgb,
            cmyk_profile: None,
            recent: &[],
            xform_ref: Default::default(),
            xform_constrain: false,
            xform_edit: None,
            align_to: amalith_commands::AlignTo::Selection,
            align_spacing: None,
            align_spacing_edit: None,
            key_object: None,
            shape_dialog: None,
            export: None,
            xform_dialog: None,
            blend_dialog: None,
            offset_dialog: None,
            layer_dialog: None,
            area_type_dialog: None,
            gradient: None,
            gradient_edit: None,
            appearance_items: Vec::new(),
            appearance_selected: None,
            appearance_drop: None,
        };
        use amalith_shell::dock::PanelKind;
        for (i, kind) in [
            PanelKind::Tools,
            PanelKind::Character,
            PanelKind::Paragraph,
            PanelKind::Align,
            PanelKind::Color,
            PanelKind::Gradient,
            PanelKind::Transform,
            PanelKind::Pathfinder,
            PanelKind::Layers,
            PanelKind::Links,
            PanelKind::Artboards,
            PanelKind::Swatches,
            PanelKind::Appearance,
        ]
        .into_iter()
        .enumerate()
        {
            let x = 15.0 + (i % 4) as f64 * 545.0;
            let y = 20.0 + (i / 4) as f64 * 760.0;
            let body = vello::kurbo::Rect::new(x, y + 30.0, x + 320.0 * scale, y + 730.0);
            tcx.draw(&mut scene, kind.label(), 12.0, Color::BLACK, x, y + 20.0);
            scene.fill(
                vello::peniko::Fill::NonZero,
                vello::kurbo::Affine::IDENTITY,
                theme.panel_bg,
                None,
                &body,
            );
            panels::paint(
                &mut scene,
                &mut tcx,
                amalith_shell::dock::PanelId(kind),
                body,
                &ctx,
            );
        }
    } else {
        let mut prefs = Prefs::new(
            Settings {
                ui_scale: scale,
                ..Settings::default()
            },
            Default::default(),
            Default::default(),
        );
        prefs.paint(&mut scene, &mut tcx, &theme, 1800.0, 750.0);
        let pk = picker::Picker::from_color(
            panels::PaintSlot::Fill,
            Point::new(30.0, 800.0),
            Some(amalith_core::Color::rgb(0.3, 0.5, 0.9)),
        );
        picker::paint(&mut scene, &pk, theme.text, &theme, &mut tcx);
    }
    let mut context = vello::util::RenderContext::new();
    let index = pollster::block_on(context.device(None)).expect("GPU adapter");
    let dev = &context.devices[index];
    let mut renderer =
        vello::Renderer::new(&dev.device, vello::RendererOptions::default()).unwrap();
    let (width, height) = if panel_gallery {
        (2200, 2300)
    } else {
        (1800, 1600)
    };
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
