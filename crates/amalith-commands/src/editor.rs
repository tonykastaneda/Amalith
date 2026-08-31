//! The command engine entry point.
//!
//! `Editor` pairs a [`Document`] with its undo/redo history and is the
//! thing GUI tools, keyboard shortcuts, plugins, scripts, the CLI, and
//! agents actually hold and call `execute`/`undo`/`redo` on — never a bare
//! `Document`. History lives here (not on `Document` itself) so
//! `amalith-core` stays free of any undo/redo concept and stays usable
//! headless (e.g. a one-shot CLI conversion has no need for a history
//! stack at all).
use crate::command::{Command, CommandOutcome, PasteStack};
use crate::edit::{self, Edit, NewId};
use crate::error::CommandError;
use crate::history::History;
use amalith_core::{
    Affine, Appearance, Artboard, ArtboardId, Color, Document, DocumentError, Layer, LayerId,
    Object, ObjectId, ObjectKind, ObjectParent, Paint, Rect, Vec2,
};
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
        let edits = self.compile(command)?;
        if edits.is_empty() {
            return Ok(CommandOutcome::None);
        }
        let mut inverses = Vec::with_capacity(edits.len());
        let mut new_id = None;
        for edit in edits {
            let (inverse, created) = edit::apply(edit, &mut self.document)?;
            inverses.push(inverse);
            if new_id.is_none() {
                new_id = created;
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
            ObjectKind::Path(pd) => Ok(pd.clone()),
            _ => Err(CommandError::NotAPath(id)),
        }
    }

    fn compile(&self, command: Command) -> Result<Vec<Edit>, CommandError> {
        let edits = match command {
            Command::CreateArtboard { name, rect, index } => {
                let artboard = Artboard::new(ArtboardId::new(), name, rect);
                let index = index.unwrap_or_else(|| self.document.artboards().len());
                vec![Edit::InsertArtboard { artboard, index }]
            }
            Command::DeleteArtboard { id } => vec![Edit::RemoveArtboard { id }],
            Command::DeleteObject { id } => vec![Edit::RemoveObject { id }],
            Command::DeleteObjects { ids } => ids
                .into_iter()
                .map(|id| Edit::RemoveObject { id })
                .collect(),
            Command::RenameArtboard { id, name } => vec![Edit::RenameArtboard { id, name }],
            Command::RenameLayer { id, name } => vec![Edit::RenameLayer { id, name }],
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
                data,
                transform,
                name,
            } => {
                let mut object = Object::new(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    amalith_core::ObjectKind::Text(data),
                );
                // Text follows Illustrator's default — black fill, no stroke —
                // not the shape tools' visible-stroke default.
                object.appearance = Appearance {
                    fill: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
                    stroke: Paint::None,
                    ..object.appearance
                };
                object.transform = transform;
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                vec![Edit::InsertObject {
                    object: Box::new(object),
                    index,
                }]
            }
            Command::SetText { object, data } => vec![Edit::SetTextData { id: object, data }],
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
                    amalith_core::set_handle(sp, anchor, side, Some(target));
                });
                vec![Edit::SetPathData { id: object, data }]
            }
            Command::ToggleAnchorSmooth { object, anchor } => {
                let mut data = self.path_data(object)?;
                data.edit_subpaths(|sp| amalith_core::toggle_anchor_smooth(sp, anchor));
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
            Command::SetFill { objects, paint } => objects
                .into_iter()
                .map(|id| Edit::SetFill { id, paint })
                .collect(),
            Command::SetStroke { objects, paint } => objects
                .into_iter()
                .map(|id| Edit::SetStroke { id, paint })
                .collect(),
            Command::SetStrokeWidth { objects, width } => objects
                .into_iter()
                .map(|id| Edit::SetStrokeWidth { id, width })
                .collect(),
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
            Command::Paste { .. } => {
                unreachable!("Editor::execute intercepts Command::Paste before calling compile")
            }
            Command::DuplicateObjects { .. } => unreachable!(
                "Editor::execute intercepts Command::DuplicateObjects before calling compile"
            ),
        };
        Ok(edits)
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

fn outcome_of(new_id: Option<NewId>) -> CommandOutcome {
    match new_id {
        Some(NewId::Artboard(id)) => CommandOutcome::Artboard(id),
        Some(NewId::Layer(id)) => CommandOutcome::Layer(id),
        Some(NewId::Object(id)) => CommandOutcome::Object(id),
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
