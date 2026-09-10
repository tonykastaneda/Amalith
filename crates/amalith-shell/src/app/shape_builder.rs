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
        let mut combined = BezPath::new();
        for &i in &touched {
            combined.extend(cache.faces[i].contour.clone());
        }
        let touched_path = amalith_core::PathData::from_bezpath(convert::bez_path_to_core(&combined));
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
