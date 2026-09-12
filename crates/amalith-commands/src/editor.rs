//! The command engine entry point.
//!
//! `Editor` pairs a [`Document`] with its undo/redo history and is the
//! thing GUI tools, keyboard shortcuts, plugins, scripts, the CLI, and
//! agents actually hold and call `execute`/`undo`/`redo` on — never a bare
//! `Document`. History lives here (not on `Document` itself) so
//! `amalith-core` stays free of any undo/redo concept and stays usable
//! headless (e.g. a one-shot CLI conversion has no need for a history
//! stack at all).
use crate::align::{self, AlignKind, AlignTo};
use crate::command::{Command, CommandOutcome, GradientRef, PasteStack, PathfinderOp};
use crate::pathfinder::{self, PathInput};
use crate::edit::{self, Edit, NewId};
use crate::error::CommandError;
use crate::history::History;
use amalith_core::{
    Affine, Appearance, Artboard, ArtboardId, Asset, AssetId, AssetKind, BlendData, BlendSpacing,
    Color, Document, DocumentError, Effect, Gradient, GradientId, GradientKind, Layer, LayerId,
    Object, ObjectId, ObjectKind, ObjectParent, Paint, PathData, Point, Rect, Vec2,
};
use kurbo::BezPath;
use std::collections::{HashMap, HashSet};

/// A document plus its undo/redo history. See module docs.
///
/// Also owns a memoized bounds cache — see [`Editor::bounds_of`] and
/// `PERFORMANCE.md` at the repo root. The cache lives here, never on
/// `Document`, and is wiped wholesale after every successful `execute` /
/// `undo` / `redo`.
///
/// And a clipboard for object copy/paste — see [`Editor::copy`] and
/// [`Command::Paste`]. Copying takes an independent deep snapshot, so
/// deleting the copied originals afterward never empties the clipboard,
/// and it survives undo/redo (it isn't part of the document or history).
#[derive(Debug)]
pub struct Editor {
    document: Document,
    history: History,
    bounds_cache: HashMap<ObjectId, Option<Rect>>,
    clipboard: Option<Clipboard>,
}

/// A deep, self-contained snapshot of copied objects. Nothing here
/// references the live document: every object in a copied subtree (each
/// root and, for groups, every descendant) is cloned in full, keyed by its
/// *original* id. Those original ids are only ever used to look up "does
/// the source still exist" for `PasteStack::InFront`/`Behind`; a fresh id
/// is minted for every object on each individual paste.
#[derive(Debug, Clone)]
struct Clipboard {
    /// Copied roots, in the order `copy` was given them.
    roots: Vec<ClipboardRoot>,
    /// Every copied object (roots and group descendants alike), keyed by
    /// its id at copy time.
    objects: HashMap<ObjectId, Object>,
}

#[derive(Debug, Clone)]
struct ClipboardRoot {
    /// The root's own id at copy time. May no longer exist in the document
    /// by the time a paste happens.
    source_id: ObjectId,
    /// The root's parent at copy time (layer or group). `None` for a root
    /// that never had one in any `Document` to begin with — content
    /// imported from external SVG via [`Editor::copy_from_svg`] — in which
    /// case placement always falls back to the top layer.
    source_parent: Option<ObjectParent>,
    /// The root's paint-order index within `source_parent` at copy time.
    /// Informational only: placement always re-resolves the source's
    /// *current* position at paste time (see `compile_paste`), since the
    /// document may have changed shape since the copy.
    #[allow(dead_code)]
    source_index: usize,
    /// Document-space bounds at copy time.
    bounds: Option<Rect>,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            history: History::default(),
            bounds_cache: HashMap::new(),
            clipboard: None,
        }
    }

    /// Drops the entire undo/redo history, making the document's current
    /// state the non-undoable baseline. Used right after seeding a new
    /// document (its starter artboards and layer) so the user can't undo
    /// past having any artboard — matching Illustrator.
    pub fn clear_history(&mut self) {
        self.history = History::default();
    }

    /// Snapshots `ids` (and, for any group, its full descendant tree) into
    /// the clipboard, replacing whatever was copied before. Does not
    /// mutate the document and is not undoable — only [`Command::Paste`],
    /// applied through [`Editor::execute`], touches the document.
    pub fn copy(&mut self, ids: &[ObjectId]) -> Result<(), CommandError> {
        let mut roots = Vec::with_capacity(ids.len());
        let mut objects = HashMap::new();
        for &id in ids {
            let object = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            let source_parent = object.parent;
            let source_index = self
                .document
                .children_of(source_parent)
                .iter()
                .position(|&child| child == id)
                .unwrap_or(0);
            let bounds = self.document.bounds_of(id);
            roots.push(ClipboardRoot {
                source_id: id,
                source_parent: Some(source_parent),
                source_index,
                bounds,
            });
            collect_subtree(&self.document, id, &mut objects);
        }
        self.clipboard = Some(Clipboard { roots, objects });
        Ok(())
    }

    /// Snapshots externally-sourced SVG (e.g. the OS clipboard's text on a
    /// Cmd+V, when it parses as SVG) into the clipboard, replacing whatever
    /// was copied before — the [`Editor::copy`] equivalent for content that
    /// never lived in this (or any) `Document`. Like `copy`, this doesn't
    /// mutate the document and isn't undoable. See `amalith_io::import_svg`
    /// for exactly what SVG this understands.
    pub fn copy_from_svg(&mut self, svg: &str) -> Result<(), CommandError> {
        let imported = amalith_io::import_svg(svg)?;
        let roots = imported
            .roots
            .iter()
            .map(|&source_id| ClipboardRoot {
                source_id,
                source_parent: None,
                source_index: 0,
                bounds: imported_bounds(&imported.objects, source_id),
            })
            .collect();
        self.clipboard = Some(Clipboard {
            roots,
            objects: imported.objects,
        });
        Ok(())
    }

    /// Whether a copy is currently held (and non-empty).
    pub fn has_clipboard(&self) -> bool {
        self.clipboard.as_ref().is_some_and(|c| !c.roots.is_empty())
    }

    /// The union of every copied root's document-space bounds, as recorded
    /// at copy time. This is what a GUI should center on the visible view
    /// to compute plain Paste's `delta`: `delta = view_center -
    /// clipboard_bounds().center()`. `None` if there's no clipboard, or
    /// every copied root had no contributing geometry (e.g. empty groups).
    pub fn clipboard_bounds(&self) -> Option<Rect> {
        self.clipboard
            .as_ref()?
            .roots
            .iter()
            .filter_map(|root| root.bounds)
            .reduce(|a, b| a.union(b))
    }

    /// Pastes the current clipboard as one undo group and returns the new
    /// id of every pasted root, in the clipboard's order — the full
    /// complement [`Editor::execute`]'s single-id [`CommandOutcome`] can't
    /// carry, for CLI/agent callers that need to select every new root.
    /// `Editor::execute(Command::Paste { .. })` is equivalent but only
    /// surfaces the first root's id.
    pub fn paste(&mut self, delta: Vec2, stack: PasteStack) -> Result<Vec<ObjectId>, CommandError> {
        let (edits, root_ids) = self.compile_paste(delta, stack)?;
        let mut inverses = Vec::with_capacity(edits.len());
        for edit in edits {
            let (inverse, _created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
        }
        inverses.reverse();
        self.history.record(inverses);
        self.bounds_cache.clear();
        Ok(root_ids)
    }

    /// Duplicates `ids` (deep-copying any group's descendants) as one undo
    /// group and returns the new id of every duplicate, in the same order
    /// as `ids` — the multi-id complement [`Editor::execute`]'s single-id
    /// [`CommandOutcome`] can't carry. Each duplicate lands as the top
    /// child of *its own* current parent, translated by `delta`; unlike
    /// [`Editor::paste`], this never touches the clipboard.
    /// `Editor::execute(Command::DuplicateObjects { .. })` is equivalent
    /// but only surfaces the first duplicate's id.
    pub fn duplicate_objects(
        &mut self,
        ids: &[ObjectId],
        delta: Vec2,
    ) -> Result<Vec<ObjectId>, CommandError> {
        let (edits, new_ids) = self.compile_duplicate_objects(ids, delta)?;
        let mut inverses = Vec::with_capacity(edits.len());
        for edit in edits {
            let (inverse, _created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
        }
        inverses.reverse();
        self.history.record(inverses);
        self.bounds_cache.clear();
        Ok(new_ids)
    }

    /// Dissolves each group in `ids` as one undo group and returns the id
    /// of every freed child, across all of them, in paint order — the
    /// multi-id complement [`Editor::execute`]'s single-id
    /// [`CommandOutcome`] can't carry (e.g. for the GUI to select
    /// everything a Cmd+Shift+G just freed). `Editor::execute(Command::
    /// Ungroup { .. })` is equivalent but only surfaces one freed child's
    /// id.
    pub fn ungroup(&mut self, ids: &[ObjectId]) -> Result<Vec<ObjectId>, CommandError> {
        let (edits, freed_ids) = self.compile_ungroup(ids)?;
        let mut inverses = Vec::with_capacity(edits.len());
        for edit in edits {
            let (inverse, _created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
        }
        inverses.reverse();
        self.history.record(inverses);
        self.bounds_cache.clear();
        Ok(freed_ids)
    }

    /// Read-only access to the underlying document. There is no
    /// `document_mut`: every mutation must go through [`Editor::execute`].
    pub fn document(&self) -> &Document {
        &self.document
    }

    /// Document-space bounds of an object, memoized. Populates the cache on
    /// a miss by calling `Document::bounds_of` (still the source of
    /// truth); the cache is wiped after every `execute` / `undo` / `redo`,
    /// so a cached value can never outlive the mutation that would have
    /// invalidated it. Takes `&mut self` so callers are forced through
    /// `Editor` rather than caching bounds themselves. See
    /// `PERFORMANCE.md`.
    pub fn bounds_of(&mut self, id: ObjectId) -> Option<Rect> {
        *self
            .bounds_cache
            .entry(id)
            .or_insert_with(|| self.document.bounds_of(id))
    }

    /// Discards the history and returns the plain document, e.g. before
    /// handing it to `amalith-io` for saving (history is not persisted).
    pub fn into_document(self) -> Document {
        self.document
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// True once the live document has diverged from the last save (or,
    /// having never been saved this session, from how `clear_history`
    /// last left it) — the real "unsaved changes" signal, unlike
    /// [`Self::can_undo`] (which just means *some* undo exists, forever,
    /// even right after saving).
    pub fn is_dirty(&self) -> bool {
        self.history.is_dirty()
    }

    /// Marks the document's current state as matching what's on disk —
    /// call this right after a successful save.
    pub fn mark_clean(&mut self) {
        self.history.mark_clean();
    }

    /// Executes `command`, recording it so it can be undone.
    pub fn execute(&mut self, command: Command) -> Result<CommandOutcome, CommandError> {
        // `Paste` can create several new top-level roots, which the single
        // id in `CommandOutcome::Object` can't fully carry; it is handled
        // directly (see `Editor::paste`) rather than through the generic
        // `compile`/apply loop below, and yields the *first* root's id
        // here (see `Command::Paste`'s docs for the full list).
        if let Command::Paste { delta, stack } = command {
            let root_ids = self.paste(delta, stack)?;
            let first = *root_ids
                .first()
                .expect("compile_paste always yields at least one root id when it succeeds");
            return Ok(CommandOutcome::Object(first));
        }
        // `DuplicateObjects` has the identical multi-id problem as `Paste`
        // above, for the identical reason — see `Editor::duplicate_objects`.
        if let Command::DuplicateObjects { objects, delta } = command {
            let new_ids = self.duplicate_objects(&objects, delta)?;
            let first = *new_ids
                .first()
                .expect("compile_duplicate_objects always yields at least one id when it succeeds");
            return Ok(CommandOutcome::Object(first));
        }
        // Same multi-id problem as `Paste`/`DuplicateObjects` above — see
        // `Editor::ungroup`. Unlike those, an empty group is possible (all
        // its children were deleted some other way first), so ungrouping
        // it validly frees zero ids — `None`, not a panic, in that case.
        if let Command::Ungroup { ids } = command {
            let freed_ids = self.ungroup(&ids)?;
            return Ok(freed_ids
                .first()
                .map_or(CommandOutcome::None, |&id| CommandOutcome::Object(id)));
        }
        // Same multi-id problem as `Paste`/`DuplicateObjects` above — see
        // `Editor::offset_path`.
        if let Command::OffsetPath { objects, offset, join, miter_limit } = command {
            let new_ids = self.offset_path(&objects, offset, join, miter_limit)?;
            let first = *new_ids
                .first()
                .expect("compile_offset_path always yields at least one id when it succeeds");
            return Ok(CommandOutcome::Object(first));
        }
        // Blend creation/option changes already regenerate their own
        // steps directly (`compile_blend_steps`); the live-rebuild pass
        // below is only for *other* commands whose edits happen to touch
        // a blend's dependencies, so it's skipped here to avoid a
        // redundant, wasteful rebuild of a blend right after it was
        // (re)built on purpose.
        let skip_live_rebuild = matches!(
            &command,
            Command::MakeBlend { .. }
                | Command::SetBlendOptions { .. }
                | Command::ReverseBlendSpine { .. }
                | Command::ReverseBlendStacking { .. }
        );
        let edits = self.compile(command)?;
        if edits.is_empty() {
            return Ok(CommandOutcome::None);
        }
        let touched: HashSet<ObjectId> = edits.iter().filter_map(touched_object_id).collect();
        let mut inverses = Vec::with_capacity(edits.len());
        let mut new_id = None;
        for edit in edits {
            let (inverse, created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
            if new_id.is_none() {
                new_id = created;
            }
        }
        // Live-updating blends: any blend whose start, end, or spine was
        // just touched regenerates its steps from the fresh state (read
        // *after* the edits above applied, so this sees the new
        // position/shape/color, not the old one).
        if !skip_live_rebuild {
            for group_id in self.blends_depending_on(&touched) {
                let Some(ObjectKind::Group(g)) = self.document.object(group_id).map(|o| &o.kind)
                else {
                    continue;
                };
                let Some(blend) = g.blend else { continue };
                let existing_children = g.children.clone();
                let rebuild_edits = self.compile_blend_steps(
                    group_id,
                    &existing_children,
                    blend.start,
                    blend.end,
                    blend.spine,
                    blend.spacing,
                    blend.spine_reversed,
                    blend.stack_reversed,
                )?;
                for edit in rebuild_edits {
                    let (inverse, _) = edit::apply(edit, &mut self.document)?;
                    inverses.push(inverse);
                }
            }
        }
        inverses.reverse();
        self.history.record(inverses);
        self.bounds_cache.clear();
        Ok(outcome_of(new_id))
    }

    /// Reverts the most recently executed (and not-yet-undone) command.
    pub fn undo(&mut self) -> Result<(), CommandError> {
        let group = self.history.pop_undo().ok_or(CommandError::NothingToUndo)?;
        let redo_group = apply_group(&mut self.document, group)?;
        self.history.push_redo(redo_group);
        self.bounds_cache.clear();
        Ok(())
    }

    /// Re-applies the most recently undone command.
    pub fn redo(&mut self) -> Result<(), CommandError> {
        let group = self.history.pop_redo().ok_or(CommandError::NothingToRedo)?;
        let undo_group = apply_group(&mut self.document, group)?;
        self.history.push_undo(undo_group);
        self.bounds_cache.clear();
        Ok(())
    }

    /// Compiles a public `Command` into the low-level `Edit` it performs.
    /// May read (but must not mutate) the document to resolve defaults
    /// like "append at the end" or "translate from the current transform".
    /// A clone of `id`'s [`PathData`](amalith_core::PathData), or an error
    /// if `id` is missing or not a path. The clone is the working copy the
    /// anchor-editing commands mutate before emitting `Edit::SetPathData`.
    fn path_data(&self, id: ObjectId) -> Result<amalith_core::PathData, CommandError> {
        let object = self
            .document
            .object(id)
            .ok_or(CommandError::ObjectNotFound(id))?;
        match &object.kind {
            kind if kind.path_data().is_some() => Ok(kind.path_data().unwrap().clone()),
            _ => Err(CommandError::NotAPath(id)),
        }
    }

    /// Edits that (re)generate a blend group's in-between steps for the
    /// given `start`/`end`/`spine`/`spacing`, replacing whatever generated
    /// steps `existing_children` currently holds (its two entries that
    /// equal `start`/`end` are left alone; everything else in it is
    /// removed and regenerated). Shared by `Command::MakeBlend` (a fresh
    /// group, `existing_children` is just `[start, end]`),
    /// `Command::SetBlendOptions`, and the live-rebuild hook in
    /// `Editor::execute`.
    fn compile_blend_steps(
        &self,
        group_id: ObjectId,
        existing_children: &[ObjectId],
        start_id: ObjectId,
        end_id: ObjectId,
        spine_id: Option<ObjectId>,
        spacing: BlendSpacing,
        spine_reversed: bool,
        stack_reversed: bool,
    ) -> Result<Vec<Edit>, CommandError> {
        let start_obj = self
            .document
            .object(start_id)
            .ok_or(CommandError::ObjectNotFound(start_id))?;
        let end_obj = self
            .document
            .object(end_id)
            .ok_or(CommandError::ObjectNotFound(end_id))?;
        let ObjectKind::Path(start_path) = &start_obj.kind else {
            return Err(CommandError::NotAPath(start_id));
        };
        let ObjectKind::Path(end_path) = &end_obj.kind else {
            return Err(CommandError::NotAPath(end_id));
        };

        // Every source point, flattened and moved into document space (the
        // blend group itself always sits at identity, at the same parent
        // the two originals shared before joining it — see
        // `Command::MakeBlend` — so this is also the group's own local
        // space).
        const TOL: f64 = 0.25;
        let start_xf = self.document.world_transform(start_id);
        let end_xf = self.document.world_transform(end_id);
        let flatten_in_place = |path: &PathData, xf: Affine| -> Vec<(Vec<Point>, bool)> {
            path.subpaths()
                .iter()
                .zip(path.flattened_points(TOL))
                .map(|(sp, pts)| (pts.into_iter().map(|p| xf * p).collect(), sp.closed))
                .collect()
        };
        let a = flatten_in_place(start_path, start_xf);
        let b = flatten_in_place(end_path, end_xf);
        let a_points: Vec<Vec<Point>> = a.iter().map(|(p, _)| p.clone()).collect();
        let b_points: Vec<Vec<Point>> = b.iter().map(|(p, _)| p.clone()).collect();
        let center_a = amalith_core::blend::shape_center(&a_points);
        let center_b = amalith_core::blend::shape_center(&b_points);

        let spine_points: Option<Vec<Point>> = match spine_id {
            Some(sid) => {
                let obj = self.document.object(sid).ok_or(CommandError::ObjectNotFound(sid))?;
                let ObjectKind::Path(p) = &obj.kind else {
                    return Err(CommandError::NotAPath(sid));
                };
                let xf = self.document.world_transform(sid);
                p.flattened_points(TOL)
                    .into_iter()
                    .next()
                    .map(|pts| pts.into_iter().map(|pt| xf * pt).collect())
            }
            None => None,
        };
        let line_or_spine_length = || match &spine_points {
            Some(pts) => pts.windows(2).map(|w| (w[1] - w[0]).hypot()).sum(),
            None => (center_b - center_a).hypot(),
        };

        let steps: u32 = match spacing {
            BlendSpacing::SmoothColor => {
                amalith_core::blend::smooth_color_steps(start_obj.appearance.fill(), end_obj.appearance.fill())
            }
            BlendSpacing::SpecifiedSteps(n) => n,
            BlendSpacing::SpecifiedDistance(d) if d > 0.0 => {
                ((line_or_spine_length() / d).round() as i64 - 1).max(0) as u32
            }
            BlendSpacing::SpecifiedDistance(_) => 0,
        };

        let mut edits = Vec::new();
        for &id in existing_children {
            if id != start_id && id != end_id {
                edits.push(Edit::RemoveObject { id });
            }
        }
        // Every new step just appends — the `SetChildOrder` edit below
        // puts everything in its final start → steps → end order
        // regardless of these intermediate append positions, so nothing
        // here depends on start/end's original relative order.
        let mut append_at = 2usize;
        let mut order = vec![start_id];
        let start_fill = start_obj.appearance.fill();
        let end_fill = end_obj.appearance.fill();
        let start_stroke = start_obj.appearance.stroke();
        let end_stroke = end_obj.appearance.stroke();
        let start_width = start_obj.appearance.stroke_width();
        let end_width = end_obj.appearance.stroke_width();
        // The topmost Fill/Stroke item's own live effect stack (Zig Zag,
        // Roughen, ...) on each endpoint — `set_fill`/`set_stroke` below
        // only ever touch paint, so without this a generated step would
        // silently render as bare, effect-less geometry even though both
        // endpoints show a live effect (see `blend::lerp_effect_stack`).
        let effects_of = |obj: &Object, is_fill: bool| -> Vec<Effect> {
            obj.appearance
                .items
                .iter()
                .rev()
                .find(|i| if is_fill { i.is_fill() } else { i.is_stroke() })
                .map(|i| i.effects().to_vec())
                .unwrap_or_default()
        };
        let start_fill_effects = effects_of(start_obj, true);
        let end_fill_effects = effects_of(end_obj, true);
        let start_stroke_effects = effects_of(start_obj, false);
        let end_stroke_effects = effects_of(end_obj, false);
        for i in 1..=steps {
            let t = i as f64 / (steps as f64 + 1.0);
            // Reverse Spine only ever changes *where* a step's center
            // sits along the spine/line — the shape/color interpolation
            // itself still runs start (t=0) to end (t=1) so `start`/`end`
            // keep their own identity.
            let spine_t = if spine_reversed { 1.0 - t } else { t };
            let center = match &spine_points {
                Some(pts) => amalith_core::blend::point_on_path(pts, spine_t),
                None => Point::new(
                    center_a.x + (center_b.x - center_a.x) * spine_t,
                    center_a.y + (center_b.y - center_a.y) * spine_t,
                ),
            };
            let path = amalith_core::blend::interpolate_step(&a, &b, center_a, center_b, center, t);
            let step_id = ObjectId::new();
            let mut obj = Object::new(step_id, ObjectParent::Group(group_id), ObjectKind::Path(path));
            obj.appearance.set_fill(amalith_core::blend::lerp_paint(start_fill, end_fill, t));
            obj.appearance.set_stroke(amalith_core::blend::lerp_paint(start_stroke, end_stroke, t));
            obj.appearance.set_stroke_width(start_width + (end_width - start_width) * t);
            if !start_fill_effects.is_empty() || !end_fill_effects.is_empty() {
                let fx = amalith_core::blend::lerp_effect_stack(&start_fill_effects, &end_fill_effects, t);
                if let Some(item) = obj.appearance.items.iter_mut().rev().find(|i| i.is_fill()) {
                    *item.effects_mut() = fx;
                }
            }
            if !start_stroke_effects.is_empty() || !end_stroke_effects.is_empty() {
                let fx = amalith_core::blend::lerp_effect_stack(&start_stroke_effects, &end_stroke_effects, t);
                if let Some(item) = obj.appearance.items.iter_mut().rev().find(|i| i.is_stroke()) {
                    *item.effects_mut() = fx;
                }
            }
            edits.push(Edit::InsertObject { object: Box::new(obj), index: append_at });
            append_at += 1;
            order.push(step_id);
        }
        order.push(end_id);
        if stack_reversed {
            order.reverse();
        }
        edits.push(Edit::SetChildOrder { parent: ObjectParent::Group(group_id), order });
        Ok(edits)
    }

    /// Every blend group whose `start`, `end`, or `spine` is in `touched`
    /// — for the live-rebuild hook in `Editor::execute`.
    fn blends_depending_on(&self, touched: &HashSet<ObjectId>) -> Vec<ObjectId> {
        self.document
            .objects()
            .filter_map(|o| match &o.kind {
                ObjectKind::Group(g) => {
                    let b = g.blend?;
                    (touched.contains(&b.start)
                        || touched.contains(&b.end)
                        || b.spine.is_some_and(|s| touched.contains(&s)))
                    .then_some(o.id)
                }
                _ => None,
            })
            .collect()
    }

    fn compile(&self, command: Command) -> Result<Vec<Edit>, CommandError> {
        let edits = match command {
            Command::CreateArtboard { name, rect, index } => {
                let artboard = Artboard::new(ArtboardId::new(), name, rect);
                let index = index.unwrap_or_else(|| self.document.artboards().len());
                vec![Edit::InsertArtboard { artboard, index }]
            }
            Command::DeleteArtboard { id } => vec![Edit::RemoveArtboard { id }],
            Command::AddGuide { orient, pos } => {
                let guide = amalith_core::Guide::new(orient, pos);
                let index = self.document.guides().len();
                vec![Edit::InsertGuide { guide, index }]
            }
            Command::MoveGuide { id, pos } => vec![Edit::SetGuidePos { id, pos }],
            Command::DeleteGuide { id } => vec![Edit::RemoveGuide { id }],
            Command::ClearGuides => self
                .document
                .guides()
                .iter()
                .map(|g| Edit::RemoveGuide { id: g.id })
                .collect(),
            Command::DeleteObject { id } => vec![Edit::RemoveObject { id }],
            Command::DeleteObjects { ids } => ids
                .into_iter()
                .map(|id| Edit::RemoveObject { id })
                .collect(),
            Command::RenameArtboard { id, name } => vec![Edit::RenameArtboard { id, name }],
            Command::SetArtboardFill { id, fill } => vec![Edit::SetArtboardFill { id, fill }],
            Command::SetDocumentUnit { unit } => vec![Edit::SetDocumentUnit { unit }],
            Command::SetColorMode { mode } => vec![Edit::SetColorMode { mode }],
            Command::ApplyGradient {
                objects,
                stroke,
                source,
            } => {
                let mut edits = Vec::with_capacity(objects.len() + 1);
                let gid = match source {
                    GradientRef::Existing(id) => {
                        self.document
                            .gradient(id)
                            .ok_or(CommandError::GradientNotFound(id))?;
                        id
                    }
                    GradientRef::New(kind) => {
                        let id = GradientId::new();
                        let gradient = match kind {
                            GradientKind::Linear => Gradient::linear(id),
                            GradientKind::Radial => Gradient::radial(id),
                            GradientKind::Freeform => Gradient::freeform(id),
                        };
                        // Must be first so `execute` reports the new id.
                        edits.push(Edit::InsertGradient {
                            gradient,
                            index: self.document.gradients().len(),
                        });
                        id
                    }
                };
                let paint = Paint::Gradient(gid);
                for id in objects {
                    edits.push(if stroke {
                        Edit::SetStroke { id, paint }
                    } else {
                        Edit::SetFill { id, paint }
                    });
                }
                edits
            }
            Command::EditGradient { id, gradient } => {
                self.document
                    .gradient(id)
                    .ok_or(CommandError::GradientNotFound(id))?;
                vec![Edit::SetGradient { id, gradient }]
            }
            Command::DeleteGradient { id } => vec![Edit::RemoveGradient { id }],
            Command::RenameLayer { id, name } => vec![Edit::RenameLayer { id, name }],
            Command::SetLayerOptions { id, options } => vec![Edit::SetLayerOptions { id, options }],
            Command::RenameObject { id, name } => vec![Edit::RenameObject { id, name }],
            Command::ResizeArtboard { id, rect } => vec![Edit::ResizeArtboard { id, rect }],
            Command::MoveArtboard { id, delta } => {
                let artboard = self
                    .document
                    .artboard(id)
                    .ok_or(CommandError::ArtboardNotFound(id))?;
                let source = artboard.rect;
                let mut edits = vec![Edit::ResizeArtboard {
                    id,
                    rect: source + delta,
                }];
                edits.extend(self.document.objects().filter_map(|object| {
                    let bounds = self.document.bounds_of(object.id)?;
                    rects_intersect(bounds, source).then(|| Edit::SetTransform {
                        id: object.id,
                        transform: Affine::translate(delta) * object.transform,
                    })
                }));
                edits
            }
            Command::DuplicateArtboard { id, delta } => {
                let source = self
                    .document
                    .artboard(id)
                    .ok_or(CommandError::ArtboardNotFound(id))?
                    .rect;
                let artboard = Artboard::new(
                    ArtboardId::new(),
                    next_artboard_name(&self.document),
                    source + delta,
                );
                let mut edits = vec![Edit::InsertArtboard {
                    artboard,
                    index: self.document.artboards().len(),
                }];
                for layer in self.document.layers() {
                    let parent = ObjectParent::Layer(layer.id);
                    let mut insert_index = self.document.children_of(parent).len();
                    for &id in self.document.children_of(parent) {
                        let Some(bounds) = self.document.bounds_of(id) else {
                            continue;
                        };
                        if !rects_intersect(bounds, source) {
                            continue;
                        }
                        let mut copy = self.document.object(id).unwrap().clone();
                        copy.id = ObjectId::new();
                        copy.transform = Affine::translate(delta) * copy.transform;
                        edits.push(Edit::InsertObject {
                            object: Box::new(copy),
                            index: insert_index,
                        });
                        insert_index += 1;
                    }
                }
                edits
            }
            Command::CreateLayer { name, index } => {
                let layer = Layer::new(LayerId::new(), name);
                let index = index.unwrap_or_else(|| self.document.layers().len());
                vec![Edit::InsertLayer { layer, index }]
            }
            Command::CreateRect { layer, rect, name } => {
                let mut object = Object::rectangle(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    rect,
                );
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                vec![Edit::InsertObject {
                    object: Box::new(object),
                    index,
                }]
            }
            Command::CreateEllipse { layer, rect, name } => {
                let mut object = Object::new(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    amalith_core::ObjectKind::Path(amalith_core::PathData::ellipse(rect)),
                );
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                vec![Edit::InsertObject {
                    object: Box::new(object),
                    index,
                }]
            }
            Command::CreateImage {
                layer,
                path,
                bounds,
                transform,
                name,
                embedded,
                modified,
                size,
            } => {
                let asset_id = AssetId::new();
                let file_name = std::path::Path::new(&path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Image".into());
                let asset = if embedded {
                    Asset::embedded(asset_id, file_name, AssetKind::Image, path)
                } else {
                    Asset::linked(asset_id, file_name, AssetKind::Image, path, modified, size)
                };
                let mut object = Object::new(
                    ObjectId::new(),
                    ObjectParent::Layer(layer),
                    ObjectKind::Image(amalith_core::ImageData {
                        asset: asset_id,
                        local_bounds: bounds,
                    }),
                );
                object.appearance.set_fill(Paint::None);
                object.appearance.set_stroke(Paint::None);
                object.transform = transform;
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                let asset_index = self.document.assets().len();
                vec![
                    Edit::InsertAsset {
                        asset,
                        index: asset_index,
                    },
                    Edit::InsertObject {
                        object: Box::new(object),
                        index,
                    },
                ]
            }
            Command::CreatePath { layer, path, name } => {
                let mut object = Object::new(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    amalith_core::ObjectKind::Path(path),
                );
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                vec![Edit::InsertObject {
                    object: Box::new(object),
                    index,
                }]
            }
            Command::CreateText {
                layer,
                mut data,
                transform,
                name,
            } => {
                if let amalith_core::TextKind::Path(pt) = data.kind {
                    let source = self.document.object(pt.path).ok_or(CommandError::ObjectNotFound(pt.path))?;
                    let amalith_core::ObjectKind::Path(geometry) = &source.kind else {
                        return Err(CommandError::NotAPath(pt.path));
                    };
                    data.path_geometry = Some(geometry.clone());
                    let mut converted = source.clone();
                    converted.kind = amalith_core::ObjectKind::Text(data);
                    converted.appearance.set_fill(Paint::Solid(Color::rgb(0.0, 0.0, 0.0)));
                    converted.appearance.set_stroke(Paint::None);
                    let index = self.document.children_of(source.parent).iter().position(|id| *id == source.id).unwrap_or(0);
                    return Ok(vec![Edit::RemoveObject { id: source.id }, Edit::InsertObject { object: Box::new(converted), index }]);
                }
                let mut edits = Vec::new();
                let mut object = Object::new(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    amalith_core::ObjectKind::Text(data),
                );
                // Text follows Illustrator's default — black fill, no stroke —
                // not the shape tools' visible-stroke default.
                object.appearance.set_fill(Paint::Solid(Color::rgb(0.0, 0.0, 0.0)));
                object.appearance.set_stroke(Paint::None);
                object.transform = transform;
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                edits.push(Edit::InsertObject {
                    object: Box::new(object),
                    index,
                });
                edits
            }
            Command::SetText { object, data } => vec![Edit::SetTextData { id: object, data }],
            Command::SetTexts { items } => items
                .into_iter()
                .map(|(id, data)| Edit::SetTextData { id, data })
                .collect(),
            Command::ThreadText { from, to } => {
                let text_of = |id: ObjectId| -> Option<amalith_core::TextData> {
                    match &self.document.object(id)?.kind {
                        amalith_core::ObjectKind::Text(t) => Some(t.clone()),
                        _ => None,
                    }
                };
                let from_td = text_of(from).ok_or(CommandError::NotText(from))?;
                let to_td = text_of(to).ok_or(CommandError::NotText(to))?;
                let old_next = from_td.thread_next;
                let mut new_from = from_td;
                new_from.thread_next = Some(to);
                let mut new_to = to_td;
                new_to.thread_prev = Some(from);
                new_to.thread_next = old_next;
                new_to.content = String::new();
                let mut edits = vec![
                    Edit::SetTextData {
                        id: from,
                        data: new_from,
                    },
                    Edit::SetTextData {
                        id: to,
                        data: new_to,
                    },
                ];
                if let Some(old) = old_next {
                    if let Some(mut old_td) = text_of(old) {
                        old_td.thread_prev = Some(to);
                        edits.push(Edit::SetTextData {
                            id: old,
                            data: old_td,
                        });
                    }
                }
                edits
            }
            Command::UnthreadText { object } => {
                let text_of = |id: ObjectId| -> Option<amalith_core::TextData> {
                    match &self.document.object(id)?.kind {
                        amalith_core::ObjectKind::Text(t) => Some(t.clone()),
                        _ => None,
                    }
                };
                let td = text_of(object).ok_or(CommandError::NotText(object))?;
                let (prev, next) = (td.thread_prev, td.thread_next);
                if prev.is_none() && next.is_none() {
                    vec![]
                } else {
                    let mut edits = Vec::new();
                    let mut standalone = td.clone();
                    standalone.thread_prev = None;
                    standalone.thread_next = None;
                    // A removed head hands its story to the next frame.
                    if prev.is_none() {
                        if let Some(n) = next {
                            if let Some(mut n_td) = text_of(n) {
                                n_td.thread_prev = None;
                                n_td.content = std::mem::take(&mut standalone.content);
                                edits.push(Edit::SetTextData { id: n, data: n_td });
                            }
                        }
                    } else if let Some(p) = prev {
                        if let Some(mut p_td) = text_of(p) {
                            p_td.thread_next = next;
                            edits.push(Edit::SetTextData { id: p, data: p_td });
                        }
                        if let Some(n) = next {
                            if let Some(mut n_td) = text_of(n) {
                                n_td.thread_prev = prev;
                                edits.push(Edit::SetTextData { id: n, data: n_td });
                            }
                        }
                    }
                    edits.push(Edit::SetTextData {
                        id: object,
                        data: standalone,
                    });
                    edits
                }
            }
            Command::MoveObject { object, delta } => {
                let current = self
                    .document
                    .object(object)
                    .ok_or(CommandError::ObjectNotFound(object))?;
                let transform = Affine::translate(delta) * current.transform;
                vec![Edit::SetTransform {
                    id: object,
                    transform,
                }]
            }
            Command::MoveObjects { objects, delta } => objects
                .into_iter()
                .map(|object| {
                    let current = self
                        .document
                        .object(object)
                        .ok_or(CommandError::ObjectNotFound(object))?;
                    Ok(Edit::SetTransform {
                        id: object,
                        transform: Affine::translate(delta) * current.transform,
                    })
                })
                .collect::<Result<Vec<_>, CommandError>>()?,
            Command::MoveAnchors { anchors, delta } => {
                let mut anchors_by_object: HashMap<ObjectId, HashSet<usize>> = HashMap::new();
                for (id, index) in anchors {
                    anchors_by_object.entry(id).or_default().insert(index);
                }
                let mut edits = Vec::with_capacity(anchors_by_object.len());
                for (id, ordinals) in anchors_by_object {
                    let mut data = self.path_data(id)?;
                    data.edit_subpaths(|sp| {
                        for n in ordinals {
                            amalith_core::translate_anchor_n(sp, n, delta);
                        }
                    });
                    edits.push(Edit::SetPathData { id, data });
                }
                edits
            }
            Command::MoveHandle {
                object,
                anchor,
                side,
                delta,
                break_mirror,
            } => {
                let mut data = self.path_data(object)?;
                let base = amalith_core::anchor_at(data.subpaths(), anchor)
                    .and_then(|a| match side {
                        amalith_core::HandleSide::In => a.handle_in,
                        amalith_core::HandleSide::Out => a.handle_out,
                    })
                    .unwrap_or_else(|| {
                        amalith_core::anchor_at(data.subpaths(), anchor)
                            .map(|a| a.point)
                            .unwrap_or_default()
                    });
                let target = base + delta;
                data.edit_subpaths(|sp| {
                    if break_mirror {
                        amalith_core::break_handle_mirror(sp, anchor);
                    }
                    amalith_core::set_handle(sp, anchor, side, Some(target));
                });
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::ToggleAnchorSmooth { object, anchor } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| amalith_core::toggle_anchor_smooth(sp, anchor));
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::SetAnchorSmooth {
                object,
                anchor,
                smooth,
            } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| amalith_core::set_anchor_smooth(sp, anchor, smooth));
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::InsertAnchor {
                object,
                segment,
                t,
            } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| {
                    amalith_core::insert_anchor(sp, segment, t);
                });
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::DeleteAnchor { object, anchor } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| amalith_core::delete_anchor(sp, anchor));
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::JoinAnchors { anchor_a: (oa, na), anchor_b: (ob, nb) } => {
                let data_a = self.path_data(oa)?;
                let data_b = self.path_data(ob)?;
                if !amalith_core::anchor_is_open_endpoint(data_a.subpaths(), na)
                    || !amalith_core::anchor_is_open_endpoint(data_b.subpaths(), nb)
                {
                    return Err(CommandError::JoinNeedsTwoOpenEndpoints);
                }
                self.splice_join(oa, data_a, na, ob, data_b, nb)?
            }
            Command::ExtendOpenPath { object, subpath, at_end, endpoint, new_anchors, close } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| {
                    amalith_core::extend_open_subpath(sp, subpath, at_end, endpoint, new_anchors);
                    if close {
                        if let Some(s) = sp.get_mut(subpath) {
                            s.closed = true;
                        }
                    }
                });
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::TrimAndJoinPaths { a, b } => {
                let mut data_a = self.path_data(a.object)?;
                let local_a = data_a
                    .edit_subpaths_ret(|sp| amalith_core::trim_to_split(sp, a.subpath, a.at_end, a.t))
                    .ok_or(CommandError::JoinNeedsTwoOpenEndpoints)?;
                let mut data_b = self.path_data(b.object)?;
                let local_b = data_b
                    .edit_subpaths_ret(|sp| amalith_core::trim_to_split(sp, b.subpath, b.at_end, b.t))
                    .ok_or(CommandError::JoinNeedsTwoOpenEndpoints)?;

                // Force the two trimmed free ends to be bit-identical — two
                // curves independently evaluated near an approximate
                // crossing won't naturally land within `join_anchors`'s
                // tight coincidence epsilon otherwise. Average the two
                // world-space points and write the shared result back into
                // each side's own local space.
                let world_a = self.document.world_transform(a.object);
                let world_b = self.document.world_transform(b.object);
                let point_a = world_a * data_a.subpaths()[a.subpath].anchors[local_a].point;
                let point_b = world_b * data_b.subpaths()[b.subpath].anchors[local_b].point;
                let shared = point_a.midpoint(point_b);
                let local_point_a = world_a.inverse() * shared;
                let local_point_b = world_b.inverse() * shared;
                data_a.edit_subpaths(|sp| sp[a.subpath].anchors[local_a].point = local_point_a);
                data_b.edit_subpaths(|sp| sp[b.subpath].anchors[local_b].point = local_point_b);

                let flat_a = amalith_core::anchor_count(&data_a.subpaths()[..a.subpath]) + local_a;
                let flat_b = amalith_core::anchor_count(&data_b.subpaths()[..b.subpath]) + local_b;
                self.splice_join(a.object, data_a, flat_a, b.object, data_b, flat_b)?
            }
            Command::SetWidthPoints { object, points } => {
                let mut data = self.path_data(object)?;
                data.width_points = points;
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::WarpPaths { items } => {
                let mut edits = Vec::new();
                for (id, homography) in items {
                    // Objects without path data (images, groups, symbols,
                    // compound paths) sit out this pass rather than
                    // aborting the whole gesture for the objects that do.
                    let Ok(data) = self.path_data(id) else { continue };
                    let data = homography.warp_path(&data).ok_or(CommandError::InvalidWarp)?;
                    edits.push(Edit::SetPathData { id, data });
                }
                edits
            }
            Command::DuplicateObject { object, delta } => {
                let source = self
                    .document
                    .object(object)
                    .ok_or(CommandError::ObjectNotFound(object))?;
                let mut copy = source.clone();
                copy.id = ObjectId::new();
                copy.transform = Affine::translate(delta) * copy.transform;
                let index = self.document.children_of(source.parent).len();
                vec![Edit::InsertObject {
                    object: Box::new(copy),
                    index,
                }]
            }
            Command::SetTransform { object, transform } => vec![Edit::SetTransform {
                id: object,
                transform,
            }],
            Command::SetTransforms { items } => items
                .into_iter()
                .map(|(object, transform)| Edit::SetTransform {
                    id: object,
                    transform,
                })
                .collect(),
            Command::NudgeStack { ids, steps } => {
                let selected: std::collections::HashSet<_> = ids.iter().copied().collect();
                let mut parents: Vec<ObjectParent> = Vec::new();
                for &id in &ids {
                    let object = self
                        .document
                        .object(id)
                        .ok_or(CommandError::ObjectNotFound(id))?;
                    if !parents.contains(&object.parent) {
                        parents.push(object.parent);
                    }
                }
                let mut edits = Vec::new();
                if steps != 0 {
                    for parent in parents {
                        let original = self.document.children_of(parent);
                        let mut order = original.to_vec();
                        for _ in 0..steps.unsigned_abs() {
                            if steps > 0 {
                                for index in (0..order.len().saturating_sub(1)).rev() {
                                    if selected.contains(&order[index])
                                        && !selected.contains(&order[index + 1])
                                    {
                                        order.swap(index, index + 1);
                                    }
                                }
                            } else {
                                for index in 1..order.len() {
                                    if selected.contains(&order[index])
                                        && !selected.contains(&order[index - 1])
                                    {
                                        order.swap(index, index - 1);
                                    }
                                }
                            }
                        }
                        if order != original {
                            edits.push(Edit::SetChildOrder { parent, order });
                        }
                    }
                }
                edits
            }
            Command::Reparent {
                ids,
                parent,
                index,
            } => {
                // Target parent must exist and be able to hold children.
                match parent {
                    ObjectParent::Layer(l) => {
                        self.document
                            .layer(l)
                            .ok_or(CommandError::LayerNotFound(l))?;
                    }
                    ObjectParent::Group(g) => {
                        let o = self
                            .document
                            .object(g)
                            .ok_or(CommandError::ObjectNotFound(g))?;
                        if !matches!(o.kind, ObjectKind::Group(_)) {
                            return Err(CommandError::Document(DocumentError::NotAGroup(g)));
                        }
                    }
                }

                let raw: Vec<ObjectId> = ids;
                let set: HashSet<ObjectId> = raw.iter().copied().collect();
                // Every id must exist; a group can't be moved into itself
                // or into one of its own descendants.
                for &id in &raw {
                    self.document
                        .object(id)
                        .ok_or(CommandError::ObjectNotFound(id))?;
                    let mut p = parent;
                    loop {
                        match p {
                            ObjectParent::Group(g) if g == id => {
                                return Err(CommandError::CannotReparent);
                            }
                            ObjectParent::Group(g) => match self.document.object(g) {
                                Some(o) => p = o.parent,
                                None => break,
                            },
                            ObjectParent::Layer(_) => break,
                        }
                    }
                }
                // Drop ids that ride along inside another moved id — their
                // own `RemoveObject` would hit a parent that's already gone.
                let ids: Vec<ObjectId> = raw
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let mut p = self.document.object(id).map(|o| o.parent);
                        while let Some(ObjectParent::Group(g)) = p {
                            if set.contains(&g) {
                                return false;
                            }
                            p = self.document.object(g).map(|o| o.parent);
                        }
                        true
                    })
                    .collect();
                if ids.is_empty() {
                    return Ok(Vec::new());
                }

                let moved: HashSet<ObjectId> = ids.iter().copied().collect();
                let cur = self.document.children_of(parent).to_vec();
                let removed_before = cur
                    .iter()
                    .take(index.min(cur.len()))
                    .filter(|c| moved.contains(c))
                    .count();
                let in_parent = cur.iter().filter(|c| moved.contains(c)).count();
                let after_len = cur.len() - in_parent;
                let base = index.saturating_sub(removed_before).min(after_len);

                // No-op guard: same parent, same resulting order.
                if ids
                    .iter()
                    .all(|id| self.document.object(*id).map(|o| o.parent) == Some(parent))
                {
                    let mut after: Vec<ObjectId> =
                        cur.iter().copied().filter(|c| !moved.contains(c)).collect();
                    for (n, &id) in ids.iter().rev().enumerate() {
                        after.insert(base + n, id);
                    }
                    if after == cur {
                        return Ok(Vec::new());
                    }
                }

                let new_world_inv = match parent {
                    ObjectParent::Layer(_) => Affine::IDENTITY,
                    ObjectParent::Group(g) => self.document.world_transform(g).inverse(),
                };

                let mut edits = Vec::with_capacity(ids.len() * 2);
                for &id in &ids {
                    edits.push(Edit::RemoveObject { id });
                }
                // Insert front-to-back at the same index: each insert nudges
                // the earlier ones one slot frontward, so `ids[0]` lands
                // frontmost of the moved block.
                for &id in &ids {
                    let obj = self.document.object(id).expect("validated above");
                    let mut clone = obj.clone();
                    // Keep the object visually put: rebase its local
                    // transform onto the new parent's coordinate space.
                    clone.transform = new_world_inv * self.document.world_transform(id);
                    clone.parent = parent;
                    edits.push(Edit::InsertObject {
                        object: Box::new(clone),
                        index: base,
                    });
                }
                edits
            }
            Command::Group { ids, name } => {
                if ids.is_empty() {
                    return Err(CommandError::NothingToGroup);
                }
                let mut parent = None;
                for &id in &ids {
                    let object = self
                        .document
                        .object(id)
                        .ok_or(CommandError::ObjectNotFound(id))?;
                    match parent {
                        None => parent = Some(object.parent),
                        Some(p) if p == object.parent => {}
                        Some(_) => return Err(CommandError::ObjectsSpanMultipleParents),
                    }
                }
                let parent =
                    parent.expect("ids is non-empty, so the loop above always sets parent");

                let selected: std::collections::HashSet<ObjectId> = ids.iter().copied().collect();
                let siblings = self.document.children_of(parent);
                let topmost_index = siblings
                    .iter()
                    .rposition(|id| selected.contains(id))
                    .expect("every id was validated to exist in this parent's children above");
                // Grouping must not change stacking relative to untouched
                // siblings, so the new group takes the position the
                // topmost grouped object occupied — counted in terms of
                // the *remaining* (non-grouped) siblings, since every
                // grouped object at or below that position is about to be
                // removed from this list.
                let group_index = siblings[..=topmost_index]
                    .iter()
                    .filter(|id| !selected.contains(id))
                    .count();
                // The grouped objects' own relative order (bottom to top)
                // becomes the new group's child order.
                let group_children: Vec<ObjectId> = siblings
                    .iter()
                    .copied()
                    .filter(|id| selected.contains(id))
                    .collect();

                let group_id = ObjectId::new();
                let mut group =
                    Object::new(group_id, parent, ObjectKind::Group(Default::default()));
                group.name = name;
                let mut edits = vec![Edit::InsertObject {
                    object: Box::new(group),
                    index: group_index,
                }];
                for (index, &child_id) in group_children.iter().enumerate() {
                    // Reparent in place: remove from the old parent, then
                    // reinsert the *same* object (same id, transform, and
                    // content — only `.parent` changes) as a child of the
                    // new group. No raw `Document` mutator does "reparent"
                    // directly; this composes the two primitives that do
                    // exist, exactly like every other multi-step command
                    // here.
                    let mut child = self
                        .document
                        .object(child_id)
                        .expect("child_id came from this parent's own children list")
                        .clone();
                    child.parent = ObjectParent::Group(group_id);
                    edits.push(Edit::RemoveObject { id: child_id });
                    edits.push(Edit::InsertObject {
                        object: Box::new(child),
                        index,
                    });
                }
                edits
            }
            Command::Ungroup { .. } => {
                unreachable!("Editor::execute intercepts Command::Ungroup before calling compile")
            }
            Command::ClipMake { objects, name } => {
                if objects.len() < 2 {
                    return Err(CommandError::NothingToGroup);
                }
                let mut parent = None;
                for &id in &objects {
                    let object = self
                        .document
                        .object(id)
                        .ok_or(CommandError::ObjectNotFound(id))?;
                    match parent {
                        None => parent = Some(object.parent),
                        Some(p) if p == object.parent => {}
                        Some(_) => return Err(CommandError::ObjectsSpanMultipleParents),
                    }
                }
                let parent = parent.expect("objects is non-empty");
                let selected: std::collections::HashSet<ObjectId> =
                    objects.iter().copied().collect();
                let siblings = self.document.children_of(parent);
                let topmost_index = siblings
                    .iter()
                    .rposition(|id| selected.contains(id))
                    .expect("validated above");
                let group_index = siblings[..=topmost_index]
                    .iter()
                    .filter(|id| !selected.contains(id))
                    .count();
                let group_children: Vec<ObjectId> = siblings
                    .iter()
                    .copied()
                    .filter(|id| selected.contains(id))
                    .collect();
                // Topmost (last in stacking order) member is the clip
                // path — it must be a plain shape.
                let clip_id = group_children.last().copied();
                if let Some(cid) = clip_id {
                    match self.document.object(cid).map(|o| &o.kind) {
                        Some(ObjectKind::Path(_) | ObjectKind::CompoundPath(_)) => {}
                        _ => return Err(CommandError::NotAPath(cid)),
                    }
                }

                let group_id = ObjectId::new();
                let mut group = Object::new(
                    group_id,
                    parent,
                    ObjectKind::Group(amalith_core::GroupData {
                        children: Vec::new(),
                        clip: clip_id,
                        blend: None,
                    }),
                );
                group.name = name;
                let mut edits = vec![Edit::InsertObject {
                    object: Box::new(group),
                    index: group_index,
                }];
                for (index, &child_id) in group_children.iter().enumerate() {
                    let mut child = self
                        .document
                        .object(child_id)
                        .expect("child_id from this parent's children")
                        .clone();
                    child.parent = ObjectParent::Group(group_id);
                    edits.push(Edit::RemoveObject { id: child_id });
                    edits.push(Edit::InsertObject {
                        object: Box::new(child),
                        index,
                    });
                }
                edits
            }
            Command::ClipRelease { group } => {
                match self.document.object(group).map(|o| &o.kind) {
                    Some(ObjectKind::Group(g)) if g.clip.is_some() => {}
                    Some(ObjectKind::Group(_)) => return Err(CommandError::NothingToUngroup),
                    _ => return Err(CommandError::ObjectNotFound(group)),
                }
                let mut edits = vec![Edit::SetClip { group, clip: None }];
                let (ungroup_edits, _freed) = self.compile_ungroup(&[group])?;
                edits.extend(ungroup_edits);
                edits
            }
            Command::MakeBlend { start, end, name } => {
                let start_obj = self
                    .document
                    .object(start)
                    .ok_or(CommandError::ObjectNotFound(start))?;
                let end_obj = self
                    .document
                    .object(end)
                    .ok_or(CommandError::ObjectNotFound(end))?;
                if !matches!(start_obj.kind, ObjectKind::Path(_)) {
                    return Err(CommandError::NotAPath(start));
                }
                if !matches!(end_obj.kind, ObjectKind::Path(_)) {
                    return Err(CommandError::NotAPath(end));
                }
                if start_obj.parent != end_obj.parent {
                    return Err(CommandError::ObjectsSpanMultipleParents);
                }
                let parent = start_obj.parent;
                let selected: std::collections::HashSet<ObjectId> = [start, end].into_iter().collect();
                let siblings = self.document.children_of(parent);
                let topmost_index = siblings
                    .iter()
                    .rposition(|id| selected.contains(id))
                    .expect("start and end were validated to exist in this parent's children above");
                let group_index = siblings[..=topmost_index]
                    .iter()
                    .filter(|id| !selected.contains(id))
                    .count();
                let group_children: Vec<ObjectId> = siblings
                    .iter()
                    .copied()
                    .filter(|id| selected.contains(id))
                    .collect();

                let group_id = ObjectId::new();
                let mut group = Object::new(
                    group_id,
                    parent,
                    ObjectKind::Group(amalith_core::GroupData {
                        children: Vec::new(),
                        clip: None,
                        blend: Some(BlendData {
                            start,
                            end,
                            spine: None,
                            spacing: BlendSpacing::SmoothColor,
                            spine_reversed: false,
                            stack_reversed: false,
                        }),
                    }),
                );
                group.name = name;
                let mut edits = vec![Edit::InsertObject {
                    object: Box::new(group),
                    index: group_index,
                }];
                for (index, &child_id) in group_children.iter().enumerate() {
                    let mut child = self
                        .document
                        .object(child_id)
                        .expect("child_id from this parent's children")
                        .clone();
                    child.parent = ObjectParent::Group(group_id);
                    edits.push(Edit::RemoveObject { id: child_id });
                    edits.push(Edit::InsertObject {
                        object: Box::new(child),
                        index,
                    });
                }
                edits.extend(self.compile_blend_steps(
                    group_id,
                    &group_children,
                    start,
                    end,
                    None,
                    BlendSpacing::SmoothColor,
                    false,
                    false,
                )?);
                edits
            }
            Command::SetBlendOptions { group, spacing, spine } => {
                let obj = self
                    .document
                    .object(group)
                    .ok_or(CommandError::ObjectNotFound(group))?;
                let ObjectKind::Group(g) = &obj.kind else {
                    return Err(CommandError::NotABlend(group));
                };
                let old_blend = g.blend.ok_or(CommandError::NotABlend(group))?;
                let existing_children = g.children.clone();
                let new_blend = BlendData {
                    start: old_blend.start,
                    end: old_blend.end,
                    spine,
                    spacing,
                    spine_reversed: old_blend.spine_reversed,
                    stack_reversed: old_blend.stack_reversed,
                };
                let mut edits = vec![Edit::SetBlendData { group, blend: Some(new_blend) }];
                edits.extend(self.compile_blend_steps(
                    group,
                    &existing_children,
                    old_blend.start,
                    old_blend.end,
                    spine,
                    spacing,
                    new_blend.spine_reversed,
                    new_blend.stack_reversed,
                )?);
                edits
            }
            Command::ReleaseBlend { group } => {
                let obj = self
                    .document
                    .object(group)
                    .ok_or(CommandError::ObjectNotFound(group))?;
                let ObjectKind::Group(g) = &obj.kind else {
                    return Err(CommandError::NotABlend(group));
                };
                let blend = g.blend.ok_or(CommandError::NotABlend(group))?;
                let existing_children = g.children.clone();
                let parent = obj.parent;
                let group_xf = obj.transform;
                let siblings = self.document.children_of(parent);
                let group_index = siblings
                    .iter()
                    .position(|&id| id == group)
                    .expect("group was validated to exist in its own parent's children above");
                let mut edits = Vec::new();
                for &id in &existing_children {
                    if id != blend.start && id != blend.end {
                        edits.push(Edit::RemoveObject { id });
                    }
                }
                for (offset, &child_id) in [blend.start, blend.end].iter().enumerate() {
                    let mut child = self
                        .document
                        .object(child_id)
                        .expect("a blend's start/end are always real objects")
                        .clone();
                    child.parent = parent;
                    child.transform = group_xf * child.transform;
                    edits.push(Edit::RemoveObject { id: child_id });
                    edits.push(Edit::InsertObject {
                        object: Box::new(child),
                        index: group_index + offset,
                    });
                }
                edits.push(Edit::RemoveObject { id: group });
                edits
            }
            Command::ExpandBlend { group } => {
                let obj = self
                    .document
                    .object(group)
                    .ok_or(CommandError::ObjectNotFound(group))?;
                let ObjectKind::Group(g) = &obj.kind else {
                    return Err(CommandError::NotABlend(group));
                };
                if g.blend.is_none() {
                    return Err(CommandError::NotABlend(group));
                }
                vec![Edit::SetBlendData { group, blend: None }]
            }
            Command::ReverseBlendSpine { group } => {
                let obj = self
                    .document
                    .object(group)
                    .ok_or(CommandError::ObjectNotFound(group))?;
                let ObjectKind::Group(g) = &obj.kind else {
                    return Err(CommandError::NotABlend(group));
                };
                let old_blend = g.blend.ok_or(CommandError::NotABlend(group))?;
                let existing_children = g.children.clone();
                let new_blend = BlendData { spine_reversed: !old_blend.spine_reversed, ..old_blend };
                let mut edits = vec![Edit::SetBlendData { group, blend: Some(new_blend) }];
                edits.extend(self.compile_blend_steps(
                    group,
                    &existing_children,
                    new_blend.start,
                    new_blend.end,
                    new_blend.spine,
                    new_blend.spacing,
                    new_blend.spine_reversed,
                    new_blend.stack_reversed,
                )?);
                edits
            }
            Command::ReverseBlendStacking { group } => {
                let obj = self
                    .document
                    .object(group)
                    .ok_or(CommandError::ObjectNotFound(group))?;
                let ObjectKind::Group(g) = &obj.kind else {
                    return Err(CommandError::NotABlend(group));
                };
                let old_blend = g.blend.ok_or(CommandError::NotABlend(group))?;
                let existing_children = g.children.clone();
                let new_blend = BlendData { stack_reversed: !old_blend.stack_reversed, ..old_blend };
                let mut edits = vec![Edit::SetBlendData { group, blend: Some(new_blend) }];
                edits.extend(self.compile_blend_steps(
                    group,
                    &existing_children,
                    new_blend.start,
                    new_blend.end,
                    new_blend.spine,
                    new_blend.spacing,
                    new_blend.spine_reversed,
                    new_blend.stack_reversed,
                )?);
                edits
            }
            Command::SetAppearanceItems { object, items } => {
                self.document
                    .object(object)
                    .ok_or(CommandError::ObjectNotFound(object))?;
                vec![Edit::SetAppearanceItems { id: object, items }]
            }
            Command::SetFill { objects, paint } => objects
                .into_iter()
                .map(|id| Edit::SetFill { id, paint })
                .collect(),
            Command::SetStroke { objects, paint } => objects
                .into_iter()
                .map(|id| Edit::SetStroke { id, paint })
                .collect(),
            Command::SetPaints {
                objects,
                fill,
                stroke,
            } => objects
                .into_iter()
                .flat_map(|id| {
                    fill.map(|paint| Edit::SetFill { id, paint })
                        .into_iter()
                        .chain(stroke.map(|paint| Edit::SetStroke { id, paint }))
                })
                .collect(),
            Command::SetStrokeWidth { objects, width } => {
                let mut edits = Vec::new();
                for id in objects {
                    if let Some(obj) = self.document.object(id) {
                        if let Some(path) = obj.kind.path_data() {
                            if !path.width_points.is_empty() && obj.appearance.stroke_width() > 0.0 {
                                let mut data = path.clone();
                                let ratio = width / obj.appearance.stroke_width();
                                for point in &mut data.width_points {
                                    point.left *= ratio;
                                    point.right *= ratio;
                                }
                                edits.push(Edit::SetPathData { id, data });
                            }
                        }
                    }
                    edits.push(Edit::SetStrokeWidth { id, width });
                }
                edits
            },
            Command::SetStrokeStyle { objects, style } => objects
                .into_iter()
                .map(|id| Edit::SetStrokeStyle { id, style })
                .collect(),
            Command::SetOpacity { objects, opacity } => objects
                .into_iter()
                .map(|id| Edit::SetOpacity { id, opacity })
                .collect(),
            Command::SetVisible { objects, visible } => objects
                .into_iter()
                .map(|id| Edit::SetVisible { id, visible })
                .collect(),
            Command::SetLocked { objects, locked } => objects
                .into_iter()
                .map(|id| Edit::SetLocked { id, locked })
                .collect(),
            Command::SetAssetSource { id, source } => vec![Edit::SetAssetSource { id, source }],
            Command::Pathfinder { op, objects } => self.compile_pathfinder(op, objects)?,
            Command::ShapeBuilder {
                objects,
                touched,
                erase,
                appearance,
            } => self.compile_shape_builder(objects, touched, erase, appearance)?,
            Command::EraseArea { objects, area } => self.compile_erase_area(objects, area)?,
            Command::ExpandStroke { objects } => self.compile_expand_stroke(objects)?,
            Command::OffsetPath { .. } => {
                unreachable!("Editor::execute intercepts Command::OffsetPath before calling compile")
            }
            Command::Align {
                objects,
                kind,
                to,
                key,
                artboard,
                spacing,
            } => self.compile_align(objects, kind, to, key, artboard, spacing)?,
            Command::Paste { .. } => {
                unreachable!("Editor::execute intercepts Command::Paste before calling compile")
            }
            Command::DuplicateObjects { .. } => unreachable!(
                "Editor::execute intercepts Command::DuplicateObjects before calling compile"
            ),
        };
        Ok(edits)
    }

    fn compile_align(
        &self,
        objects: Vec<ObjectId>,
        kind: AlignKind,
        to: AlignTo,
        key: Option<ObjectId>,
        artboard: Option<ArtboardId>,
        spacing: Option<f64>,
    ) -> Result<Vec<Edit>, CommandError> {
        if objects.is_empty() {
            return Err(CommandError::NothingToAlign);
        }
        let mut bounds = Vec::new();
        for &id in &objects {
            let b = self
                .document
                .bounds_of(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            bounds.push((id, b));
        }
        let frame = match to {
            AlignTo::Artboard => {
                let id = artboard.ok_or(CommandError::NothingToAlign)?;
                self.document
                    .artboard(id)
                    .ok_or(CommandError::ArtboardNotFound(id))?
                    .rect
            }
            _ => Rect::new(0.0, 0.0, 0.0, 0.0),
        };
        // Illustrator: Align To Key Object with no click uses the frontmost
        // selected object as the key (it stays put).
        let key = if to == AlignTo::KeyObject {
            match key {
                Some(k) if objects.iter().any(|&id| id == k) => Some(k),
                _ => self.frontmost_of(&objects),
            }
        } else {
            None
        };
        let moves = align::deltas(&bounds, kind, to, key, frame, spacing);
        let mut edits = Vec::new();
        for (id, world_delta) in moves {
            let obj = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            // One parent-chain walk: world = parent * local. Layer children
            // skip the inverse entirely.
            let new_local = match obj.parent {
                ObjectParent::Layer(_) => Affine::translate(world_delta) * obj.transform,
                ObjectParent::Group(g) => {
                    let p = self.document.world_transform(g);
                    p.inverse() * Affine::translate(world_delta) * p * obj.transform
                }
            };
            if new_local
                .as_coeffs()
                .iter()
                .zip(obj.transform.as_coeffs())
                .all(|(a, b)| (*a - b).abs() < 1e-9)
            {
                continue;
            }
            edits.push(Edit::SetTransform {
                id,
                transform: new_local,
            });
        }
        Ok(edits)
    }

    /// Last in layer-child order among `ids` — the frontmost selected object.
    fn frontmost_of(&self, ids: &[ObjectId]) -> Option<ObjectId> {
        let want: HashSet<ObjectId> = ids.iter().copied().collect();
        let mut last = None;
        for layer in self.document.layers() {
            for &id in &layer.children {
                if want.contains(&id) {
                    last = Some(id);
                }
            }
        }
        last.or_else(|| ids.last().copied())
    }

    fn path_objects_in_paint_order(&self, selected: &[ObjectId]) -> Vec<ObjectId> {
        let want: HashSet<ObjectId> = selected.iter().copied().collect();
        let mut out = Vec::new();
        fn walk(
            doc: &Document,
            ids: &[ObjectId],
            want: &HashSet<ObjectId>,
            out: &mut Vec<ObjectId>,
        ) {
            for &id in ids {
                let Some(obj) = doc.object(id) else { continue };
                match &obj.kind {
                    ObjectKind::Group(g) => walk(doc, &g.children, want, out),
                    ObjectKind::Path(_) | ObjectKind::CompoundPath(_) if want.contains(&id) => {
                        out.push(id);
                    }
                    _ => {}
                }
            }
        }
        for layer in self.document.layers() {
            walk(&self.document, &layer.children, &want, &mut out);
        }
        out
    }

    fn world_path(&self, id: ObjectId) -> Option<BezPath> {
        let obj = self.document.object(id)?;
        let local = match &obj.kind {
            ObjectKind::Path(p) => p.geometry.clone(),
            ObjectKind::CompoundPath(c) => {
                let mut p = BezPath::new();
                for s in &c.subpaths {
                    p.extend(s.iter());
                }
                p
            }
            _ => return None,
        };
        Some(self.document.world_transform(id) * local)
    }

    /// Splices two already-resolved `PathData` clones together at `na`/`nb`
    /// (flat anchor ordinals into each's own `data_a`/`data_b`) and joins
    /// them — the shared tail of both `Command::JoinAnchors` (whose clones
    /// are the objects' current path data, untouched) and
    /// `Command::TrimAndJoinPaths` (whose clones have already been
    /// trimmed to a shared coincident point). `oa` survives, keeping its
    /// id/appearance/z-order; when `ob != oa`, `ob`'s clone is transformed
    /// into `oa`'s local space and appended before joining, then `ob`
    /// itself is removed.
    fn splice_join(
        &self,
        oa: ObjectId,
        mut data_a: PathData,
        na: usize,
        ob: ObjectId,
        data_b: PathData,
        nb: usize,
    ) -> Result<Vec<Edit>, CommandError> {
        if oa == ob {
            data_a.edit_subpaths(|sp| amalith_core::join_anchors(sp, na, nb));
            return Ok(vec![Edit::SetPathData { id: oa, data: data_a }]);
        }
        let obj_a = self.document.object(oa).ok_or(CommandError::ObjectNotFound(oa))?;
        let obj_b = self.document.object(ob).ok_or(CommandError::ObjectNotFound(ob))?;
        if obj_a.parent != obj_b.parent {
            return Err(CommandError::ObjectsSpanMultipleParents);
        }
        // `ob`'s geometry expressed in `oa`'s local space — the shared
        // parent-space factor in each `world_transform` cancels, so this
        // is correct regardless of whether the parent is a Group or a
        // Layer, unlike `compile_pathfinder`'s own Group/Layer special-
        // casing (not needed here since we convert object-to-object
        // directly rather than through a computed result's own space).
        let rel = self.document.world_transform(oa).inverse() * self.document.world_transform(ob);
        let b_in_a_space = PathData::from_bezpath(rel * data_b.geometry.clone());
        let offset = amalith_core::anchor_count(data_a.subpaths());
        data_a.edit_subpaths(|sp| {
            sp.extend(b_in_a_space.subpaths().iter().cloned());
            amalith_core::join_anchors(sp, na, offset + nb);
        });
        Ok(vec![
            Edit::SetPathData { id: oa, data: data_a },
            Edit::RemoveObject { id: ob },
        ])
    }

    fn compile_pathfinder(
        &self,
        op: PathfinderOp,
        objects: Vec<ObjectId>,
    ) -> Result<Vec<Edit>, CommandError> {
        let ordered = self.path_objects_in_paint_order(&objects);
        if ordered.len() < 2 {
            return Err(CommandError::PathfinderNeedTwo);
        }
        let mut parent = None;
        let mut inputs = Vec::new();
        for &id in &ordered {
            let obj = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            match parent {
                None => parent = Some(obj.parent),
                Some(p) if p == obj.parent => {}
                Some(_) => return Err(CommandError::ObjectsSpanMultipleParents),
            }
            let path = self.world_path(id).ok_or(CommandError::NotAPath(id))?;
            inputs.push(PathInput {
                contours: pathfinder::flatten_path(&path),
                appearance: obj.appearance.clone(),
            });
        }
        let parent = parent.unwrap();
        let parent_world = match parent {
            ObjectParent::Group(g) => self.document.world_transform(g),
            ObjectParent::Layer(_) => Affine::IDENTITY,
        };
        let results = pathfinder::apply(op, &inputs);
        if results.is_empty() {
            return Err(CommandError::PathfinderEmpty);
        }

        let selected: HashSet<ObjectId> = ordered.iter().copied().collect();
        let siblings = self.document.children_of(parent);
        let topmost = siblings
            .iter()
            .rposition(|id| selected.contains(id))
            .unwrap();
        let insert_at = siblings[..=topmost]
            .iter()
            .filter(|id| !selected.contains(id))
            .count();

        let mut edits: Vec<Edit> = ordered
            .iter()
            .rev()
            .map(|&id| Edit::RemoveObject { id })
            .collect();

        for (i, result) in results.into_iter().enumerate() {
            let geom = parent_world.inverse() * result.path.geometry.clone();
            let path = PathData::from_bezpath(geom);
            let mut object = Object::new(ObjectId::new(), parent, ObjectKind::Path(path));
            object.appearance = result.appearance;
            object.transform = Affine::IDENTITY;
            edits.push(Edit::InsertObject {
                object: Box::new(object),
                index: insert_at + i,
            });
        }
        Ok(edits)
    }

    /// See [`Command::ShapeBuilder`]. Only the objects that actually
    /// overlap `touched` are ever removed/replaced — the rest of
    /// `objects` (and every other sibling) keeps its id, name and
    /// position untouched, unlike a plain Pathfinder op which always
    /// consumes its whole input set.
    fn compile_shape_builder(
        &self,
        objects: Vec<ObjectId>,
        touched: PathData,
        erase: bool,
        appearance: Option<Appearance>,
    ) -> Result<Vec<Edit>, CommandError> {
        let ordered = self.path_objects_in_paint_order(&objects);
        if ordered.is_empty() {
            return Err(CommandError::PathfinderNeedTwo);
        }
        let mut parent = None;
        let mut inputs = Vec::new();
        let mut sources = Vec::new();
        for &id in &ordered {
            let obj = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            match parent {
                None => parent = Some(obj.parent),
                Some(p) if p == obj.parent => {}
                Some(_) => return Err(CommandError::ObjectsSpanMultipleParents),
            }
            let path = self.world_path(id).ok_or(CommandError::NotAPath(id))?;
            sources.push(path.clone());
            inputs.push((
                id,
                PathInput {
                    contours: pathfinder::flatten_path(&path),
                    appearance: obj.appearance.clone(),
                },
            ));
        }
        let parent = parent.unwrap();
        let parent_world = match parent {
            ObjectParent::Group(g) => self.document.world_transform(g),
            ObjectParent::Layer(_) => Affine::IDENTITY,
        };
        let touched_contours = pathfinder::flatten_path(&touched.geometry);

        let (ids,inputs): (Vec<_>,Vec<_>)=inputs.into_iter().unzip();
        let (consumed,results)=pathfinder::shape_builder_results(&inputs,&touched_contours,&sources,if erase { None } else { appearance.or_else(||inputs.last().map(|i|i.appearance.clone())) });
        let touched_ids: Vec<_>=consumed.into_iter().map(|i|ids[i]).collect();
        if touched_ids.is_empty() { return Err(CommandError::PathfinderEmpty); }

        let selected: HashSet<ObjectId> = touched_ids.iter().copied().collect();
        let siblings = self.document.children_of(parent);
        let topmost = siblings
            .iter()
            .rposition(|id| selected.contains(id))
            .unwrap();
        let insert_at = siblings[..=topmost]
            .iter()
            .filter(|id| !selected.contains(id))
            .count();

        let mut edits: Vec<Edit> = touched_ids
            .iter()
            .rev()
            .map(|&id| Edit::RemoveObject { id })
            .collect();
        for (i, result) in results.into_iter().enumerate() {
            let geom = parent_world.inverse() * result.path.geometry.clone();
            let path = PathData::from_bezpath(geom);
            let mut object = Object::new(ObjectId::new(), parent, ObjectKind::Path(path));
            object.appearance = result.appearance;
            object.transform = Affine::IDENTITY;
            edits.push(Edit::InsertObject {
                object: Box::new(object),
                index: insert_at + i,
            });
        }
        Ok(edits)
    }

    /// See [`Command::EraseArea`]. Each object is handled entirely on
    /// its own — no shared parent, no minimum count, no merge case —
    /// since erasing never needs to combine anything, just remove part
    /// of what's already there. An object the stroke never actually
    /// overlaps is skipped outright: no edit references it at all.
    fn compile_erase_area(&self, objects: Vec<ObjectId>, area: PathData) -> Result<Vec<Edit>, CommandError> {
        let mut edits = Vec::new();
        let wanted: HashSet<_> = objects.into_iter().collect();
        // Work from front to back so insertions do not shift later targets.
        let mut ordered: Vec<_> = wanted.iter().copied().collect();
        ordered.sort_by_key(|id| std::cmp::Reverse(self.document.object(*id)
            .and_then(|o| self.document.children_of(o.parent).iter().position(|s| s == id)).unwrap_or(0)));
        for id in ordered.into_iter().filter(|id| wanted.contains(id)) {
            let Some(obj) = self.document.object(id) else { continue };
            let Some(world) = self.world_path(id) else { continue };
            let Some(remaining) = crate::eraser::erase(&world, &area.geometry) else { continue };
            let index = self.document.children_of(obj.parent).iter().position(|&s| s == id).unwrap();
            let inverse = self.document.world_transform(id).inverse();
            edits.push(Edit::RemoveObject { id });
            for (offset, geometry) in remaining.into_iter().enumerate() {
                let mut object = obj.clone();
                object.id = ObjectId::new();
                object.kind = ObjectKind::Path(PathData::from_bezpath(inverse * geometry));
                edits.push(Edit::InsertObject { object: Box::new(object), index: index + offset });
            }
        }
        Ok(edits)
    }

    fn compile_expand_stroke(&self, objects: Vec<ObjectId>) -> Result<Vec<Edit>, CommandError> {
        let mut edits = Vec::new();
        let mut any = false;
        for id in objects {
            let obj = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?
                .clone();
            if !pathfinder::has_visible_stroke(&obj.appearance) {
                continue;
            }
            let world = self.world_path(id).ok_or(CommandError::NotAPath(id))?;
            let Some(outlined) = pathfinder::expand_stroke(&world, &obj.appearance) else {
                continue;
            };
            any = true;
            let parent_world = match obj.parent {
                ObjectParent::Group(g) => self.document.world_transform(g),
                ObjectParent::Layer(_) => Affine::IDENTITY,
            };
            let local = PathData::from_bezpath(parent_world.inverse() * outlined.geometry.clone());
            let stroke_paint = obj.appearance.stroke();
            let mut stroke_app = obj.appearance.clone();
            stroke_app.set_fill(stroke_paint);
            stroke_app.set_stroke(Paint::None);

            if obj.appearance.fill() == Paint::None {
                edits.push(Edit::SetPathData {
                    id,
                    data: local,
                });
                edits.push(Edit::SetFill {
                    id,
                    paint: stroke_paint,
                });
                edits.push(Edit::SetStroke {
                    id,
                    paint: Paint::None,
                });
            } else {
                let siblings = self.document.children_of(obj.parent);
                let index = siblings.iter().position(|&x| x == id).unwrap_or(siblings.len()) + 1;
                let new_id = ObjectId::new();
                let mut stroke_obj = Object::new(new_id, obj.parent, ObjectKind::Path(local));
                stroke_obj.appearance = stroke_app;
                stroke_obj.name = obj.name.clone();
                edits.push(Edit::SetStroke {
                    id,
                    paint: Paint::None,
                });
                edits.push(Edit::InsertObject {
                    object: Box::new(stroke_obj),
                    index,
                });
            }
        }
        if !any {
            return Err(CommandError::NoStrokeToExpand);
        }
        Ok(edits)
    }

    /// Object ▸ Path ▸ Offset Path: for each path, inserts a *new* sibling
    /// object — positioned directly *behind* it (paints first, i.e. one
    /// index lower — see `Layer::children`'s stacking-order convention),
    /// with its own appearance — holding the boundary grown/shrunk by
    /// `offset` (see `pathfinder::offset_path`). The source path is left
    /// completely untouched, matching Illustrator's own Offset Path (it
    /// isn't a live effect that replaces the original). Non-path objects
    /// in `objects` (and any individual offset that collapses to
    /// nothing) are skipped rather than aborting the whole command; only
    /// an entirely empty result errors. Returns the new id per offset
    /// path created, in the same relative order as `objects` — the
    /// multi-id complement `CommandOutcome`'s single `Object(id)` can't
    /// carry (e.g. for the GUI to select every result), same reason as
    /// `Editor::duplicate_objects`.
    fn compile_offset_path(
        &self,
        objects: Vec<ObjectId>,
        offset: f64,
        join: amalith_core::LineJoin,
        miter_limit: f64,
    ) -> Result<(Vec<Edit>, Vec<ObjectId>), CommandError> {
        let mut edits = Vec::new();
        let mut new_ids = Vec::new();
        for id in objects {
            let Some(obj) = self.document.object(id) else { continue };
            let Some(data) = obj.kind.path_data() else { continue };
            let world = self.document.world_transform(id) * data.geometry.clone();
            let Some(offset_pd) = pathfinder::offset_path(&world, offset, join, miter_limit) else {
                continue;
            };
            let parent_world = match obj.parent {
                ObjectParent::Group(g) => self.document.world_transform(g),
                ObjectParent::Layer(_) => Affine::IDENTITY,
            };
            let local = PathData::from_bezpath(parent_world.inverse() * offset_pd.geometry);
            let siblings = self.document.children_of(obj.parent);
            let index = siblings.iter().position(|&x| x == id).unwrap_or(0);
            let new_id = ObjectId::new();
            let mut new_obj = Object::new(new_id, obj.parent, ObjectKind::Path(local));
            new_obj.appearance = obj.appearance.clone();
            new_ids.push(new_id);
            edits.push(Edit::InsertObject {
                object: Box::new(new_obj),
                index,
            });
        }
        if edits.is_empty() {
            return Err(CommandError::PathfinderEmpty);
        }
        Ok((edits, new_ids))
    }

    /// Object ▸ Path ▸ Offset Path: creates a new, independent sibling for
    /// each path in `objects`, holding its boundary grown/shrunk by
    /// `offset`, and returns every new id in the same relative order as
    /// `objects` — the multi-id complement [`Editor::execute`]'s single-id
    /// [`CommandOutcome`] can't carry (e.g. for the GUI to select every
    /// result after OK, matching Illustrator). See `compile_offset_path`
    /// for the exact geometry/placement rules; the source objects are
    /// never touched. `Editor::execute(Command::OffsetPath { .. })` is
    /// equivalent but only surfaces the first result's id.
    pub fn offset_path(
        &mut self,
        objects: &[ObjectId],
        offset: f64,
        join: amalith_core::LineJoin,
        miter_limit: f64,
    ) -> Result<Vec<ObjectId>, CommandError> {
        let (edits, new_ids) = self.compile_offset_path(objects.to_vec(), offset, join, miter_limit)?;
        let mut inverses = Vec::with_capacity(edits.len());
        for edit in edits {
            let (inverse, _created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
        }
        inverses.reverse();
        self.history.record(inverses);
        self.bounds_cache.clear();
        Ok(new_ids)
    }

    /// Builds the paste edits (root-first, then each group's descendants in
    /// original order, so every `InsertObject` lands after its parent
    /// already exists) plus the new id of every pasted root, in clipboard
    /// order. Read-only over `self.document`, like `compile`.
    fn compile_paste(
        &self,
        delta: Vec2,
        stack: PasteStack,
    ) -> Result<(Vec<Edit>, Vec<ObjectId>), CommandError> {
        let clipboard = self
            .clipboard
            .as_ref()
            .filter(|c| !c.roots.is_empty())
            .ok_or(CommandError::EmptyClipboard)?;

        // Fresh id for every copied object (roots and group descendants),
        // minted once per paste so repeated pastes never collide.
        let id_map: HashMap<ObjectId, ObjectId> = clipboard
            .objects
            .keys()
            .map(|&old_id| (old_id, ObjectId::new()))
            .collect();

        let top_layer = self
            .document
            .layers()
            .last()
            .map(|l| ObjectParent::Layer(l.id));
        let root_delta = Affine::translate(delta);

        // A lazily-populated, purely local view of each touched parent's
        // child list, seeded from the real document and then updated in
        // step with the `Edit`s we queue — so index math for a later root
        // (or the InFront/Behind fallback) accounts for earlier roots this
        // same paste already queued into the same parent, without ever
        // mutating `self.document` (this method stays read-only).
        let mut shadow: HashMap<ObjectParent, Vec<ObjectId>> = HashMap::new();

        let mut edits = Vec::new();
        let mut root_ids = Vec::with_capacity(clipboard.roots.len());
        for root in &clipboard.roots {
            let (target_parent, target_index) = match stack {
                PasteStack::Top => {
                    let parent = resolve_parent(&self.document, root.source_parent, top_layer)?;
                    let index = shadow_children(&self.document, &mut shadow, parent).len();
                    (parent, index)
                }
                PasteStack::InFront => {
                    if let Some(source) = self.document.object(root.source_id) {
                        let parent = source.parent;
                        let list = shadow_children(&self.document, &mut shadow, parent);
                        let position = list
                            .iter()
                            .position(|&id| id == root.source_id)
                            .map_or(list.len(), |i| i + 1);
                        (parent, position)
                    } else {
                        let parent = resolve_parent(&self.document, root.source_parent, top_layer)?;
                        let index = shadow_children(&self.document, &mut shadow, parent).len();
                        (parent, index)
                    }
                }
                PasteStack::Behind => {
                    if let Some(source) = self.document.object(root.source_id) {
                        let parent = source.parent;
                        let list = shadow_children(&self.document, &mut shadow, parent);
                        let position = list
                            .iter()
                            .position(|&id| id == root.source_id)
                            .unwrap_or(0);
                        (parent, position)
                    } else {
                        let parent = resolve_parent(&self.document, root.source_parent, top_layer)?;
                        (parent, 0)
                    }
                }
            };

            let new_root_id = id_map[&root.source_id];
            push_deep_copy_edits(
                &clipboard.objects,
                &id_map,
                root.source_id,
                target_parent,
                target_index,
                Some(root_delta),
                &mut edits,
            );
            let list = shadow_children(&self.document, &mut shadow, target_parent);
            let clamped = target_index.min(list.len());
            list.insert(clamped, new_root_id);
            root_ids.push(new_root_id);
        }
        Ok((edits, root_ids))
    }

    /// Builds the duplicate edits (deep-copying any group descendants,
    /// preserving relative order among `ids` that share a parent) plus the
    /// new id of every duplicate, in `ids`' order. Read-only over
    /// `self.document`, like `compile`. Errors on an empty `ids`, same as
    /// `compile_paste` errors on an empty clipboard — a no-op command isn't
    /// meaningful to record as an undo step.
    fn compile_duplicate_objects(
        &self,
        ids: &[ObjectId],
        delta: Vec2,
    ) -> Result<(Vec<Edit>, Vec<ObjectId>), CommandError> {
        if ids.is_empty() {
            return Err(CommandError::NothingToDuplicate);
        }
        let root_delta = Affine::translate(delta);
        // Same purely-local, read-only index bookkeeping as `compile_paste`
        // — needed here too since multiple duplicated ids can share a
        // parent.
        let mut shadow: HashMap<ObjectParent, Vec<ObjectId>> = HashMap::new();

        let mut edits = Vec::new();
        let mut new_ids = Vec::with_capacity(ids.len());
        for &id in ids {
            let source = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            let parent = source.parent;

            let mut subtree = HashMap::new();
            collect_subtree(&self.document, id, &mut subtree);
            let id_map: HashMap<ObjectId, ObjectId> = subtree
                .keys()
                .map(|&old_id| (old_id, ObjectId::new()))
                .collect();
            let new_id = id_map[&id];

            let index = shadow_children(&self.document, &mut shadow, parent).len();
            push_deep_copy_edits(
                &subtree,
                &id_map,
                id,
                parent,
                index,
                Some(root_delta),
                &mut edits,
            );
            shadow_children(&self.document, &mut shadow, parent).push(new_id);
            new_ids.push(new_id);
        }
        Ok((edits, new_ids))
    }

    /// Builds the ungroup edits (dissolving each group, splicing its
    /// children back into its own parent at the position it occupied) plus
    /// every freed child's id, across all groups in `ids`, in paint order.
    /// Read-only over `self.document`, like `compile`.
    fn compile_ungroup(
        &self,
        ids: &[ObjectId],
    ) -> Result<(Vec<Edit>, Vec<ObjectId>), CommandError> {
        if ids.is_empty() {
            return Err(CommandError::NothingToUngroup);
        }
        // Same purely-local, read-only index bookkeeping as
        // `compile_paste`/`compile_duplicate_objects` — needed here too
        // since ungrouping several groups that share a parent must not use
        // indices left stale by an earlier one's splice in this same
        // batch.
        let mut shadow: HashMap<ObjectParent, Vec<ObjectId>> = HashMap::new();
        let mut edits = Vec::new();
        let mut freed_ids = Vec::new();
        for &id in ids {
            let object = self
                .document
                .object(id)
                .ok_or(CommandError::ObjectNotFound(id))?;
            let ObjectKind::Group(group) = &object.kind else {
                return Err(DocumentError::NotAGroup(id).into());
            };
            let parent = object.parent;
            let children = group.children.clone();
            // World of a child is `group.transform * child.transform`.
            // After the group is gone the child sits in the group's parent,
            // so that product has to become the child's own transform or
            // the group's move/scale/rotate would vanish.
            let group_xf = object.transform;

            let list = shadow_children(&self.document, &mut shadow, parent);
            let group_index = list
                .iter()
                .position(|&sibling| sibling == id)
                .expect("id was validated to exist as this parent's child above");
            list.remove(group_index);
            for (offset, &child_id) in children.iter().enumerate() {
                list.insert(group_index + offset, child_id);
            }

            // Children must come out of the group (and land back in
            // `parent`) *before* the now-empty group itself is removed —
            // each `RemoveObject{child_id}` edit needs the group to still
            // exist, since `child_id`'s recorded parent is still the group
            // right up until this point.
            for (offset, &child_id) in children.iter().enumerate() {
                let mut child = self
                    .document
                    .object(child_id)
                    .expect("a group's own children list references a real object")
                    .clone();
                child.parent = parent;
                child.transform = group_xf * child.transform;
                edits.push(Edit::RemoveObject { id: child_id });
                edits.push(Edit::InsertObject {
                    object: Box::new(child),
                    index: group_index + offset,
                });
                freed_ids.push(child_id);
            }
            edits.push(Edit::RemoveObject { id });
        }
        Ok((edits, freed_ids))
    }
}

/// Recursively clones `old_id` (and, for a group, its descendants) out of
/// `source_objects`, assigning each a fresh id from `id_map`, reparenting
/// the root under `new_parent` at `new_index`, and applying `delta` to the
/// root's transform only — descendants keep their copied relative
/// transform, since it composes through the (also freshly-inserted) parent
/// chain. Descendant `InsertObject` edits are appended after their
/// parent's, in the original child order, so every parent already exists
/// in the document by the time its children are applied. Shared by
/// `compile_paste` (`source_objects` is the clipboard's) and
/// `compile_duplicate_objects` (`source_objects` is a one-off subtree
/// collected straight from `self.document`) — the deep-copy logic is
/// identical either way, only where the source objects come from differs.
fn push_deep_copy_edits(
    source_objects: &HashMap<ObjectId, Object>,
    id_map: &HashMap<ObjectId, ObjectId>,
    old_id: ObjectId,
    new_parent: ObjectParent,
    new_index: usize,
    delta: Option<Affine>,
    edits: &mut Vec<Edit>,
) {
    let source = &source_objects[&old_id];
    let mut clone = source.clone();
    clone.id = id_map[&old_id];
    clone.parent = new_parent;
    if let Some(delta) = delta {
        clone.transform = delta * clone.transform;
    }
    let new_id = clone.id;
    let original_children = if let ObjectKind::Group(group) = &mut clone.kind {
        // The clone starts as an empty group; each child below is inserted
        // through the normal `InsertObject` path, which is what actually
        // populates `GroupData::children` (see `Document::insert_object`).
        Some(std::mem::take(&mut group.children))
    } else {
        None
    };
    edits.push(Edit::InsertObject {
        object: Box::new(clone),
        index: new_index,
    });
    if let Some(children) = original_children {
        for (index, child_old_id) in children.into_iter().enumerate() {
            push_deep_copy_edits(
                source_objects,
                id_map,
                child_old_id,
                ObjectParent::Group(new_id),
                index,
                None,
                edits,
            );
        }
    }
}

/// The recorded source parent if there is one and it still exists (a
/// layer, or an object that's still a group), otherwise the top layer.
/// `source_parent` is `None` for a root with no real source (SVG imported
/// from outside the document, see [`Editor::copy_from_svg`]), which always
/// takes the fallback. Errors if there is no top layer to fall back to
/// either.
fn resolve_parent(
    document: &Document,
    source_parent: Option<ObjectParent>,
    top_layer: Option<ObjectParent>,
) -> Result<ObjectParent, CommandError> {
    let exists = source_parent.is_some_and(|parent| match parent {
        ObjectParent::Layer(id) => document.layer(id).is_some(),
        ObjectParent::Group(id) => document.object(id).is_some_and(Object::is_group),
    });
    match (exists, source_parent) {
        (true, Some(parent)) => Ok(parent),
        _ => top_layer.ok_or(CommandError::NoLayerAvailable),
    }
}

/// Composes local transforms from `id` up through parent groups *within
/// `objects`* to that root's own coordinate space — the same algorithm as
/// `Document::world_transform`, but over a flat, standalone object map
/// with no live `Document` behind it (SVG-imported content isn't in one).
/// Stops at any parent not present in `objects`, since that means `id` is
/// itself a root with nothing above it to compose.
fn imported_world_transform(objects: &HashMap<ObjectId, Object>, id: ObjectId) -> Affine {
    let Some(object) = objects.get(&id) else {
        return Affine::IDENTITY;
    };
    let parent_transform = match object.parent {
        ObjectParent::Group(parent_id) if objects.contains_key(&parent_id) => {
            imported_world_transform(objects, parent_id)
        }
        _ => Affine::IDENTITY,
    };
    parent_transform * object.transform
}

/// The `Document::bounds_of` equivalent for a standalone imported object
/// map — see `imported_world_transform`. Used by `Editor::copy_from_svg`
/// so `Editor::clipboard_bounds()` works the same for SVG-imported content
/// as for an ordinary in-document copy.
fn imported_bounds(objects: &HashMap<ObjectId, Object>, id: ObjectId) -> Option<Rect> {
    let object = objects.get(&id)?;
    match &object.kind {
        ObjectKind::Group(group) => group
            .children
            .iter()
            .filter_map(|&child| imported_bounds(objects, child))
            .reduce(|a, b| a.union(b)),
        _ => {
            let local = object.kind.own_local_bounds()?;
            Some(imported_world_transform(objects, id).transform_rect_bbox(local))
        }
    }
}

/// Returns the shadow child list for `parent`, seeding it from the real
/// document on first access. See `compile_paste`.
fn shadow_children<'a>(
    document: &Document,
    shadow: &'a mut HashMap<ObjectParent, Vec<ObjectId>>,
    parent: ObjectParent,
) -> &'a mut Vec<ObjectId> {
    shadow
        .entry(parent)
        .or_insert_with(|| document.children_of(parent).to_vec())
}

/// Deep-collects `id` (and, recursively, every descendant of a group) from
/// `document` into `into`, keyed by original id. Used by `Editor::copy` to
/// build a clipboard snapshot that's fully independent of the live
/// document.
fn collect_subtree(document: &Document, id: ObjectId, into: &mut HashMap<ObjectId, Object>) {
    let Some(object) = document.object(id) else {
        return;
    };
    if let ObjectKind::Group(group) = &object.kind {
        for &child in &group.children {
            collect_subtree(document, child, into);
        }
    }
    into.insert(id, object.clone());
}

fn rects_intersect(a: amalith_core::Rect, b: amalith_core::Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn next_artboard_name(document: &Document) -> String {
    let max_number = document
        .artboards()
        .iter()
        .filter_map(|artboard| artboard.name.strip_prefix("Artboard "))
        .filter_map(|suffix| suffix.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Artboard {}", max_number + 1)
}

/// The object `edit` touches, for the blend live-rebuild hook in
/// `Editor::execute` — not every `Edit` variant names an object, so this
/// is deliberately not exhaustive.
fn touched_object_id(edit: &Edit) -> Option<ObjectId> {
    match edit {
        Edit::InsertObject { object, .. } => Some(object.id),
        Edit::RemoveObject { id }
        | Edit::RenameObject { id, .. }
        | Edit::SetTransform { id, .. }
        | Edit::SetPathData { id, .. }
        | Edit::SetTextData { id, .. }
        | Edit::SetFill { id, .. }
        | Edit::SetStroke { id, .. }
        | Edit::SetStrokeWidth { id, .. }
        | Edit::SetStrokeStyle { id, .. }
        | Edit::SetOpacity { id, .. }
        | Edit::SetVisible { id, .. }
        | Edit::SetLocked { id, .. } => Some(*id),
        _ => None,
    }
}

fn outcome_of(new_id: Option<NewId>) -> CommandOutcome {
    match new_id {
        Some(NewId::Artboard(id)) => CommandOutcome::Artboard(id),
        Some(NewId::Layer(id)) => CommandOutcome::Layer(id),
        Some(NewId::Object(id)) => CommandOutcome::Object(id),
        Some(NewId::Guide(id)) => CommandOutcome::Guide(id),
        Some(NewId::Gradient(id)) => CommandOutcome::Gradient(id),
        None => CommandOutcome::None,
    }
}

/// Applies every edit in `group` (in order), returning the group's exact
/// inverse in the order needed to undo *that* application. Used
/// symmetrically by both `undo` (which then files the result as the redo
/// entry) and `redo` (which files it as the undo entry) — see `edit.rs`.
fn apply_group(document: &mut Document, group: Vec<Edit>) -> Result<Vec<Edit>, CommandError> {
    let mut inverses = Vec::with_capacity(group.len());
    for edit in group {
        let (inverse, _) = edit::apply(edit, document)?;
        inverses.push(inverse);
    }
    inverses.reverse();
    Ok(inverses)
}
