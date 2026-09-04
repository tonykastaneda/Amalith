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
    /// Every colour stop: `(offset, colour, is the panel-selected stop)`.
    /// Drawn as a circle on the axis; the ones at `t≈0` / `t≈1` sit inside
    /// the origin / end handle frames.
    pub stops: Vec<(f32, amalith_core::Color, bool)>,
    /// Absolute slider position (`0..1` along the axis) of the midpoint
    /// diamond in each gap between consecutive stops (`stops.len() - 1`).
    pub mids: Vec<f32>,
    /// Radial only: the rotate handle (turns the ellipse) and the aspect
    /// handle (squishes it), in document space.
    pub rotate_handle: Option<Point>,
    pub aspect_handle: Option<Point>,
}

/// Radial-only: the (rotate handle, aspect handle) positions in **unit
/// space**, derived from the gradient's current geometry. Both track the
/// axis's current angle (`radial_axis_rad`, i.e. `start`→`end`): the
/// rotate handle sits on that axis, opposite `end` ("behind" the origin);
/// the aspect handle sits perpendicular to it, on the opposite side from
/// where `+90°` would put it ("off to the side"), at `radius * aspect`
/// from the centre so it slides with the squish.
fn radial_handle_units(g: &Gradient) -> ([f64; 2], [f64; 2]) {
    use std::f64::consts::{FRAC_PI_2, PI};
    let axis = g.radial_axis_rad();
    let r = g.radius();
    let rotate = [
        g.start[0] + r * (axis + PI).cos(),
        g.start[1] + r * (axis + PI).sin(),
    ];
    let aspect = [
        g.start[0] + r * g.aspect * (axis - FRAC_PI_2).cos(),
        g.start[1] + r * g.aspect * (axis - FRAC_PI_2).sin(),
    ];
    (rotate, aspect)
}

/// Clicking the axis line within this fraction of an end won't drop a new
/// stop there (it would just land under the endpoint handle).
pub(in crate::app) const TERMINAL_EPS: f32 = 0.03;

/// What a Gradient-tool press landed on, if the annotator is showing.
/// Hit zones are concentric at the ends: the inner disc is the stop, the
/// surrounding ring is the axis endpoint handle.
pub(in crate::app) enum AnnotHit {
    /// A colour stop circle — drag it along the axis to relocate it.
    Stop(ObjectId, usize),
    /// The midpoint diamond in gap `index` — drag it to shift the blend
    /// balance between stops `index` and `index + 1`.
    Mid(ObjectId, usize),
    /// The round origin handle (t=0) — drag to move that end of the axis.
    Start(ObjectId),
    /// The square end handle (t=1) — drag to move that end of the axis.
    End(ObjectId),
    /// Radial only: the rotate handle — drag around the centre to turn
    /// the ellipse.
    Rotate(ObjectId),
    /// Radial only: the aspect handle — drag toward/away from the centre
    /// to squish the ellipse.
    Aspect(ObjectId),
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

    /// Reverse Gradient: flip the stop order (offsets mirrored about 0.5),
    /// midpoints too.
    pub(in crate::app) fn gradient_reverse(&mut self) {
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        let n = g.stops.len();
        for s in &mut g.stops {
            s.offset = 1.0 - s.offset;
            s.midpoint = 1.0 - s.midpoint;
        }
        g.stops.reverse();
        // `reverse` misaligns each stop's midpoint (it belongs to the gap
        // *after* the stop): shift them back by one.
        if n >= 2 {
            let mids: Vec<f32> = g.stops.iter().map(|s| s.midpoint).collect();
            for i in 0..n - 1 {
                g.stops[i].midpoint = 1.0 - mids[i + 1];
            }
        }
        g.normalize();
        self.gradient_stop = n.saturating_sub(1).saturating_sub(self.gradient_stop.min(n - 1));
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.request_main_redraw();
    }

    // ---- Gradient panel: typed numeric fields ----------------------

    /// Click a Gradient-panel field: seed its buffer with the current value.
    pub(in crate::app) fn begin_gradient_edit(&mut self, field: panels::gradient::GradField) {
        use panels::gradient::GradField;
        // Commit any field already being edited before switching.
        if self.gradient_edit.is_some() {
            self.commit_gradient_edit();
        }
        let seed = self
            .target_gradient()
            .map(|(_, g)| {
                let sel = self.gradient_stop.min(g.stops.len().saturating_sub(1));
                let stop = g.stops.get(sel).copied().unwrap_or(g.stops[0]);
                match field {
                    GradField::Angle => format!("{:.0}", g.angle_deg()),
                    GradField::Aspect => format!("{:.2}", g.aspect),
                    GradField::Location => format!("{:.0}", stop.offset * 100.0),
                    GradField::Opacity => format!("{:.0}", stop.opacity * 100.0),
                }
            })
            .unwrap_or_default();
        self.gradient_edit = Some((field, seed, true));
        self.text_blink = Instant::now();
        self.request_main_redraw();
    }

    /// Commit the typed buffer (no-op if the seed was never touched).
    pub(in crate::app) fn commit_gradient_edit(&mut self) {
        let Some((field, buf, fresh)) = self.gradient_edit.take() else {
            return;
        };
        if fresh {
            self.request_main_redraw();
            return;
        }
        if let Some(v) = panels::gradient::parse_field(field, &buf) {
            self.gradient_set_field(field, v);
        }
        self.request_main_redraw();
    }

    /// Set one gradient numeric to an absolute value (from a committed edit).
    /// Location / Opacity arrive as a 0..1 fraction.
    pub(in crate::app) fn gradient_set_field(
        &mut self,
        field: panels::gradient::GradField,
        value: f64,
    ) {
        use panels::gradient::GradField;
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        let sel = self.gradient_stop.min(g.stops.len().saturating_sub(1));
        match field {
            GradField::Angle => g.set_angle_deg(value),
            GradField::Aspect => g.aspect = value.clamp(0.05, 20.0),
            GradField::Location => {
                g.stops[sel].offset = (value as f32).clamp(0.0, 1.0);
                g.normalize();
            }
            GradField::Opacity => g.stops[sel].opacity = (value as f32).clamp(0.0, 1.0),
        }
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.request_main_redraw();
    }

    /// Digit / Enter / Esc / Tab stay in the field; anything else commits
    /// and returns `false` so the rest of `on_key` runs.
    pub(in crate::app) fn gradient_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.gradient_edit.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        use winit::keyboard::{Key, NamedKey};
        match &event.logical_key {
            Key::Named(NamedKey::Enter) => {
                self.commit_gradient_edit();
                true
            }
            Key::Named(NamedKey::Escape) => {
                self.gradient_edit = None;
                self.request_main_redraw();
                true
            }
            Key::Named(NamedKey::Tab) => {
                self.commit_gradient_edit();
                false
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some((_, buf, fresh)) = self.gradient_edit.as_mut() {
                    if *fresh {
                        buf.clear();
                        *fresh = false;
                    } else {
                        buf.pop();
                    }
                }
                self.request_main_redraw();
                true
            }
            Key::Character(s)
                if s.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-') =>
            {
                if let Some((_, buf, fresh)) = self.gradient_edit.as_mut() {
                    if *fresh {
                        buf.clear();
                        *fresh = false;
                    }
                    buf.push_str(s);
                }
                self.request_main_redraw();
                true
            }
            Key::Character(_) => {
                // A non-numeric key: commit and let it fall through.
                self.commit_gradient_edit();
                false
            }
            _ => true,
        }
    }

    /// Drag the midpoint diamond between stops `index` and `index+1` to the
    /// pointer's slider position `pos` (0..1, absolute).
    pub(in crate::app) fn gradient_move_midpoint(&mut self, index: usize, pos: f32) {
        let Some((id, mut g)) = self.target_gradient() else {
            return;
        };
        if index + 1 >= g.stops.len() {
            return;
        }
        let a = g.stops[index].offset;
        let b = g.stops[index + 1].offset;
        let span = (b - a).max(1e-4);
        g.stops[index].midpoint = ((pos - a) / span).clamp(0.05, 0.95);
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id, gradient: g });
        self.gradient_target = Some(id);
        self.request_main_redraw();
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

    /// The inverse of [`Self::gradient_unit_of`]: map a bounding-box unit
    /// point back into document space for object `id`.
    fn gradient_doc_of(&self, id: ObjectId, u: [f64; 2]) -> Option<Point> {
        let doc = self.doc.editor.document();
        let b = doc.object(id)?.kind.own_local_bounds()?;
        let wt = doc.world_transform(id);
        let lp = amalith_core::Point::new(b.x0 + u[0] * b.width(), b.y0 + u[1] * b.height());
        let wp = wt * lp;
        Some(Point::new(wp.x, wp.y))
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

    /// Gradient tool press router: a press near the annotator's handles
    /// edits that handle; anything else lays down a fresh axis. Call this
    /// from the press handler instead of `begin_gradient_drag` directly.
    /// `double` = this was a double-click (opens the stop colour picker).
    pub(in crate::app) fn gradient_tool_press(&mut self, dp: Point, double: bool) {
        if let Some(hit) = self.gradient_annot_hit(dp) {
            // Grabbing a stop selects it for the panel / picker.
            if let AnnotHit::Stop(_, i) = hit {
                self.gradient_stop = i;
                if double {
                    self.ensure_panel("gradient");
                    self.gradient_stop_picker();
                    return;
                }
            }
            match hit {
                AnnotHit::Stop(obj, i) => {
                    self.drag = Drag::GradientStopOnCanvas {
                        object: obj,
                        index: i,
                    };
                }
                AnnotHit::Mid(obj, i) => {
                    self.drag = Drag::GradientMidOnCanvas {
                        object: obj,
                        index: i,
                    };
                }
                AnnotHit::Start(obj) | AnnotHit::End(obj) => {
                    let (orig_start, orig_end) = self
                        .gradient_axis_doc()
                        .map(|(_, a, b)| (a, b))
                        .unwrap_or((dp, dp));
                    self.drag = Drag::GradientEndpoint {
                        object: obj,
                        start: matches!(hit, AnnotHit::Start(_)),
                        press: dp,
                        orig_start,
                        orig_end,
                    };
                }
                AnnotHit::Rotate(obj) => {
                    self.drag = Drag::GradientRotate { object: obj };
                }
                AnnotHit::Aspect(obj) => {
                    self.drag = Drag::GradientAspect { object: obj };
                }
            }
            self.ensure_panel("gradient");
            self.request_main_redraw();
            return;
        }
        // A press on the axis line itself (between the handles) adds a stop
        // there and starts dragging it — Illustrator's "click the bar".
        if let Some(t) = self.gradient_on_axis_line(dp) {
            if let Some((id, _, _)) = self.gradient_axis_doc() {
                self.gradient_add_stop(t as f32);
                self.drag = Drag::GradientStopOnCanvas {
                    object: id,
                    index: self.gradient_stop,
                };
                self.ensure_panel("gradient");
                self.request_main_redraw();
                return;
            }
        }
        self.begin_gradient_drag(dp);
    }

    /// If `dp` (document space) lies on the axis line *segment* between the
    /// two handles (not at an end), the projected parameter `t` in
    /// `0..1` — for "click the bar to add a stop".
    fn gradient_on_axis_line(&self, dp: Point) -> Option<f64> {
        if self.active_tool != Tool::Gradient {
            return None;
        }
        let (_, a, b) = self.gradient_axis_doc()?;
        let ab = b - a;
        let len2 = ab.hypot2();
        if len2 < 1.0 {
            return None;
        }
        let t = (dp - a).dot(ab) / len2;
        // Stay clear of the end handles.
        if !(TERMINAL_EPS as f64..1.0 - TERMINAL_EPS as f64).contains(&t) {
            return None;
        }
        let foot = a + ab * t;
        let tol = 7.0 / self.doc.view.zoom.max(1e-6);
        (dp.distance(foot) <= tol).then_some(t)
    }

    /// The target gradient's axis endpoints in **document space**
    /// (`own_local_bounds` unit coords mapped through the object's world
    /// transform), plus the object id.
    fn gradient_axis_doc(&self) -> Option<(ObjectId, Point, Point)> {
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
            let wp = wt * amalith_core::Point::new(b.x0 + u[0] * b.width(), b.y0 + u[1] * b.height());
            Point::new(wp.x, wp.y)
        };
        Some((id, map(g.start), map(g.end)))
    }

    /// The minimum fraction a colour stop is kept away from each end of
    /// the axis, so it can never sit on the origin dot / end square. About
    /// 22 screen px of clearance, with a 3% floor and a 42% ceiling — the
    /// annotator, the hit test and the drag all use this so the displayed,
    /// grabbable and stored positions agree.
    fn grad_end_margin(&self, a: Point, b: Point) -> f64 {
        let len = (b - a).hypot();
        if len < 1e-6 {
            return 0.06;
        }
        (22.0 / self.doc.view.zoom.max(1e-6) / len).clamp(0.03, 0.42)
    }

    /// What the Gradient-tool press at `dp` (document space) landed on.
    /// Stops are tested at their **shown** positions, then midpoints, then
    /// the endpoint handles (the ring just outside the stop discs).
    fn gradient_annot_hit(&self, dp: Point) -> Option<AnnotHit> {
        if self.active_tool != Tool::Gradient {
            return None;
        }
        let (id, a, b) = self.gradient_axis_doc()?;
        let px = 1.0 / self.doc.view.zoom.max(1e-6); // one screen px in doc units
        let stop_r = 8.0 * px;
        let ring_r = 15.0 * px;
        let mf = self.grad_end_margin(a, b);
        let (_, g) = self.target_gradient()?;
        let ab = b - a;
        let n = g.stops.len();

        // Every colour stop, at its shown position on the axis.
        for (i, stop) in g.stops.iter().enumerate() {
            let t = (stop.offset as f64).clamp(mf, 1.0 - mf);
            if dp.distance(a + ab * t) <= stop_r {
                return Some(AnnotHit::Stop(id, i));
            }
        }
        // Midpoint diamonds.
        for i in 0..n.saturating_sub(1) {
            let o0 = (g.stops[i].offset as f64).clamp(mf, 1.0 - mf);
            let o1 = (g.stops[i + 1].offset as f64).clamp(mf, 1.0 - mf);
            let frac = o0 + (o1 - o0) * g.stops[i].midpoint as f64;
            if dp.distance(a + ab * frac) <= stop_r {
                return Some(AnnotHit::Mid(id, i));
            }
        }
        // Radial only: the rotate / aspect handles on the ellipse ring.
        if g.kind == GradientKind::Radial {
            let (rot_u, asp_u) = radial_handle_units(&g);
            if let Some(p) = self.gradient_doc_of(id, rot_u) {
                if dp.distance(p) <= stop_r {
                    return Some(AnnotHit::Rotate(id));
                }
            }
            if let Some(p) = self.gradient_doc_of(id, asp_u) {
                if dp.distance(p) <= stop_r {
                    return Some(AnnotHit::Aspect(id));
                }
            }
        }
        // Endpoint handles: the annulus around a / b.
        if dp.distance(a) <= ring_r {
            return Some(AnnotHit::Start(id));
        }
        if dp.distance(b) <= ring_r {
            return Some(AnnotHit::End(id));
        }
        None
    }

    /// Radial only: rotate the ellipse by spinning `end` itself around
    /// `start` to track `dp` — the rotate handle sits 180° from `end`, so
    /// `end` (and the axis line, and the aspect handle) all turn together
    /// with it, one rigid unit. The radius (`end`'s distance from `start`)
    /// is unchanged; only its angle moves.
    pub(in crate::app) fn gradient_set_rotation(&mut self, id: ObjectId, dp: Point) {
        let Some(u) = self.gradient_unit_of(id, dp) else {
            return;
        };
        let Some((gid, mut g)) = self.target_gradient() else {
            return;
        };
        if g.kind != GradientKind::Radial {
            return;
        }
        let pointer = (u[1] - g.start[1]).atan2(u[0] - g.start[0]);
        let end_angle = pointer - std::f64::consts::PI; // handle rests at end_angle + 180°
        let r = g.radius();
        g.end = [
            g.start[0] + r * end_angle.cos(),
            g.start[1] + r * end_angle.sin(),
        ];
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id: gid, gradient: g });
        self.gradient_target = Some(gid);
        self.request_main_redraw();
    }

    /// Radial only: set the ellipse's aspect from how far `dp` sits toward
    /// or away from the centre, along the (rotated) perpendicular axis —
    /// "off to the side" of the `start`→`end` line, not behind it.
    pub(in crate::app) fn gradient_set_aspect(&mut self, id: ObjectId, dp: Point) {
        use std::f64::consts::FRAC_PI_2;
        let Some(u) = self.gradient_unit_of(id, dp) else {
            return;
        };
        let Some((gid, mut g)) = self.target_gradient() else {
            return;
        };
        if g.kind != GradientKind::Radial {
            return;
        }
        let dir = g.radial_axis_rad() - FRAC_PI_2;
        let (ux, uy) = (u[0] - g.start[0], u[1] - g.start[1]);
        let proj = ux * dir.cos() + uy * dir.sin();
        let radius = g.radius().max(1e-6);
        g.aspect = (proj / radius).clamp(0.05, 4.0);
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id: gid, gradient: g });
        self.gradient_target = Some(gid);
        self.request_main_redraw();
    }

    /// Project `dp` onto the target gradient's axis → parameter `t`, for
    /// dragging a stop along the on-canvas line. Clamped by
    /// [`Self::grad_end_margin`] so a stop can never reach an endpoint.
    pub(in crate::app) fn gradient_axis_param(&self, dp: Point) -> Option<f64> {
        let (_, a, b) = self.gradient_axis_doc()?;
        let ab = b - a;
        let len = ab.hypot();
        if len < 1e-6 {
            return Some(0.5);
        }
        let raw = (dp - a).dot(ab) / (len * len);
        let mf = self.grad_end_margin(a, b);
        Some(raw.clamp(mf, 1.0 - mf))
    }

    /// Move a gradient-tool endpoint handle, using the axis as it stood at
    /// press time (`orig_start`/`orig_end`) as the stable reference:
    ///
    /// - dragging the **origin** translates the whole axis by the pointer's
    ///   total movement since the press — angle *and* length both stay
    ///   fixed, only position changes;
    /// - dragging the **end** keeps the origin fixed and slides just that
    ///   end along the original direction (angle fixed, extent changes).
    ///
    /// Changing the angle itself is done by dragging in empty space, not by
    /// grabbing a handle.
    pub(in crate::app) fn gradient_set_endpoint(
        &mut self,
        id: ObjectId,
        start: bool,
        press: Point,
        orig_start: Point,
        orig_end: Point,
        dp: Point,
    ) {
        let (new_start, new_end) = if start {
            let delta = dp - press;
            (orig_start + delta, orig_end + delta)
        } else {
            let dir = orig_end - orig_start;
            let len2 = dir.hypot2();
            let new_end = if len2 < 1e-9 {
                dp
            } else {
                let t = ((dp - orig_start).dot(dir) / len2).max(0.02);
                orig_start + dir * t
            };
            (orig_start, new_end)
        };
        let (Some(su), Some(eu)) = (
            self.gradient_unit_of(id, new_start),
            self.gradient_unit_of(id, new_end),
        ) else {
            return;
        };
        let Some((gid, mut g)) = self.target_gradient() else {
            return;
        };
        g.start = su;
        g.end = eu;
        // Guard against a zero-length axis.
        if (g.end[0] - g.start[0]).abs() < 1e-4 && (g.end[1] - g.start[1]).abs() < 1e-4 {
            g.end[0] += 1e-3;
        }
        let _ = self
            .doc
            .editor
            .execute(Command::EditGradient { id: gid, gradient: g });
        self.gradient_target = Some(gid);
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
        let (as_, bs) = (map(g.start), map(g.end));
        let mf = self.grad_end_margin(as_, bs) as f32;
        let shown = |off: f32| off.clamp(mf, 1.0 - mf);
        let n = g.stops.len();
        let sel = self.gradient_stop.min(n.saturating_sub(1));
        let mids = (0..n.saturating_sub(1))
            .map(|i| {
                let (o0, o1) = (shown(g.stops[i].offset), shown(g.stops[i + 1].offset));
                o0 + (o1 - o0) * g.stops[i].midpoint
            })
            .collect();
        let (rotate_handle, aspect_handle) = if g.kind == GradientKind::Radial {
            let (rot_u, asp_u) = radial_handle_units(g);
            (Some(map(rot_u)), Some(map(asp_u)))
        } else {
            (None, None)
        };
        Some(GradientAnnot {
            start: as_,
            end: bs,
            kind: g.kind,
            stops: g
                .stops
                .iter()
                .enumerate()
                .map(|(i, s)| (shown(s.offset), s.color, i == sel))
                .collect(),
            mids,
            rotate_handle,
            aspect_handle,
        })
    }
}
