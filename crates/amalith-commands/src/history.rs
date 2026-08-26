//! Undo/redo stacks of edit groups.
//!
//! A "group" is the one or more [`crate::edit::Edit`] values a `Command`
//! compiled down to. For example, moving an artboard records its rectangle
//! and the transforms of intersecting artwork as one user action.
use crate::edit::Edit;

#[derive(Debug, Default)]
pub(crate) struct History {
    undo_stack: Vec<Vec<Edit>>,
    redo_stack: Vec<Vec<Edit>>,
}

impl History {
    pub(crate) fn record(&mut self, inverse_group: Vec<Edit>) {
        self.undo_stack.push(inverse_group);
        self.redo_stack.clear();
    }

    pub(crate) fn pop_undo(&mut self) -> Option<Vec<Edit>> {
        self.undo_stack.pop()
    }

    pub(crate) fn push_redo(&mut self, group: Vec<Edit>) {
        self.redo_stack.push(group);
    }

    pub(crate) fn pop_redo(&mut self) -> Option<Vec<Edit>> {
        self.redo_stack.pop()
    }

    pub(crate) fn push_undo(&mut self, group: Vec<Edit>) {
        self.undo_stack.push(group);
    }

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}
