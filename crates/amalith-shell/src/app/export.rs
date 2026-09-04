//! Export for Screens — the `App`-side glue: spawning the floating panel
//! window, closing it, its keyboard, the folder picker, and kicking off
//! the export. The panel UI lives in [`crate::export`]; the render-and-
//! write backend is [`App::run_export`]. Split out of `app/mod.rs`.

use super::*;

/// The float-only panel id for the Export for Screens dialog.
pub(in crate::app) const EXPORT_PID: PanelId = PanelId("export-screens");

impl App {
    /// Queue the dialog to open on the next `about_to_wait` (the menu /
    /// shortcut paths have no `event_loop` to spawn a window with).
    pub(in crate::app) fn request_export_dialog(&mut self) {
        if self.doc.editor.document().artboards().is_empty() {
            self.doc.io_error = Some("Nothing to export — the document has no artboards.".into());
            self.request_main_redraw();
            return;
        }
        self.pending_export = true;
    }

    pub(in crate::app) fn spawn_export_dialog(&mut self, event_loop: &ActiveEventLoop) {
        self.close_export(false);

        let items: Vec<crate::export::Item> = self
            .doc
            .editor
            .document()
            .artboards()
            .iter()
            .map(|a| crate::export::Item {
                name: a.name.clone(),
                rect: a.rect,
                checked: true,
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let dest = self
            .doc
            .file_path
            .as_ref()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .or_else(|| dirs_desktop())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.export = Some(crate::export::ExportForScreens::new(items, dest));
        self.text_blink = Instant::now();

        let fw = crate::export::W;
        let fh = crate::export::H + self.theme.tab_strip_h;
        let (mw, mh) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let o = self.main_inner_origin();
        let pos = Point::new(
            o.x + ((mw - fw) * 0.5).max(4.0),
            o.y + ((mh - fh) * 0.5).max(4.0),
        );
        let rect = [pos.x as f32, pos.y as f32, fw as f32, fh as f32];
        let id = self.dock.float_alone(EXPORT_PID, rect);

        let attrs = Window::default_attributes()
            .with_title(tab_label(EXPORT_PID))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create export window"),
        );
        let wid = window.id();
        let host = self.make_host(window.clone(), Role::Floating(id));
        self.hosts.insert(wid, host);
        window.request_redraw();
        self.request_main_redraw();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
    }

    /// Close the dialog and its window. `run` first performs the export.
    pub(in crate::app) fn close_export(&mut self, run: bool) {
        let Some(dlg) = self.export.take() else {
            return;
        };
        if run {
            self.run_export(&dlg);
        }
        self.dock.remove(EXPORT_PID);
        let dead: Vec<WindowId> = self
            .hosts
            .iter()
            .filter_map(|(wid, h)| match h.role {
                Role::Floating(fid) if self.dock.floating(fid).is_none() => Some(*wid),
                _ => None,
            })
            .collect();
        for wid in dead {
            self.hosts.remove(&wid);
            self.focused.remove(&wid);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
        self.request_main_redraw();
    }

    /// Pick the export destination folder.
    pub(in crate::app) fn export_pick_folder(&mut self) {
        let start = self.export.as_ref().map(|d| d.dest.clone());
        let mut dlg = rfd::FileDialog::new();
        if let Some(s) = start.filter(|p| p.is_dir()) {
            dlg = dlg.set_directory(s);
        }
        if let Some(dir) = dlg.pick_folder() {
            if let Some(d) = self.export.as_mut() {
                d.dest = dir;
            }
        }
        self.request_main_redraw();
    }

    pub(in crate::app) fn export_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_export(false);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_export(true);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.export.as_mut() else {
            return;
        };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::Tab) => dlg.defocus(),
            _ => {
                if let Some(txt) = &event.text {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        dlg.push_char(ch);
                    }
                }
            }
        }
        self.text_blink = Instant::now();
        self.request_main_redraw();
    }

    /// Perform the export described by `dlg`: for every selected artboard
    /// and every Formats row, render / serialize the artboard region and
    /// write a file under `dlg.dest`.
    pub(in crate::app) fn run_export(&mut self, dlg: &crate::export::ExportForScreens) {
        use crate::export::Format;

        let sel = dlg.selected();
        if sel.is_empty() {
            self.doc.io_error = Some("Nothing selected to export.".into());
            self.request_main_redraw();
            return;
        }
        if !dlg.dest.is_dir() {
            if let Err(e) = std::fs::create_dir_all(&dlg.dest) {
                self.doc.io_error = Some(format!("Can't create {}: {e}", dlg.dest.display()));
                self.request_main_redraw();
                return;
            }
        }

        let bleed = if dlg.include_bleed {
            self.doc.editor.document().settings.bleed
        } else {
            amalith_core::Bleed::default()
        };
        let jobs: Vec<(usize, crate::export::Row)> = sel
            .iter()
            .flat_map(|&i| dlg.rows.iter().cloned().map(move |r| (i, r)))
            .collect();

        let mut written = 0usize;
        let mut first_err: Option<String> = None;
        for (ab_i, row) in jobs {
            let (name, ab_rect, ab_fill) = {
                let doc = self.doc.editor.document();
                let Some(ab) = doc.artboards().get(ab_i) else {
                    continue;
                };
                (ab.name.clone(), ab.rect, ab.fill)
            };
            let bg = ab_fill.map(|c| vello::peniko::Color::new([c.r, c.g, c.b, c.a]));
            let src = amalith_core::Rect::new(
                ab_rect.x0 - bleed.left,
                ab_rect.y0 - bleed.top,
                ab_rect.x1 + bleed.right,
                ab_rect.y1 + bleed.bottom,
            );

            let dir = self.export_subdir(dlg, &row);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                first_err.get_or_insert(format!("{}: {e}", dir.display()));
                continue;
            }
            let path = dir.join(format!(
                "{}{}{}.{}",
                dlg.prefix,
                name,
                row.suffix,
                row.format.ext()
            ));

            let result = match row.format {
                Format::Png | Format::Jpg => {
                    self.export_raster(src, row.scale, row.format, bg, &path)
                }
                Format::Svg => self.export_svg_file(ab_rect, src, &path),
                Format::Pdf => self.export_pdf_file(src, ab_fill, &path),
            };
            match result {
                Ok(()) => written += 1,
                Err(e) => {
                    first_err.get_or_insert(format!("{}: {e}", path.display()));
                }
            }
        }

        self.doc.io_error = Some(match &first_err {
            Some(e) if written == 0 => format!("Export failed — {e}"),
            Some(e) => format!("Exported {written} file(s); some failed — {e}"),
            None => format!("Exported {written} file(s) to {}", dlg.dest.display()),
        });
        if first_err.is_none() && dlg.open_after {
            reveal(&dlg.dest);
        }
        self.request_main_redraw();
    }

    /// The output directory for `row`, honouring Create Sub-folders.
    fn export_subdir(
        &self,
        dlg: &crate::export::ExportForScreens,
        row: &crate::export::Row,
    ) -> std::path::PathBuf {
        use crate::export::SubBy;
        if !dlg.subfolders {
            return dlg.dest.clone();
        }
        let leaf = match dlg.sub_by {
            SubBy::Scale => crate::export::scale_label(row.scale),
            SubBy::Format => row.format.label().to_string(),
        };
        dlg.dest.join(leaf)
    }

    fn export_raster(
        &mut self,
        src: amalith_core::Rect,
        scale: f64,
        fmt: crate::export::Format,
        bg: Option<vello::peniko::Color>,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        let w = ((src.x1 - src.x0) * scale).round().max(1.0) as u32;
        let h = ((src.y1 - src.y0) * scale).round().max(1.0) as u32;
        let ksrc = vello::kurbo::Rect::new(src.x0, src.y0, src.x1, src.y1);
        let scene = crate::canvas::export_scene(
            self.doc.editor.document(),
            ksrc,
            scale,
            bg,
            &self.image_cache,
            self.outline_mode,
            &mut self.text,
        );
        let rgba = self
            .render_scene_to_rgba(&scene, w, h)
            .ok_or_else(|| io_err("offscreen render failed"))?;

        let img = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| io_err("bad pixel buffer"))?;
        let file = std::fs::File::create(path)?;
        let mut out = std::io::BufWriter::new(file);
        match fmt {
            crate::export::Format::Jpg => {
                // JPEG has no alpha — flatten onto white.
                let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92)
                    .encode_image(&rgb)
                    .map_err(|e| io_err(&e.to_string()))?;
            }
            _ => {
                image::DynamicImage::ImageRgba8(img)
                    .write_to(&mut out, image::ImageFormat::Png)
                    .map_err(|e| io_err(&e.to_string()))?;
            }
        }
        Ok(())
    }

    fn export_svg_file(
        &self,
        ab_rect: amalith_core::Rect,
        src: amalith_core::Rect,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        let doc = self.doc.editor.document();
        // Top-level objects that touch the artboard region.
        let ids: Vec<amalith_core::ObjectId> = doc
            .layers()
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| l.children.iter().copied())
            .filter(|&id| {
                doc.bounds_of(id)
                    .is_some_and(|b| rects_overlap(b, ab_rect))
            })
            .collect();
        let inner = amalith_io::export_svg(doc, &ids).unwrap_or_default();
        // Re-frame the viewBox onto the artboard (± bleed) instead of the
        // union of contents.
        let svg = reframe_svg(
            &inner,
            src.x0,
            src.y0,
            src.x1 - src.x0,
            src.y1 - src.y0,
        );
        std::fs::write(path, svg)
    }

    /// A real vector PDF (paths, text-as-outlines, gradients as shading
    /// patterns, images as JPEG XObjects) — see `crate::pdfexport` for
    /// the writer and its documented coordinate/alpha strategy and known
    /// simplifications. `bg` is the artboard's own fill, painted as a
    /// full-page backdrop rect.
    fn export_pdf_file(
        &mut self,
        src: amalith_core::Rect,
        bg: Option<amalith_core::Color>,
        path: &std::path::Path,
    ) -> std::io::Result<()> {
        let doc = self.doc.editor.document();
        let ids: Vec<amalith_core::ObjectId> = doc
            .layers()
            .iter()
            .filter(|l| l.visible)
            .flat_map(|l| l.children.iter().copied())
            .filter(|&id| doc.bounds_of(id).is_some_and(|b| rects_overlap(b, src)))
            .collect();
        let cmyk = doc.settings.color_mode == amalith_core::ColorMode::Cmyk;
        let pdf = crate::pdfexport::export_vector_pdf(
            doc,
            &ids,
            src,
            bg,
            cmyk,
            self.cmyk_profile.as_ref(),
            &mut self.text,
            &self.image_cache,
        );
        std::fs::write(path, pdf)
    }

    /// Render `scene` (already sized in pixels) headlessly to RGBA8 bytes,
    /// sRGB-encoded and un-premultiplied, top-to-bottom.
    fn render_scene_to_rgba(&mut self, scene: &Scene, w: u32, h: u32) -> Option<Vec<u8>> {
        let dev_id = self.hosts.get(&self.main_id?)?.surface.dev_id;
        if self.export_renderer.is_none() {
            let dev = &self.context.devices[dev_id].device;
            self.export_renderer = Renderer::new(
                dev,
                RendererOptions {
                    use_cpu: false,
                    antialiasing_support: vello::AaSupport::area_only(),
                    num_init_threads: NonZeroUsize::new(1),
                    pipeline_cache: None,
                },
            )
            .ok();
        }
        let renderer = self.export_renderer.as_mut()?;
        let dev = &self.context.devices[dev_id];

        let tex = dev.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("export target"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        renderer
            .render_to_texture(
                &dev.device,
                &dev.queue,
                scene,
                &view,
                &vello::RenderParams {
                    base_color: vello::peniko::Color::TRANSPARENT,
                    width: w,
                    height: h,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .ok()?;

        // Copy the texture into a padded buffer and map it.
        let bpp = 4u32;
        let unpadded = w * bpp;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buf = dev.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("export readback"),
            size: (padded * h) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut enc = dev
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        dev.queue.submit([enc.finish()]);

        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = dev.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range();
        let mut out = vec![0u8; (unpadded * h) as usize];
        for y in 0..h as usize {
            let s = y * padded as usize;
            let d = y * unpadded as usize;
            out[d..d + unpadded as usize]
                .copy_from_slice(&mapped[s..s + unpadded as usize]);
        }
        drop(mapped);
        buf.unmap();

        // vello writes linear, un-premultiplied RGBA into the storage
        // texture; PNG/JPEG want sRGB.
        for px in out.chunks_exact_mut(4) {
            for c in &mut px[..3] {
                *c = lin_to_srgb(*c);
            }
        }
        Some(out)
    }
}

fn io_err(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, msg.to_string())
}

fn rects_overlap(a: amalith_core::Rect, b: amalith_core::Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn lin_to_srgb(v: u8) -> u8 {
    let c = v as f32 / 255.0;
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Swap an exported `<svg>`'s `viewBox` for the given frame.
fn reframe_svg(svg: &str, x: f64, y: f64, w: f64, h: f64) -> String {
    let vb = format!("viewBox=\"{x} {y} {w} {h}\"");
    if let (Some(a), Some(_)) = (svg.find("viewBox=\""), svg.find("\">")) {
        let end = svg[a..].find('"').map(|i| a + i + 1).unwrap_or(a);
        let close = svg[end..].find('"').map(|i| end + i + 1).unwrap_or(end);
        format!(
            "{}width=\"{w}\" height=\"{h}\" {vb}{}",
            &svg[..a],
            &svg[close..]
        )
    } else {
        svg.to_string()
    }
}

fn reveal(dir: &std::path::Path) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

fn dirs_desktop() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Desktop"))
}
