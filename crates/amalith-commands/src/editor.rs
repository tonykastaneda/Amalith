//! The command engine entry point.
//!
//! `Editor` pairs a [`Document`] with its undo/redo history and is the
//! thing GUI tools, keyboard shortcuts, plugins, scripts, the CLI, and
//! agents actually hold and call `execute`/`undo`/`redo` on — never a bare
//! `Document`. History lives here (not on `Document` itself) so
//! `amalith-core` stays free of any undo/redo concept and stays usable
//! headless (e.g. a one-shot CLI conversion has no need for a history
//! stack at all).
use crate::command::{Command, CommandOutcome};
use crate::edit::{self, Edit, NewId};
use crate::error::CommandError;
use crate::history::History;
use amalith_core::{
    Affine, Artboard, ArtboardId, Document, Layer, LayerId, Object, ObjectId, ObjectParent, Rect,
};
use std::collections::HashMap;

/// A document plus its undo/redo history. See module docs.
///
/// Also owns a memoized bounds cache — see [`Editor::bounds_of`] and
/// `PERFORMANCE.md` at the repo root. The cache lives here, never on
/// `Document`, and is wiped wholesale after every successful `execute` /
/// `undo` / `redo`.
#[derive(Debug)]
pub struct Editor {
    document: Document,
    history: History,
    bounds_cache: HashMap<ObjectId, Option<Rect>>,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            history: History::default(),
            bounds_cache: HashMap::new(),
        }
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
        let edits = self.compile(command)?;
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
    fn compile(&self, command: Command) -> Result<Vec<Edit>, CommandError> {
        let edits = match command {
            Command::CreateArtboard { name, rect, index } => {
                let artboard = Artboard::new(ArtboardId::new(), name, rect);
                let index = index.unwrap_or_else(|| self.document.artboards().len());
                vec![Edit::InsertArtboard { artboard, index }]
            }
            Command::DeleteArtboard { id } => vec![Edit::RemoveArtboard { id }],
            Command::DeleteObject { id } => vec![Edit::RemoveObject { id }],
            Command::RenameArtboard { id, name } => vec![Edit::RenameArtboard { id, name }],
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
        };
        Ok(edits)
    }
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
