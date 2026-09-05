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
    /// Undo-stack depth at the last save, or `None` for "depth 0" (never
    /// saved this session, or invalidated — see `record`). The document
    /// is clean exactly when the live stack is back at this depth: undo
    /// and redo are exact inverses, so equal depth means equal content,
    /// regardless of the path taken to get there.
    saved_depth: Option<usize>,
}

impl History {
    pub(crate) fn record(&mut self, inverse_group: Vec<Edit>) {
        if !self.redo_stack.is_empty() {
            // Branching away from a previously redo-able future. If the
            // clean checkpoint was strictly ahead of where we are now, it
            // lived only in that now-discarded branch — no sequence of
            // undo/redo from here can reach it again, so the document
            // stays dirty until the next save. (A checkpoint at or below
            // the current depth is in the shared past both branches
            // still agree on, and remains valid.)
            let depth = self.saved_depth.unwrap_or(0);
            if depth > self.undo_stack.len() {
                self.saved_depth = None;
            }
        }
        self.undo_stack.push(inverse_group);
        self.redo_stack.clear();
    }

    /// Marks the current undo-stack depth as matching what's on disk —
    /// called right after a successful save.
    pub(crate) fn mark_clean(&mut self) {
        self.saved_depth = Some(self.undo_stack.len());
    }

    /// True once the live document has diverged from the last save (or,
    /// having never been saved this session, from its starting state).
    pub(crate) fn is_dirty(&self) -> bool {
        self.undo_stack.len() != self.saved_depth.unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Mimics `Editor::undo`'s stack juggling — `History` alone doesn't
    /// pair the two calls into one "undo" step.
    fn undo(h: &mut History) {
        let group = h.pop_undo().expect("something to undo");
        h.push_redo(group);
    }

    /// Mimics `Editor::redo`'s stack juggling.
    fn redo(h: &mut History) {
        let group = h.pop_redo().expect("something to redo");
        h.push_undo(group);
    }

    #[test]
    fn a_fresh_history_is_clean() {
        assert!(!History::default().is_dirty());
    }

    #[test]
    fn recording_an_edit_makes_it_dirty_and_saving_cleans_it() {
        let mut h = History::default();
        h.record(vec![]);
        assert!(h.is_dirty());
        h.mark_clean();
        assert!(!h.is_dirty());
    }

    #[test]
    fn undoing_back_to_the_save_point_is_clean_again() {
        let mut h = History::default();
        h.record(vec![]);
        h.mark_clean();
        h.record(vec![]);
        assert!(h.is_dirty());
        undo(&mut h);
        assert!(!h.is_dirty(), "back at the exact depth we saved at");
    }

    #[test]
    fn redoing_back_up_to_the_save_point_is_clean_again() {
        let mut h = History::default();
        h.record(vec![]);
        h.record(vec![]);
        h.mark_clean();
        undo(&mut h);
        assert!(h.is_dirty());
        redo(&mut h);
        assert!(!h.is_dirty());
    }

    #[test]
    fn a_new_edit_after_undoing_past_the_save_point_stays_dirty_even_at_the_same_depth() {
        // Save at depth 1, undo to depth 0, then make a *different* new
        // edit instead of redoing — landing back at depth 1, but with
        // content that was never saved. Naive depth-only comparison
        // would wrongly call this clean.
        let mut h = History::default();
        h.record(vec![]);
        h.mark_clean();
        undo(&mut h);
        h.record(vec![]); // a new branch, not a redo
        assert!(h.is_dirty(), "same depth as the save point, but a different branch");
    }

    #[test]
    fn branching_above_the_save_point_leaves_it_valid() {
        // Save at depth 3, do one more edit (depth 4), undo back down to
        // 3, then branch there instead of redoing. The save point is
        // strictly *before* the branch, so both branches still agree on
        // it — undoing back down to depth 3 from the new branch lands on
        // the exact state that was saved.
        let mut h = History::default();
        h.record(vec![]);
        h.record(vec![]);
        h.record(vec![]);
        h.mark_clean();
        h.record(vec![]); // depth 4
        undo(&mut h); // depth 3 -- exactly the save point
        assert!(!h.is_dirty());
        h.record(vec![]); // branch at depth 3 -> depth 4, different content
        assert!(h.is_dirty());
        undo(&mut h); // back down to depth 3
        assert!(!h.is_dirty(), "the save point survives branching above it");
    }

    #[test]
    fn branching_below_the_save_point_invalidates_it() {
        // Save at depth 3, undo past it to depth 1, then branch. The
        // only path back to depth 3's content was through the discarded
        // redo stack, so the checkpoint can never be reached again.
        let mut h = History::default();
        h.record(vec![]);
        h.record(vec![]);
        h.record(vec![]);
        h.mark_clean();
        undo(&mut h);
        undo(&mut h); // depth 1, below the save point (3)
        h.record(vec![]); // branch -> depth 2, discarding the old redo stack
        assert!(h.is_dirty(), "the save point is unreachable once its branch is discarded");
    }
}
