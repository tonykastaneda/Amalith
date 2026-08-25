use amalith_commands::{Command, CommandError, CommandOutcome, Editor};
use amalith_core::{ArtboardId, Point, Rect};

const MIN_SIZE: f64 = 1.0;
const DUPLICATE_MIN_DISTANCE: f64 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    NorthWest,
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
}

impl Handle {
    pub(crate) const ALL: [Self; 8] = [
        Self::NorthWest,
        Self::North,
        Self::NorthEast,
        Self::East,
        Self::SouthEast,
        Self::South,
        Self::SouthWest,
        Self::West,
    ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DragKind {
    Move,
    Duplicate,
    Resize(Handle),
}

#[derive(Debug, Clone, Copy)]
struct Drag {
    id: ArtboardId,
    kind: DragKind,
    start_rect: Rect,
    start_pointer: Point,
}

#[derive(Default)]
pub(crate) struct ArtboardTool {
    pub(crate) active: bool,
    pub(crate) selected: Option<ArtboardId>,
    pub(crate) preview_rect: Option<Rect>,
    drag: Option<Drag>,
}

pub(crate) fn mode_after_keys(
    active: bool,
    shift_o_pressed: bool,
    escape_pressed: bool,
    selection_pressed: bool,
) -> bool {
    if escape_pressed || selection_pressed {
        false
    } else if shift_o_pressed {
        !active
    } else {
        active
    }
}

impl ArtboardTool {
    pub(crate) fn set_active(&mut self, active: bool, first_artboard: Option<ArtboardId>) {
        self.active = active;
        self.drag = None;
        self.preview_rect = None;
        if active && self.selected.is_none() {
            self.selected = first_artboard;
        }
    }

    pub(crate) fn select(&mut self, id: ArtboardId) {
        self.selected = Some(id);
        self.drag = None;
        self.preview_rect = None;
    }

    pub(crate) fn begin_drag(
        &mut self,
        id: ArtboardId,
        rect: Rect,
        kind: DragKind,
        pointer: Point,
    ) {
        self.selected = Some(id);
        self.preview_rect = Some(rect);
        self.drag = Some(Drag {
            id,
            kind,
            start_rect: rect,
            start_pointer: pointer,
        });
    }

    pub(crate) fn update_drag(&mut self, pointer: Point) {
        let Some(drag) = self.drag else { return };
        let delta = pointer - drag.start_pointer;
        self.preview_rect = Some(match drag.kind {
            DragKind::Move | DragKind::Duplicate => Rect::new(
                drag.start_rect.x0 + delta.x,
                drag.start_rect.y0 + delta.y,
                drag.start_rect.x1 + delta.x,
                drag.start_rect.y1 + delta.y,
            ),
            DragKind::Resize(handle) => resized_by_delta(drag.start_rect, handle, delta.x, delta.y),
        });
    }

    pub(crate) fn finish_drag(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        let Some(drag) = self.drag.take() else {
            return Ok(false);
        };
        let preview = self.preview_rect.take().unwrap_or(drag.start_rect);
        if preview == drag.start_rect {
            return Ok(false);
        }
        match drag.kind {
            DragKind::Duplicate => {
                let dx = preview.x0 - drag.start_rect.x0;
                let dy = preview.y0 - drag.start_rect.y0;
                let moved = (dx * dx + dy * dy).sqrt();
                if moved < DUPLICATE_MIN_DISTANCE {
                    return Ok(false);
                }
                // Artboards are regions, not object parents: duplicate only the rect.
                let name = next_artboard_name(editor);
                let outcome = editor.execute(Command::CreateArtboard {
                    name,
                    rect: preview,
                    index: None,
                })?;
                if let CommandOutcome::Artboard(id) = outcome {
                    self.selected = Some(id);
                }
            }
            DragKind::Move | DragKind::Resize(_) => {
                editor.execute(Command::ResizeArtboard {
                    id: drag.id,
                    rect: preview,
                })?;
            }
        }
        Ok(true)
    }

    pub(crate) fn display_rect(&self, id: ArtboardId, stored: Rect) -> Rect {
        if self.selected == Some(id) && !self.is_duplicate_drag() {
            self.preview_rect.unwrap_or(stored)
        } else {
            stored
        }
    }

    pub(crate) fn is_duplicate_drag(&self) -> bool {
        self.drag
            .is_some_and(|drag| drag.kind == DragKind::Duplicate)
    }

    pub(crate) fn duplicate_preview(&self) -> Option<Rect> {
        self.is_duplicate_drag()
            .then_some(self.preview_rect)
            .flatten()
    }
}

pub(crate) fn next_artboard_name(editor: &Editor) -> String {
    let highest = editor
        .document()
        .artboards()
        .iter()
        .filter_map(|artboard| artboard.name.strip_prefix("Artboard "))
        .filter_map(|number| number.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Artboard {}", highest + 1)
}

pub(crate) fn resized_rect(rect: Rect, handle: Handle, pointer: Point) -> Rect {
    let mut x0 = rect.x0;
    let mut y0 = rect.y0;
    let mut x1 = rect.x1;
    let mut y1 = rect.y1;
    match handle {
        Handle::NorthWest | Handle::West | Handle::SouthWest => x0 = pointer.x.min(x1 - MIN_SIZE),
        Handle::NorthEast | Handle::East | Handle::SouthEast => x1 = pointer.x.max(x0 + MIN_SIZE),
        _ => {}
    }
    match handle {
        Handle::NorthWest | Handle::North | Handle::NorthEast => y0 = pointer.y.min(y1 - MIN_SIZE),
        Handle::SouthWest | Handle::South | Handle::SouthEast => y1 = pointer.y.max(y0 + MIN_SIZE),
        _ => {}
    }
    Rect::new(x0, y0, x1, y1)
}

fn resized_by_delta(rect: Rect, handle: Handle, dx: f64, dy: f64) -> Rect {
    let pointer = Point::new(
        match handle {
            Handle::NorthWest | Handle::West | Handle::SouthWest => rect.x0 + dx,
            _ => rect.x1 + dx,
        },
        match handle {
            Handle::NorthWest | Handle::North | Handle::NorthEast => rect.y0 + dy,
            _ => rect.y1 + dy,
        },
    );
    resized_rect(rect, handle, pointer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_commands::CommandOutcome;
    use amalith_core::Document;

    fn editor_with_board() -> (Editor, ArtboardId) {
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Artboard(id) = editor
            .execute(Command::CreateArtboard {
                name: "Artboard 1".into(),
                rect: Rect::new(10.0, 20.0, 110.0, 220.0),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        (editor, id)
    }

    #[test]
    fn tool_key_paths_select_and_exit_modes() {
        assert!(mode_after_keys(false, true, false, false));
        assert!(!mode_after_keys(true, true, false, false));
        assert!(!mode_after_keys(true, false, true, false));
        assert!(!mode_after_keys(true, false, false, true));
        assert!(!mode_after_keys(false, false, false, true));
    }

    #[test]
    fn handle_resize_keeps_opposite_edge() {
        let rect = Rect::new(10.0, 20.0, 110.0, 220.0);
        let resized = resized_rect(rect, Handle::NorthWest, Point::new(30.0, 50.0));
        assert_eq!(resized, Rect::new(30.0, 50.0, 110.0, 220.0));
    }

    #[test]
    fn move_does_not_change_size() {
        let (mut editor, id) = editor_with_board();
        let start = editor.document().artboard(id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(id, start, DragKind::Move, Point::new(20.0, 30.0));
        tool.update_drag(Point::new(55.0, 80.0));
        let preview = tool.preview_rect.unwrap();
        assert_eq!(preview.width(), start.width());
        assert_eq!(preview.height(), start.height());
        tool.finish_drag(&mut editor).unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().rect, preview);
    }

    #[test]
    fn preview_does_not_mutate_and_release_commits_one_command() {
        let (mut editor, id) = editor_with_board();
        let start = editor.document().artboard(id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(
            id,
            start,
            DragKind::Resize(Handle::East),
            Point::new(110.0, 100.0),
        );
        tool.update_drag(Point::new(160.0, 100.0));
        assert_eq!(editor.document().artboard(id).unwrap().rect, start);
        assert!(tool.finish_drag(&mut editor).unwrap());
        let committed = editor.document().artboard(id).unwrap().rect;
        editor.undo().unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().rect, start);
        editor.redo().unwrap();
        assert_eq!(editor.document().artboard(id).unwrap().rect, committed);
    }

    #[test]
    fn duplicate_drag_creates_copy_without_moving_original() {
        let (mut editor, original_id) = editor_with_board();
        let original = editor.document().artboard(original_id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(
            original_id,
            original,
            DragKind::Duplicate,
            Point::new(20.0, 30.0),
        );
        tool.update_drag(Point::new(60.0, 80.0));
        let preview = tool.duplicate_preview().unwrap();

        assert_eq!(tool.display_rect(original_id, original), original);
        assert!(tool.finish_drag(&mut editor).unwrap());
        assert_eq!(
            editor.document().artboard(original_id).unwrap().rect,
            original
        );
        assert_eq!(editor.document().artboards().len(), 2);
        assert_eq!(editor.document().artboards()[1].rect, preview);
        assert_eq!(editor.document().artboards()[1].name, "Artboard 2");
        assert_eq!(tool.selected, Some(editor.document().artboards()[1].id));
    }

    #[test]
    fn undo_duplicate_removes_only_copy() {
        let (mut editor, original_id) = editor_with_board();
        let original = editor.document().artboard(original_id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(
            original_id,
            original,
            DragKind::Duplicate,
            Point::new(20.0, 30.0),
        );
        tool.update_drag(Point::new(80.0, 90.0));
        tool.finish_drag(&mut editor).unwrap();

        editor.undo().unwrap();
        assert_eq!(editor.document().artboards().len(), 1);
        assert_eq!(
            editor.document().artboard(original_id).unwrap().rect,
            original
        );
    }

    #[test]
    fn zero_length_duplicate_drag_does_not_create() {
        let (mut editor, original_id) = editor_with_board();
        let original = editor.document().artboard(original_id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(
            original_id,
            original,
            DragKind::Duplicate,
            Point::new(20.0, 30.0),
        );
        tool.update_drag(Point::new(20.0, 30.0));

        assert!(!tool.finish_drag(&mut editor).unwrap());
        assert_eq!(editor.document().artboards().len(), 1);
    }

    #[test]
    fn normal_move_still_resizes_existing_artboard() {
        let (mut editor, id) = editor_with_board();
        let original = editor.document().artboard(id).unwrap().rect;
        let mut tool = ArtboardTool::default();
        tool.begin_drag(id, original, DragKind::Move, Point::new(20.0, 30.0));
        tool.update_drag(Point::new(30.0, 45.0));
        tool.finish_drag(&mut editor).unwrap();

        assert_eq!(editor.document().artboards().len(), 1);
        assert_eq!(
            editor.document().artboard(id).unwrap().rect,
            Rect::new(20.0, 35.0, 120.0, 235.0)
        );
    }
}
