//! Shape Builder tool: Illustrator's drag-to-merge / Alt-drag-to-erase
//! over a multi-object selection. Canvas-wide within that selection (like
//! Join, not scoped to a single object): the tool decomposes the
//! selected shapes into their non-overlapping faces (the same planar
//! split `Pathfinder ▸ Divide` already computes), lets a drag sweep over
//! any number of them, and on release either unions the swept faces into
//! one new object (plain drag) or deletes them (Alt-drag) — leaving every
//! object the drag never touched completely alone.

use super::*;
use amalith_commands::PathInput;
#[cfg(test)]
use amalith_commands::{PathfinderOp, pathfinder_apply};
use amalith_core::Appearance;
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
    snapshot: Vec<(amalith_core::Object, amalith_core::Affine)>,
    last_pointer: Option<Point>,
}

impl ShapeBuilderCache {
    fn matches(&self, doc: &Document, source: &[ObjectId]) -> bool {
        self.source == source
            && self
                .snapshot
                .iter()
                .all(|(o, w)| doc.object(o.id) == Some(o) && doc.world_transform(o.id) == *w)
    }
}

/// Paths with a common parent, in paint order. A compound path or the result
/// of the previous gesture remains editable without selecting another object.
fn eligible(doc: &Document, selection: &[ObjectId]) -> Option<Vec<ObjectId>> {
    let ids: Vec<_> = selection
        .iter()
        .copied()
        .filter(|id| {
            doc.object(*id)
                .is_some_and(|o| o.visible && !o.locked && o.kind.path_data().is_some())
        })
        .collect();
    let parent = doc.object(*ids.first()?)?.parent;
    if ids
        .iter()
        .any(|id| doc.object(*id).is_none_or(|o| o.parent != parent))
    {
        return None;
    }
    Some(
        doc.children_of(parent)
            .iter()
            .copied()
            .filter(|id| ids.contains(id))
            .collect(),
    )
}

fn sweep_faces(faces: &[ShapeBuilderFace], from: Point, to: Point) -> Vec<usize> {
    let line = vello::kurbo::Line::new(from, to);
    let mut hits = Vec::new();
    for (i, face) in faces.iter().enumerate() {
        let mut cuts = vec![0.0, 1.0];
        for seg in face.contour.segments() {
            cuts.extend(seg.intersect_line(line).into_iter().map(|h| h.line_t));
        }
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-10);
        if let Some(w) = cuts.windows(2).find(|w| {
            face.contour
                .winding(from + (to - from) * ((w[0] + w[1]) * 0.5))
                != 0
        }) {
            hits.push((w[0], i));
        }
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    hits.into_iter().map(|(_, i)| i).collect()
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
    let faces = amalith_commands::shape_builder_regions(&inputs)
        .into_iter()
        .map(|r| ShapeBuilderFace {
            contour: convert::bez_path(&amalith_commands::polygon_path(&r.contours).geometry),
            core_contours: r.contours,
            appearance: r.appearance,
        })
        .collect();
    let snapshot = ids
        .iter()
        .map(|id| (doc.object(*id).unwrap().clone(), doc.world_transform(*id)))
        .collect();
    Some(ShapeBuilderCache {
        source,
        faces,
        snapshot,
        last_pointer: None,
    })
}

impl App {
    /// Rebuilds the region cache if the live selection no longer matches
    /// what it was built from, then returns it (`None` if the current
    /// selection isn't eligible at all).
    pub(in crate::app) fn shape_builder_cache(&mut self) -> Option<&ShapeBuilderCache> {
        let mut current =
            eligible(self.doc.editor.document(), &self.doc.selection).unwrap_or_default();
        current.sort();
        let stale = self
            .shape_builder
            .as_ref()
            .map(|c| !c.matches(self.doc.editor.document(), &current))
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
        let dp = self.doc_point(self.pointer);
        if let Some(cache) = self.shape_builder.as_mut() {
            cache.last_pointer = Some(dp);
        }
        self.drag = Drag::ShapeBuilderDrag {
            erase: self.alt_down,
            touched,
        };
        self.request_main_redraw();
        true
    }

    /// Include every region crossed between pointer samples, in sweep order.
    pub(in crate::app) fn shape_builder_move(&mut self, touched: &mut Vec<usize>) {
        let dp = self.doc_point(self.pointer);
        if let Some(cache) = self.shape_builder.as_mut() {
            let from = cache.last_pointer.replace(dp).unwrap_or(dp);
            for i in sweep_faces(&cache.faces, from, dp) {
                if !touched.contains(&i) {
                    touched.push(i);
                }
            }
        }
    }

    /// Commits the finished drag as one undo step. No-op if nothing was
    /// ever touched (a click on empty space, or a drag that never
    /// crossed a face).
    pub(in crate::app) fn commit_shape_builder(&mut self, erase: bool, touched: Vec<usize>) {
        let Some(cache) = self.shape_builder.as_ref() else {
            return;
        };
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
        let contours = amalith_commands::shape_builder_union(&touched_inputs);
        if contours.is_empty() {
            return;
        }
        let touched_path = amalith_commands::polygon_path(&contours);
        let appearance = cache.faces[touched[0]].appearance;
        let objects = cache.source.clone();

        let before: std::collections::HashSet<_> =
            self.doc.editor.document().objects().map(|o| o.id).collect();
        let source = objects.clone();
        let parent = self.doc.editor.document().object(source[0]).unwrap().parent;
        let cmd = Command::ShapeBuilder {
            objects,
            touched: touched_path,
            erase,
            appearance: (!erase).then_some(appearance),
        };
        if self.doc.editor.execute(cmd).is_ok() {
            self.doc.selection = self
                .doc
                .editor
                .document()
                .children_of(parent)
                .iter()
                .copied()
                .filter(|id| source.contains(id) || !before.contains(id))
                .collect();
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

    #[test]
    fn sweeping_all_circle_regions_leaves_one_curved_object() {
        let (mut editor, a, b) = overlapping_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        let inputs: Vec<_> = cache
            .faces
            .iter()
            .map(|f| PathInput {
                contours: f.core_contours.clone(),
                appearance: f.appearance,
            })
            .collect();
        let geometry =
            amalith_commands::polygon_path(&amalith_commands::shape_builder_union(&inputs))
                .geometry;
        editor
            .execute(Command::ShapeBuilder {
                objects: vec![a, b],
                touched: PathData::from_bezpath(geometry),
                erase: false,
                appearance: Some(cache.faces[0].appearance),
            })
            .unwrap();
        let objects: Vec<_> = editor.document().objects().collect();
        assert_eq!(
            objects.len(),
            1,
            "a complete sweep must consume both originals without leftover slivers"
        );
        let path = objects[0].kind.path_data().unwrap();
        assert!(path.subpaths().iter().all(|s| s.closed));
        assert_eq!(path.subpaths().len(), 1, "one union boundary");
        assert!(
            path.subpaths()[0].anchors.len() <= 12,
            "retain source curve spans rather than polygon vertices"
        );
    }

    #[test]
    fn fast_sweep_hits_the_lens_between_pointer_events() {
        let (editor, a, b) = overlapping_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        let touched = sweep_faces(&cache.faces, Point::new(-10., 20.), Point::new(70., 20.));
        assert_eq!(
            touched.len(),
            3,
            "all three regions lie between the two pointer samples"
        );
        assert_eq!(
            cache.faces[touched[0]]
                .contour
                .winding(Point::new(10., 20.)),
            1
        );
    }

    #[test]
    fn three_circle_merge_preserves_curve_spans_and_undo_redo() {
        let mut editor = Editor::new(Document::new("Three circles"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let mut ids = Vec::new();
        let mut originals = Vec::new();
        for rect in [
            CoreRect::new(0., 150., 400., 550.),
            CoreRect::new(20., 0., 420., 400.),
            CoreRect::new(180., 120., 580., 520.),
        ] {
            let path = PathData::ellipse(rect);
            originals.push(path.geometry.clone());
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreatePath {
                    layer,
                    path,
                    name: None,
                })
                .unwrap()
            else {
                panic!()
            };
            ids.push(id);
        }
        let cache = build_cache(editor.document(), &ids).unwrap();
        let inputs: Vec<_> = cache
            .faces
            .iter()
            .map(|f| PathInput {
                contours: f.core_contours.clone(),
                appearance: f.appearance,
            })
            .collect();
        let touched =
            amalith_commands::polygon_path(&amalith_commands::shape_builder_union(&inputs));
        editor
            .execute(Command::ShapeBuilder {
                objects: ids.clone(),
                touched,
                erase: false,
                appearance: Some(cache.faces[0].appearance),
            })
            .unwrap();
        let objects: Vec<_> = editor.document().objects().collect();
        assert_eq!(
            objects.len(),
            1,
            "no remnants from a complete sweep of three circles: {:?}",
            objects
                .iter()
                .map(|o| {
                    let p = &o.kind.path_data().unwrap().geometry;
                    (p.area(), p.bounding_box())
                })
                .collect::<Vec<_>>()
        );
        let merged = objects[0].kind.path_data().unwrap();
        assert_eq!(merged.subpaths().len(), 1);
        assert!(
            merged.subpaths()[0].anchors.len() <= 18,
            "only source spans and intersection splits should survive: {}",
            merged.subpaths()[0].anchors.len()
        );
        use kurbo::{ParamCurve, ParamCurveNearest};
        for seg in merged.geometry.segments() {
            for i in 0..=20 {
                let p = seg.eval(i as f64 / 20.);
                let distance = originals
                    .iter()
                    .flat_map(|p| p.segments())
                    .map(|s| s.nearest(p, 1e-8).distance_sq)
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    distance < 0.001 * 0.001,
                    "the restored boundary must follow original curves, deviation {}",
                    distance.sqrt()
                );
            }
        }
        let saved = merged.geometry.clone();
        editor.undo().unwrap();
        assert!(ids.iter().all(|id| editor.document().object(*id).is_some()));
        editor.redo().unwrap();
        assert_eq!(
            editor
                .document()
                .objects()
                .next()
                .unwrap()
                .kind
                .path_data()
                .unwrap()
                .geometry,
            saved
        );
        let id = editor.document().objects().next().unwrap().id;
        assert!(
            build_cache(editor.document(), &[id]).is_some(),
            "the result remains usable for the next gesture"
        );
    }

    #[test]
    fn cache_detects_geometry_changes_even_when_selection_ids_stay_the_same() {
        let (mut editor, a, b) = overlapping_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        let mut ids = vec![a, b];
        ids.sort();
        assert!(cache.matches(editor.document(), &ids));
        editor
            .execute(Command::MoveObjects {
                objects: vec![b],
                delta: amalith_core::Vec2::new(40., 0.),
            })
            .unwrap();
        assert!(!cache.matches(editor.document(), &ids));
        editor.undo().unwrap();
        assert!(cache.matches(editor.document(), &ids));
    }

    /// Two circles the user selected and asked Shape Builder to merge —
    /// exactly stacked, same size and position, matching the reported
    /// "two circles on top of each other" repro.
    fn stacked_circles() -> (Editor, ObjectId, ObjectId) {
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let r = CoreRect::new(0.0, 0.0, 40.0, 40.0);
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(r),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(r),
                name: None,
            })
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
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
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
        assert_eq!(
            cache.faces.len(),
            3,
            "two partially-overlapping circles should divide into 3 faces"
        );

        let touched = [0usize, 1usize];
        let touched_inputs: Vec<PathInput> = touched
            .iter()
            .map(|&i| PathInput {
                contours: cache.faces[i].core_contours.clone(),
                appearance: cache.faces[i].appearance,
            })
            .collect();
        let united = pathfinder_apply(PathfinderOp::Unite, &touched_inputs);
        assert_eq!(
            united.len(),
            1,
            "adjacent faces should union into exactly one piece, not fragment"
        );

        // The union of "A-only" + "the lens" is just all of circle A —
        // its bounds should match A's own 40x40 bounding box, not be
        // inflated by a stray sliver hanging off it. Tolerance is wide
        // enough to absorb curve-fitting's own (intentional) deviation
        // from the flattened polygon, not just floating-point noise.
        let bb = united[0].path.geometry.bounding_box();
        assert!((bb.width() - 40.0).abs() < 1.5, "width {}", bb.width());
        assert!((bb.height() - 40.0).abs() < 1.5, "height {}", bb.height());
    }

    #[test]
    fn large_overlapping_circles_divide_with_no_sliver_fragments() {
        // Real-world-scale circles (~380pt, matching the actual bug
        // report) — the small 40pt ones used elsewhere in this file
        // don't have enough flattened points to trigger the numerical
        // noise this regression test catches.
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(CoreRect::new(0.0, 0.0, 380.0, 380.0)),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreatePath {
                layer,
                path: PathData::ellipse(CoreRect::new(230.0, 0.0, 610.0, 380.0)),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        assert_eq!(cache.faces.len(), 3);
        for (i, f) in cache.faces.iter().enumerate() {
            assert_eq!(
                f.core_contours.len(),
                1,
                "face {i} split into {} contours — a sliver leaked through",
                f.core_contours.len()
            );
        }
    }

    #[test]
    fn stacked_circles_are_eligible_and_form_one_face() {
        let (editor, a, b) = stacked_circles();
        let cache = build_cache(editor.document(), &[a, b]).expect("2 paths should be eligible");
        assert_eq!(
            cache.faces.len(),
            1,
            "two identical circles should divide into exactly one face"
        );
    }

    #[test]
    fn hovering_the_circle_center_finds_that_one_face() {
        let (editor, a, b) = stacked_circles();
        let cache = build_cache(editor.document(), &[a, b]).unwrap();
        let center = Point::new(20.0, 20.0);
        let hit = cache
            .faces
            .iter()
            .position(|f| f.contour.winding(center) != 0);
        assert_eq!(
            hit,
            Some(0),
            "the circle's own center should land inside its one face"
        );
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
        assert!(
            matches!(outcome, CommandOutcome::Object(_)),
            "merging should yield the new object"
        );
        assert!(
            editor.document().object(a).is_none(),
            "both originals should be consumed"
        );
        assert!(editor.document().object(b).is_none());
    }
}
