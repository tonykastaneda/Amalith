//! Width tool: Illustrator-style variable-width stroke points, placed and
//! dragged directly on a selected path's own stroke. The data model and
//! outline math this drives live in `amalith_core::width`; this module is
//! only the on-canvas interaction (hit-testing, drag, commit) — rendering
//! the ribbon is `canvas::paint_object`, and the handle overlay is
//! `render::overlays::paint_width_points`.
//!
//! Scoped to a single selected open, single-subpath `Path` object, same
//! as `pathtext`'s own path-following limitation and for the same
//! reason — see `amalith_core::width`'s doc comment.

use super::*;

/// Screen-px grab radius for a width-point handle (the on-path diamond or
/// either edge dot).
pub(in crate::app) const WIDTH_HANDLE_GRAB: f64 = 8.0;
/// Screen-px tolerance for "close enough to the bare stroke" to plant a
/// new width point there.
const WIDTH_PATH_GRAB: f64 = 8.0;

/// Which handle of an existing width point a [`Drag::WidthPoint`] is
/// moving. The on-path diamond only repositions the point *along* the
/// path — it must never touch `left`/`right`, or every click resets
/// whatever width was already dialed in there. Reshaping the width is the
/// two edge dots' job, one per side (independently, for an asymmetric
/// taper) — `Left` grows/shrinks only `left`, `Right` only `right`, both
/// clamped so dragging past the centerline just stops at zero rather than
/// going negative. `New` is the one exception: a freshly *created* point
/// (no established position yet) still lets one drag both place it and
/// size it, matching how it worked before this distinction existed.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(in crate::app) enum WidthDragPart {
    Center,
    Left,
    Right,
    New,
}

/// Everything a Width-tool gesture needs about the current selection,
/// resolved once per press/move rather than threaded through as loose
/// arguments.
pub(in crate::app) struct WidthTarget {
    pub id: ObjectId,
    pub arc: amalith_core::ArcLengthPath,
    /// Path-local space → screen.
    pub m: Affine,
    pub base_half: f64,
    pub points: Vec<amalith_core::WidthPoint>,
}

/// The on-path center handle and its two edge dots, in screen space, for
/// one width point. Shared by hit-testing here and the overlay painter.
pub(in crate::app) fn width_handle_points(t: &WidthTarget, wp: &amalith_core::WidthPoint) -> (Point, Point, Point) {
    let (p, angle) = t.arc.point_and_tangent(wp.distance);
    let (nx, ny) = (-angle.sin(), angle.cos());
    let local_left = amalith_core::Point::new(p.x + nx * wp.left, p.y + ny * wp.left);
    let local_right = amalith_core::Point::new(p.x - nx * wp.right, p.y - ny * wp.right);
    (
        t.m * convert::point(p),
        t.m * convert::point(local_left),
        t.m * convert::point(local_right),
    )
}

impl App {
    /// Seeds every eligible selected object's `width_points` from a
    /// preset taper shape (Options bar ▸ Stroke ▸ Profile) — same
    /// eligibility as [`Self::width_target`] (open, single-subpath
    /// `Path`), just over the whole selection at once rather than
    /// requiring exactly one. Ignored objects are silently skipped, same
    /// as the Width tool itself silently doing nothing on them.
    pub(in crate::app) fn apply_width_profile(&mut self, preset: amalith_core::WidthProfilePreset) {
        for id in self.doc.selection.clone() {
            let doc = self.doc.editor.document();
            let Some(obj) = doc.object(id) else { continue };
            let amalith_core::ObjectKind::Path(pd) = &obj.kind else { continue };
            if pd.subpaths().len() != 1 || pd.subpaths()[0].closed {
                continue;
            }
            let Some(pts) = pd.flattened_points(0.05).into_iter().next() else { continue };
            let arc = amalith_core::ArcLengthPath::new(&pts, false);
            let total = arc.total_length();
            if total <= 0.0 {
                continue;
            }
            let base_half = obj.appearance.stroke_width() * 0.5;
            let points = amalith_core::preset_points(preset, total, base_half);
            let _ = self.doc.editor.execute(Command::SetWidthPoints { object: id, points });
        }
        self.request_main_redraw();
    }

    /// The single selected object's width-tool target, or `None` when the
    /// tool doesn't apply right now: not exactly one object selected, not
    /// a plain `Path`, closed, multi-subpath, or degenerate.
    pub(in crate::app) fn width_target(&self) -> Option<WidthTarget> {
        let [id] = self.doc.selection[..] else { return None };
        let doc = self.doc.editor.document();
        let obj = doc.object(id)?;
        let amalith_core::ObjectKind::Path(pd) = &obj.kind else { return None };
        if pd.subpaths().len() != 1 || pd.subpaths()[0].closed {
            return None;
        }
        let pts = pd.flattened_points(0.05).into_iter().next()?;
        let arc = amalith_core::ArcLengthPath::new(&pts, false);
        if arc.total_length() <= 0.0 {
            return None;
        }
        let m = self.doc.view.to_screen() * convert::affine(doc.world_transform(id));
        Some(WidthTarget {
            id,
            arc,
            m,
            base_half: obj.appearance.stroke_width() * 0.5,
            points: pd.width_points.clone(),
        })
    }

    /// The existing point (and which of its three handles) nearest the
    /// pointer, if within [`WIDTH_HANDLE_GRAB`] screen px.
    fn width_hit_point(&self, t: &WidthTarget) -> Option<(usize, WidthDragPart)> {
        let mut best: Option<(usize, WidthDragPart, f64)> = None;
        for (i, wp) in t.points.iter().enumerate() {
            let (center, left, right) = width_handle_points(t, wp);
            for (part, screen_p) in [
                (WidthDragPart::Center, center),
                (WidthDragPart::Left, left),
                (WidthDragPart::Right, right),
            ] {
                let d = (screen_p - self.pointer).hypot();
                if d <= WIDTH_HANDLE_GRAB && best.as_ref().is_none_or(|&(_, _, bd)| d < bd) {
                    best = Some((i, part, d));
                }
            }
        }
        best.map(|(i, part, _)| (i, part))
    }

    /// Width-tool press: Alt+click near an existing point deletes it
    /// outright (one undo step, no drag); a plain click near one of its
    /// three handles grabs that handle (see [`WidthDragPart`]); a plain
    /// click on the bare stroke elsewhere plants a new point there —
    /// seeded at the stroke's current effective width so it doesn't
    /// visually jump — and grabs it to place and size in one drag.
    /// Returns whether the press was handled (so the caller doesn't fall
    /// through to a selection / transform gesture).
    pub(in crate::app) fn width_tool_press(&mut self) -> bool {
        let Some(t) = self.width_target() else { return false };
        if self.alt_down {
            let Some((idx, _)) = self.width_hit_point(&t) else { return false };
            let mut points = t.points;
            points.remove(idx);
            let _ = self
                .doc.editor
                .execute(Command::SetWidthPoints { object: t.id, points });
            self.request_main_redraw();
            return true;
        }
        if let Some((idx, part)) = self.width_hit_point(&t) {
            self.drag = Drag::WidthPoint {
                object: t.id,
                points: t.points,
                index: idx,
                part,
            };
            self.request_main_redraw();
            return true;
        }
        let local = t.m.inverse() * self.pointer;
        let local = amalith_core::Point::new(local.x, local.y);
        let distance = t.arc.nearest_distance(local);
        let (p, _) = t.arc.point_and_tangent(distance);
        if (t.m * convert::point(p) - self.pointer).hypot() > WIDTH_PATH_GRAB {
            return false;
        }
        let total = t.arc.total_length();
        let (left, right) = amalith_core::width_at(&t.points, total, t.base_half, distance);
        let mut points = t.points;
        points.push(amalith_core::WidthPoint { distance, left, right });
        let index = points.len() - 1;
        self.drag = Drag::WidthPoint { object: t.id, points, index, part: WidthDragPart::New };
        self.request_main_redraw();
        true
    }

    /// Live-updates `points[index]` from the current pointer, per `part`:
    /// `Center` only slides its arc-length position (left/right
    /// untouched); `Left`/`Right` only resize that one side (its position
    /// stays put) — dragging past the centerline clamps at zero rather
    /// than going negative; `New` (a point just created this gesture)
    /// does both together, since it has no established position to
    /// preserve yet.
    pub(in crate::app) fn width_tool_move(
        &self,
        object: ObjectId,
        points: &mut [amalith_core::WidthPoint],
        index: usize,
        part: WidthDragPart,
    ) {
        let Some(t) = self.width_target() else { return };
        if t.id != object {
            return;
        }
        let total = t.arc.total_length();
        let local = t.m.inverse() * self.pointer;
        let (lx, ly) = (local.x, local.y);

        if part == WidthDragPart::Center {
            let distance = t.arc.nearest_distance(amalith_core::Point::new(lx, ly)).clamp(0.0, total);
            if let Some(wp) = points.get_mut(index) {
                wp.distance = distance;
            }
            return;
        }

        let Some(&fixed) = points.get(index) else { return };
        let distance = if part == WidthDragPart::New {
            t.arc.nearest_distance(amalith_core::Point::new(lx, ly)).clamp(0.0, total)
        } else {
            fixed.distance
        };
        let (p, angle) = t.arc.point_and_tangent(distance);
        let (nx, ny) = (-angle.sin(), angle.cos());
        // Positive = the `left` side (see `width_outline`'s own
        // convention), negative = `right`.
        let signed = (lx - p.x) * nx + (ly - p.y) * ny;
        if let Some(wp) = points.get_mut(index) {
            wp.distance = distance;
            match part {
                WidthDragPart::Left => wp.left = signed.max(0.0),
                WidthDragPart::Right => wp.right = (-signed).max(0.0),
                WidthDragPart::New => {
                    let half = signed.abs();
                    wp.left = half;
                    wp.right = half;
                }
                WidthDragPart::Center => unreachable!(),
            }
        }
    }

    /// Commits an in-progress width-point drag as one undo step.
    pub(in crate::app) fn commit_width_point(
        &mut self,
        object: ObjectId,
        points: Vec<amalith_core::WidthPoint>,
    ) {
        let _ = self
            .doc.editor
            .execute(Command::SetWidthPoints { object, points });
        self.request_main_redraw();
    }
}
