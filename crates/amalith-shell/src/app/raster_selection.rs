//! Pixel selection tools. Contours are stored in image-local coordinates.
use super::*;

pub(super) struct PolygonSelection {
    object: ObjectId,
    /// Fixed anchors in document space. The pointer supplies the moving edge.
    points: Vec<Point>,
}

impl App {
    pub(super) fn raster_polygon_press(&mut self, double: bool) {
        let point = self.doc_point(self.pointer);
        if let Some(polygon) = &mut self.raster_polygon {
            // A click near the first anchor closes the polygon. The second
            // event of a double-click closes it without adding a duplicate.
            let close = polygon.points.len() >= 3
                && (double || (self.doc.view.to_screen() * polygon.points[0] - self.pointer).hypot() <= 6.0);
            if close {
                self.finish_raster_polygon();
                return;
            }
            if polygon.points.last().is_none_or(|p| p.distance(point) * self.doc.view.zoom >= 1.0) {
                polygon.points.push(point);
            }
            self.request_main_redraw();
            return;
        }
        if self.current_layer_kind() != Some(amalith_core::LayerKind::Raster) {
            self.doc.io_error = Some("Select a raster layer to use pixel selection tools.".into());
            self.request_main_redraw();
            return;
        }
        let object = match self.raster_target() {
            Ok(Some((_, object))) => object,
            Ok(None) => {
                self.doc.io_error = Some("This layer has no pixels yet. Use Create Sublayer to add a pixel layer.".into());
                self.request_main_redraw();
                return;
            }
            Err(message) => {
                self.doc.io_error = Some(message.into());
                self.request_main_redraw();
                return;
            }
        };
        self.doc.io_error = None;
        self.doc.pixel_selection = None;
        self.raster_polygon = Some(PolygonSelection { object, points: vec![point] });
        self.request_main_redraw();
    }

    pub(super) fn finish_raster_polygon(&mut self) {
        let Some(polygon) = self.raster_polygon.take() else { return };
        let doc = self.doc.editor.document();
        let Some(amalith_core::ObjectKind::Image(image)) = doc.object(polygon.object).map(|o| &o.kind) else {
            self.request_main_redraw();
            return;
        };
        let inverse = doc.world_transform(polygon.object).inverse();
        if !inverse.as_coeffs().iter().all(|v| v.is_finite()) {
            self.request_main_redraw();
            return;
        }
        let local: Vec<Point> = polygon.points.into_iter().map(|p| {
            let p = inverse * amalith_core::Point::new(p.x, p.y);
            Point::new(p.x, p.y)
        }).collect();
        let clipped = clip_contour(local, crate::convert::rect(image.local_bounds));
        self.doc.pixel_selection = valid_polygon(&clipped).then_some(PixelSelection {
            object: polygon.object, contours: vec![clipped],
        });
        self.request_main_redraw();
    }

    pub(super) fn paint_raster_polygon_preview(&mut self) {
        let Some(polygon) = &self.raster_polygon else { return };
        if self.active_tool != Tool::RasterPolygonLasso { return; }
        let to_screen = self.doc.view.to_screen();
        let mut path = vello::kurbo::BezPath::new();
        let Some(&first) = polygon.points.first() else { return };
        path.move_to(to_screen * first);
        for &point in &polygon.points[1..] { path.line_to(to_screen * point); }
        if self.canvas_viewport().contains(self.pointer) { path.line_to(self.pointer); }
        self.content.push_clip_layer(vello::peniko::Fill::NonZero, ID, &self.canvas_viewport());
        self.content.stroke(&vello::kurbo::Stroke::new(1.5), ID, self.theme.accent, None, &path);
        for &point in &polygon.points {
            self.content.fill(vello::peniko::Fill::NonZero, ID, self.theme.accent, None,
                &vello::kurbo::Circle::new(to_screen * point, 2.5));
        }
        self.content.pop_layer();
    }

    pub(super) fn raster_selection_press(&mut self) {
        if self.current_layer_kind() != Some(amalith_core::LayerKind::Raster) {
            self.doc.io_error = Some("Select a raster layer to use pixel selection tools.".into());
            return;
        }
        // The same pixel layer Brush would paint.
        let object = match self.raster_target() {
            Ok(Some((_, object))) => object,
            Ok(None) => {
                self.doc.io_error = Some("This layer has no pixels yet. Use Create Sublayer to add a pixel layer.".into());
                self.request_main_redraw();
                return;
            }
            Err(message) => {
                self.doc.io_error = Some(message.into());
                self.request_main_redraw();
                return;
            }
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

fn valid_polygon(points: &[Point]) -> bool {
    if points.len() < 3 { return false; }
    let area: f64 = points.iter().zip(points.iter().cycle().skip(1))
        .take(points.len()).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
    area.abs() > 0.001
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
    #[test]
    fn polygon_needs_three_noncollinear_anchors_and_clips_to_image() {
        assert!(!valid_polygon(&[Point::new(1., 1.), Point::new(2., 2.), Point::new(3., 3.)]));
        let clipped = clip_contour(vec![Point::new(-5., 5.), Point::new(5., -5.), Point::new(15., 5.), Point::new(5., 15.)], Rect::new(0., 0., 10., 10.));
        assert!(valid_polygon(&clipped));
        assert!(clipped.iter().all(|p| (0.0..=10.0).contains(&p.x) && (0.0..=10.0).contains(&p.y)));
    }
}
