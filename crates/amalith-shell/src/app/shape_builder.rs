//! Shape Builder tool: Illustrator's drag-to-merge / Alt-drag-to-erase
//! over a multi-object selection. Canvas-wide within that selection (like
//! Join, not scoped to a single object): the tool decomposes the
//! selected shapes into their non-overlapping faces (the same planar
//! split `Pathfinder ▸ Divide` already computes), lets a drag sweep over
//! any number of them, and on release either unions the swept faces into
//! one new object (plain drag) or deletes them (Alt-drag) — leaving every
//! object the drag never touched completely alone.

use super::*;
use amalith_commands::{pathfinder_apply, PathInput, PathfinderOp};
use amalith_core::{Appearance, ObjectKind};
use vello::kurbo::Shape;

/// Fill/stroke ink while Alt-dragging (erase mode); a plain drag uses the
/// theme accent instead, painted by `overlays::paint_shape_builder_preview`.
pub(in crate::app) const ERASE_INK: Color = Color::from_rgb8(0xff, 0x3b, 0x30);

/// One face of the current region cache: a maximal non-overlapping piece
/// of the selection's combined outline, in document space, carrying
/// whichever selected object was topmost there.
pub(in crate::app) struct ShapeBuilderFace {
    pub contour: BezPath,
    /// The exact flattened contours Pathfinder Divide produced for this
    /// face — kept verbatim (not re-derived from `contour` later) so a
    /// commit re-unions the *same* polygon two adjacent faces already
    /// agree on the shared edge of, rather than two independently
    /// round-tripped copies that can drift a hair apart and leave a
    /// self-intersecting sliver where they're supposed to meet exactly.
    core_contours: Vec<Vec<[f64; 2]>>,
    pub appearance: Appearance,
}

/// The Shape Builder tool's region cache — see the module docs.
pub(in crate::app) struct ShapeBuilderCache {
    /// The selection this was built from, sorted — compared against the
    /// live selection each frame to decide whether to rebuild, and
    /// reused as-is for `Command::ShapeBuilder::objects` on commit
    /// (paint order doesn't matter there; `compile_shape_builder` derives
    /// its own).
    source: Vec<ObjectId>,
    pub faces: Vec<ShapeBuilderFace>,
}

/// Selected objects eligible for Shape Builder: paths / compound paths.
/// `None` if fewer than two qualify — the tool has nothing to do with
/// just zero or one. Mixed parents are allowed here (the hover highlight
/// is still meaningful); `Command::ShapeBuilder` itself rejects that
/// combination at commit time.
fn eligible(doc: &Document, selection: &[ObjectId]) -> Option<Vec<ObjectId>> {
    let ids: Vec<ObjectId> = selection
        .iter()
        .copied()
        .filter(|&id| {
            doc.object(id)
                .is_some_and(|o| matches!(o.kind, ObjectKind::Path(_) | ObjectKind::CompoundPath(_)))
        })
        .collect();
    (ids.len() >= 2).then_some(ids)
}

fn build_cache(doc: &Document, selection: &[ObjectId]) -> Option<ShapeBuilderCache> {
    let ids = eligible(doc, selection)?;
    let mut source = ids.clone();
    source.sort();

    let mut inputs = Vec::new();
    for &id in &ids {
        let contour = select::object_contour(doc, id)?;
        let core_path = convert::bez_path_to_core(&contour);
        let appearance = doc.object(id)?.appearance;
        inputs.push(PathInput {
            contours: amalith_commands::flatten_path(&core_path),
            appearance,
        });
    }
    let faces = pathfinder_apply(PathfinderOp::Divide, &inputs)
        .into_iter()
        .map(|r| ShapeBuilderFace {
            contour: convert::bez_path(&r.path.geometry),
            core_contours: amalith_commands::flatten_path(&r.path.geometry),
            appearance: r.appearance,
        })
        .collect();
    Some(ShapeBuilderCache { source, faces })
}

impl App {
    /// Rebuilds the region cache if the live selection no longer matches
    /// what it was built from, then returns it (`None` if the current
    /// selection isn't eligible at all).
    pub(in crate::app) fn shape_builder_cache(&mut self) -> Option<&ShapeBuilderCache> {
        let mut current = self.doc.selection.clone();
        current.sort();
        let stale = self
            .shape_builder
            .as_ref()
            .map(|c| c.source != current)
            .unwrap_or(true);
        if stale {
            self.shape_builder = build_cache(self.doc.editor.document(), &current);
        }
        self.shape_builder.as_ref()
    }

    /// The topmost cached face under document-space point `p`, or `None`.
    /// Faces never overlap, so "topmost" only matters for tie-breaking a
    /// shared edge; any match found there is fine.
    pub(in crate::app) fn shape_builder_face_at(&self, p: Point) -> Option<usize> {
        let cache = self.shape_builder.as_ref()?;
        cache.faces.iter().position(|f| f.contour.winding(p) != 0)
    }

    /// Press: starts a drag if the tool has an eligible selection.
    /// Returns whether the press was handled. `erase` is fixed for the
    /// whole gesture from Alt's state right now.
    pub(in crate::app) fn shape_builder_press(&mut self) -> bool {
        if self.shape_builder_cache().is_none() {
            return false;
        }
        let mut touched = Vec::new();
        if let Some(i) = self.shape_builder_face_at(self.doc_point(self.pointer)) {
            touched.push(i);
        }
        self.drag = Drag::ShapeBuilderDrag { erase: self.alt_down, touched };
        self.request_main_redraw();
        true
    }

    /// Live move: appends the face under the pointer to `touched`, if
    /// it isn't already in there.
    pub(in crate::app) fn shape_builder_move(&mut self, touched: &mut Vec<usize>) {
        if let Some(i) = self.shape_builder_face_at(self.doc_point(self.pointer)) {
            if !touched.contains(&i) {
                touched.push(i);
            }
        }
    }

    /// Commits the finished drag as one undo step. No-op if nothing was
    /// ever touched (a click on empty space, or a drag that never
    /// crossed a face).
    pub(in crate::app) fn commit_shape_builder(&mut self, erase: bool, touched: Vec<usize>) {
        let Some(cache) = self.shape_builder.as_ref() else { return };
        if touched.is_empty() {
            return;
        }
        // A real boolean union, not a bare concatenation of the touched
        // faces' own boundaries — two adjacent faces agree on their
        // shared edge in principle, but each is its own independent
        // `PathResult`, and simply drawing both loops into one path
        // trusts that shared edge to be bit-for-bit identical. Handing
        // both through the same overlay engine that produced them
        // resolves that edge properly instead of risking a sliver of
        // self-intersecting garbage right where they're supposed to meet.
        let touched_inputs: Vec<PathInput> = touched
            .iter()
            .map(|&i| PathInput {
                contours: cache.faces[i].core_contours.clone(),
                appearance: cache.faces[i].appearance,
            })
            .collect();
        let united = pathfinder_apply(PathfinderOp::Unite, &touched_inputs);
        let Some((first, rest)) = united.split_first() else {
            return;
        };
        let mut combined = first.path.geometry.clone();
        for r in rest {
            combined.extend(r.path.geometry.elements().iter().copied());
        }
        let touched_path = amalith_core::PathData::from_bezpath(combined);
        let appearance = cache.faces[touched[0]].appearance;
        let objects = cache.source.clone();

        let cmd = Command::ShapeBuilder {
            objects,
            touched: touched_path,
            erase,
            appearance: (!erase).then_some(appearance),
        };
        if let Ok(outcome) = self.doc.editor.execute(cmd) {
            if let CommandOutcome::Object(id) = outcome {
                self.doc.selection = vec![id];
            }
            self.doc.anchor_sel.clear();
            self.prune_selection();
        }
        self.shape_builder = None;
        self.request_main_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::PathData;
    use kurbo::Shape as _;
    type CoreRect = amalith_core::geom::Rect;

    /// Two circles the user selected and asked Shape Builder to merge —
    /// exactly stacked, same size and position, matching the reported
    /// "two circles on top of each other" repro.
    fn stacked_circles() -> (Editor, ObjectId, ObjectId) {
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer { name: "Layer 1".into(), index: None })
            .unwrap()
        else {
            panic!()
        };
        let r = CoreRect::new(0.0, 0.0, 40.0, 40.0);
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreatePath { layer, path: PathData::ellipse(r), name: None })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreatePath { layer, path: PathData::ellipse(r), name: None })
            .unwrap()
        else {
            panic!()
        };
        (editor, a, b)
    }

    /// Two circles that only partially overlap, like the actual bug
    /// report's Venn diagram — `divide` splits this into 3 faces
    /// (A-only, the lens, B-only).
    fn overlapping_circles() -> (Editor, ObjectId, ObjectId) {
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer { name: "Layer 1".into(), index: None })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(CoreRect::new(0.0, 0.0, 40.0, 40.0)),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(CoreRect::new(20.0, 0.0, 60.0, 40.0)),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        (editor, a, b)
    }

    /// Reproduces the bug report exactly: a drag that swept faces 0 and
    /// 1 (A-only plus the shared lens — "all of A") of a real 3-face
    /// overlap must union into one clean piece, not fragment into a
    /// disconnected sliver where the two faces' independently-derived
    /// edges don't quite line up.
    #[test]
    fn merging_two_adjacent_faces_of_a_real_overlap_yields_one_clean_piece() {
        let (editor, a, b) = overlapping_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        assert_eq!(cache.faces.len(), 3, "two partially-overlapping circles should divide into 3 faces");

        let touched = [0usize, 1usize];
        let touched_inputs: Vec<PathInput> = touched
            .iter()
            .map(|&i| PathInput {
                contours: cache.faces[i].core_contours.clone(),
                appearance: cache.faces[i].appearance,
            })
            .collect();
        let united = pathfinder_apply(PathfinderOp::Unite, &touched_inputs);
        assert_eq!(united.len(), 1, "adjacent faces should union into exactly one piece, not fragment");

        // The union of "A-only" + "the lens" is just all of circle A —
        // its bounds should match A's own 40x40 bounding box, not be
        // inflated by a stray sliver hanging off it.
        let bb = united[0].path.geometry.bounding_box();
        assert!((bb.width() - 40.0).abs() < 0.5, "width {}", bb.width());
        assert!((bb.height() - 40.0).abs() < 0.5, "height {}", bb.height());
    }

    #[test]
    fn stacked_circles_are_eligible_and_form_one_face() {
        let (editor, a, b) = stacked_circles();
        let cache = build_cache(editor.document(), &[a, b]).expect("2 paths should be eligible");
        assert_eq!(cache.faces.len(), 1, "two identical circles should divide into exactly one face");
    }

    #[test]
    fn hovering_the_circle_center_finds_that_one_face() {
        let (editor, a, b) = stacked_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        let center = Point::new(20.0, 20.0);
        let hit = cache.faces.iter().position(|f| f.contour.winding(center) != 0);
        assert_eq!(hit, Some(0), "the circle's own center should land inside its one face");
    }

    #[test]
    fn dragging_across_the_only_face_merges_both_circles_into_one_object() {
        let (mut editor, a, b) = stacked_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        assert_eq!(cache.faces.len(), 1);
        let touched_path =
            PathData::from_bezpath(convert::bez_path_to_core(&cache.faces[0].contour));
        let outcome = editor
            .execute(Command::ShapeBuilder {
                objects: vec![a, b],
                touched: touched_path,
                erase: false,
                appearance: Some(cache.faces[0].appearance),
            })
            .unwrap();
        assert!(matches!(outcome, CommandOutcome::Object(_)), "merging should yield the new object");
        assert!(editor.document().object(a).is_none(), "both originals should be consumed");
        assert!(editor.document().object(b).is_none());
    }
}
