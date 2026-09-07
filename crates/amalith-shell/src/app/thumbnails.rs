//! Recent-file preview thumbnails for the Home screen.
//!
//! Rendered headlessly the same way Export for Screens rasterises an
//! artboard (see `export.rs`, whose `render_scene_to_rgba` this reuses),
//! then cached to disk next to `recents.txt` so a returning visit to Home
//! is instant. `render/mod.rs` generates one per frame while any recent
//! tile is still missing its preview, so a cold cache fills in over a
//! handful of frames instead of stalling the first paint.

use super::*;
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};

/// Longest edge of a generated thumbnail, in px.
const THUMB_PX: f64 = 480.0;

impl App {
    /// Render (or load from cache) a small preview of the document at
    /// `path`, for a Home-screen recent-file tile. `None` on any failure
    /// — the caller falls back to the tile's flat placeholder colour.
    pub(in crate::app) fn recent_thumbnail(&mut self, path: &std::path::Path) -> Option<ImageData> {
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
        let secs = mtime.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        let cache_path = recent::thumbnail_cache_path(path, secs)?;

        if let Ok(bytes) = std::fs::read(&cache_path) {
            if let Some((rgba, w, h)) = appicon::decode_png(&bytes) {
                return Some(ImageData {
                    data: Blob::from(rgba),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::Alpha,
                    width: w,
                    height: h,
                });
            }
        }

        let is_ai = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ai"));
        let doc: Document = if is_ai {
            std::fs::read(path)
                .ok()
                .and_then(|b| amalith_io::import_ai(&b).ok())?
        } else {
            amalith_io::load(path).ok()?.0
        };

        // Prefer the first artboard (matches Illustrator's own document
        // icon convention); fall back to the union of every object's
        // bounds for a document with none.
        let (src, fill) = if let Some(ab) = doc.artboards().first() {
            (convert::rect(ab.rect), ab.fill)
        } else {
            let mut bounds: Option<amalith_core::Rect> = None;
            for layer in doc.layers().iter().filter(|l| l.visible) {
                for &id in &layer.children {
                    if let Some(r) = doc.bounds_of(id) {
                        bounds = Some(match bounds {
                            Some(acc) => acc.union(r),
                            None => r,
                        });
                    }
                }
            }
            (convert::rect(bounds?), None)
        };
        if src.width() <= 0.0 || src.height() <= 0.0 {
            return None;
        }
        let scale = (THUMB_PX / src.width().max(src.height())).min(4.0);
        let w = (src.width() * scale).round().max(1.0) as u32;
        let h = (src.height() * scale).round().max(1.0) as u32;
        let bg = Some(fill.map(|c| Color::new([c.r, c.g, c.b, c.a])).unwrap_or(Color::WHITE));

        // No image cache — a recent file's linked/embedded raster assets
        // aren't decoded for this, so they render blank. Acceptable for a
        // thumbnail this small; the vector content still comes through.
        let scene =
            canvas::export_scene(&doc, src, scale, bg, &HashMap::new(), false, &mut self.text);
        let rgba = self.render_scene_to_rgba(&scene, w, h)?;

        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Some(img) = image::RgbaImage::from_raw(w, h, rgba.clone()) {
            if let Ok(file) = std::fs::File::create(&cache_path) {
                let mut out = std::io::BufWriter::new(file);
                let _ = image::DynamicImage::ImageRgba8(img)
                    .write_to(&mut out, image::ImageFormat::Png);
            }
        }

        Some(ImageData {
            data: Blob::from(rgba),
            format: ImageFormat::Rgba8,
            alpha_type: ImageAlphaType::Alpha,
            width: w,
            height: h,
        })
    }
}
