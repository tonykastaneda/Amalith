//! Selected-pixel transform: Free Transform, scoped to `doc.pixel_selection`
//! instead of the whole object. Lifts the selected pixels into a floating
//! buffer (rendered live via a plain GPU image draw — no CPU resampling
//! needed until commit, which reuses the same `ReplaceImageAsset`
//! copy-on-write path every other raster edit already commits through.
use super::*;
use image::Pixel;

/// Which handle-drag started this gesture — captured once at press so a
/// modifier change mid-drag never re-derives a different gesture kind.
#[derive(Clone, Copy)]
enum Gesture {
    Move,
    Scale(Handle),
    Rotate,
}

pub(super) struct Lift {
    object: ObjectId,
    /// The original, untouched image (for the final "nothing moved" check).
    base: image::RgbaImage,
    /// The lifted selection's own pixels, cropped to its bounding box.
    floating: image::RgbaImage,
    floating_gpu: vello::peniko::ImageData,
    /// Pixel-space top-left of `floating` within `base`.
    floating_origin: Point,
    /// `base` with the selection area cleared to transparent — what's
    /// left behind once the pixels are lifted.
    hole: image::RgbaImage,
    hole_preview: crate::lod::ImageLods,
    pixel_to_doc: Affine,
    gesture: Gesture,
    start_pointer: Point,
    start_bounds: Rect,
    start_angle: f64,
    live_xf: Affine,
    preview_asset: amalith_core::AssetId,
    revision: u64,
    preview_doc: Option<Document>,
}

impl Lift {
    fn preview_document(&self, revision: u64) -> Option<&Document> {
        (self.revision == revision).then_some(self.preview_doc.as_ref()).flatten()
    }

    /// The doc-space transform placing `floating`'s own `(0, 0)` where it
    /// was lifted from, before `live_xf` is applied on top.
    fn floating_placement(&self) -> Affine {
        self.pixel_to_doc * Affine::translate(self.floating_origin.to_vec2())
    }

    /// Recompute live from press-time state — never accumulates, so a
    /// modifier flip mid-drag can't compound with the previous frame.
    pub(super) fn update(&mut self, dp: Point, uniform: bool, from_center: bool) {
        self.live_xf = match self.gesture {
            Gesture::Move => Affine::translate(dp - self.start_pointer),
            Gesture::Scale(h) => handles::scaled_transform(self.start_bounds, h, dp, uniform, from_center),
            Gesture::Rotate => handles::rotate_transform(self.start_bounds.center(), self.start_angle, dp, uniform),
        };
    }
}

/// Crops `base` to the selection contours' bounding box (already in the
/// image's own pixel space), masking every pixel outside the contours to
/// transparent, and clears that same area in a clone of `base`.
fn lift(base: &image::RgbaImage, contours: &[Vec<Point>]) -> (image::RgbaImage, Point, image::RgbaImage) {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in contours {
        for &p in c {
            x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y);
        }
    }
    let x0 = x0.floor().max(0.0) as u32;
    let y0 = y0.floor().max(0.0) as u32;
    let x1 = (x1.ceil() as u32).min(base.width()).max(x0 + 1);
    let y1 = (y1.ceil() as u32).min(base.height()).max(y0 + 1);
    let mut floating = image::RgbaImage::new(x1 - x0, y1 - y0);
    let mut hole = base.clone();
    for y in y0..y1 {
        for x in x0..x1 {
            let center = Point::new(x as f64 + 0.5, y as f64 + 0.5);
            if raster_brush::inside(contours, center) {
                floating.put_pixel(x - x0, y - y0, *base.get_pixel(x, y));
                hole.put_pixel(x, y, image::Rgba([0, 0, 0, 0]));
            }
        }
    }
    (floating, Point::new(x0 as f64, y0 as f64), hole)
}

fn to_gpu_image(img: &image::RgbaImage) -> vello::peniko::ImageData {
    vello::peniko::ImageData {
        width: img.width(),
        height: img.height(),
        data: vello::peniko::Blob::from(img.as_raw().clone()),
        format: vello::peniko::ImageFormat::Rgba8,
        alpha_type: vello::peniko::ImageAlphaType::Alpha,
    }
}

fn bilinear(img: &image::RgbaImage, x: f64, y: f64) -> Option<image::Rgba<u8>> {
    let (w, h) = (img.width() as f64, img.height() as f64);
    if x < -0.5 || y < -0.5 || x > w - 0.5 || y > h - 0.5 { return None; }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let sample = |xi: f64, yi: f64| -> [f64; 4] {
        let xi = xi.clamp(0.0, w - 1.0) as u32;
        let yi = yi.clamp(0.0, h - 1.0) as u32;
        img.get_pixel(xi, yi).0.map(|c| c as f64)
    };
    let (p00, p10, p01, p11) = (sample(x0, y0), sample(x0 + 1.0, y0), sample(x0, y0 + 1.0), sample(x0 + 1.0, y0 + 1.0));
    let mut out = [0u8; 4];
    for i in 0..4 {
        let top = p00[i] * (1.0 - fx) + p10[i] * fx;
        let bot = p01[i] * (1.0 - fx) + p11[i] * fx;
        out[i] = (top * (1.0 - fy) + bot * fy).round().clamp(0.0, 255.0) as u8;
    }
    Some(image::Rgba(out))
}

/// Bakes `src` into `dst` at `xf` (mapping `src`'s local pixel space into
/// `dst`'s), bilinear-sampled and alpha-composited over whatever's
/// already there. Pixels whose inverse-mapped source falls outside
/// `src`'s bounds are left untouched.
fn resample_into(dst: &mut image::RgbaImage, src: &image::RgbaImage, xf: Affine) {
    if !xf.as_coeffs().iter().all(|v| v.is_finite()) { return; }
    let inv = xf.inverse();
    if !inv.as_coeffs().iter().all(|v| v.is_finite()) { return; }
    let corners = [
        Point::new(0.0, 0.0),
        Point::new(src.width() as f64, 0.0),
        Point::new(src.width() as f64, src.height() as f64),
        Point::new(0.0, src.height() as f64),
    ].map(|p| xf * p);
    let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in corners { x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y); }
    let x0 = x0.floor().max(0.0) as u32;
    let y0 = y0.floor().max(0.0) as u32;
    let x1 = (x1.ceil() as u32).min(dst.width());
    let y1 = (y1.ceil() as u32).min(dst.height());
    for y in y0..y1 {
        for x in x0..x1 {
            let p = inv * Point::new(x as f64 + 0.5, y as f64 + 0.5);
            let Some(sampled) = bilinear(src, p.x - 0.5, p.y - 0.5) else { continue };
            if sampled[3] == 0 { continue; }
            let mut px = *dst.get_pixel(x, y);
            px.blend(&sampled);
            dst.put_pixel(x, y, px);
        }
    }
}

impl App {
    /// Enters pixel-transform mode if the press lands on a handle, the
    /// rotate halo, or inside the current pixel selection's bounds.
    /// `false` (no drag started) otherwise — the caller falls through to
    /// the ordinary object Free Transform in that case.
    pub(super) fn pixel_transform_press(&mut self) -> bool {
        let Some(sel) = self.doc.pixel_selection.clone() else { return false };
        let doc = self.doc.editor.document();
        let Some(layer) = panels::layers::owning_layer(doc, sel.object) else { return false };
        let Some(amalith_core::ObjectKind::Image(img)) = doc.object(sel.object).map(|o| &o.kind) else { return false };
        let (asset, bounds, world) = (img.asset, img.local_bounds, convert::affine(doc.world_transform(sel.object)));
        self.magic_wand_cache = None;
        let Some(base) = self.magic_wand_image(asset).cloned() else { return false };
        let pixels_to_local = Affine::translate((bounds.x0, bounds.y0))
            * Affine::scale_non_uniform(bounds.width() / base.width() as f64, bounds.height() / base.height() as f64);
        let pixel_to_doc = world * pixels_to_local;
        if !pixel_to_doc.inverse().as_coeffs().iter().all(|v| v.is_finite()) { return false; }

        let (floating, floating_origin, hole) = lift(&base, &sel.contours);
        let floating_placement = pixel_to_doc * Affine::translate(floating_origin.to_vec2());
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(floating.width() as f64, 0.0),
            Point::new(floating.width() as f64, floating.height() as f64),
            Point::new(0.0, floating.height() as f64),
        ].map(|p| floating_placement * p);
        let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in corners { x0 = x0.min(p.x); y0 = y0.min(p.y); x1 = x1.max(p.x); y1 = y1.max(p.y); }
        let start_bounds = Rect::new(x0, y0, x1, y1);

        let to_screen = self.doc.view.to_screen();
        let scr_quad = handles::rect_quad(start_bounds).map(|p| to_screen * p);
        let dp = self.doc_point(self.pointer);
        let gesture = if let Some(h) = handles::hit_handle(self.pointer, scr_quad) {
            Gesture::Scale(h)
        } else if handles::hit_rotate_halo(self.pointer, scr_quad) {
            Gesture::Rotate
        } else if start_bounds.contains(dp) {
            Gesture::Move
        } else {
            return false;
        };

        let preview_asset = amalith_core::AssetId::new();
        let mut hole_preview = crate::lod::ImageLods::default();
        hole_preview.set(2, to_gpu_image(&hole));
        let preview_doc = Some(self.raster_preview_doc(preview_asset, Some(sel.object), layer, &hole, pixel_to_doc));
        let floating_gpu = to_gpu_image(&floating);
        let lift = Lift {
            object: sel.object, base, floating, floating_gpu, floating_origin, hole, hole_preview,
            pixel_to_doc, gesture, start_pointer: dp, start_bounds,
            start_angle: handles::angle_to(start_bounds.center(), dp),
            live_xf: Affine::IDENTITY, preview_asset, revision: self.doc.editor.revision(), preview_doc,
        };
        self.drag = Drag::PixelTransform(Box::new(lift));
        self.request_main_redraw();
        true
    }

    pub(super) fn commit_pixel_transform(&mut self, lift: Lift) {
        if lift.revision != self.doc.editor.revision() { return; }
        let placement = lift.floating_placement();
        let pixel_xf = lift.pixel_to_doc.inverse() * lift.live_xf * placement;
        let mut result = lift.hole.clone();
        resample_into(&mut result, &lift.floating, pixel_xf);
        if result == lift.base { return; }
        let mut bytes = std::io::Cursor::new(Vec::new());
        if let Err(error) = result.write_to(&mut bytes, image::ImageFormat::Png) {
            self.doc.io_error = Some(format!("Transform could not encode pixels: {error}"));
            return;
        }
        let path = format!("images/transform-{}.png", amalith_core::AssetId::new());
        self.doc.asset_store.insert(&path, bytes.into_inner());
        let command = Command::ReplaceImageAsset {
            object: lift.object,
            asset: amalith_core::Asset::embedded(amalith_core::AssetId::new(), "Transformed pixels", amalith_core::AssetKind::Image, &path),
        };
        match self.doc.editor.execute(command) {
            Ok(_) => self.doc.pixel_selection = None,
            Err(error) => self.doc.io_error = Some(format!("Transform failed: {error}")),
        }
        self.magic_wand_cache = None;
        self.request_main_redraw();
    }

    pub(super) fn paint_pixel_transform_preview(&mut self) {
        let Drag::PixelTransform(lift) = &self.drag else { return };
        let to_screen = self.doc.view.to_screen();
        let m = to_screen * lift.live_xf * lift.floating_placement();
        self.content.push_clip_layer(Fill::NonZero, ID, &self.canvas_viewport());
        self.content.draw_image(&lift.floating_gpu, m);
        let quad = handles::rect_quad(Rect::new(0.0, 0.0, lift.floating.width() as f64, lift.floating.height() as f64)).map(|p| m * p);
        let mut path = BezPath::new();
        path.move_to(quad[0]);
        for &p in &quad[1..] { path.line_to(p); }
        path.close_path();
        self.content.stroke(&vello::kurbo::Stroke::new(1.0), Affine::IDENTITY, vello::peniko::Color::WHITE, None, &path);
        self.content.pop_layer();
    }

    /// Keep the transient "hole" asset out of the document and undo
    /// history, same convention as `prepare_raster_preview`.
    pub(super) fn prepare_pixel_transform_preview(&mut self) {
        let next = match &self.drag {
            Drag::PixelTransform(lift) if lift.preview_document(self.doc.editor.revision()).is_some() => {
                Some((lift.preview_asset, lift.hole_preview.clone()))
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

    fn solid(w: u32, h: u32, c: [u8; 4]) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba(c))
    }

    #[test]
    fn identity_resample_reconstructs_the_original_pixels() {
        let src = image::RgbaImage::from_fn(8, 8, |x, y| image::Rgba([x as u8 * 10, y as u8 * 10, 0, 255]));
        let mut dst = image::RgbaImage::new(8, 8);
        resample_into(&mut dst, &src, Affine::IDENTITY);
        assert_eq!(dst, src);
    }

    #[test]
    fn translated_resample_lands_at_the_offset() {
        let src = solid(4, 4, [255, 0, 0, 255]);
        let mut dst = image::RgbaImage::new(10, 10);
        resample_into(&mut dst, &src, Affine::translate((3.0, 2.0)));
        assert_eq!(*dst.get_pixel(4, 3), image::Rgba([255, 0, 0, 255]));
        assert_eq!(*dst.get_pixel(0, 0), image::Rgba([0, 0, 0, 0]));
    }

    #[test]
    fn out_of_bounds_source_leaves_destination_untouched() {
        let src = solid(4, 4, [255, 0, 0, 255]);
        let mut dst = solid(4, 4, [0, 255, 0, 255]);
        resample_into(&mut dst, &src, Affine::translate((100.0, 100.0)));
        assert_eq!(dst, solid(4, 4, [0, 255, 0, 255]));
    }

    #[test]
    fn transparent_floating_pixels_do_not_overwrite_the_hole() {
        let mut src = image::RgbaImage::new(2, 2);
        src.put_pixel(0, 0, image::Rgba([10, 20, 30, 255]));
        let mut dst = solid(2, 2, [1, 2, 3, 255]);
        resample_into(&mut dst, &src, Affine::IDENTITY);
        assert_eq!(*dst.get_pixel(0, 0), image::Rgba([10, 20, 30, 255]));
        assert_eq!(*dst.get_pixel(1, 1), image::Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn lift_crops_to_the_contour_bbox_and_punches_a_hole() {
        let base = image::RgbaImage::from_fn(10, 10, |x, y| image::Rgba([x as u8, y as u8, 0, 255]));
        let square = vec![Point::new(2.0, 2.0), Point::new(6.0, 2.0), Point::new(6.0, 6.0), Point::new(2.0, 6.0)];
        let (floating, origin, hole) = lift(&base, &[square]);
        assert_eq!((floating.width(), floating.height()), (4, 4));
        assert_eq!(origin, Point::new(2.0, 2.0));
        assert_eq!(*floating.get_pixel(0, 0), *base.get_pixel(2, 2));
        assert_eq!(hole.get_pixel(3, 3)[3], 0);
        assert_eq!(hole.get_pixel(0, 0)[3], 255);
    }
}
