use amalith_commands::{Command, CommandError, CommandOutcome, Editor};
use amalith_core::{ObjectKind, Point, Rect};

#[derive(Default)]
pub(crate) struct RectangleTool {
    pub(crate) active: bool,
    pub(crate) preview_rect: Option<Rect>,
    start: Option<Point>,
}

impl RectangleTool {
    pub(crate) fn set_active(&mut self, active: bool) {
        self.active = active;
        if !active {
            self.cancel_drag();
        }
    }

    pub(crate) fn begin_drag(&mut self, point: Point) {
        self.start = Some(point);
        self.preview_rect = Some(Rect::new(point.x, point.y, point.x, point.y));
    }

    pub(crate) fn update_drag(&mut self, point: Point, constrain_square: bool) {
        let Some(start) = self.start else { return };
        let end = if constrain_square {
            let dx = point.x - start.x;
            let dy = point.y - start.y;
            let size = dx.abs().max(dy.abs());
            Point::new(start.x + size.copysign(dx), start.y + size.copysign(dy))
        } else {
            point
        };
        self.preview_rect = Some(normalized_rect(start, end));
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.start = None;
        self.preview_rect = None;
    }

    pub(crate) fn finish_drag(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        self.start = None;
        let Some(rect) = self.preview_rect.take() else {
            return Ok(false);
        };
        if rect.width() <= 0.0 || rect.height() <= 0.0 {
            return Ok(false);
        }

        let layer = if let Some(layer) = editor.document().layers().last() {
            layer.id
        } else {
            match editor.execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })? {
                CommandOutcome::Layer(id) => id,
                _ => unreachable!("CreateLayer must return its layer id"),
            }
        };
        editor.execute(Command::CreateRect {
            layer,
            rect,
            name: Some(next_rectangle_name(editor)),
        })?;
        Ok(true)
    }
}

fn normalized_rect(start: Point, end: Point) -> Rect {
    Rect::new(
        start.x.min(end.x),
        start.y.min(end.y),
        start.x.max(end.x),
        start.y.max(end.y),
    )
}

fn next_rectangle_name(editor: &Editor) -> String {
    let highest = editor
        .document()
        .objects()
        .filter(|object| matches!(object.kind, ObjectKind::Path(_)))
        .filter_map(|object| object.name.as_deref())
        .filter_map(|name| name.strip_prefix("Rectangle "))
        .filter_map(|number| number.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Rectangle {}", highest + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::Document;

    #[test]
    fn drag_commits_one_rectangle_and_undo_removes_it() {
        let mut editor = Editor::new(Document::new("Test"));
        editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap();
        let mut tool = RectangleTool::default();
        tool.set_active(true);
        tool.begin_drag(Point::new(10.0, 20.0));
        tool.update_drag(Point::new(70.0, 50.0), false);

        assert!(tool.finish_drag(&mut editor).unwrap());
        assert_eq!(editor.document().objects().count(), 1);
        assert_eq!(
            editor.document().objects().next().unwrap().name.as_deref(),
            Some("Rectangle 1")
        );

        editor.undo().unwrap();
        assert_eq!(editor.document().objects().count(), 0);
    }

    #[test]
    fn shift_constrains_preview_to_square() {
        let mut tool = RectangleTool::default();
        tool.begin_drag(Point::new(10.0, 20.0));
        tool.update_drag(Point::new(70.0, 50.0), true);

        let preview = tool.preview_rect.unwrap();
        assert_eq!(preview.width(), 60.0);
        assert_eq!(preview.height(), 60.0);
    }

    #[test]
    fn zero_size_click_does_not_create() {
        let mut editor = Editor::new(Document::new("Test"));
        let mut tool = RectangleTool::default();
        tool.begin_drag(Point::new(10.0, 20.0));

        assert!(!tool.finish_drag(&mut editor).unwrap());
        assert_eq!(editor.document().objects().count(), 0);
        assert!(editor.document().layers().is_empty());
    }
}
