//! Pointer motion and release: the `Drag` state machine, hover tooltips,
//! cursor shape, and drop resolution. `window_event` delegates its
//! `CursorMoved` and left-mouse-up arms here.

use super::super::*;

impl App {
    pub(in crate::app) fn on_cursor_move(&mut self) {
        // A live text-selection drag in the About panel.
        if self.about.as_ref().is_some_and(about::About::is_dragging) {
            if let Some(a) = &mut self.about {
                a.on_drag(&mut self.text, self.pointer.to_vec2());
            }
            self.request_main_redraw();
            return;
        }
        // Command palette: hovering a row selects it.
        if let Some(p) = &mut self.palette {
            if p.hover(self.pointer) {
                self.request_main_redraw();
            }
            return;
        }
        self.update_canvas_cursor();
        self.refresh_tooltip();
        // Redraw so any painted cursor glyph tracks the pointer — this
        // covers the scale / rotate / loaded-text glyphs, not just Glyph.
        // The Rotate tool shows path nodes but keeps the OS crosshair, so
        // it also needs a per-move repaint for the node hover-swell.
        if self.cursor_mode.is_drawn()
            || self.ctx_menu.is_some()
            || (self.active_tool == Tool::Rotate
                && !self.doc.selection.is_empty()
                && matches!(self.drag, Drag::None))
        {
            self.request_main_redraw();
        }
        match &self.drag {
            Drag::MovePicker { offset } => {
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };
                if let Some(pk) = &mut self.picker {
                    pk.origin = Point::new(
                        (self.pointer.x - offset.x).clamp(4.0, (w - picker::W - 4.0).max(4.0)),
                        (self.pointer.y - offset.y).clamp(4.0, (h - picker::H - 4.0).max(4.0)),
                    );
                    self.request_main_redraw();
                }
            }
            Drag::MasterWidth { master, edge, start_w, start_x } => {
                let (master, edge, start_w, start_x) = (*master, *edge, *start_w, *start_x);
                let dx = (self.pointer.x - start_x) as f32;
                let raw = match edge {
                    ResizeEdge::Right => start_w + dx,
                    ResizeEdge::Left => start_w - dx,
                };
                let min_w = if self.dock.master(master).is_some_and(Master::is_tools) {
                    layout::TOOLS_MIN_W as f32
                } else {
                    layout::MASTER_MIN_W as f32
                };
                let clamped = raw.clamp(min_w, layout::MASTER_MAX_W as f32);
                if let Some(m) = self.dock.master_mut(master) {
                    m.rect[2] = clamped;
                }
                self.sync_floating_window_height(master);
                self.request_main_redraw();
            }
            Drag::GroupContentResize { master, group, start_h, start_y } => {
                let (master, group, start_h, start_y) = (*master, *group, *start_h, *start_y);
                let dy = (self.pointer.y - start_y) as f32;
                let next = (start_h + dy)
                    .clamp(crate::dock::TAB_CONTENT_MIN_H, crate::dock::TAB_CONTENT_MAX_H);
                if let Some(m) = self.dock.master_mut(master) {
                    if let Some(g) = m.group_mut(group) {
                        g.content_h = Some(next);
                    }
                }
                self.sync_floating_window_height(master);
                self.request_main_redraw();
            }
            Drag::Pan { last } => {
                let last = *last;
                self.doc.view.pan += self.pointer - last;
                self.drag = Drag::Pan { last: self.pointer };
                self.request_main_redraw();
            }
            Drag::ScrubZoom { anchor, last } => {
                let (anchor, last) = (*anchor, *last);
                let dx = self.pointer.x - last.x;
                if dx.abs() > 0.01 {
                    self.zoom_sign = if dx < 0.0 { -1 } else { 1 };
                    self.doc.view.zoom_at(2f64.powf(dx / 180.0), anchor);
                }
                self.drag = Drag::ScrubZoom {
                    anchor,
                    last: self.pointer,
                };
                self.update_canvas_cursor();
                self.request_main_redraw();
            }
            Drag::MoveObjects {
                start_doc,
                last_doc: _,
                moved,
                hit,
            } => {
                let start_doc = *start_doc;
                let hit = *hit;
                let already = *moved;
                let dp = self.doc_point(self.pointer);
                // Click-to-set-key-object needs a slop so a 1px jitter
                // isn't treated as a move. Threshold is screen px.
                let screen = (dp - start_doc).hypot() * self.doc.view.zoom;
                let moved = already || screen > 4.0;
                self.drag = Drag::MoveObjects {
                    start_doc,
                    last_doc: dp,
                    moved,
                    hit,
                };
                if moved {
                    self.request_main_redraw();
                }
            }
            Drag::Marquee { start } | Drag::AnchorMarquee { start, .. } => {
                self.marquee = Some(Rect::from_points(*start, self.pointer));
                self.request_main_redraw();
            }
            Drag::MoveAnchors { start_doc, .. } => {
                let start_doc = *start_doc;
                self.drag = Drag::MoveAnchors {
                    start_doc,
                    last_doc: self.doc_point(self.pointer),
                    moved: true,
                };
                self.request_main_redraw();
            }
            Drag::MoveHandle {
                object,
                anchor,
                side,
                start_doc,
                ..
            } => {
                let (object, anchor, side, start_doc) = (*object, *anchor, *side, *start_doc);
                self.drag = Drag::MoveHandle {
                    object,
                    anchor,
                    side,
                    start_doc,
                    last_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::PickColor { in_hue } => {
                let in_hue = *in_hue;
                if let Some(pk) = self.picker {
                    let (h, s, v) = picker::drag_value(&pk, self.pointer, in_hue);
                    if let Some(p) = &mut self.picker {
                        p.h = h;
                        p.s = s;
                        p.v = v;
                    }
                    self.request_main_redraw();
                }
            }
            Drag::ColorScrub { channel, track } => {
                let channel = *channel;
                let track = *track;
                let t = ((self.pointer.x - track.x0) / track.width()).clamp(0.0, 1.0) as f32;
                self.set_color_channel(channel, t);
                self.drag = Drag::ColorScrub { channel, track };
            }
            Drag::ColorSpectrum { track } => {
                let track = *track;
                let t = ((self.pointer.x - track.x0) / track.width()).clamp(0.0, 1.0) as f32;
                self.set_color_spectrum(t);
                self.drag = Drag::ColorSpectrum { track };
            }
            Drag::GradientStop { index, bar } => {
                let (index, bar) = (*index, *bar);
                let off = ((self.pointer.x - bar.x0) / bar.width()).clamp(0.0, 1.0) as f32;
                self.gradient_move_stop(index, off);
                // The stop may have been reordered; keep dragging the one
                // that's now selected.
                self.drag = Drag::GradientStop {
                    index: self.gradient_stop,
                    bar,
                };
            }
            Drag::GradientAxis { object, start_doc } => {
                let (object, start_doc) = (*object, *start_doc);
                let cur = self.doc_point(self.pointer);
                self.gradient_axis_to(object, start_doc, cur);
                self.drag = Drag::GradientAxis { object, start_doc };
            }
            Drag::GradientStopOnCanvas { object, index } => {
                let (object, index) = (*object, *index);
                let dp = self.doc_point(self.pointer);
                if let Some(t) = self.gradient_axis_param(dp) {
                    self.gradient_move_stop(index, t as f32);
                }
                // The stop may have been reordered mid-drag; keep the one
                // that's now selected.
                self.drag = Drag::GradientStopOnCanvas {
                    object,
                    index: self.gradient_stop,
                };
            }
            Drag::GradientEndpoint {
                object,
                start,
                press,
                orig_start,
                orig_end,
            } => {
                let (object, start, press, orig_start, orig_end) =
                    (*object, *start, *press, *orig_start, *orig_end);
                let dp = self.doc_point(self.pointer);
                self.gradient_set_endpoint(object, start, press, orig_start, orig_end, dp);
                self.drag = Drag::GradientEndpoint {
                    object,
                    start,
                    press,
                    orig_start,
                    orig_end,
                };
            }
            Drag::GradientMidOnCanvas { object, index } => {
                let (object, index) = (*object, *index);
                let dp = self.doc_point(self.pointer);
                if let Some(t) = self.gradient_axis_param(dp) {
                    self.gradient_move_midpoint(index, t as f32);
                }
                self.drag = Drag::GradientMidOnCanvas { object, index };
            }
            Drag::GradientRotate { object } => {
                let object = *object;
                let dp = self.doc_point(self.pointer);
                self.gradient_set_rotation(object, dp);
                self.drag = Drag::GradientRotate { object };
            }
            Drag::GradientAspect { object } => {
                let object = *object;
                let dp = self.doc_point(self.pointer);
                self.gradient_set_aspect(object, dp);
                self.drag = Drag::GradientAspect { object };
            }
            Drag::GradientMid { index, bar } => {
                let (index, bar) = (*index, *bar);
                let pos = ((self.pointer.x - bar.x0) / bar.width()).clamp(0.0, 1.0) as f32;
                self.gradient_move_midpoint(index, pos);
                self.drag = Drag::GradientMid { index, bar };
            }
            Drag::GradientPointOnCanvas { object, index } => {
                let (object, index) = (*object, *index);
                let dp = self.doc_point(self.pointer);
                self.gradient_move_point(object, index, dp);
                self.drag = Drag::GradientPointOnCanvas { object, index };
            }
            Drag::GradientPointSpread { object, index } => {
                let (object, index) = (*object, *index);
                let dp = self.doc_point(self.pointer);
                self.gradient_set_point_spread(object, index, dp);
                self.drag = Drag::GradientPointSpread { object, index };
            }
            Drag::LayerDrag { body, press, moved } => {
                let (body, press, was_moved) = (*body, *press, *moved);
                let far = (self.pointer - press).hypot() > 4.0;
                if !was_moved && !far {
                    return;
                }
                self.drag = Drag::LayerDrag {
                    body,
                    press,
                    moved: true,
                };
                let ids = crate::panels::layers::order_front_to_back(
                    self.doc.editor.document(),
                    &self.doc.expanded_groups,
                    &self.doc.selection,
                );
                self.layer_drop = crate::panels::layers::drop_target(
                    body,
                    self.pointer,
                    self.doc.editor.document(),
                    &self.doc.expanded_groups,
                    &self.layer_query,
                    self.panel_scroll_of(PanelId("layers")),
                    &ids,
                )
                .map(|d| (d.parent, d.index, d.row, d.into));
                self.request_main_redraw();
            }
            Drag::DrawShape {
                tool, start_doc, ..
            } => {
                let (tool, start_doc) = (*tool, *start_doc);
                self.drag = Drag::DrawShape {
                    tool,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::DrawArtboard { start_doc, .. } => {
                let start_doc = *start_doc;
                self.drag = Drag::DrawArtboard {
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::PenHandle { .. } => self.drag_pen_handle(),
            Drag::DrawText { start_doc, .. } => {
                let start_doc = *start_doc;
                self.drag = Drag::DrawText {
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::ThreadNewBox { from, start_doc, .. } => {
                let (from, start_doc) = (*from, *start_doc);
                self.drag = Drag::ThreadNewBox {
                    from,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::TextSelect => {
                if let Some(p) = self.text_editor_point(self.pointer) {
                    if let Some(te) = &mut self.text_edit {
                        te.pointer_drag(p, &mut self.text);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::NewdocSelect { field } => {
                let field = *field;
                let p = self.pointer;
                if let Some(form) = self.newdoc.as_mut() {
                    form.field(field).pointer_drag(p, &mut self.text);
                }
                self.request_main_redraw();
            }
            Drag::MoveArtboard { id, start_doc, .. } => {
                let (id, start_doc) = (*id, *start_doc);
                self.drag = Drag::MoveArtboard {
                    id,
                    start_doc,
                    last_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::ResizeArtboard {
                id,
                handle,
                start_rect,
                start_doc,
                ..
            } => {
                let (id, handle, start_rect, start_doc) =
                    (*id, *handle, *start_rect, *start_doc);
                self.drag = Drag::ResizeArtboard {
                    id,
                    handle,
                    start_rect,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::Scale {
                handle,
                start_bounds,
                start_xf,
                ..
            } => {
                let (handle, start_bounds) = (*handle, *start_bounds);
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let m = handles::scaled_transform(
                    start_bounds,
                    handle,
                    dp,
                    self.shift_down,
                    self.alt_down,
                );
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                self.drag = Drag::Scale {
                    handle,
                    start_bounds,
                    start_xf,
                    preview,
                };
                self.request_main_redraw();
            }
            Drag::ResizeTextBox {
                handle,
                start_bounds,
                frames,
                start_doc,
                ..
            } => {
                let (handle, start_bounds, start_doc) = (*handle, *start_bounds, *start_doc);
                let frames = frames.clone();
                self.drag = Drag::ResizeTextBox {
                    handle,
                    start_bounds,
                    frames,
                    start_doc,
                    cur_doc: self.doc_point(self.pointer),
                };
                self.request_main_redraw();
            }
            Drag::Rotate {
                center,
                start_angle,
                start_xf,
                ..
            } => {
                let (center, start_angle) = (*center, *start_angle);
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let m = handles::rotate_transform(center, start_angle, dp, self.shift_down);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                self.drag = Drag::Rotate {
                    center,
                    start_angle,
                    start_xf,
                    preview,
                };
                self.request_main_redraw();
            }
            Drag::NewGuide { orient, .. } => {
                use amalith_core::GuideOrient;
                let orient = *orient;
                let dp = self.doc_point(self.pointer);
                let pos = match orient {
                    GuideOrient::Horizontal => dp.y,
                    GuideOrient::Vertical => dp.x,
                };
                self.drag = Drag::NewGuide { orient, pos };
                self.request_main_redraw();
            }
            Drag::MoveGuide {
                id,
                orient,
                orig,
                grab,
                press,
                moved,
                ..
            } => {
                use amalith_core::GuideOrient;
                let (id, orient, orig, grab, press) = (*id, *orient, *orig, *grab, *press);
                // Hold the guide put until the pointer leaves a 3px slop
                // circle — a click shouldn't nudge it.
                let far = *moved || (self.pointer - press).hypot() > 3.0;
                let pos = if far {
                    let dp = self.doc_point(self.pointer);
                    (match orient {
                        GuideOrient::Horizontal => dp.y,
                        GuideOrient::Vertical => dp.x,
                    }) - grab
                } else {
                    orig
                };
                self.drag = Drag::MoveGuide {
                    id,
                    orient,
                    pos,
                    orig,
                    grab,
                    press,
                    moved: far,
                };
                self.request_main_redraw();
            }
            Drag::RotateTool {
                pivot,
                start_angle,
                start_xf,
                copy,
                moved,
                ..
            } => {
                let (pivot, start_angle, copy, was_moved) =
                    (*pivot, *start_angle, *copy, *moved);
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let m = handles::rotate_transform(pivot, start_angle, dp, self.shift_down);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                let moved =
                    was_moved || (handles::angle_to(pivot, dp) - start_angle).abs() > 1e-3;
                self.drag = Drag::RotateTool {
                    pivot,
                    start_angle,
                    start_xf,
                    preview,
                    copy,
                    moved,
                };
                self.update_canvas_cursor();
                self.request_main_redraw();
            }
            Drag::PendingMasterMove { master, press, grab, was_docked } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let (master, grab) = (*master, *grab);
                    if was_docked.is_some() {
                        // event_loop isn't handed to cursor events; defer
                        // the actual window spawn to the caller.
                        self.pending_master_undock = Some(master);
                    } else {
                        self.drag = Drag::MovingMaster { master, grab };
                    }
                }
            }
            Drag::PendingGroupDrag { source, press } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let (source, press) = (*source, *press);
                    // Already the sole group of an already-floating
                    // Master (its own window, pressed here) — nothing to
                    // detach, just start moving that window live.
                    let already_alone = self
                        .dock
                        .master(source.0)
                        .is_some_and(|m| m.dock.is_none() && m.groups.len() == 1);
                    if already_alone {
                        self.drag = Drag::DraggingGroup { current: source, grab: press.to_vec2() };
                    } else {
                        // event_loop isn't handed to cursor events; defer
                        // the actual live-detach spawn to the caller.
                        self.pending_group_live_detach = Some(source);
                    }
                }
            }
            Drag::PendingPanelDrag { panel, press } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let (panel, press) = (*panel, *press);
                    let already_alone = self.dock.locate(panel).is_some_and(|(mid, ..)| {
                        self.dock.master(mid).is_some_and(|m| m.dock.is_none() && m.panels().len() == 1)
                    });
                    if already_alone {
                        let master = self.dock.locate(panel).map(|(mid, ..)| mid).unwrap();
                        self.drag = Drag::DraggingPanel { panel, master, grab: press.to_vec2() };
                    } else {
                        self.pending_panel_live_detach = Some(panel);
                    }
                }
            }
            _ => {}
        }

        // Move a floating Master by locking it to the cursor: the cursor's
        // position *inside* the window, versus where it was grabbed, is
        // the move. Nothing reads the OS window rect back, so it can't
        // drift or jitter (⇐ `onPointerMove`'s master branch).
        if let Drag::MovingMaster { master, grab } = self.drag {
            let Some(global) = self.current_global_cursor() else {
                return;
            };
            let new_pos = global - grab;
            if let Some(w) = self.floating_window(master) {
                w.set_outer_position(LogicalPosition::new(new_pos.x, new_pos.y));
                w.request_redraw();
            }
            if let Some(m) = self.dock.master_mut(master) {
                m.rect[0] = new_pos.x as f32;
                m.rect[1] = new_pos.y as f32;
            }
            let (dock, group_drop) = self.resolve_master_drop(global, master);
            if dock != self.master_dock_preview || group_drop != self.group_drop_preview {
                self.master_dock_preview = dock;
                let old = self.group_drop_preview.as_ref().map(|(m, _)| *m);
                let new = group_drop.as_ref().map(|(m, _)| *m);
                self.group_drop_preview = group_drop;
                for cand in [old, new].into_iter().flatten() {
                    if let Some(w) = self.floating_window(cand) {
                        w.request_redraw();
                    }
                }
            }
            self.request_main_redraw();
        }

        // Dragging a Group: it's already its own (real) floating Master
        // window by this point — move it with the cursor exactly like
        // `MovingMaster` does, and keep computing a fine-grained
        // merge/new-sibling preview against every *other* Master (⇐
        // `current`, the group's own `(master, index)`, always `(_, 0)`
        // once live-detached, since that Master then holds nothing else).
        if let Drag::DraggingGroup { current, grab } = self.drag {
            let Some(global) = self.current_global_cursor() else {
                return;
            };
            let new_pos = global - grab;
            if let Some(w) = self.floating_window(current.0) {
                w.set_outer_position(LogicalPosition::new(new_pos.x, new_pos.y));
                w.request_redraw();
            }
            if let Some(m) = self.dock.master_mut(current.0) {
                m.rect[0] = new_pos.x as f32;
                m.rect[1] = new_pos.y as f32;
            }
            let next = self.resolve_group_drop(global, current);
            if next != self.group_drop_preview {
                let old = self.group_drop_preview.as_ref().map(|(m, _)| *m);
                let new = next.as_ref().map(|(m, _)| *m);
                self.group_drop_preview = next;
                for cand in [old, new].into_iter().flatten() {
                    if let Some(w) = self.floating_window(cand) {
                        w.request_redraw();
                    }
                }
            }
            self.request_main_redraw();
        }

        // Same, for dragging a single Panel.
        if let Drag::DraggingPanel { master, grab, .. } = self.drag {
            let Some(global) = self.current_global_cursor() else {
                return;
            };
            let new_pos = global - grab;
            if let Some(w) = self.floating_window(master) {
                w.set_outer_position(LogicalPosition::new(new_pos.x, new_pos.y));
                w.request_redraw();
            }
            if let Some(m) = self.dock.master_mut(master) {
                m.rect[0] = new_pos.x as f32;
                m.rect[1] = new_pos.y as f32;
            }
            let next = self.resolve_panel_drop(global, master);
            if next != self.panel_drop_preview {
                let old = self.panel_drop_preview.as_ref().map(|(m, _)| *m);
                let new = next.as_ref().map(|(m, _)| *m);
                self.panel_drop_preview = next;
                for cand in [old, new].into_iter().flatten() {
                    if let Some(w) = self.floating_window(cand) {
                        w.request_redraw();
                    }
                }
            }
            self.request_main_redraw();
        }
    }

    pub(in crate::app) fn on_release(&mut self) {
        // End any About-window text-selection drag.
        if let Some(a) = &mut self.about {
            a.on_release();
        }
        // A quick tap on the Shape slot (released before the hold opened
        // the flyout) just re-activates the last shape tool.
        if self.shape_press.take().is_some() && self.shape_flyout.is_none() {
            let t = self.last_shape_tool;
            self.set_tool(t);
        }
        match std::mem::take(&mut self.drag) {
            Drag::None
            | Drag::MasterWidth { .. }
            | Drag::GroupContentResize { .. }
            | Drag::PendingGroupDrag { .. }
            | Drag::PendingMasterMove { .. }
            | Drag::Pan { .. } => {}
            // A scrubby-zoom that never moved = a click: step-zoom at the
            // point (Alt / left-drag direction = out).
            Drag::ScrubZoom { anchor, last } => {
                if (last - anchor).hypot() < 4.0 {
                    let f = if self.alt_down { 1.0 / 1.6 } else { 1.6 };
                    self.doc.view.zoom_at(f, anchor);
                    self.request_main_redraw();
                }
            }
            Drag::MovePicker { .. } => {}
            // The dialog keeps edits pending until its OK button (or Enter).
            Drag::PickColor { .. } => {}
            Drag::ColorScrub { .. } | Drag::ColorSpectrum { .. } => {
                if let Some(c) = self.active_paint().color() {
                    self.push_recent(c);
                }
            }
            Drag::GradientStop { index, bar } => {
                // Released well below the ramp = "drag off to delete".
                if self.pointer.y > bar.y1 + crate::panels::gradient::REMOVE_DROP {
                    self.gradient_remove_stop(index);
                }
            }
            Drag::GradientAxis { object, start_doc } => {
                // A plain click (no real drag) still applied a default
                // axis in `begin_gradient_drag`; a drag committed live.
                let cur = self.doc_point(self.pointer);
                self.gradient_axis_to(object, start_doc, cur);
            }
            // These all committed live on every move; nothing to finalise.
            Drag::GradientMid { .. }
            | Drag::GradientStopOnCanvas { .. }
            | Drag::GradientEndpoint { .. }
            | Drag::GradientMidOnCanvas { .. }
            | Drag::GradientRotate { .. }
            | Drag::GradientAspect { .. }
            | Drag::GradientPointOnCanvas { .. }
            | Drag::GradientPointSpread { .. } => {}
            Drag::LayerDrag { body, moved, .. } => {
                if moved {
                    let ids = crate::panels::layers::order_front_to_back(
                        self.doc.editor.document(),
                        &self.doc.expanded_groups,
                        &self.doc.selection,
                    );
                    let target = crate::panels::layers::drop_target(
                        body,
                        self.pointer,
                        self.doc.editor.document(),
                        &self.doc.expanded_groups,
                        &self.layer_query,
                        self.panel_scroll_of(PanelId("layers")),
                        &ids,
                    );
                    if let Some(d) = target {
                        if self
                            .doc
                            .editor
                            .execute(amalith_commands::Command::Reparent {
                                ids,
                                parent: d.parent,
                                index: d.index,
                            })
                            .is_ok()
                        {
                            self.sync_align_mode();
                        }
                    }
                }
                self.layer_drop = None;
                self.request_main_redraw();
            }
            Drag::MoveObjects {
                start_doc,
                last_doc,
                moved,
                hit,
            } => {
                if !moved {
                    if let Some(id) = hit.filter(|id| self.doc.selection.contains(id)) {
                        if self.doc.selection.len() >= 2 {
                            if self.key_object == Some(id) {
                                self.key_object = None;
                                self.align_to = amalith_commands::AlignTo::Selection;
                            } else {
                                self.key_object = Some(id);
                                self.align_to = amalith_commands::AlignTo::KeyObject;
                            }
                            self.request_main_redraw();
                        }
                    }
                } else if !self.doc.selection.is_empty() {
                    let mut d = last_doc - start_doc;
                    if self.shift_down {
                        d = snap8(d);
                    }
                    let delta = convert::vec2_to_core(d);
                    if self.alt_down {
                        if let Ok(new_ids) = self
                            .doc.editor
                            .duplicate_objects(&self.doc.selection.clone(), delta)
                        {
                            self.doc.selection = new_ids;
                        }
                    } else {
                        let _ = self.doc.editor.execute(Command::MoveObjects {
                            objects: self.doc.selection.clone(),
                            delta,
                        });
                    }
                    self.request_main_redraw();
                }
            }
            // The handle drag has already been written into `self.pen` by
            // `on_cursor_move`; the anchor stays placed either way.
            Drag::PenHandle { .. } => {}
            // Line tool: press → release makes a two-anchor open path.
            // Shift snaps the angle to 45°.
            Drag::DrawShape {
                tool: Tool::Line,
                start_doc,
                cur_doc,
            } => {
                let end = if self.shift_down {
                    constrained(Some(start_doc), cur_doc, true)
                } else {
                    cur_doc
                };
                if (end - start_doc).hypot() > 1.5 {
                    let layer = self.ensure_layer();
                    let cp = |p: Point| amalith_core::Point::new(p.x, p.y);
                    let corner = |p: Point| amalith_core::Anchor {
                        point: cp(p),
                        handle_in: None,
                        handle_out: None,
                        mode: amalith_core::HandleMode::Corner,
                    };
                    let path = amalith_core::PathData::from_subpaths(vec![amalith_core::Subpath {
                        anchors: vec![corner(start_doc), corner(end)],
                        closed: false,
                    }]);
                    if let Ok(CommandOutcome::Object(id)) =
                        self.doc.editor.execute(Command::CreatePath {
                            layer,
                            path,
                            name: None,
                        })
                    {
                        self.doc.selection = vec![id];
                        self.apply_new_appearance(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::DrawShape {
                tool,
                start_doc,
                cur_doc,
            } => {
                // A plain click — under ~3 screen px of travel — opens the
                // exact-size dialog instead of dropping a zero-size shape.
                // The window is spawned in `window_event` (no `event_loop`
                // here).
                if tool.is_shape()
                    && (cur_doc - start_doc).hypot() * self.doc.view.zoom < 3.0
                {
                    self.pending_shape_dialog = Some((tool, start_doc));
                    return;
                }
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if r.width() > 0.5 && r.height() > 0.5 {
                    let layer = self.ensure_layer();
                    let cmd = match tool {
                        Tool::Rectangle => Command::CreateRect {
                            layer,
                            rect: r,
                            name: None,
                        },
                        Tool::Ellipse => Command::CreateEllipse {
                            layer,
                            rect: r,
                            name: None,
                        },
                        Tool::RoundedRect | Tool::Polygon | Tool::Star => {
                            match primitive_path(tool, r) {
                                Some(path) => Command::CreatePath {
                                    layer,
                                    path,
                                    name: None,
                                },
                                None => return,
                            }
                        }
                        Tool::Select
                        | Tool::DirectSelect
                        | Tool::Pen
                        | Tool::Line
                        | Tool::Text
                        | Tool::Artboard
                        | Tool::Hand
                        | Tool::Zoom
                        | Tool::Eyedropper
                        | Tool::Gradient
                        | Tool::Rotate => return,
                    };
                    if let Ok(CommandOutcome::Object(id)) = self.doc.editor.execute(cmd) {
                        self.doc.selection = vec![id];
                        self.apply_new_appearance(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::TextSelect | Drag::NewdocSelect { .. } => {}
            Drag::DrawText { start_doc, cur_doc } => {
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if r.width() > 4.0 && r.height() > 4.0 {
                    // A real drag → area / paragraph type. The dragged
                    // rectangle is the text box: fixed width and height,
                    // text wraps inside it and overflows past the bottom.
                    self.create_text(
                        amalith_core::TextKind::Area {
                            width: r.width(),
                            height: Some(r.height()),
                        },
                        Point::new(r.x0, r.y0),
                    );
                } else {
                    // A click → point type.
                    self.create_text(amalith_core::TextKind::Point, start_doc);
                }
            }
            Drag::ThreadNewBox {
                from,
                start_doc,
                cur_doc,
            } => {
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                let (w, h) = if r.width() > 4.0 && r.height() > 4.0 {
                    (r.width(), r.height())
                } else {
                    // A plain click drops a default-size frame here.
                    (360.0, 220.0)
                };
                if let Some(to) = self.create_empty_area_text(w, h, Point::new(r.x0, r.y0)) {
                    self.thread_text(from, to);
                }
            }
            Drag::DrawArtboard {
                start_doc,
                cur_doc,
            } => {
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if r.width() > 1.0 && r.height() > 1.0 {
                    let n = self.doc.editor.document().artboards().len() + 1;
                    if let Ok(CommandOutcome::Artboard(id)) =
                        self.doc.editor.execute(Command::CreateArtboard {
                            name: format!("Artboard {n}"),
                            rect: r,
                            index: None,
                        })
                    {
                        self.doc.selected_artboard = Some(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::MoveArtboard {
                id,
                start_doc,
                last_doc,
            } => {
                let mut d = last_doc - start_doc;
                if self.shift_down {
                    d = snap8(d);
                }
                let delta = convert::vec2_to_core(d);
                if delta.x != 0.0 || delta.y != 0.0 {
                    let cmd = if self.alt_down {
                        Command::DuplicateArtboard { id, delta }
                    } else {
                        Command::MoveArtboard { id, delta }
                    };
                    if let Ok(CommandOutcome::Artboard(new_id)) = self.doc.editor.execute(cmd) {
                        self.doc.selected_artboard = Some(new_id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::ResizeArtboard {
                id,
                handle,
                start_rect,
                start_doc,
                cur_doc,
            } => {
                let d = convert::vec2_to_core(cur_doc - start_doc);
                let rect = resize_rect(start_rect, handle, d);
                if (rect.width() - start_rect.width()).abs() > f64::EPSILON
                    || (rect.height() - start_rect.height()).abs() > f64::EPSILON
                    || (rect.x0 - start_rect.x0).abs() > f64::EPSILON
                    || (rect.y0 - start_rect.y0).abs() > f64::EPSILON
                {
                    let _ = self.doc.editor.execute(Command::ResizeArtboard { id, rect });
                    self.request_main_redraw();
                }
            }
            Drag::Rotate {
                start_xf, preview, ..
            } => {
                if preview != start_xf {
                    let items = preview
                        .into_iter()
                        .map(|(id, a)| (id, convert::affine_to_core(a)))
                        .collect();
                    let _ = self.doc.editor.execute(Command::SetTransforms { items });
                    self.request_main_redraw();
                }
            }
            Drag::NewGuide { orient, pos } => {
                // Released on the canvas → commit; on a ruler / off-canvas
                // → discard.
                if self.ruler_strip_at(self.pointer).is_none()
                    && self.canvas_viewport().contains(self.pointer)
                {
                    let _ = self
                        .doc
                        .editor
                        .execute(Command::AddGuide { orient, pos });
                }
                self.request_main_redraw();
            }
            Drag::MoveGuide {
                id, pos, orig, ..
            } => {
                if self.ruler_strip_at(self.pointer).is_some()
                    || !self.canvas_viewport().contains(self.pointer)
                {
                    let _ = self.doc.editor.execute(Command::DeleteGuide { id });
                    self.selected_guides.retain(|g| *g != id);
                } else if pos != orig {
                    let _ = self.doc.editor.execute(Command::MoveGuide { id, pos });
                }
                self.request_main_redraw();
            }
            Drag::RotateTool {
                start_xf,
                preview,
                copy,
                moved,
                ..
            } => {
                if !moved {
                    // A click, not a drag: re-place the reference point.
                    self.transform_pivot = Some(self.doc_point(self.pointer));
                } else if preview != start_xf {
                    if copy {
                        let ids: Vec<ObjectId> = self.doc.selection.clone();
                        if let Ok(new_ids) = self
                            .doc
                            .editor
                            .duplicate_objects(&ids, convert::vec2_to_core(Vec2::ZERO))
                        {
                            let items: Vec<_> = ids
                                .iter()
                                .zip(&new_ids)
                                .filter_map(|(src, dst)| {
                                    preview.get(src).map(|a| (*dst, convert::affine_to_core(*a)))
                                })
                                .collect();
                            let _ =
                                self.doc.editor.execute(Command::SetTransforms { items });
                            self.doc.selection = new_ids;
                        }
                    } else {
                        let items = preview
                            .into_iter()
                            .map(|(id, a)| (id, convert::affine_to_core(a)))
                            .collect();
                        let _ = self.doc.editor.execute(Command::SetTransforms { items });
                    }
                }
                self.request_main_redraw();
            }
            Drag::Scale {
                start_xf, preview, ..
            } => {
                if preview != start_xf {
                    // Text objects bake a uniform scale into their font
                    // size / box, so the point size actually changes (the
                    // rest just take the new transform).
                    self.commit_scaled(preview);
                    self.request_main_redraw();
                }
            }
            Drag::ResizeTextBox {
                handle,
                start_bounds,
                frames,
                start_doc,
                cur_doc,
            } => {
                if cur_doc != start_doc {
                    let rects = self.text_box_resize_rects(
                        handle,
                        start_bounds,
                        &frames,
                        start_doc,
                        cur_doc,
                    );
                    self.resize_text_boxes(&rects);
                }
            }
            Drag::Marquee { start } => {
                let r_screen = Rect::from_points(start, self.pointer);
                let r_doc = self
                    .doc.view
                    .to_screen()
                    .inverse()
                    .transform_rect_bbox(r_screen);
                let hits = match self.isolation_root() {
                    Some(root) => select::within_in(self.doc.editor.document(), root, r_doc),
                    None => select::within(self.doc.editor.document(), r_doc),
                };
                if self.shift_down {
                    for id in hits {
                        if !self.doc.selection.contains(&id) {
                            self.doc.selection.push(id);
                        }
                    }
                } else {
                    self.doc.selection = hits;
                }
                // Guides the band crosses join the selection too.
                if !self.guides_hidden && !self.guides_locked {
                    use amalith_core::GuideOrient;
                    let guide_hits: Vec<_> = self
                        .doc
                        .editor
                        .document()
                        .guides()
                        .iter()
                        .filter(|g| match g.orient {
                            GuideOrient::Horizontal => r_doc.y0 <= g.pos && g.pos <= r_doc.y1,
                            GuideOrient::Vertical => r_doc.x0 <= g.pos && g.pos <= r_doc.x1,
                        })
                        .map(|g| g.id)
                        .collect();
                    if !self.shift_down {
                        self.selected_guides.clear();
                    }
                    for id in guide_hits {
                        if !self.selected_guides.contains(&id) {
                            self.selected_guides.push(id);
                        }
                    }
                }
                self.sync_align_mode();
                self.marquee = None;
                self.request_main_redraw();
            }
            Drag::MoveAnchors {
                start_doc,
                last_doc,
                moved,
            } => {
                if moved && !self.doc.anchor_sel.is_empty() {
                    let delta = convert::vec2_to_core(last_doc - start_doc);
                    let _ = self.doc.editor.execute(Command::MoveAnchors {
                        anchors: self.doc.anchor_sel.clone(),
                        delta,
                    });
                    self.request_main_redraw();
                }
            }
            Drag::MoveHandle {
                object,
                anchor,
                side,
                start_doc,
                last_doc,
            } => {
                let delta = convert::vec2_to_core(last_doc - start_doc);
                if delta.x != 0.0 || delta.y != 0.0 {
                    let _ = self.doc.editor.execute(Command::MoveHandle {
                        object,
                        anchor,
                        side,
                        delta,
                    });
                    self.request_main_redraw();
                }
            }
            Drag::AnchorMarquee { start, candidate } => {
                let moved = (self.pointer - start).hypot() > 3.0;
                if moved {
                    // A real drag: rubber-band every node inside the box,
                    // across all paths — Illustrator's white-arrow marquee
                    // reaches objects that weren't selected first. The
                    // objects it catches then show their contour + nodes.
                    let r_doc = self
                        .doc.view
                        .to_screen()
                        .inverse()
                        .transform_rect_bbox(Rect::from_points(start, self.pointer));
                    let hits = anchors::within(self.doc.editor.document(), r_doc);
                    if self.shift_down {
                        for a in hits {
                            if !self.doc.anchor_sel.contains(&a) {
                                self.doc.anchor_sel.push(a);
                            }
                        }
                    } else {
                        self.doc.anchor_sel = hits;
                    }
                } else if let Some(id) = candidate {
                    // A click on an object: select it, revealing its nodes.
                    if self.shift_down {
                        if !self.doc.selection.contains(&id) {
                            self.doc.selection.push(id);
                        }
                    } else {
                        self.doc.selection = vec![id];
                    }
                    self.doc.anchor_sel.clear();
                } else if !self.shift_down {
                    // A click on empty canvas: clear everything.
                    self.doc.selection.clear();
                    self.doc.anchor_sel.clear();
                }
                self.marquee = None;
                self.request_main_redraw();
            }
            // Released before the drag threshold: a plain click. A Tabs
            // tab already activated at press; a Stack row opens its
            // flyout (⇐ `beginPanelPress`'s click branch).
            Drag::PendingPanelDrag { panel, .. } => {
                if let Some((m, g, i)) = self.dock.locate(panel) {
                    if self.dock.master(m).is_some_and(|mm| mm.layout == MasterLayout::Stack) {
                        self.toggle_stack_flyout(m, g, i);
                    }
                }
                self.request_main_redraw();
            }
            Drag::MovingMaster { master, .. } => {
                let dock = self.master_dock_preview.take();
                let group_drop = self.group_drop_preview.take();
                if let Some((target, drop)) = group_drop {
                    // A single-Group Master behaves exactly like dragging
                    // that Group directly (merge its panels into an
                    // existing group's tabs, or land as a new sibling at
                    // a position); a multi-Group Master has no single
                    // group whose panels it'd make sense to merge into,
                    // so its whole group list moves as one contiguous
                    // block to that same position instead (⇐ the
                    // reference's `mergeMasters` with a live placeholder
                    // position — it doesn't special-case a multi-group
                    // source either).
                    let single_group = self.dock.master(master).is_some_and(|m| m.groups.len() == 1);
                    let moved = if single_group {
                        match drop {
                            GroupDrop::MergeInto { group } => {
                                self.dock.merge_groups((master, 0), (target, group), usize::MAX)
                            }
                            GroupDrop::NewSibling { at } => self.dock.move_group((master, 0), target, at),
                        }
                    } else {
                        let at = match drop {
                            GroupDrop::MergeInto { group } => group,
                            GroupDrop::NewSibling { at } => at,
                        };
                        self.dock.merge_masters(master, target, at)
                    };
                    if moved {
                        if let Some((wid, _)) =
                            self.hosts.iter().find(|(_, h)| matches!(h.role, Role::Floating(f) if f == master))
                        {
                            let wid = *wid;
                            self.hosts.remove(&wid); // Arc<Window> drops -> closes
                        }
                        if let Some(w) = self.floating_window(target) {
                            w.request_redraw();
                        }
                    }
                } else if let Some((side, index)) = dock {
                    self.dock.dock_master(master, side, index);
                    if let Some((wid, _)) =
                        self.hosts.iter().find(|(_, h)| matches!(h.role, Role::Floating(f) if f == master))
                    {
                        let wid = *wid;
                        self.hosts.remove(&wid); // Arc<Window> drops -> closes
                    }
                }
                self.reap_closed_floating_windows();
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if let Some(m) = &self.native_menu {
                    m.sync_window(&self.dock);
                }
                self.request_main_redraw();
            }
            Drag::DraggingGroup { current, .. } => {
                match self.group_drop_preview.take() {
                    Some((target, GroupDrop::MergeInto { group })) => {
                        self.dock.merge_groups(current, (target, group), usize::MAX);
                    }
                    Some((target, GroupDrop::NewSibling { at })) => {
                        self.dock.move_group(current, target, at);
                    }
                    // No target under the cursor: it's already its own
                    // live Master window (⇐ `detach_group_live`), so
                    // there's nothing left to do — it just stays floating
                    // right where it was released.
                    None => {}
                }
                // The source Master may have just emptied out (its last
                // Group left) — if it was floating, its window is now
                // orphaned and needs closing.
                self.reap_closed_floating_windows();
                self.request_main_redraw();
            }
            Drag::DraggingPanel { panel, .. } => {
                match self.panel_drop_preview.take() {
                    Some((target, PanelDrop::IntoGroup { group, at })) => {
                        self.dock.move_panel_into_group(panel, (target, group), at);
                    }
                    Some((target, PanelDrop::NewGroup { at })) => {
                        self.dock.move_panel_new_group(panel, target, at);
                    }
                    None => {}
                }
                // Same as above: the panel's old Master may have just
                // emptied out from under it.
                self.reap_closed_floating_windows();
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if let Some(m) = &self.native_menu {
                    m.sync_window(&self.dock);
                }
                self.request_main_redraw();
            }
        }
        self.update_canvas_cursor();
    }
}
