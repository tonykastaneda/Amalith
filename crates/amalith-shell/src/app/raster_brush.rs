//! Round pixel brush, eraser, paint bucket, and Clone Stamp. Each
//! completed stroke creates an immutable embedded image asset, leaving
//! the previous bytes available to undo and duplicates.
use super::*;
use super::paint_tiles::PaintTiles;

/// Where a stamp's color comes from. `Flat` is Brush/Eraser/Fill's own
/// fixed color; `Clone` samples the immutable `base` image at a fixed
/// pixel-space offset from the destination, Photoshop's Clone Stamp.
#[derive(Clone, Copy)]
pub(super) enum PixelSource {
    Flat(image::Rgba<u8>),
    Clone { offset: vello::kurbo::Vec2 },
}

pub(super) struct Stroke {
    object: Option<ObjectId>,
    layer: amalith_core::LayerId,
    base: image::RgbaImage,
    ink: PaintTiles,
    pixel_to_doc: vello::kurbo::Affine,
    last: Point,
    radius: f64,
    hardness: f64,
    source: PixelSource,
    selection: Option<Vec<Vec<Point>>>,
    changed: bool,
    revision: u64,
    preview: Option<crate::lod::ImageLods>,
    erase: bool,
    preview_asset: amalith_core::AssetId,
    preview_doc: Option<Document>,
    /// Commit into `object`'s layer mask instead of its own pixels.
    editing_mask: bool,
}

impl Stroke {
    fn result(&self) -> image::RgbaImage {
        self.ink.result(&self.base, self.erase)
    }

    pub(super) fn preview_document(&self, revision: u64) -> Option<&Document> {
        (self.revision == revision).then_some(self.preview_doc.as_ref()).flatten()
    }

    /// The color a stamp centered wherever should lay down at `dest`
    /// (destination pixel-space, pixel center) — `None` means skip this
    /// pixel entirely (Clone Stamp sampling outside `base`'s bounds).
    fn color_at(&self, dest: Point) -> Option<image::Rgba<u8>> {
        match self.source {
            PixelSource::Flat(c) => Some(c),
            PixelSource::Clone { offset } => {
                let src = dest + offset;
                if src.x < 0.0 || src.y < 0.0 || src.x >= self.base.width() as f64 || src.y >= self.base.height() as f64 {
                    return None;
                }
                Some(*self.base.get_pixel(src.x as u32, src.y as u32))
            }
        }
    }

    fn bucket(&mut self) {
        let p = self.last;
        if p.x < 0. || p.y < 0. || p.x >= self.base.width() as f64 || p.y >= self.base.height() as f64 { return; }
        let PixelSource::Flat(color) = self.source else { return };
        let mask = crate::magicwand::flood_fill_masked(&self.base, (p.x.floor() as u32, p.y.floor() as u32), 32.0, |x, y| {
            self.selection.as_ref().is_none_or(|paths| inside(paths, Point::new(x as f64 + 0.5, y as f64 + 0.5)))
        });
        for y in 0..self.ink.height() { for x in 0..self.ink.width() {
            if mask.get(x as i64, y as i64) { self.ink.put_pixel(x,y,color); self.changed |= color[3] > 0; }
        }}
    }

    fn refresh_preview(&mut self) {
        if let Some(preview) = self.ink.publish(&self.base, self.erase) { self.preview = Some(preview); }
    }
    fn stamp(&mut self, p: Point) {
        let r = self.radius;
        let x0 = (p.x - r - 1.).max(0.) as u32;
        let y0 = (p.y - r - 1.).max(0.) as u32;
        let x1 = (p.x + r + 1.).max(0.).min(self.ink.width() as f64) as u32;
        let y1 = (p.y + r + 1.).max(0.).min(self.ink.height() as f64) as u32;
        for y in y0..y1 { for x in x0..x1 {
            let center = Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let coverage = super::brush_tip::coverage(center.distance(p), r, self.hardness);
            if coverage == 0. || self.selection.as_ref().is_some_and(|paths| !inside(paths, center)) { continue; }
            let Some(sampled) = self.color_at(center) else { continue };
            let alpha = (sampled[3] as f64 * coverage).round() as u8;
            if alpha > self.ink.get_pixel(x, y)[3] {
                let mut color = sampled;
                color[3] = alpha;
                self.ink.put_pixel(x, y, color);
                self.changed = true;
            }
        }}
    }

    pub(super) fn advance(&mut self, doc_point: Point) {
        let p = self.pixel_to_doc.inverse() * doc_point;
        if !p.x.is_finite() || !p.y.is_finite() { return; }
        let from = self.last;
        let steps = (from.distance(p) / (self.radius * 0.25).max(0.5)).ceil().max(1.) as usize;
        for i in 1..=steps { self.stamp(from + (p - from) * (i as f64 / steps as f64)); }
        self.last = p;
        self.refresh_preview();
    }
}

pub(super) fn inside(paths: &[Vec<Point>], p: Point) -> bool {
    let mut hit = false;
    for path in paths {
        let Some(&last) = path.last() else { continue };
        let mut a = last;
        for &b in path {
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x { hit = !hit; }
            a = b;
        }
    }
    hit
}

/// What a raster paint tool (Brush/Eraser/Fill/Clone Stamp) is about to
/// touch: an existing image (under the cursor or selected) or a brand-new
/// blank canvas on an empty Raster layer.
struct PaintTarget {
    object: Option<ObjectId>,
    layer: amalith_core::LayerId,
    base: image::RgbaImage,
    pixel_to_doc: vello::kurbo::Affine,
    local_to_pixel: vello::kurbo::Affine,
    /// Whether this target is `object`'s layer mask rather than its own
    /// pixels — set when `Doc::editing_mask` names this object and it
    /// actually has a mask. Only ever true when `object.is_some()`.
    editing_mask: bool,
}

impl App {
    /// Resolves the current paint target. `allow_new_canvas` is false for
    /// tools that only make sense against pixels that already exist
    /// (Eraser, Clone Stamp) — Brush and Fill pass `true` so an empty
    /// Raster layer gets a fresh canvas sized to the artboard under it.
    fn raster_paint_target(&mut self, allow_new_canvas: bool) -> Option<PaintTarget> {
        if self.current_layer_kind() != Some(amalith_core::LayerKind::Raster) { return None; }
        let doc = self.doc.editor.document();
        let layer = self.doc.selection.first().and_then(|&id| panels::layers::owning_layer(doc, id)).or(self.doc.selected_layer)?;
        if doc.layer(layer).is_none_or(|l| l.locked || !l.visible) { return None; }
        let selected = self.doc.selection.first().copied();
        let hit = select::topmost_selectable_at(doc, self.doc_point(self.pointer), self.visible_doc_rect(), 0.0);
        let object = selected.or(hit).filter(|&id| panels::layers::owning_layer(doc, id) == Some(layer));
        if let Some(id) = object {
            let obj = doc.object(id)?;
            let mut parent = Some(id);
            while let Some(pid) = parent {
                let o = doc.object(pid)?;
                if o.locked || !o.visible { return None; }
                parent = match o.parent { amalith_core::ObjectParent::Group(g) => Some(g), _ => None };
            }
            let amalith_core::ObjectKind::Image(image) = &obj.kind else {
                self.doc.io_error = Some("Shapes and text stay editable. Select a pixel image or an empty raster layer to paint.".into());
                return None;
            };
            let (bounds, world) = (image.local_bounds, crate::convert::affine(doc.world_transform(id)));
            let editing_mask = self.doc.editing_mask == Some(id);
            let asset = if editing_mask {
                let Some(mask) = image.mask else {
                    self.doc.io_error = Some("This image has no layer mask yet — click Mask in the Layers panel to add one.".into());
                    return None;
                };
                mask.asset
            } else {
                image.asset
            };
            self.magic_wand_cache = None;
            let base = self.magic_wand_image(asset).cloned()?;
            if base.width() as u64 * base.height() as u64 > 16_777_216 {
                self.doc.io_error = Some("Brush currently supports images up to 16 megapixels.".into());
                return None;
            }
            let pixels_to_local = vello::kurbo::Affine::translate((bounds.x0, bounds.y0))
                * vello::kurbo::Affine::scale_non_uniform(bounds.width() / base.width() as f64, bounds.height() / base.height() as f64);
            Some(PaintTarget { object: Some(id), layer, base, pixel_to_doc: world * pixels_to_local, local_to_pixel: pixels_to_local.inverse(), editing_mask })
        } else {
            if !allow_new_canvas { return None; }
            if !doc.children_of(amalith_core::ObjectParent::Layer(layer)).is_empty() {
                self.doc.io_error = Some("Select an image on this layer before painting.".into());
                return None;
            }
            let pointer = self.doc_point(self.pointer);
            let bounds = doc.artboards().iter().find(|a| crate::convert::rect(a.rect).contains(pointer))
                .or_else(|| doc.artboards().first()).map(|a| crate::convert::rect(a.rect)).unwrap_or(self.visible_doc_rect());
            let (w, h) = (bounds.width().ceil().max(1.) as u32, bounds.height().ceil().max(1.) as u32);
            if w as u64 * h as u64 > 16_777_216 { self.doc.io_error = Some("Brush canvas exceeds 16 megapixels.".into()); return None; }
            Some(PaintTarget {
                object: None, layer, base: image::RgbaImage::new(w, h),
                pixel_to_doc: vello::kurbo::Affine::translate((bounds.x0, bounds.y0)), local_to_pixel: vello::kurbo::Affine::IDENTITY,
                editing_mask: false,
            })
        }
    }

    /// A transient scene with `preview_asset` standing in for the real
    /// target image (or, on an empty layer, a brand-new one) — kept out
    /// of the document and undo history entirely (see
    /// `prepare_raster_preview`).
    pub(super) fn raster_preview_doc(&self, preview_asset: amalith_core::AssetId, object: Option<ObjectId>, layer: amalith_core::LayerId, base: &image::RgbaImage, pixel_to_doc: vello::kurbo::Affine, editing_mask: bool) -> Document {
        let mut doc = self.doc.editor.document().clone();
        if let Some(id) = object {
            if let Some(obj) = doc.object_mut(id) {
                if let amalith_core::ObjectKind::Image(image) = &mut obj.kind {
                    if editing_mask {
                        if let Some(mask) = &mut image.mask { mask.asset = preview_asset; }
                    } else {
                        image.asset = preview_asset;
                    }
                }
            }
        } else {
            let mut image = amalith_core::Object::new(ObjectId::new(), amalith_core::ObjectParent::Layer(layer), amalith_core::ObjectKind::Image(amalith_core::ImageData {
                asset: preview_asset, local_bounds: amalith_core::Rect::new(0., 0., base.width() as f64, base.height() as f64), mask: None,
            }));
            image.transform = crate::convert::affine_to_core(pixel_to_doc);
            let index = doc.children_of(amalith_core::ObjectParent::Layer(layer)).len();
            let _ = doc.insert_object(image, index);
        }
        doc.insert_asset(amalith_core::Asset::embedded(preview_asset, "Pixel preview", amalith_core::AssetKind::Image, "preview.png"), doc.assets().len());
        doc
    }

    pub(super) fn raster_brush_press(&mut self) {
        let erase = self.active_tool == Tool::RasterEraser;
        let Some(target) = self.raster_paint_target(!erase) else { return };
        let PaintTarget { object, layer, base, pixel_to_doc, local_to_pixel, editing_mask } = target;
        let Some(color) = (if erase { Some(amalith_core::Color::rgb(1.,1.,1.)) } else { self.doc.fill.color() }) else {
            self.doc.io_error = Some("Painting needs a solid foreground color.".into());
            return;
        };
        if !pixel_to_doc.inverse().as_coeffs().iter().all(|v| v.is_finite()) { return; }
        if self.doc.pixel_selection.as_ref().is_some_and(|s| Some(s.object) != object) {
            self.doc.io_error = Some("Deselect the other image's pixels before painting.".into()); return;
        }
        let selection = self.doc.pixel_selection.as_ref().map(|s| s.contours.iter().map(|path| path.iter().map(|&p| local_to_pixel * p).collect()).collect());
        let start = pixel_to_doc.inverse() * self.doc_point(self.pointer);
        let preview_asset = amalith_core::AssetId::new();
        let preview_doc = Some(self.raster_preview_doc(preview_asset, object, layer, &base, pixel_to_doc, editing_mask));
        let mut stroke = Stroke {
            object, layer, ink: PaintTiles::new(base.width(), base.height()), base, pixel_to_doc,
            last: start, radius: if erase { self.raster_eraser_size } else { self.raster_brush_size } * 0.5,
            hardness: if erase { self.raster_eraser_hardness } else { self.raster_brush_hardness },
            source: PixelSource::Flat(image::Rgba([color.r, color.g, color.b, color.a * self.doc.opacity as f32].map(|v| (v.clamp(0., 1.) * 255.).round() as u8))),
            selection, changed: false, revision: self.doc.editor.revision(), preview: None,
            erase, preview_asset, preview_doc, editing_mask,
        };
        if self.active_tool == Tool::RasterFill {
            stroke.bucket();
            self.commit_raster_brush(stroke);
            return;
        }
        stroke.stamp(start);
        stroke.refresh_preview();
        self.doc.io_error = None;
        self.drag = Drag::RasterBrush(Box::new(stroke));
        self.request_main_redraw();
    }

    /// Option-click sets the clone source; a plain click paints, sampling
    /// from that source offset by however far the destination has moved
    /// (Aligned) or from the original source point every new stroke (not
    /// Aligned). Scoped to one image at a time — the source and the paint
    /// target must be the same object, matching Brush's own single-target
    /// scope and `magic_wand_cache` only ever holding one decoded image.
    pub(super) fn raster_clone_stamp_press(&mut self) {
        let dp = self.doc_point(self.pointer);
        if self.alt_down {
            let Some(target) = self.raster_paint_target(false) else { return };
            let Some(object) = target.object else {
                self.doc.io_error = Some("Option-click a pixel image to set a clone source.".into());
                self.request_main_redraw();
                return;
            };
            let anchor = target.local_to_pixel * (target.pixel_to_doc.inverse() * dp);
            self.raster_clone_source = Some((object, anchor));
            self.raster_clone_offset = None;
            self.doc.io_error = None;
            self.request_main_redraw();
            return;
        }
        let Some(target) = self.raster_paint_target(false) else { return };
        let PaintTarget { object, layer, base, pixel_to_doc, local_to_pixel, editing_mask } = target;
        let Some((source_object, source_anchor)) = self.raster_clone_source else {
            self.doc.io_error = Some("Option-click to set a clone source first.".into());
            return;
        };
        if Some(source_object) != object {
            self.doc.io_error = Some("Option-click a source on this image before cloning.".into());
            return;
        }
        if !pixel_to_doc.inverse().as_coeffs().iter().all(|v| v.is_finite()) { return; }
        if self.doc.pixel_selection.as_ref().is_some_and(|s| Some(s.object) != object) {
            self.doc.io_error = Some("Deselect the other image's pixels before painting.".into()); return;
        }
        let selection = self.doc.pixel_selection.as_ref().map(|s| s.contours.iter().map(|path| path.iter().map(|&p| local_to_pixel * p).collect()).collect());
        let start = pixel_to_doc.inverse() * dp;
        let offset = if self.raster_clone_aligned {
            *self.raster_clone_offset.get_or_insert(source_anchor - start)
        } else {
            let o = source_anchor - start;
            self.raster_clone_offset = Some(o);
            o
        };
        let preview_asset = amalith_core::AssetId::new();
        let preview_doc = Some(self.raster_preview_doc(preview_asset, object, layer, &base, pixel_to_doc, editing_mask));
        let mut stroke = Stroke {
            object, layer, ink: PaintTiles::new(base.width(), base.height()), base, pixel_to_doc,
            last: start, radius: self.raster_clone_size * 0.5, hardness: self.raster_clone_hardness,
            source: PixelSource::Clone { offset },
            selection, changed: false, revision: self.doc.editor.revision(), preview: None,
            erase: false, preview_asset, preview_doc, editing_mask,
        };
        stroke.stamp(start);
        stroke.refresh_preview();
        self.doc.io_error = None;
        self.drag = Drag::RasterBrush(Box::new(stroke));
        self.request_main_redraw();
    }

    pub(super) fn commit_raster_brush(&mut self, stroke: Stroke) {
        if !stroke.changed || stroke.revision != self.doc.editor.revision() { return; }
        let result = stroke.result();
        if result == stroke.base { return; }
        let mut bytes = std::io::Cursor::new(Vec::new());
        if let Err(error) = result.write_to(&mut bytes, image::ImageFormat::Png) {
            self.doc.io_error = Some(format!("Brush could not encode pixels: {error}")); return;
        }
        let path = format!("images/brush-{}.png", amalith_core::AssetId::new());
        self.doc.asset_store.insert(&path, bytes.into_inner());
        let command = if let Some(object) = stroke.object {
            let asset = amalith_core::Asset::embedded(amalith_core::AssetId::new(), "Brush pixels", amalith_core::AssetKind::Image, &path);
            if stroke.editing_mask {
                Command::ReplaceMaskAsset { object, asset }
            } else {
                Command::ReplaceImageAsset { object, asset }
            }
        } else {
            Command::CreateImage { layer: stroke.layer, path, bounds: amalith_core::Rect::new(0., 0., stroke.base.width() as f64, stroke.base.height() as f64), transform: crate::convert::affine_to_core(stroke.pixel_to_doc), name: Some("Paint".into()), embedded: true, modified: None, size: None }
        };
        match self.doc.editor.execute(command) {
            Ok(CommandOutcome::Object(id)) => self.doc.selection = vec![id],
            Ok(_) => {},
            Err(error) => self.doc.io_error = Some(format!("Brush failed: {error}")),
        }
        self.magic_wand_cache = None;
        self.request_main_redraw();
    }

    pub(super) fn paint_raster_brush_preview(&mut self) {
        if matches!(self.active_tool, Tool::RasterBrush | Tool::RasterEraser | Tool::RasterCloneStamp) && self.canvas_viewport().contains(self.pointer) && matches!(self.drag, Drag::None) {
            let (size, hardness) = match self.active_tool {
                Tool::RasterEraser => (self.raster_eraser_size, self.raster_eraser_hardness),
                Tool::RasterCloneStamp => (self.raster_clone_size, self.raster_clone_hardness),
                _ => (self.raster_brush_size, self.raster_brush_hardness),
            };
            let label = if self.active_tool == Tool::RasterCloneStamp {
                format!("{} px · {:.0}% hard · {}", size as u32, hardness * 100.0, if self.raster_clone_aligned { "Aligned" } else { "Not Aligned" })
            } else {
                format!("{} px · {:.0}% hard", size as u32, hardness * 100.0)
            };
            self.text.draw(&mut self.content, &label, 11.0, self.theme.text, self.pointer.x + 12.0, self.pointer.y + 16.0);
        }
        let Drag::RasterBrush(stroke) = &self.drag else { return };
        if stroke.preview.is_none() { return; }
        self.content.push_clip_layer(Fill::NonZero, ID, &self.canvas_viewport());
        let to_screen = self.doc.view.to_screen() * stroke.pixel_to_doc;
        self.content.stroke(&vello::kurbo::Stroke::new(1.0 / self.doc.view.zoom), to_screen, vello::peniko::Color::WHITE, None, &vello::kurbo::Circle::new(stroke.last, stroke.radius));
        if let PixelSource::Clone { offset } = stroke.source {
            // The moving source crosshair, so aiming stays visible while
            // the destination-side circle above tracks the actual cursor.
            let source = stroke.last + offset;
            let gold = vello::peniko::Color::from_rgb8(0xff, 0xd5, 0x00);
            self.content.stroke(&vello::kurbo::Stroke::new(1.0 / self.doc.view.zoom), to_screen, gold, None, &vello::kurbo::Circle::new(source, stroke.radius));
            let cross = 4.0 / self.doc.view.zoom;
            self.content.stroke(&vello::kurbo::Stroke::new(1.0 / self.doc.view.zoom), to_screen, gold, None,
                &vello::kurbo::Line::new(Point::new(source.x - cross, source.y), Point::new(source.x + cross, source.y)));
            self.content.stroke(&vello::kurbo::Stroke::new(1.0 / self.doc.view.zoom), to_screen, gold, None,
                &vello::kurbo::Line::new(Point::new(source.x, source.y - cross), Point::new(source.x, source.y + cross)));
        }
        self.content.pop_layer();
    }

    /// Keep the transient asset out of the document and undo history.
    pub(super) fn prepare_raster_preview(&mut self) {
        let next = match &self.drag {
            Drag::RasterBrush(stroke) if stroke.preview_document(self.doc.editor.revision()).is_some() => {
                stroke.preview.as_ref().map(|gpu| (stroke.preview_asset, gpu.clone()))
            }
            _ => None,
        };
        if self.raster_preview_asset != next.as_ref().map(|(id, _)| *id) {
            if let Some(old) = self.raster_preview_asset.take() { self.image_cache.remove(&old); }
        }
        if let Some((id, lods)) = next {
            self.image_cache.insert(id, lods);
            self.raster_preview_asset = Some(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn stroke() -> Stroke {
        Stroke { object: None, layer: amalith_core::LayerId::new(), base: image::RgbaImage::new(32, 32), ink: PaintTiles::new(32, 32), pixel_to_doc: vello::kurbo::Affine::IDENTITY, last: Point::new(4., 16.), radius: 2., hardness: 1.0, source: PixelSource::Flat(image::Rgba([255, 0, 0, 128])), selection: None, changed: false, revision: 0, preview: None, erase: false, preview_asset: amalith_core::AssetId::new(), preview_doc: None, editing_mask: false }
    }
    fn flat(s: &mut Stroke) -> &mut image::Rgba<u8> {
        let PixelSource::Flat(c) = &mut s.source else { unreachable!() };
        c
    }
    #[test]
    fn fast_strokes_have_no_gaps_and_opacity_does_not_accumulate() {
        let mut s = stroke();
        s.stamp(s.last);
        s.advance(Point::new(28., 16.));
        for x in 4..28 { assert_eq!(s.ink.get_pixel(x, 16)[3], 128); }
        s.advance(Point::new(4., 16.));
        assert_eq!(s.ink.get_pixel(16, 16)[3], 128);
        assert_eq!(s.base.get_pixel(16, 16)[3], 0);
    }
    #[test]
    fn brush_respects_selection_holes_and_image_edges() {
        let mut s = stroke();
        let rect = |a, b| vec![Point::new(a,a), Point::new(b,a), Point::new(b,b), Point::new(a,b)];
        s.selection = Some(vec![rect(0.,32.), rect(12.,20.)]);
        s.radius = 30.;
        s.stamp(Point::new(16.,16.));
        assert_eq!(s.ink.get_pixel(16,16)[3],0);
        assert_eq!(s.ink.get_pixel(5,5)[3],128);
        s.stamp(Point::new(-100.,-100.));
    }

    #[test]
    fn eraser_reduces_alpha_without_changing_color_or_unselected_pixels() {
        let mut s = stroke();
        s.erase = true;
        s.base = image::RgbaImage::from_pixel(32, 32, image::Rgba([40,80,120,200]));
        s.selection = Some(vec![vec![Point::new(0.,0.),Point::new(16.,0.),Point::new(16.,32.),Point::new(0.,32.)]]);
        s.radius = 30.;
        s.stamp(Point::new(16.,16.));
        let result = s.result();
        assert_eq!(result.get_pixel(8,16).0, [40,80,120,100]);
        assert_eq!(result.get_pixel(24,16).0, [40,80,120,200]);
        assert_eq!(s.base.get_pixel(8,16)[3], 200);
        flat(&mut s)[3] = 255;
        s.stamp(Point::new(16.,16.));
        assert_eq!(s.result().get_pixel(8,16)[3], 0);
    }

    #[test]
    fn soft_brush_and_eraser_share_feathered_coverage() {
        let mut s = stroke();
        s.radius = 10.0;
        s.hardness = 0.0;
        s.source = PixelSource::Flat(image::Rgba([255,0,0,255]));
        s.stamp(Point::new(16.5,16.5));
        let painted = s.result();
        assert_eq!(painted.get_pixel(16,16)[3],255);
        assert!(painted.get_pixel(21,16)[3] > painted.get_pixel(24,16)[3]);
        assert_eq!(painted.get_pixel(26,16)[3],0);
        s.erase = true;
        s.base = image::RgbaImage::from_pixel(32,32,image::Rgba([20,40,60,255]));
        let erased = s.result();
        for x in 16..27 {
            assert_eq!(erased.get_pixel(x,16)[3] as u16 + painted.get_pixel(x,16)[3] as u16,255);
        }
    }

    #[test]
    fn bucket_stops_at_color_and_selection_boundaries() {
        let mut s = stroke();
        s.source = PixelSource::Flat(image::Rgba([255,0,0,255]));
        s.base = image::RgbaImage::from_pixel(32,32,image::Rgba([255,255,255,255]));
        for y in 0..32 { s.base.put_pixel(16,y,image::Rgba([0,0,0,255])); }
        s.bucket();
        let result = s.result();
        assert_eq!(result.get_pixel(4,16).0, [255,0,0,255]);
        assert_eq!(result.get_pixel(16,16).0, [0,0,0,255]);
        assert_eq!(result.get_pixel(20,16).0, [255,255,255,255]);

        // Disconnected selected islands must not connect through excluded pixels.
        s.ink = PaintTiles::new(32,32);
        s.base = image::RgbaImage::new(32,32);
        let band = |a,b| vec![Point::new(a,0.),Point::new(b,0.),Point::new(b,32.),Point::new(a,32.)];
        s.selection = Some(vec![band(0.,10.),band(20.,32.)]);
        s.bucket();
        let result = s.result();
        assert_eq!(result.get_pixel(4,16)[3],255);
        assert_eq!(result.get_pixel(15,16)[3],0);
        assert_eq!(result.get_pixel(24,16)[3],0);
    }

    #[test]
    fn bucket_outside_image_is_no_op_and_transparent_regions_ignore_hidden_rgb() {
        let mut s = stroke();
        s.last = Point::new(-0.5,16.);
        s.bucket();
        assert!(!s.changed);
        s.last = Point::new(4.,16.);
        s.base.put_pixel(5,16,image::Rgba([100,150,200,0]));
        s.bucket();
        assert_eq!(s.result().get_pixel(5,16).0,[255,0,0,128]);
    }

    #[test]
    fn clone_stamp_samples_the_immutable_base_not_the_growing_ink() {
        let mut s = stroke();
        s.base = image::RgbaImage::from_fn(32, 32, |x, _| image::Rgba([x as u8, 0, 0, 255]));
        s.source = PixelSource::Clone { offset: vello::kurbo::Vec2::new(-10.0, 0.0) };
        s.radius = 3.0;
        // Painting over the same spot repeatedly must not smear the
        // already-painted `ink` into later samples — every stamp reads
        // color from `base`, which never changes mid-stroke.
        s.stamp(Point::new(20.0, 16.0));
        s.stamp(Point::new(20.0, 16.0));
        let expected = s.base.get_pixel(10, 16)[0];
        assert_eq!(s.result().get_pixel(20, 16)[0], expected);
    }

    #[test]
    fn clone_stamp_skips_pixels_whose_source_falls_outside_the_image() {
        let mut s = stroke();
        s.source = PixelSource::Clone { offset: vello::kurbo::Vec2::new(-100.0, 0.0) };
        s.radius = 3.0;
        s.stamp(Point::new(16.0, 16.0));
        assert!(!s.changed);
        assert_eq!(s.result(), s.base);
    }
}
