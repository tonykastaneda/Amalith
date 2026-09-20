//! Pixel selection tools. Contours are stored in image-local coordinates.
use super::*;

impl App {
    pub(super) fn raster_selection_press(&mut self) {
        if self.current_layer_kind() != Some(amalith_core::LayerKind::Raster) {
            self.doc.io_error = Some("Select a raster layer to use pixel selection tools.".into());
            return;
        }
        let doc = self.doc.editor.document();
        let layer = self.doc.selection.first().and_then(|&id| panels::layers::owning_layer(doc, id)).or(self.doc.selected_layer);
        let selected = self.doc.selection.iter().copied().find(|&id| {
            matches!(doc.object(id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Image(_)))
                && panels::layers::owning_layer(doc, id) == layer
        });
        let hit = select::topmost_selectable_at(doc, self.doc_point(self.pointer), self.visible_doc_rect(), 0.0)
            .filter(|&id| panels::layers::owning_layer(doc, id) == layer)
            .filter(|&id| matches!(doc.object(id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Image(_))));
        let Some(object) = selected.or(hit) else {
            self.doc.io_error = Some("Select an image on this raster layer first.".into());
            self.request_main_redraw();
            return;
        };
        self.doc.io_error = None;
        self.doc.pixel_selection = None;
        let dp = self.doc_point(self.pointer);
        self.drag = Drag::RasterSelection { object, tool: self.active_tool, points: vec![dp, dp] };
        self.request_main_redraw();
    }

    pub(super) fn raster_selection_move(&mut self, object: ObjectId, tool: Tool, points: &mut Vec<Point>) {
        let dp = self.doc_point(self.pointer);
        if tool == Tool::RasterLasso {
            if points.last().is_none_or(|p| p.distance(dp) * self.doc.view.zoom >= 1.0) { points.push(dp); }
        } else if let Some(last) = points.last_mut() {
            *last = dp;
        }
        let doc = self.doc.editor.document();
        let Some(amalith_core::ObjectKind::Image(image)) = doc.object(object).map(|o| &o.kind) else { return };
        let inverse = doc.world_transform(object).inverse();
        if !inverse.as_coeffs().iter().all(|v| v.is_finite()) { return; }
        let contour = selection_contour(tool, points);
        let local: Vec<Point> = contour.into_iter().map(|p| {
            let p = inverse * amalith_core::Point::new(p.x, p.y);
            Point::new(p.x, p.y)
        }).collect();
        let contour = clip_contour(local, crate::convert::rect(image.local_bounds));
        self.doc.pixel_selection = (contour.len() >= 3).then_some(PixelSelection { object, contours: vec![contour] });
        self.request_main_redraw();
    }
}

fn selection_contour(tool: Tool, points: &[Point]) -> Vec<Point> {
    let (Some(&start), Some(&end)) = (points.first(), points.last()) else { return Vec::new() };
    if tool == Tool::RasterLasso { return points.to_vec(); }
    let r = Rect::from_points(start, end);
    if r.width() < 0.001 || r.height() < 0.001 { return Vec::new(); }
    if tool == Tool::RasterEllipse {
        (0..128).map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 128.0;
            Point::new(r.center().x + r.width() * 0.5 * angle.cos(), r.center().y + r.height() * 0.5 * angle.sin())
        }).collect()
    } else {
        vec![Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)]
    }
}

/// Sutherland–Hodgman clipping retains edge intersections rather than
/// clamping vertices, including selections begun outside the image.
fn clip_contour(mut points: Vec<Point>, bounds: Rect) -> Vec<Point> {
    for (axis, edge, greater) in [(0, bounds.x0, true), (0, bounds.x1, false), (1, bounds.y0, true), (1, bounds.y1, false)] {
        let coord = |p: Point| if axis == 0 { p.x } else { p.y };
        let inside = |p: Point| if greater { coord(p) >= edge } else { coord(p) <= edge };
        let mut output = Vec::new();
        if let Some(&last) = points.last() {
            let mut previous = last;
            for &point in &points {
                if inside(previous) != inside(point) {
                    let t = (edge - coord(previous)) / (coord(point) - coord(previous));
                    output.push(previous + (point - previous) * t);
                }
                if inside(point) { output.push(point); }
                previous = point;
            }
        }
        points = output;
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marquee_supports_reverse_drag_and_image_clipping() {
        let shape = selection_contour(Tool::RasterMarquee, &[Point::new(30., 30.), Point::new(-10., -10.)]);
        let clipped = clip_contour(shape, Rect::new(0., 0., 20., 20.));
        assert_eq!(clipped.len(), 4);
        for p in clipped { assert!((0.0..=20.0).contains(&p.x) && (0.0..=20.0).contains(&p.y)); }
    }
    #[test]
    fn outside_selection_is_empty_and_ellipse_is_closed_by_renderer() {
        let shape = selection_contour(Tool::RasterEllipse, &[Point::new(30., 30.), Point::new(50., 50.)]);
        assert_eq!(shape.len(), 128);
        assert!(clip_contour(shape, Rect::new(0., 0., 20., 20.)).is_empty());
        assert!(selection_contour(Tool::RasterMarquee, &[Point::ZERO, Point::ZERO]).is_empty());
    }
}
