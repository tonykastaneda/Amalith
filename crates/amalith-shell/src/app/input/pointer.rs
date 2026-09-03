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
        self.update_canvas_cursor();
        self.refresh_tooltip();
        // Redraw so any painted cursor glyph tracks the pointer — this
        // covers the scale / rotate / loaded-text glyphs, not just Glyph.
        if self.cursor_mode.is_drawn() {
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
            Drag::RailWidth { side } => {
                let side = *side;
                let Some((w, _)) = self.main_logical_size() else {
                    return;
                };
                let raw = match side {
                    RailSide::Left => self.pointer.x,
                    RailSide::Right => w - self.pointer.x,
                };
                let clamped = raw.clamp(RAIL_MIN_W, (w * 0.7).max(RAIL_MIN_W));
                let gap = self.theme.splitter_thickness as f32;
                self.dock.rail_mut(side).set_width_absorbing(
                    clamped as f32,
                    gap,
                    matches!(side, RailSide::Left),
                );
                self.request_main_redraw();
            }
            Drag::Splitter { side, path, gap } => {
                let (side, path, gap) = (*side, path.clone(), *gap);
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };
                let rect = rail_rect_for(side, self.dock.rail(side).width as f64, w, h);
                let laid =
                    build_rail_layout(self.dock.rail(side), &self.theme, &mut self.text, rect);
                if let Some(sp) = laid
                    .splitters
                    .iter()
                    .find(|s| s.path == path && s.index == gap)
                {
                    let mut frac = sp.frac_at(self.pointer);
                    // A vertical drag must not crush either neighbour below
                    // its active panel's own content height.
                    if sp.axis == Axis::Vertical {
                        if let Some(Node::Split { children, .. }) =
                            self.dock.rail(side).node_at(&path)
                        {
                            if let (Some(a), Some(b)) =
                                (children.get(gap), children.get(gap + 1))
                            {
                                let strip_h = self.theme.tab_strip_h;
                                let g = self.theme.splitter_thickness;
                                let w = sp.before.width();
                                let min_a =
                                    a.node.min_height(w, strip_h, g, &panels::rail_floor);
                                let min_b =
                                    b.node.min_height(w, strip_h, g, &panels::rail_floor);
                                let span = sp.after.y1 - sp.before.y0;
                                if span > 0.0 {
                                    let lo = (min_a / span) as f32;
                                    let hi = 1.0 - (min_b / span) as f32;
                                    frac = if lo <= hi {
                                        frac.clamp(lo, hi)
                                    } else {
                                        (lo + hi) * 0.5
                                    };
                                }
                            }
                        }
                    }
                    self.dock.rail_mut(side).set_boundary(&path, gap, frac);
                    self.request_main_redraw();
                }
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
            Drag::PendingTearoff { panel, press, .. } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let (panel, press) = (*panel, *press);
                    // event_loop isn't handed to cursor events; defer the
                    // actual spawn to the caller via a queued request.
                    self.pending_tearoff = Some((panel, press));
                }
            }
            Drag::PendingFloatMove { id, press, .. } => {
                if (self.pointer - *press).hypot() > DRAG_THRESHOLD {
                    let id = *id;
                    let pos = self
                        .dock
                        .floating(id)
                        .map(|f| Point::new(f.rect[0] as f64, f.rect[1] as f64))
                        .unwrap_or(Point::ZERO);
                    self.drag = Drag::MovingFloating {
                        id,
                        grab: press.to_vec2(),
                        pos,
                    };
                }
            }
            _ => {}
        }

        // Move a floating window by locking it to the cursor: the cursor's
        // position *inside* the window, versus where it was grabbed, is the
        // move. Both come from the same `position / scale`, so there is no
        // scale or acceleration mismatch, and nothing reads the OS window
        // rect back — so it can't drift or jitter.
        if let Drag::MovingFloating { id, grab, pos } = self.drag {
            let float_wid = self.floating_window(id).map(|w| w.id());
            let global = if self.pointer_win == float_wid {
                pos + self.pointer.to_vec2()
            } else if self.pointer_win == self.main_id {
                self.main_inner_origin() + self.pointer.to_vec2()
            } else {
                return;
            };
            let new_pos = global - grab;
            self.drag = Drag::MovingFloating {
                id,
                grab,
                pos: new_pos,
            };
            if let Some(w) = self.floating_window(id) {
                w.set_outer_position(LogicalPosition::new(new_pos.x, new_pos.y));
                w.request_redraw();
            }
            // The colour picker is a free-floating dialog: never offer a
            // re-dock target, so dragging it off the document window just
            // moves it.
            if self.dock.floating_id_of(PanelId("picker")) != Some(id) {
                self.redock_preview = Some(self.resolve_redock(global));
            } else {
                self.redock_preview = None;
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
            | Drag::Splitter { .. }
            | Drag::RailWidth { .. }
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
                        | Tool::Zoom => return,
                    };
                    if let Ok(CommandOutcome::Object(id)) = self.doc.editor.execute(cmd) {
                        self.doc.selection = vec![id];
                        self.apply_new_appearance(id);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::TextSelect => {}
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
            Drag::Scale {
                start_xf, preview, ..
            }
            | Drag::Rotate {
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
                let hits = select::within(self.doc.editor.document(), r_doc);
                if self.shift_down {
                    for id in hits {
                        if !self.doc.selection.contains(&id) {
                            self.doc.selection.push(id);
                        }
                    }
                } else {
                    self.doc.selection = hits;
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
            Drag::PendingTearoff {
                side, path, tab, ..
            } => {
                self.dock.rail_mut(side).activate_tab(&path, tab);
                self.request_main_redraw();
            }
            Drag::PendingFloatMove { id, tab, .. } => {
                if let Some(f) = self.dock.floating_mut(id) {
                    if let Node::Tabs { active, panels } = &mut f.node {
                        *active = tab.min(panels.len().saturating_sub(1));
                    }
                }
                if let Some(w) = self.floating_window(id) {
                    w.request_redraw();
                }
            }
            Drag::MovingFloating { id, grab, pos } => {
                let global_cursor = pos + grab;
                let picker_float = self.dock.floating_id_of(PanelId("picker")) == Some(id);
                let (side, target) = if picker_float {
                    (RailSide::Right, DropTarget::Float)
                } else {
                    self.resolve_redock(global_cursor)
                };
                if matches!(target, DropTarget::Float) {
                    if let Some(f) = self.dock.floating_mut(id) {
                        // Keep the size the window currently has.
                        let [_, _, w, h] = f.rect;
                        f.rect = [pos.x as f32, pos.y as f32, w, h];
                    }
                    if let Some(w) = self.floating_window(id) {
                        w.request_redraw();
                    }
                } else {
                    self.dock.redock(id, side, target);
                    if let Some((wid, _)) = self
                        .hosts
                        .iter()
                        .find(|(_, h)| matches!(h.role, Role::Floating(f) if f == id))
                    {
                        let wid = *wid;
                        self.hosts.remove(&wid); // Arc<Window> drops -> closes
                    }
                }
                self.redock_preview = None;
                self.request_main_redraw();
            }
        }
        self.update_canvas_cursor();
    }
}
