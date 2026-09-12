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
        // Home screen: hovering a tile / Open / Import highlights it.
        if let Some(hm) = &mut self.home {
            if hm.on_move(self.pointer.to_vec2()) {
                self.request_main_redraw();
            }
            return;
        }
        self.update_canvas_cursor();
        self.refresh_tooltip();
        self.refresh_smart_guides();
        // Redraw so any painted cursor glyph tracks the pointer — this
        // covers the scale / rotate / loaded-text glyphs, not just Glyph.
        // The Rotate tool shows path nodes but keeps the OS crosshair, so
        // it also needs a per-move repaint for the node hover-swell.
        if self.cursor_mode.is_drawn()
            || self.ctx_menu.is_some()
            || self.smart_guide_hit.is_some()
            || self.sg_hovered_path.is_some()
            || self.active_tool == Tool::Eraser
            || (matches!(self.active_tool, Tool::Rotate | Tool::Reflect | Tool::Shear | Tool::Scale)
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
                        (self.pointer.x - offset.x).clamp(4.0, (w - picker::metric_w() - 4.0).max(4.0)),
                        (self.pointer.y - offset.y).clamp(4.0, (h - picker::metric_h() - 4.0).max(4.0)),
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
                    layout::metric_tools_min_w() as f32
                } else {
                    layout::metric_master_min_w() as f32
                };
                let clamped = raw.clamp(min_w, layout::metric_master_max_w() as f32);
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
                    .clamp(crate::dock::metric_tab_content_min_h(), crate::dock::metric_tab_content_max_h());
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
                let raw_dp = self.doc_point(self.pointer);
                // Smart Guides: snap the whole selection's bounding box to
                // nearby objects' edges/centers before this becomes the
                // live position — affects both the preview and the commit,
                // since both derive from `last_doc`.
                let (dp, sg_hit) = match select::union_bounds(self.doc.editor.document(), &self.doc.selection) {
                    Some(bounds) => {
                        let (delta, hit) = if self.shift_down { (snap8(raw_dp-start_doc),None) } else { self.sg_move_snap(raw_dp - start_doc, bounds, &self.doc.selection) };
                        (start_doc + delta, hit)
                    }
                    None => (raw_dp, None),
                };
                self.smart_guide_hit = sg_hit;
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
                // Anchor-level, not object-level: dragging one anchor of a
                // path can still snap onto a *sibling* anchor of the same
                // path (or its own center/other segments) — only the
                // anchors actually being dragged are excluded from
                // attracting themselves.
                let exclude: Vec<(ObjectId, usize)> = self.doc.anchor_sel.clone();
                let raw = self.doc_point(self.pointer);
                let origin = self.doc.anchor_sel.iter().flat_map(|&(id,n)| anchors::anchors_of(self.doc.editor.document(), id).into_iter().filter(move |(i,_)| *i == n).map(|(_,p)| p))
                    .min_by(|a,b| (*a-start_doc).hypot2().total_cmp(&(*b-start_doc).hypot2())).unwrap_or(start_doc);
                let proposed = origin + (raw-start_doc);
                let (snapped, hit) = if self.shift_down { (origin+snap8(raw-start_doc),None) } else { self.sg_point_snap(proposed, &exclude) };
                let dp = start_doc + (snapped-origin);
                self.smart_guide_hit = hit;
                self.drag = Drag::MoveAnchors {
                    start_doc,
                    last_doc: dp,
                    moved: true,
                };
                self.request_main_redraw();
            }
            Drag::MoveHandle { .. } => {
                self.recompute_move_handle();
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
            Drag::XformDialAngle { field, center } => {
                let (field, center) = (*field, *center);
                let deg = xformdlg::angle_at(center, self.pointer);
                if let Some(dlg) = self.xform_dialog.as_mut() {
                    dlg.set_dial_angle(field, deg);
                }
                self.apply_xform_preview();
                self.request_main_redraw();
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
                    self.panel_scroll_of(PanelId(PanelKind::Layers)),
                    &ids,
                )
                .map(|d| (d.parent, d.index, d.row, d.into));
                self.request_main_redraw();
            }
            Drag::AppearanceDrag { body, press, moved } => {
                let (body, press, was_moved) = (*body, *press, *moved);
                let far = (self.pointer - press).hypot() > 4.0;
                if !was_moved && !far {
                    return;
                }
                self.drag = Drag::AppearanceDrag { body, press, moved: true };
                let count = self.appearance_items().len();
                self.appearance_drop = crate::panels::appearance::drop_target(body, self.pointer, count)
                    .map(|(row, _)| row);
                self.request_main_redraw();
            }
            Drag::DrawShape {
                tool, start_doc, ..
            } => {
                let (tool, start_doc) = (*tool, *start_doc);
                let raw=self.doc_point(self.pointer);
                let (end,hit)=if self.shift_down { (raw,None) } else { self.sg_point_snap(raw,&[]) };
                self.smart_guide_hit=hit;
                self.drag = Drag::DrawShape {
                    tool,
                    start_doc,
                    cur_doc: end,
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
                let raw = self.doc_point(self.pointer);
                let (delta, hit) = if self.shift_down {
                    (snap8(raw - start_doc), None)
                } else {
                    let bounds = self
                        .doc.editor
                        .document()
                        .artboard(id)
                        .map(|a| convert::rect(a.rect))
                        .unwrap_or_default();
                    self.sg_artboard_move_snap(raw - start_doc, bounds, id)
                };
                self.smart_guide_hit = hit;
                self.drag = Drag::MoveArtboard {
                    id,
                    start_doc,
                    last_doc: start_doc + delta,
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
                let raw = self.doc_point(self.pointer);
                let dp = if self.shift_down {
                    self.smart_guide_hit = None;
                    raw
                } else {
                    // `cur_doc - start_doc` is a *relative* delta added to
                    // `start_rect`'s edge (the press point needn't land
                    // exactly on the handle's pixel) — unlike Scale's own
                    // `pointer`, which *is* the corner directly. Recover
                    // the actual candidate edge point first, snap that,
                    // then translate the snap back into the same
                    // `cur_doc` space `resize_rect` expects.
                    let sr = convert::rect(start_rect);
                    let left = matches!(handle, Handle::Nw | Handle::W | Handle::Sw);
                    let right = matches!(handle, Handle::Ne | Handle::E | Handle::Se);
                    let top = matches!(handle, Handle::Nw | Handle::N | Handle::Ne);
                    let bottom = matches!(handle, Handle::Sw | Handle::S | Handle::Se);
                    let edge_x = if left { sr.x0 } else if right { sr.x1 } else { raw.x };
                    let edge_y = if top { sr.y0 } else if bottom { sr.y1 } else { raw.y };
                    let raw_edge = Point::new(edge_x + (raw.x - start_doc.x), edge_y + (raw.y - start_doc.y));
                    let (snapped_edge, hit) = self.sg_artboard_resize_snap(handle, raw_edge, sr, id);
                    self.smart_guide_hit = hit;
                    raw + (snapped_edge - raw_edge)
                };
                self.drag = Drag::ResizeArtboard {
                    id,
                    handle,
                    start_rect,
                    start_doc,
                    cur_doc: dp,
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
                let raw = self.doc_point(self.pointer);
                let exclude: Vec<ObjectId> = start_xf.keys().copied().collect();
                let (dp, hit) = self.sg_scale_snap(handle, raw, start_bounds, &exclude, &[]);
                self.smart_guide_hit = hit;
                // Free Transform's Constrain toggle forces the same
                // uniform-scale behavior Shift gives every other tool.
                let uniform = self.shift_down
                    || (self.active_tool == Tool::FreeTransform && self.free_transform_constrain);
                let m = handles::scaled_transform(start_bounds, handle, dp, uniform, self.alt_down);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                self.drag = Drag::Scale {
                    handle,
                    start_bounds,
                    start_xf,
                    preview,
                };
                self.request_main_redraw();
            }
            Drag::Warp { .. } => self.update_free_transform_drag(),
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
            Drag::PathTextBracket { object, edit } => {
                let object = *object;
                let mut edit = edit.clone();
                let doc = self.doc.editor.document();
                if let Some((arc, _)) = pathtext::resolve(doc, object, &edit.original) {
                    edit.update(&arc, pathtext::to_path_local(doc, object, self.doc_point(self.pointer)));
                }
                self.drag = Drag::PathTextBracket { object, edit };
                self.request_main_redraw();
            }
            Drag::WidthPoint { object, points, index, part } => {
                let object = *object;
                let mut points = points.clone();
                let (index, part) = (*index, *part);
                self.width_tool_move(object, &mut points, index, part);
                self.drag = Drag::WidthPoint { object, points, index, part };
                self.request_main_redraw();
            }
            Drag::JoinScrub { from, path, .. } => {
                let from = *from;
                let mut path = path.clone();
                path.push(self.doc_point(self.pointer));
                let target = self.join_tool_move(from);
                self.drag = Drag::JoinScrub { from, path, target };
                self.request_main_redraw();
            }
            Drag::ShapeBuilderDrag { erase, touched } => {
                let erase = *erase;
                let mut touched = touched.clone();
                self.shape_builder_move(&mut touched);
                self.drag = Drag::ShapeBuilderDrag { erase, touched };
                self.request_main_redraw();
            }
            Drag::EraserStroke { path } => {
                let mut path = path.clone();
                self.eraser_move(&mut path);
                self.drag = Drag::EraserStroke { path };
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
                let raw = self.doc_point(self.pointer);
                let constrained = self.shift_down || (self.active_tool == Tool::FreeTransform && self.free_transform_constrain);
                // Shift's fixed 45° lock wins outright; otherwise the
                // user's own construction-angle list gets first say, and
                // the plain pre-drag reference line is the fallback when
                // neither applies — the same priority Pen's construction
                // guides already use (an exact match beats a generic cue).
                let (dp, angle_hit) = if constrained {
                    (raw, None)
                } else {
                    let exclude: Vec<ObjectId> = start_xf.keys().copied().collect();
                    self.sg_construction_snap_with_object_angles(center, raw, &exclude)
                };
                let m = handles::rotate_transform(center, start_angle, dp, constrained);
                self.smart_guide_hit = angle_hit.or_else(|| {
                    (self.settings.smart_guides_enabled && self.settings.sg_transform_tools)
                        .then_some(smart_guides::SmartGuideHit::TransformReference { center, angle: start_angle })
                });
                let preview = start_xf.iter().map(|(&id, &s)| {
                    let parent = if self.active_tool == Tool::FreeTransform {
                        let doc = self.doc.editor.document();
                        match doc.object(id).map(|o|o.parent) {
                            Some(amalith_core::ObjectParent::Group(id)) => convert::affine(doc.world_transform(id)),
                            _ => ID,
                        }
                    } else { ID };
                    (id, parent.inverse()*m*parent*s)
                }).collect();
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
                moved,
                ..
            } => {
                let (pivot, start_angle, was_moved) = (*pivot, *start_angle, *moved);
                // Read live, not the value captured at press — Alt can be
                // pressed or released mid-drag, same as Illustrator.
                let copy = self.alt_down;
                let start_xf = start_xf.clone();
                let raw = self.doc_point(self.pointer);
                let (dp, angle_hit) = if self.shift_down {
                    (raw, None)
                } else {
                    let exclude: Vec<ObjectId> = start_xf.keys().copied().collect();
                    self.sg_construction_snap_with_object_angles(pivot, raw, &exclude)
                };
                self.smart_guide_hit = angle_hit.or_else(|| {
                    (self.settings.smart_guides_enabled && self.settings.sg_transform_tools)
                        .then_some(smart_guides::SmartGuideHit::TransformReference { center: pivot, angle: start_angle })
                });
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
            Drag::ReflectTool {
                pivot,
                press,
                start_xf,
                moved,
                ..
            } => {
                let (pivot, press, was_moved) = (*pivot, *press, *moved);
                let copy = self.alt_down;
                let start_xf = start_xf.clone();
                let raw = self.doc_point(self.pointer);
                let (dp, angle_hit) = if self.shift_down {
                    (raw, None)
                } else {
                    let exclude: Vec<ObjectId> = start_xf.keys().copied().collect();
                    self.sg_construction_snap_with_object_angles(pivot, raw, &exclude)
                };
                self.smart_guide_hit = angle_hit;
                let mut axis_deg = handles::angle_to(pivot, dp).to_degrees();
                if self.shift_down {
                    axis_deg = (axis_deg / 45.0).round() * 45.0;
                }
                let m = handles::reflect_transform(pivot, axis_deg);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                let moved = was_moved || (self.pointer - press).hypot() > metric_drag_threshold();
                self.drag = Drag::ReflectTool { pivot, press, start_xf, preview, copy, moved };
                self.update_canvas_cursor();
                self.request_main_redraw();
            }
            Drag::ShearTool {
                pivot,
                press,
                start_xf,
                moved,
                ..
            } => {
                let (pivot, press, was_moved) = (*pivot, *press, *moved);
                let copy = self.alt_down;
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                // Bounded by |run|, not a raw full-circle angle_to: the
                // shear angle must stay in (-90°, 90°) — the underlying
                // tan() blows up and flips sign right at 90°, which is
                // exactly what a signed run would hit as the drag crosses
                // above/below the pivot.
                let v = dp - pivot;
                let mut shear_deg = v.y.atan2(v.x.abs()).to_degrees();
                if self.shift_down {
                    shear_deg = (shear_deg / 45.0).round() * 45.0;
                    self.smart_guide_hit = None;
                } else {
                    let (snapped, hit) = self.sg_shear_snap(pivot, shear_deg);
                    shear_deg = snapped;
                    self.smart_guide_hit = hit;
                }
                shear_deg = shear_deg.clamp(-89.0, 89.0);
                let m = handles::shear_transform(pivot, shear_deg, 0.0);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                let moved = was_moved || (self.pointer - press).hypot() > metric_drag_threshold();
                self.drag = Drag::ShearTool { pivot, press, start_xf, preview, copy, moved };
                self.update_canvas_cursor();
                self.request_main_redraw();
            }
            Drag::ScaleTool {
                pivot,
                press,
                start_xf,
                moved,
                ..
            } => {
                let (pivot, press, was_moved) = (*pivot, *press, *moved);
                let copy = self.alt_down;
                let start_xf = start_xf.clone();
                let dp = self.doc_point(self.pointer);
                let press_doc = self.doc_point(press);
                let eps = 4.0 / self.doc.view.zoom;
                let m = handles::scale_tool_transform(pivot, press_doc, dp, self.shift_down, eps);
                let preview = start_xf.iter().map(|(id, s)| (*id, m * *s)).collect();
                let moved = was_moved || (self.pointer - press).hypot() > metric_drag_threshold();
                self.drag = Drag::ScaleTool { pivot, press, start_xf, preview, copy, moved };
                self.update_canvas_cursor();
                self.request_main_redraw();
            }
            Drag::PendingMasterMove { master, press, grab, was_docked } => {
                if (self.pointer - *press).hypot() > metric_drag_threshold() {
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
                if (self.pointer - *press).hypot() > metric_drag_threshold() {
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
                if (self.pointer - *press).hypot() > metric_drag_threshold() {
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
        // Only catch up a Free Transform gesture that never got a single
        // pointer-move event (a very fast click-drag-release can coalesce
        // away every intermediate move on some platforms) — `dst_quad`
        // still equalling `src_quad` is exactly that "never updated" state.
        // If a move already ran, re-running this here would re-evaluate
        // live modifier-key state fresh at this exact instant, which can
        // differ from whatever was true during the last frame actually
        // painted — silently committing a different mode/quad than what
        // the user was just looking at. Trust the last-rendered state
        // instead: what you saw is what commits.
        if matches!(&self.drag, Drag::Warp { src_quad, dst_quad, .. } if src_quad == dst_quad) {
            self.update_free_transform_drag();
        }
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
        // Same, for a quick tap on a flyout-group slot.
        if let Some((_, _, group)) = self.tool_flyout_press.take() {
            if self.tool_flyout.is_none() {
                let t = match group {
                    ToolGroup::RotateReflect => self.last_rotate_tool,
                    ToolGroup::ScaleShear => self.last_scale_tool,
                    ToolGroup::Type => self.last_type_tool,
                };
                self.set_tool(t);
            }
        }
        self.smart_guide_hit = None;
        self.sg_hovered_path = None;
        self.request_main_redraw();
        match std::mem::take(&mut self.drag) {
            Drag::None
            | Drag::MasterWidth { .. }
            | Drag::GroupContentResize { .. }
            | Drag::PendingGroupDrag { .. }
            | Drag::PendingMasterMove { .. }
            | Drag::XformDialAngle { .. }
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
                if self.pointer.y > bar.y1 + crate::panels::gradient::metric_remove_drop() {
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
                        self.panel_scroll_of(PanelId(PanelKind::Layers)),
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
            Drag::AppearanceDrag { body, moved, .. } => {
                if moved {
                    let count = self.appearance_items().len();
                    if let Some((_, real_index)) = crate::panels::appearance::drop_target(body, self.pointer, count) {
                        // Alt held at drop time (not press time — matches
                        // `Drag::MoveObjects`'s own `dup: self.alt_down`,
                        // read live so toggling Alt mid-drag still works)
                        // drops a duplicate instead of moving the row.
                        if self.alt_down {
                            self.appearance_duplicate_to(real_index);
                        } else {
                            self.appearance_reorder(real_index);
                        }
                    }
                }
                self.appearance_drop = None;
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
                    self.record_transform_again(amalith_core::Affine::translate(delta), self.alt_down);
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
                if tool.has_exact_size_dialog()
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
                        Tool::RoundedRect | Tool::Polygon | Tool::Star | Tool::Arc | Tool::Spiral => {
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
                        | Tool::Rotate
                        | Tool::Reflect
                        | Tool::Shear
                        | Tool::Scale
                        | Tool::Blend
                        | Tool::Width
                        | Tool::FreeTransform
                        | Tool::Join
                        | Tool::ShapeBuilder
                        | Tool::Eraser
                        | Tool::VerticalText
                        | Tool::AreaType
                        | Tool::PathType
                        | Tool::VerticalAreaType
                        | Tool::VerticalPathType => return,
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
                // Shift temporarily swaps to this tool's vertical (or
                // horizontal) sibling — see `effective_tool`.
                let effective = self.effective_tool();
                let vertical = matches!(effective, Tool::VerticalText | Tool::VerticalAreaType);
                let forced_area = matches!(effective, Tool::AreaType | Tool::VerticalAreaType);
                let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                if forced_area || (r.width() > 4.0 && r.height() > 4.0) {
                    // A real drag → area / paragraph type. The dragged
                    // rectangle is the text box: fixed width and height,
                    // text wraps inside it and overflows past the bottom.
                    // Area Type / Vertical Area Type always create a box,
                    // even from a plain click — a default-size one, same
                    // as Illustrator's own Area Type Tool.
                    let (w, h) = if r.width() > 4.0 && r.height() > 4.0 {
                        (r.width(), r.height())
                    } else {
                        (AREA_TYPE_DEFAULT_W, AREA_TYPE_DEFAULT_H)
                    };
                    // A vertical box anchors at its top-RIGHT corner —
                    // column 0 starts there and further columns extend
                    // left, mirroring how a horizontal box anchors
                    // top-left and lines extend right/down.
                    let origin = if vertical { Point::new(r.x0 + w, r.y0) } else { Point::new(r.x0, r.y0) };
                    self.create_text(
                        amalith_core::TextKind::Area {
                            width: w,
                            height: Some(h),
                        },
                        origin,
                        vertical,
                    );
                } else {
                    // A click → point type.
                    self.create_text(amalith_core::TextKind::Point, start_doc, vertical);
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
                    let sample = preview.keys().next().copied();
                    let delta = sample.and_then(|id| self.preview_delta(&preview, id));
                    let items = preview
                        .into_iter()
                        .map(|(id, a)| (id, convert::affine_to_core(a)))
                        .collect();
                    let _ = self.doc.editor.execute(Command::SetTransforms { items });
                    if let Some(delta) = delta {
                        self.record_transform_again(delta, false);
                    }
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
                    // A click, not a drag: re-place the reference point,
                    // snapping it onto a nearby anchor/center/guide the
                    // same way any other point placement does.
                    self.transform_pivot = Some(self.sg_point_snap(self.doc_point(self.pointer), &[]).0);
                } else if preview != start_xf {
                    let sample = self.doc.selection.first().copied();
                    let delta = sample.and_then(|id| self.preview_delta(&preview, id));
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
                    if let Some(delta) = delta {
                        self.record_transform_again(delta, copy);
                    }
                }
                self.request_main_redraw();
            }
            Drag::ReflectTool { start_xf, preview, copy, moved, .. }
            | Drag::ShearTool { start_xf, preview, copy, moved, .. }
            | Drag::ScaleTool { start_xf, preview, copy, moved, .. } => {
                if !moved {
                    // A click, not a drag: re-place the reference point.
                    self.transform_pivot = Some(self.sg_point_snap(self.doc_point(self.pointer), &[]).0);
                } else if preview != start_xf {
                    let sample = self.doc.selection.first().copied();
                    let delta = sample.and_then(|id| self.preview_delta(&preview, id));
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
                    if let Some(delta) = delta {
                        self.record_transform_again(delta, copy);
                    }
                }
                self.request_main_redraw();
            }
            Drag::Scale {
                start_xf, preview, ..
            } => {
                if preview != start_xf {
                    let sample = self.doc.selection.first().copied();
                    let delta = sample.and_then(|id| self.preview_delta(&preview, id));
                    // Text objects bake a uniform scale into their font
                    // size / box, so the point size actually changes (the
                    // rest just take the new transform).
                    self.commit_scaled(preview);
                    if let Some(delta) = delta {
                        self.record_transform_again(delta, false);
                    }
                    self.request_main_redraw();
                }
            }
            Drag::Warp { src_quad, dst_quad, objects, start_xf, preview, warping, .. } => {
                if warping {
                    self.commit_warp(src_quad, dst_quad, objects);
                } else if start_xf != preview {
                    let items = preview.into_iter().map(|(id,m)| (id,convert::affine_to_core(m))).collect();
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
            Drag::PathTextBracket { object, mut edit } => {
                let doc = self.doc.editor.document();
                if let Some((arc, _)) = pathtext::resolve(doc, object, &edit.original) {
                    edit.update(&arc, pathtext::to_path_local(doc, object, self.doc_point(self.pointer)));
                }
                self.commit_path_text_bracket(object, edit);
            }
            Drag::WidthPoint { object, mut points, index, part } => {
                self.width_tool_move(object, &mut points, index, part);
                self.commit_width_point(object, points);
            }
            Drag::JoinScrub { from, .. } => {
                let target = self.join_tool_move(from);
                self.commit_join(from, target);
            }
            Drag::ShapeBuilderDrag { erase, mut touched } => {
                self.shape_builder_move(&mut touched);
                self.commit_shape_builder(erase, touched);
            }
            Drag::EraserStroke { mut path } => {
                self.eraser_move(&mut path);
                self.commit_eraser(path);
            }
            Drag::Marquee { start } => {
                let r_screen = Rect::from_points(start, self.pointer);
                let r_doc = self
                    .doc.view
                    .to_screen()
                    .inverse()
                    .transform_rect_bbox(r_screen);
                // A click must not run bounds-based marquee selection:
                // even a zero-area box can overlap a diagonal path's bounds.
                let hits = if (self.pointer - start).hypot() > 3.0 {
                    match self.isolation_root() {
                        Some(root) => select::within_in(self.doc.editor.document(), root, r_doc),
                        None => select::within(self.doc.editor.document(), r_doc),
                    }
                } else {
                    Vec::new()
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
                    let d = last_doc - start_doc;
                    let delta = convert::vec2_to_core(if self.shift_down { snap8(d) } else { d });
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
                        break_mirror: self.alt_down,
                    });
                    self.request_main_redraw();
                }
            }
            Drag::AnchorMarquee { start } => {
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
