use amalith_commands::{Command, CommandError, CommandOutcome, Editor};
use amalith_core::{ObjectKind, PathData, Point};

/// Click-to-place Pen tool state. Paths stay entirely local until they are
/// explicitly finished, so an in-progress path never pollutes undo history
/// or requires a temporary document object.
#[derive(Default)]
pub(crate) struct PenTool {
    pub(crate) active: bool,
    pub(crate) preview: Option<PathData>,
    points: Vec<Point>,
    closed: bool,
}

impl PenTool {
    pub(crate) fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.cancel();
        }
    }

    pub(crate) fn is_drawing(&self) -> bool {
        !self.points.is_empty()
    }

    pub(crate) fn anchors(&self) -> &[Point] {
        &self.points
    }

    pub(crate) fn can_close_at(&self, point: Point, close_radius: f64) -> bool {
        self.points.len() >= 3
            && self
                .points
                .first()
                .is_some_and(|first| (*first - point).hypot() <= close_radius)
    }

    /// Adds an anchor. Returns true when the click lands on the first anchor
    /// and the path should be closed and committed.
    pub(crate) fn press(&mut self, point: Point, close_radius: f64, constrain: bool) -> bool {
        if self.can_close_at(point, close_radius) {
            self.closed = true;
            self.preview = Some(PathData::polygon(&self.points));
            return true;
        }

        self.points.push(constrained_point(
            self.points.last().copied(),
            point,
            constrain,
        ));
        self.closed = false;
        self.refresh_preview(None);
        false
    }

    pub(crate) fn update_hover(&mut self, point: Point, constrain: bool) {
        if self.is_drawing() {
            let point = constrained_point(self.points.last().copied(), point, constrain);
            self.refresh_preview(Some(point));
        }
    }

    pub(crate) fn cancel(&mut self) {
        self.points.clear();
        self.preview = None;
        self.closed = false;
    }

    /// Commits two or more anchors as one undoable path. Closing is optional;
    /// Enter commits an open line, while clicking its first anchor commits a
    /// closed path.
    pub(crate) fn finish(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        if self.points.len() < 2 {
            self.cancel();
            return Ok(false);
        }
        let path = if self.closed {
            PathData::polygon(&self.points)
        } else {
            PathData::polyline(&self.points)
        };
        self.cancel();
        let layer = if let Some(layer) = editor.document().layers().last() {
            layer.id
        } else {
            match editor.execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })? {
                CommandOutcome::Layer(id) => id,
                _ => unreachable!(),
            }
        };
        editor.execute(Command::CreatePath {
            layer,
            path,
            name: Some(next_path_name(editor)),
        })?;
        Ok(true)
    }

    fn refresh_preview(&mut self, hover: Option<Point>) {
        let mut points = self.points.clone();
        if let Some(point) = hover {
            points.push(point);
        }
        self.preview = (points.len() >= 2).then(|| PathData::polyline(&points));
    }
}

fn constrained_point(previous: Option<Point>, point: Point, constrain: bool) -> Point {
    let Some(previous) = previous.filter(|_| constrain) else {
        return point;
    };
    let delta = point - previous;
    let length = delta.hypot();
    if length == 0.0 {
        return point;
    }
    let angle = delta.y.atan2(delta.x);
    let snapped = (angle / std::f64::consts::FRAC_PI_4).round() * std::f64::consts::FRAC_PI_4;
    Point::new(
        previous.x + length * snapped.cos(),
        previous.y + length * snapped.sin(),
    )
}

fn next_path_name(editor: &Editor) -> String {
    let highest = editor
        .document()
        .objects()
        .filter(|object| matches!(object.kind, ObjectKind::Path(_)))
        .filter_map(|object| object.name.as_deref())
        .filter_map(|name| name.strip_prefix("Path ")?.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Path {}", highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{geom::PathEl, Document};

    fn editor() -> Editor {
        let mut editor = Editor::new(Document::new("Pen test"));
        editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap();
        editor
    }

    #[test]
    fn enter_commits_an_open_path() {
        let mut editor = editor();
        let mut tool = PenTool::default();
        tool.set_active(true);
        tool.press(Point::new(10.0, 20.0), 2.0, false);
        tool.press(Point::new(40.0, 20.0), 2.0, false);
        assert!(tool.finish(&mut editor).unwrap());
        let object = editor.document().objects().next().unwrap();
        let ObjectKind::Path(path) = &object.kind else {
            panic!()
        };
        assert!(matches!(
            path.geometry.elements().last(),
            Some(PathEl::LineTo(_))
        ));
    }

    #[test]
    fn clicking_the_first_anchor_closes_the_path() {
        let mut editor = editor();
        let mut tool = PenTool::default();
        tool.set_active(true);
        for point in [
            Point::new(0.0, 0.0),
            Point::new(20.0, 0.0),
            Point::new(10.0, 20.0),
        ] {
            assert!(!tool.press(point, 2.0, false));
        }
        assert!(tool.press(Point::new(0.5, 0.5), 2.0, false));
        assert!(tool.finish(&mut editor).unwrap());
        let object = editor.document().objects().next().unwrap();
        let ObjectKind::Path(path) = &object.kind else {
            panic!()
        };
        assert!(matches!(
            path.geometry.elements().last(),
            Some(PathEl::ClosePath)
        ));
    }
}
