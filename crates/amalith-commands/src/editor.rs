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
use amalith_core::{Affine, Artboard, ArtboardId, Document, Layer, LayerId, Object, ObjectParent};

/// A document plus its undo/redo history. See module docs.
#[derive(Debug)]
pub struct Editor {
    document: Document,
    history: History,
}

impl Editor {
    pub fn new(document: Document) -> Self {
        Self {
            document,
            history: History::default(),
        }
    }

    /// Read-only access to the underlying document. There is no
    /// `document_mut`: every mutation must go through [`Editor::execute`].
    pub fn document(&self) -> &Document {
        &self.document
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
        let edit = self.compile(command)?;
        let (inverse, new_id) = edit::apply(edit, &mut self.document)?;
        self.history.record(vec![inverse]);
        Ok(outcome_of(new_id))
    }

    /// Reverts the most recently executed (and not-yet-undone) command.
    pub fn undo(&mut self) -> Result<(), CommandError> {
        let group = self.history.pop_undo().ok_or(CommandError::NothingToUndo)?;
        let redo_group = apply_group(&mut self.document, group)?;
        self.history.push_redo(redo_group);
        Ok(())
    }

    /// Re-applies the most recently undone command.
    pub fn redo(&mut self) -> Result<(), CommandError> {
        let group = self.history.pop_redo().ok_or(CommandError::NothingToRedo)?;
        let undo_group = apply_group(&mut self.document, group)?;
        self.history.push_undo(undo_group);
        Ok(())
    }

    /// Compiles a public `Command` into the low-level `Edit` it performs.
    /// May read (but must not mutate) the document to resolve defaults
    /// like "append at the end" or "translate from the current transform".
    fn compile(&self, command: Command) -> Result<Edit, CommandError> {
        Ok(match command {
            Command::CreateArtboard { name, rect, index } => {
                let artboard = Artboard::new(ArtboardId::new(), name, rect);
                let index = index.unwrap_or_else(|| self.document.artboards().len());
                Edit::InsertArtboard { artboard, index }
            }
            Command::DeleteArtboard { id } => Edit::RemoveArtboard { id },
            Command::RenameArtboard { id, name } => Edit::RenameArtboard { id, name },
            Command::ResizeArtboard { id, rect } => Edit::ResizeArtboard { id, rect },
            Command::CreateLayer { name, index } => {
                let layer = Layer::new(LayerId::new(), name);
                let index = index.unwrap_or_else(|| self.document.layers().len());
                Edit::InsertLayer { layer, index }
            }
            Command::CreateRect { layer, rect, name } => {
                let mut object = Object::rectangle(
                    amalith_core::ObjectId::new(),
                    ObjectParent::Layer(layer),
                    rect,
                );
                object.name = name;
                let index = self.document.children_of(ObjectParent::Layer(layer)).len();
                Edit::InsertObject {
                    object: Box::new(object),
                    index,
                }
            }
            Command::MoveObject { object, delta } => {
                let current = self
                    .document
                    .object(object)
                    .ok_or(CommandError::ObjectNotFound(object))?;
                let transform = Affine::translate(delta) * current.transform;
                Edit::SetTransform {
                    id: object,
                    transform,
                }
            }
            Command::SetTransform { object, transform } => Edit::SetTransform {
                id: object,
                transform,
            },
        })
    }
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
