//! Gradient plumbing: applying a gradient paint, and reading / writing the
//! gradient the panel and the on-canvas gradient tool currently target.
//!
//! The gradient *definition* (kind, stops, geometry) lives in the document
//! pool (`Document::gradients`); an object's fill/stroke only holds a
//! [`amalith_core::Paint::Gradient`] id. Every edit here goes through
//! `Command::EditGradient` so it is one undo step.

use super::*;
use amalith_commands::GradientRef;
use amalith_core::{Gradient, GradientId, GradientKind, Paint};

impl App {
    /// The paint currently on the active slot — selection's if there is
    /// one, else the "next object" paint.
    fn slot_paint(&self, slot: panels::PaintSlot) -> Paint {
        self.representative()
            .map(|a| match slot {
                panels::PaintSlot::Fill => a.fill,
                panels::PaintSlot::Stroke => a.stroke,
            })
            .unwrap_or(match slot {
                panels::PaintSlot::Fill => self.doc.fill,
                panels::PaintSlot::Stroke => self.doc.stroke,
            })
    }

    /// Fill/Stroke proxy "Gradient" cell, or the Gradient tool with nothing
    /// yet applied: ensure the active slot carries a gradient, then target
    /// it for editing.
    pub(in crate::app) fn apply_gradient_paint(&mut self) {
        self.apply_gradient_kind(GradientKind::Linear);
    }

    /// Like [`Self::apply_gradient_paint`] but forces the gradient kind
    /// (used by the Gradient panel's Linear / Radial buttons when the
    /// current paint isn't a gradient yet).
    pub(in crate::app) fn apply_gradient_kind(&mut self, kind: GradientKind) {
        let slot = self.active_slot;
        let stroke = slot == panels::PaintSlot::Stroke;

        // Already a gradient on this slot — just retarget the panel (and,
        // if the kind differs, switch it).
        if let Paint::Gradient(id) = self.slot_paint(slot) {
            self.gradient_target = Some(id);
            self.gradient_slot = slot;
            if let Some(mut g) = self.doc.editor.document().gradient(id).cloned() {
                if g.kind != kind {
                    g.kind = kind;
                    let _ = self
                        .doc
                        .editor
                        .execute(Command::EditGradient { id, gradient: g });
                }
            }
            self.request_main_redraw();
            return;
        }

        let objects = self.doc.selection.clone();
        let outcome = self.doc.editor.execute(Command::ApplyGradient {
            objects,
            stroke,
            source: GradientRef::New(kind),
        });
        if let Ok(CommandOutcome::Gradient(id)) = outcome {
            let paint = Paint::Gradient(id);
            match slot {
                panels::PaintSlot::Fill => self.doc.fill = paint,
                panels::PaintSlot::Stroke => self.doc.stroke = paint,
            }
            self.gradient_target = Some(id);
            self.gradient_slot = slot;
        }
        self.request_main_redraw();
    }

    /// The gradient the panel / tool edits: `gradient_target` resolved
    /// against the pool, or — if that's stale / unset — whatever gradient
    /// the current selection's active slot points at.
    pub(in crate::app) fn target_gradient(&self) -> Option<(GradientId, Gradient)> {
        let doc = self.doc.editor.document();
        if let Some(id) = self.gradient_target {
            if let Some(g) = doc.gradient(id) {
                return Some((id, g.clone()));
            }
        }
        for slot in [self.active_slot, panels::PaintSlot::Fill, panels::PaintSlot::Stroke] {
            if let Paint::Gradient(id) = self.slot_paint(slot) {
                if let Some(g) = doc.gradient(id) {
                    return Some((id, g.clone()));
                }
            }
        }
        None
    }

    /// Replace the target gradient's whole definition (one undo step).
    pub(in crate::app) fn set_target_gradient(&mut self, gradient: Gradient) {
        let id = gradient.id;
        if self.doc.editor.document().gradient(id).is_none() {
            return;
        }
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient });
        self.gradient_target = Some(id);
        self.request_main_redraw();
    }

    /// Keep `gradient_target` pointed at the current selection's gradient
    /// (or clear it). Call after any selection change.
    pub(in crate::app) fn sync_gradient_target(&mut self) {
        for slot in [panels::PaintSlot::Fill, panels::PaintSlot::Stroke] {
            if let Paint::Gradient(id) = self.slot_paint(slot) {
                if self.doc.editor.document().gradient(id).is_some() {
                    self.gradient_target = Some(id);
                    self.gradient_slot = slot;
                    return;
                }
            }
        }
        self.gradient_target = None;
    }
}
