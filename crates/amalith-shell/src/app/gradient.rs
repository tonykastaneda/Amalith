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

/// The on-canvas gradient annotator to draw for the Gradient tool: the
/// axis endpoints in **document space** plus enough of the gradient to
/// paint the handle dots. `main_view` maps it to the screen.
#[derive(Clone)]
pub(in crate::app) struct GradientAnnot {
    pub start: Point,
    pub end: Point,
    pub kind: GradientKind,
    pub stops: Vec<(f32, amalith_core::Color)>,
}

impl App {
    /// The paint on `slot` for object `id` (fill or stroke).
    fn obj_slot_paint(doc: &Document, id: ObjectId, stroke: bool) -> Option<Paint> {
        let o = doc.object(id)?;
        Some(if stroke {
            o.appearance.stroke
        } else {
            o.appearance.fill
        })
    }

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
            self.ensure_panel("gradient");
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
            self.gradient_stop = 0;
        }
        self.ensure_panel("gradient");
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

    /// The `(gradient clone, selected stop)` pair for `panels::Ctx`.
    pub(in crate::app) fn gradient_ctx(&self) -> Option<(Gradient, usize)> {
        let (_, g) = self.target_gradient()?;
        let sel = self.gradient_stop.min(g.stops.len().saturating_sub(1));
        Some((g, sel))
    }

    /// Show a Window-menu panel docked in the right rail if it isn't visible.
    pub(in crate::app) fn ensure_panel(&mut self, id: &str) {
        let pid = match WINDOW_PANELS.iter().find(|(p, _)| *p == id) {
            Some((p, _)) => PanelId(*p),
            None => return,
        };
        if self.dock.contains(pid) {
            return;
        }
        let path = self.dock.right.any_tab_path().unwrap_or_default();
        self.dock.rail_mut(RailSide::Right).dock(
            pid,
            DropTarget::Tab {
                path,
                index: usize::MAX,
            },
        );
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
    }

    // ---- Gradient panel edits (each one undo step) --------------------

    /// Type button. Applies a fresh gradient if the slot isn't one yet,
    /// otherwise re-types the target; either way opens the panel.
    pub(in crate::app) fn gradient_set_kind(&mut self, kind: GradientKind) {
        match self.target_gradient() {
            Some((id, mut g)) if g.kind != kind => {
                g.kind = kind;
                // Give a re-typed radial a sensible centered geometry.
                if kind == GradientKind::Radial {
                    g.start = [0.5, 0.5];
                    g.end = [1.0, 0.5];
                }
                let _ = self
                    .doc
                    .editor
                    .execute(Command::EditGradient { id, gradient: g });
                self.gradient_target = Some(id);
            }
            Some(_) => {}
            None => self.apply_gradient_kind(kind),
        }
        self.ensure_panel("gradient");
        self.request_main_redraw();
    }

    /// Add a stop at slider position `offset`, colour sampled from the ramp.
    pub(in crate::app) fn gradient_add_stop(&mut self, offset: f32) {
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        let color = g.sample(offset);
        g.stops.push(amalith_core::GradientStop::new(offset, color));
        g.normalize();
        let sel = g
            .stops
            .iter()
            .position(|s| (s.offset - offset).abs() < 1e-4)
            .unwrap_or(0);
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.gradient_stop = sel;
        self.request_main_redraw();
    }

    /// Select stop `index` (double-click opens the picker for it).
    pub(in crate::app) fn gradient_select_stop(&mut self, index: usize, double: bool) {
        self.gradient_stop = index;
        if double {
            self.gradient_stop_picker();
        }
        self.request_main_redraw();
    }

    /// Live drag: move the selected stop to slider position `offset`.
    pub(in crate::app) fn gradient_move_stop(&mut self, index: usize, offset: f32) {
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        if index >= g.stops.len() {
            return;
        }
        let offset = offset.clamp(0.0, 1.0);
        g.stops[index].offset = offset;
        g.normalize();
        // Follow the handle across any reorder `normalize` did.
        let sel = g
            .stops
            .iter()
            .position(|s| (s.offset - offset).abs() < 1e-4)
            .unwrap_or(index.min(g.stops.len() - 1));
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.gradient_stop = sel;
        self.request_main_redraw();
    }

    /// Delete stop `index` (never below two stops).
    pub(in crate::app) fn gradient_remove_stop(&mut self, index: usize) {
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        if g.stops.len() <= 2 || index >= g.stops.len() {
            return;
        }
        g.stops.remove(index);
        g.normalize();
        let last = g.stops.len() - 1;
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.gradient_stop = self.gradient_stop.min(last);
        self.request_main_redraw();
    }

    /// Chevron / scroll nudge on a gradient numeric.
    pub(in crate::app) fn gradient_step(&mut self, field: panels::gradient::GradField, delta: f64) {
        use panels::gradient::GradField;
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        let sel = self.gradient_stop.min(g.stops.len().saturating_sub(1));
        match field {
            GradField::Angle => {
                let a = g.angle_deg() + delta;
                g.set_angle_deg(a);
            }
            GradField::Aspect => g.aspect = (g.aspect + delta).clamp(0.1, 10.0),
            GradField::Location => {
                g.stops[sel].offset = (g.stops[sel].offset + delta as f32).clamp(0.0, 1.0);
                g.normalize();
            }
            GradField::Opacity => {
                g.stops[sel].opacity = (g.stops[sel].opacity + delta as f32).clamp(0.0, 1.0);
            }
        }
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.request_main_redraw();
    }

    /// Open the colour picker retargeted at the selected stop.
    pub(in crate::app) fn gradient_stop_picker(&mut self) {
        let Some((_, g)) = self.target_gradient() else {
            return;
        };
        let sel = self.gradient_stop.min(g.stops.len().saturating_sub(1));
        let c = g.stops[sel].color;
        let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let origin = Point::new(
            ((w - picker::W) * 0.5).max(4.0),
            ((h - picker::H) * 0.5).max(4.0),
        );
        self.picker_gradient_stop = Some(sel);
        self.picker = Some(picker::Picker::from_color(
            self.active_slot,
            origin,
            Some(c),
        ));
        self.request_main_redraw();
    }

    /// Write a picked colour into the targeted gradient stop (called by
    /// `dismiss_picker` when `picker_gradient_stop` is set).
    pub(in crate::app) fn apply_picker_to_stop(&mut self, color: amalith_core::Color) {
        let Some(sel) = self.picker_gradient_stop else {
            return;
        };
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        if let Some(stop) = g.stops.get_mut(sel) {
            stop.color = color;
        }
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
    }

    // ---- Gradient tool (G) -----------------------------------------

    /// Map a document-space point into `id`'s bounding-box unit space
    /// (`0..1` across `own_local_bounds`) — the space gradient geometry is
    /// stored in.
    fn gradient_unit_of(&self, id: ObjectId, dp: Point) -> Option<[f64; 2]> {
        let doc = self.doc.editor.document();
        let b = doc.object(id)?.kind.own_local_bounds()?;
        let inv = doc.world_transform(id).inverse();
        let lp = inv * amalith_core::Point::new(dp.x, dp.y);
        Some([
            (lp.x - b.x0) / b.width().max(1e-6),
            (lp.y - b.y0) / b.height().max(1e-6),
        ])
    }

    /// Gradient tool press: pick the object (selection first, else topmost
    /// under the cursor), make sure it carries a gradient on the active
    /// slot, and arm the axis drag.
    pub(in crate::app) fn begin_gradient_drag(&mut self, dp: Point) {
        let stroke = self.active_slot == panels::PaintSlot::Stroke;
        let vis = self.visible_doc_rect();
        let target = {
            let doc = self.doc.editor.document();
            self.doc
                .selection
                .iter()
                .copied()
                .find(|id| {
                    doc.object(*id)
                        .and_then(|o| o.kind.own_local_bounds())
                        .is_some()
                })
                .or_else(|| crate::select::topmost_selectable_at(doc, dp, vis))
        };
        let Some(id) = target else {
            return;
        };

        let existing = Self::obj_slot_paint(self.doc.editor.document(), id, stroke)
            .and_then(|p| p.gradient_id());
        match existing {
            Some(gid) => self.gradient_target = Some(gid),
            None => {
                let outcome = self.doc.editor.execute(Command::ApplyGradient {
                    objects: vec![id],
                    stroke,
                    source: GradientRef::New(GradientKind::Linear),
                });
                if let Ok(CommandOutcome::Gradient(gid)) = outcome {
                    self.gradient_target = Some(gid);
                }
            }
        }
        self.gradient_slot = self.active_slot;
        self.gradient_stop = 0;
        self.doc.selection = vec![id];
        self.ensure_panel("gradient");
        self.drag = Drag::GradientAxis {
            object: id,
            start_doc: dp,
        };
        self.gradient_axis_to(id, dp, dp);
        self.request_main_redraw();
    }

    /// Live axis drag: set the target gradient's `start` / `end` from the
    /// press point and the current pointer (both document space).
    pub(in crate::app) fn gradient_axis_to(&mut self, id: ObjectId, start: Point, cur: Point) {
        let (Some(su), Some(eu)) = (
            self.gradient_unit_of(id, start),
            self.gradient_unit_of(id, cur),
        ) else {
            return;
        };
        let Some((gid, mut g)) = self.target_gradient() else {
            return;
        };
        g.start = su;
        // Never a zero-length axis (undefined gradient direction).
        g.end = if (eu[0] - su[0]).abs() < 1e-4 && (eu[1] - su[1]).abs() < 1e-4 {
            [su[0] + 1e-3, su[1]]
        } else {
            eu
        };
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id: gid, gradient: g });
        self.gradient_target = Some(gid);
        self.request_main_redraw();
    }

    /// The annotator to overlay while the Gradient tool is active and a
    /// gradient-painted object is selected.
    pub(in crate::app) fn gradient_annot(&self) -> Option<GradientAnnot> {
        if self.active_tool != Tool::Gradient {
            return None;
        }
        let stroke = self.active_slot == panels::PaintSlot::Stroke;
        let doc = self.doc.editor.document();
        let id = self.doc.selection.iter().copied().find(|id| {
            Self::obj_slot_paint(doc, *id, stroke)
                .and_then(|p| p.gradient_id())
                .is_some()
        })?;
        let obj = doc.object(id)?;
        let gid = Self::obj_slot_paint(doc, id, stroke)?.gradient_id()?;
        let g = doc.gradient(gid)?;
        let b = obj.kind.own_local_bounds()?;
        let wt = doc.world_transform(id);
        let map = |u: [f64; 2]| {
            let lp =
                amalith_core::Point::new(b.x0 + u[0] * b.width(), b.y0 + u[1] * b.height());
            let wp = wt * lp;
            Point::new(wp.x, wp.y)
        };
        Some(GradientAnnot {
            start: map(g.start),
            end: map(g.end),
            kind: g.kind,
            stops: g.stops.iter().map(|s| (s.offset, s.color)).collect(),
        })
    }
}
