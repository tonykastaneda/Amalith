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
    let trace_gallery = std::env::args().nth(3).as_deref() == Some("trace");
    let recolor_gallery = std::env::args().nth(3).as_deref() == Some("recolor");
    if recolor_gallery {
        use amalith_core::{Color as ArtColor, Document, Layer, LayerId, Object, ObjectId, ObjectParent, Paint};
        use amalith_shell::recolordlg;
        use vello::{kurbo::{Rect, Affine}, peniko::Fill};
        let mut doc = Document::new("Recolor review");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Artwork"), 0);
        let mut ids = Vec::new();
        for (i, c) in [ArtColor::rgb(0.22,0.43,0.7), ArtColor::rgb(0.14,0.14,0.14), ArtColor::rgb(0.33,0.33,0.33), ArtColor::rgb(0.9,0.65,0.18), ArtColor::rgb(1.,1.,1.)].into_iter().enumerate() {
            let id = ObjectId::new();
            let mut object = Object::rectangle(id, ObjectParent::Layer(layer), amalith_core::Rect::new(0.,0.,10.,10.));
            object.appearance.set_fill(Paint::Solid(c));
            doc.insert_object(object, i).unwrap();
            ids.push(id);
        }
        let d = recolordlg::RecolorDialog::open(ObjectId::new(), 0, doc, ids).unwrap();
        let body = Rect::new(0., 0., recolordlg::width(), recolordlg::height());
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &body);
        recolordlg::paint(&mut scene, &mut tcx, &theme, body, &d);
    } else if trace_gallery {
        use amalith_shell::{image_trace,convert};
        use vello::{kurbo::{Rect,Affine},peniko::{Fill,ImageData,ImageFormat,ImageAlphaType,Blob}};
        let img=image::RgbaImage::from_fn(128,128,|x,y| {
            let dx=x as f64-64.;let dy=y as f64-56.;let d=(dx*dx+dy*dy).sqrt();
            image::Rgba(if d<22. {[0,0,0,0]}else if d<45. {[238,76,62,255]}else if y>101&&y<117&&x>20&&x<108 {[40,126,220,255]}else{[0,0,0,0]})
        });
        let mut png=std::io::Cursor::new(Vec::new());image::DynamicImage::ImageRgba8(img.clone()).write_to(&mut png,image::ImageFormat::Png).unwrap();
        std::fs::write(format!("{output}.source.png"),png.get_ref()).unwrap();
        let mut tracer=amalith_trace::Tracer::decode(png.get_ref()).unwrap();
        let options=amalith_trace::Options{mode:amalith_trace::Mode::Color,colors:6,noise:1,paths:80.,..Default::default()};
        let result=tracer.trace(&options,true,&amalith_trace::CancelToken::new(),&mut |_|{}).unwrap();
        let p=image_trace::Panel{options,enabled:true,can_expand:true,counts:Some((result.paths.len(),result.anchors,result.colors)),status:"Trace ready".into(),preset:None,..Default::default()};
        scene.fill(Fill::NonZero,Affine::IDENTITY,theme.bg,None,&Rect::new(0.,0.,1100.*scale,760.*scale));
        let body=Rect::new(20.*scale,50.*scale,340.*scale,660.*scale);
        scene.fill(Fill::NonZero,Affine::IDENTITY,theme.panel_bg,None,&body);
        tcx.draw(&mut scene,"Image Trace",16.,theme.text,20.*scale,30.*scale);
        image_trace::paint(&mut scene,&mut tcx,body,&theme,&p);
        let image=ImageData{data:Blob::from(img.into_raw()),format:ImageFormat::Rgba8,alpha_type:ImageAlphaType::Alpha,width:128,height:128};
        for (x,title) in [(380.,"Original PNG"),(740.,"Editable trace")] {
            tcx.draw(&mut scene,title,14.,theme.text,x*scale,78.*scale);
            let area=Rect::new(x*scale,100.*scale,(x+320.)*scale,420.*scale);
            scene.fill(Fill::NonZero,Affine::IDENTITY,theme.panel_bg,None,&area);
            let xf=Affine::translate((x*scale,100.*scale))*Affine::scale(2.5*scale);
            if x==380. {scene.draw_image(&image,xf);}else{for(path,color)in &result.paths{scene.fill(Fill::NonZero,xf,convert::color(*color),None,&convert::bez_path(&path.geometry));}}
        }
        tcx.draw(&mut scene,"Window > Panels > Image Trace",12.,theme.text_dim,380.*scale,475.*scale);
        tcx.draw(&mut scene,"Preview keeps the original. Expand creates editable paths.",12.,theme.text_dim,380.*scale,500.*scale);
    } else if panel_gallery {
        let doc = amalith_shell::sample::document();
        let expanded = std::collections::HashSet::new();
        let ctx = panels::Ctx {
            image_trace: &Default::default(),
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
            hide_wip_tools: false,
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
            symbols_view: Default::default(),
            symbol_thumbnails: &Default::default(),
            symbol_tiles: &Default::default(),
            symbols_scroll: 0.0,
            selected_symbol: None,
            symbols_drop_hover: false,
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
            effect_dialog: None,
            symbol_name_dialog: None,
            layer_dialog: None,
            recolor_dialog: None,
            area_type_dialog: None,
            gradient: None,
            gradient_edit: None,
            appearance_items: Vec::new(),
            appearance_selected: None,
            appearance_drop: None,
            appearance_fx_menu: false,
            appearance_width_edit: None,
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
    let (width, height) = if recolor_gallery { ((520.*scale) as u32, (530.*scale) as u32) } else if trace_gallery { ((1100.*scale) as u32,(760.*scale) as u32) } else if panel_gallery {
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
