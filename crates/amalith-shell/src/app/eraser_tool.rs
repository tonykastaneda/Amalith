//! Eraser tool: drag a round brush over the canvas to cut away whatever
//! it sweeps across. Scoped to the current selection when there is one
//! (matching Illustrator's own scoping), else every eligible object
//! visible in the document — same idea as Shape Builder's own targeting,
//! just without needing 2+ objects since erasing never combines
//! anything.

use super::*;
use amalith_core::ObjectKind;
use vello::kurbo::{Cap, Join as KurboJoin, Stroke as KurboStroke, StrokeOpts};

/// Screen-px brush diameter bounds and per-key-press step.
pub(in crate::app) const ERASER_MIN_SIZE: f64 = 4.0;
pub(in crate::app) const ERASER_MAX_SIZE: f64 = 300.0;
pub(in crate::app) const ERASER_STEP: f64 = 4.0;

impl App {
    /// Objects the eraser is allowed to touch right now.
    fn eraser_targets(&self) -> Vec<ObjectId> {
        let doc = self.doc.editor.document();
        let eligible = |id: ObjectId| -> bool {
            doc.object(id).is_some_and(|o| {
                o.visible
                    && !o.locked
                    && matches!(o.kind, ObjectKind::Path(_) | ObjectKind::CompoundPath(_))
            })
        };
        if !self.doc.selection.is_empty() {
            return self.doc.selection.iter().copied().filter(|&id| eligible(id)).collect();
        }
        select::visible_top_level_bounds(doc, self.visible_doc_rect(), &[])
            .into_iter()
            .map(|(id, _)| id)
            .filter(|&id| eligible(id))
            .collect()
    }

    /// Press: always starts a stroke — there's no "missed" state the way
    /// Shape Builder has, since the brush itself defines the touched
    /// area regardless of what (if anything) ends up under it.
    pub(in crate::app) fn eraser_press(&mut self) -> bool {
        self.drag = Drag::EraserStroke { path: vec![self.doc_point(self.pointer)] };
        self.request_main_redraw();
        true
    }

    /// Appends the current pointer position if it's moved far enough
    /// (screen-space) from the last sample to be worth a new brush
    /// segment — keeps a fast drag from bloating `path` with near-
    /// duplicate points.
    pub(in crate::app) fn eraser_move(&mut self, path: &mut Vec<Point>) {
        let p = self.doc_point(self.pointer);
        let zoom = self.doc.view.zoom.max(1e-6);
        if path.last().is_none_or(|&last| (last - p).hypot() * zoom > 1.5) {
            path.push(p);
        }
    }

    /// The brush stroke's swept shape (document space) for the finished
    /// `path` — a filled ribbon along the drag, or a single dot for a
    /// plain click. `None` for a truly empty gesture.
    pub(in crate::app) fn eraser_brush_area(&self, path: &[Point]) -> Option<amalith_core::PathData> {
        let &first = path.first()?;
        let diameter = self.eraser_size / self.doc.view.zoom.max(1e-6);
        if path.len() < 2 || path.iter().all(|&p| (p - first).hypot() < 1e-6) {
            let c = convert::point_to_core(first);
            let r = diameter * 0.5;
            let dot = amalith_core::geom::Rect::new(c.x - r, c.y - r, c.x + r, c.y + r);
            return Some(amalith_core::PathData::ellipse(dot));
        }
        let mut spine = BezPath::new();
        spine.move_to(first);
        for &p in &path[1..] {
            spine.line_to(p);
        }
        let style = KurboStroke::new(diameter).with_caps(Cap::Round).with_join(KurboJoin::Round);
        let outline = vello::kurbo::stroke(spine, &style, &StrokeOpts::default(), 0.1);
        Some(amalith_core::PathData::from_bezpath(convert::bez_path_to_core(&outline)))
    }

    /// Commits the finished stroke as one undo step. No-op if the
    /// gesture never actually swept over anything eligible.
    pub(in crate::app) fn commit_eraser(&mut self, path: Vec<Point>) {
        let Some(area) = self.eraser_brush_area(&path) else { return };
        let objects = self.eraser_targets();
        if objects.is_empty() {
            return;
        }
        if self.doc.editor.execute(Command::EraseArea { objects, area }).is_ok() {
            self.prune_selection();
        }
        self.request_main_redraw();
    }
}
