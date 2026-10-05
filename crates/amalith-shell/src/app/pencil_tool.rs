//! Freehand vector drawing. Pointer samples become an ordinary editable
//! open path; the document is changed only by `Command::CreateStyledPath`.

use super::*;

/// Keep enough samples for a faithful preview without storing every event.
const SAMPLE_SPACING_PX: f64 = 1.0;
/// Maximum screen-space deviation when removing redundant anchors.
const SIMPLIFY_TOLERANCE_PX: f64 = 0.8;

fn distance_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let length_sq = ab.dot(ab);
    if length_sq <= f64::EPSILON {
        return (p - a).hypot();
    }
    let t = ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0);
    (p - (a + t * ab)).hypot()
}

/// Ramer-Douglas-Peucker with retained endpoints. The iterative stack
/// avoids recursion depth growing with a long drawing gesture.
fn simplify(points: &[Point], tolerance: f64) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut ranges = vec![(0, points.len() - 1)];
    while let Some((start, end)) = ranges.pop() {
        if end <= start + 1 {
            continue;
        }
        let (index, distance) = ((start + 1)..end)
            .map(|i| {
                (
                    i,
                    distance_to_segment(points[i], points[start], points[end]),
                )
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap();
        if distance > tolerance {
            keep[index] = true;
            ranges.push((start, index));
            ranges.push((index, end));
        }
    }
    points
        .iter()
        .zip(keep)
        .filter_map(|(&p, retain)| retain.then_some(p))
        .collect()
}

impl App {
    pub(in crate::app) fn pencil_press(&mut self) {
        self.drag = Drag::PencilStroke {
            points: vec![self.doc_point(self.pointer)],
        };
        self.request_main_redraw();
    }

    pub(in crate::app) fn pencil_move(&self, points: &mut Vec<Point>) {
        let current = self.doc_point(self.pointer);
        let spacing = SAMPLE_SPACING_PX / self.doc.view.zoom.max(1e-6);
        if points
            .last()
            .is_none_or(|last| (*last - current).hypot() >= spacing)
        {
            points.push(current);
        }
    }

    pub(in crate::app) fn commit_pencil(&mut self, points: Vec<Point>) {
        let tolerance = SIMPLIFY_TOLERANCE_PX / self.doc.view.zoom.max(1e-6);
        let points = simplify(&points, tolerance);
        let length: f64 = points
            .windows(2)
            .map(|pair| (pair[1] - pair[0]).hypot())
            .sum();
        if points.len() < 2 || length * self.doc.view.zoom < 2.0 {
            return;
        }
        let anchors = points
            .into_iter()
            .map(|p| amalith_core::Anchor {
                point: convert::point_to_core(p),
                handle_in: None,
                handle_out: None,
                mode: amalith_core::HandleMode::Corner,
            })
            .collect();
        let path = amalith_core::PathData::from_subpaths(vec![amalith_core::Subpath {
            anchors,
            closed: false,
        }]);
        let mut appearance = amalith_core::Appearance::default();
        appearance.set_fill(amalith_core::Paint::None);
        appearance.set_stroke(self.doc.stroke);
        appearance.set_stroke_width(self.doc.stroke_w);
        appearance.set_stroke_style(self.doc.stroke_style);
        appearance.opacity = self.doc.opacity;
        let (container, _) = self.ensure_container();
        match self.doc
                .execute_new_vector_object(Command::CreateStyledPath {
                    parent: container,
                    path,
                    name: None,
                    appearance,
                })
        {
            Ok(CommandOutcome::Object(id)) => {
                self.doc.selection = vec![id];
                self.reparent_new_object_into_isolation(id);
                self.doc.io_error = None;
            }
            Err(error) => self.doc.io_error = Some(format!("Pencil could not create a path: {error}")),
            _ => {}
        }
        self.request_main_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_commands::Editor;
    use amalith_core::{Color, Document, Layer, LayerId, ObjectKind, Paint};

    #[test]
    fn straight_stroke_keeps_only_endpoints() {
        let points: Vec<_> = (0..100).map(|x| Point::new(x as f64, 0.0)).collect();
        assert_eq!(simplify(&points, 0.8), vec![points[0], points[99]]);
    }

    #[test]
    fn corner_survives_simplification() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
        ];
        assert_eq!(simplify(&points, 0.8), points);
    }

    #[test]
    fn committing_a_stroke_creates_an_editable_unfilled_path_with_one_undo() {
        let mut document = Document::new("Pencil");
        let layer = LayerId::new();
        document.insert_layer(Layer::new(layer, "Drawing"), 0);
        let mut app = App::new();
        app.doc = Doc::new(Editor::new(document));
        app.doc.selected_layer = Some(layer);
        app.doc.stroke = Paint::Solid(Color::rgb(0.2, 0.4, 0.6));
        app.doc.stroke_w = 4.0;
        app.commit_pencil(vec![Point::new(1.0, 2.0), Point::new(20.0, 12.0)]);
        let id = app.doc.selection[0];
        let object = app.doc.editor.document().object(id).expect("created path");
        let ObjectKind::Path(path) = &object.kind else { panic!("Pencil creates an editable path") };
        assert!(!path.subpaths()[0].closed);
        assert_eq!(object.appearance.fill(), Paint::None);
        assert_eq!(object.appearance.stroke(), app.doc.stroke);
        assert_eq!(object.appearance.stroke_width(), 4.0);
        app.doc.editor.undo().unwrap();
        assert!(app.doc.editor.document().object(id).is_none());
    }
}
