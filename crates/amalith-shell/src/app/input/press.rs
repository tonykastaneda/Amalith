//! Press routing for the main and floating windows. `window_event`
//! delegates its left-mouse-down arm here. The guard order (modals →
//! chrome → rails → canvas) is load-bearing.

use super::super::*;

impl App {
    pub(in crate::app) fn on_press(&mut self, id: WindowId, double: bool) {
        let Some(role) = self.hosts.get(&id).map(|h| h.role) else {
            return;
        };
        // Any press ends the "⌘Z re-opens the last pen path" window.
        self.last_pen = None;
        self.tooltip = None;
        // A press anywhere blurs the Layers search field; a hit on the
        // field itself re-focuses it later in this handler.
        self.layer_search_focused = false;
        // An open Character-panel dropdown is topmost — it eats the press.
        if self.font_menu.is_some() && self.font_menu_click(self.pointer) {
            return;
        }
        // The Preferences modal.
        if self.prefs.is_some() {
            let hit = self.prefs.as_mut().unwrap().on_press(self.pointer);
            match hit {
                prefs::Hit::Backdrop | prefs::Hit::Cancel => self.prefs = None,
                prefs::Hit::Ok => {
                    self.settings = self.prefs.take().unwrap().working;
                    self.apply_theme_accent();
                    settings::save(&self.settings);
                }
                prefs::Hit::SetAccent(rgb) => {
                    if let Some(p) = &mut self.prefs {
                        p.working.accent = rgb;
                    }
                }
                prefs::Hit::StartRecording(i) => {
                    if let Some(p) = &mut self.prefs {
                        p.recording = Some(i);
                    }
                }
                prefs::Hit::ResetKeys => {
                    if let Some(p) = &mut self.prefs {
                        p.working.tool_keys = prefs::Settings::default().tool_keys;
                        p.recording = None;
                    }
                }
                prefs::Hit::Category(i) => {
                    if let Some(p) = &mut self.prefs {
                        p.category = i;
                    }
                }
                prefs::Hit::IncStep(v) => {
                    if let Some(p) = &mut self.prefs {
                        p.working.nudge_step = v;
                    }
                }
                prefs::Hit::ToggleTips => {
                    if let Some(p) = &mut self.prefs {
                        p.working.show_tooltips = !p.working.show_tooltips;
                    }
                }
                prefs::Hit::ToggleHome => {
                    if let Some(p) = &mut self.prefs {
                        p.working.home_on_last_close = !p.working.home_on_last_close;
                    }
                }
                prefs::Hit::None => {}
            }
            self.request_main_redraw();
            return;
        }
        // A press anywhere commits an in-progress rename (unless it's the
        // double-click that's about to start one).
        if self.doc.rename.is_some() && !double {
            self.commit_rename();
        }
        match role {
            Role::Main => {
                let Some((w, h)) = self.main_logical_size() else {
                    return;
                };

                // The About panel is modal — it captures every press.
                if self.about.is_some() {
                    let hit = self
                        .about
                        .as_mut()
                        .map(|a| a.on_press(&mut self.text, self.pointer.to_vec2()));
                    match hit {
                        Some(about::Hit::Backdrop) => self.close_about(),
                        Some(about::Hit::Github) => {
                            if let Some(a) = &self.about {
                                about::open_url(a.github_url());
                            }
                        }
                        Some(about::Hit::Toggle) => {
                            if let Some(a) = &mut self.about {
                                a.toggle();
                            }
                        }
                        _ => {}
                    }
                    self.request_main_redraw();
                    return;
                }

                // The New Document modal is, well, modal.
                if let Some(form) = &self.newdoc {
                    let lay = newdoc::layout(Rect::new(0.0, 0.0, w, h), form.scroll);
                    let hit = newdoc::hit(form, &lay, self.pointer);
                    self.apply_newdoc_hit(hit);
                    return;
                }

                // The Home screen captures every press.
                if self.home.is_some() {
                    let hit = self
                        .home
                        .as_ref()
                        .map(|hm| hm.on_press(self.pointer.to_vec2()));
                    match hit {
                        Some(home::Hit::NewDocument) => self.open_new_doc(),
                        Some(home::Hit::Youtube) => about::open_url(home::YOUTUBE_URL),
                        Some(home::Hit::Github) => about::open_url(home::GITHUB_URL),
                        Some(home::Hit::Recent(i)) => {
                            let path = self
                                .home
                                .as_ref()
                                .and_then(|hm| hm.recent_path(i))
                                .map(|p| p.to_path_buf());
                            if let Some(path) = path {
                                self.open_path(&path);
                            }
                        }
                        _ => {}
                    }
                    self.request_main_redraw();
                    return;
                }

                // The primitive flyout captures the next click: pick a
                // shape tool, or click away to dismiss.
                if let Some(anchor) = self.shape_flyout {
                    for (i, t) in panels::tools::SHAPE_TOOLS.iter().enumerate() {
                        if shape_flyout_cell(anchor, i).contains(self.pointer) {
                            self.last_shape_tool = *t;
                            self.set_tool(*t);
                            break;
                        }
                    }
                    self.shape_flyout = None;
                    self.shape_press = None;
                    self.request_main_redraw();
                    return;
                }

                // The Stroke flyout captures clicks while it's open.
                if self.stroke_popover {
                    let lay = self.stroke_flyout_layout(w);
                    let repr = self.stroke_style_repr();
                    match stroke_panel::hit(&lay, &repr, self.pointer) {
                        stroke_panel::Hit::Outside => {
                            self.stroke_popover = false;
                            self.request_main_redraw();
                            return;
                        }
                        hit => {
                            let dir = match hit {
                                stroke_panel::Hit::WeightStep(d)
                                | stroke_panel::Hit::LimitStep(d)
                                | stroke_panel::Hit::DashStep(d)
                                | stroke_panel::Hit::GapStep(d) => d,
                                _ => 0,
                            };
                            self.apply_stroke_flyout(hit, dir);
                            return;
                        }
                    }
                }

                // The app bar swallows clicks (unless the picker is up).
                if self.picker.is_none() && self.pointer.y < APP_BAR_H {
                    return;
                }

                // The context / control bar — one hit walk over its segments.
                if self.picker.is_none()
                    && self.pointer.y >= APP_BAR_H
                    && self.pointer.y < APP_BAR_H + OPT_BAR_H
                {
                    let action = {
                        let cx = self.context_bar_ctx();
                        context_bar::hit(opt_bar_rect(w), self.pointer, &cx)
                    };
                    if !matches!(action, panels::Action::None) {
                        self.apply_panel_action(action, double);
                    }
                    return;
                }

                // The document-tab strip (only across the canvas x-span —
                // the rails' own tab strips share this y band and must
                // still be reachable for panel tear-off).
                {
                    let (left_x, right_x) = self.canvas_x_span();
                    let strip = tab_bar_rect(left_x, right_x);
                    if self.picker.is_none() && strip.contains(self.pointer) {
                        let labels: Vec<String> =
                            (0..self.tabs.len()).map(|i| self.tab_label(i)).collect();
                        for (i, (whole, close)) in
                            layout_tabs(&mut self.text, &labels, strip).into_iter().enumerate()
                        {
                            if close.contains(self.pointer) {
                                self.close_tab(i);
                                return;
                            }
                            if whole.contains(self.pointer) {
                                self.switch_to(i);
                                return;
                            }
                        }
                        return;
                    }
                }

                // The colour picker is modal while open.
                if let Some(pk) = self.picker {
                    match picker::hit(&pk, self.pointer) {
                        picker::Hit::Sv(s, v) => {
                            if let Some(p) = &mut self.picker {
                                p.s = s;
                                p.v = v;
                            }
                            self.drag = Drag::PickColor { in_hue: false };
                        }
                        picker::Hit::Hue(hue) => {
                            if let Some(p) = &mut self.picker {
                                p.h = hue;
                            }
                            self.drag = Drag::PickColor { in_hue: true };
                        }
                        picker::Hit::NoneButton => {
                            if !self.doc.selection.is_empty() {
                                let objects = self.doc.selection.clone();
                                let paint = amalith_core::Paint::None;
                                let _ = self.doc.editor.execute(match pk.slot {
                                    panels::PaintSlot::Fill => Command::SetFill { objects, paint },
                                    panels::PaintSlot::Stroke => {
                                        Command::SetStroke { objects, paint }
                                    }
                                });
                            }
                            self.picker = None;
                        }
                        picker::Hit::Inside => {}
                        picker::Hit::Outside => {
                            self.apply_picker_color();
                            self.picker = None;
                        }
                    }
                    self.request_main_redraw();
                    return;
                }

                for side in [RailSide::Left, RailSide::Right] {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        continue;
                    }
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    // The rail's inner edge widens the whole rail — check it
                    // first, since its grab zone spills onto the canvas.
                    if rail_edge_bar(side, rect)
                        .inflate(GRAB_SLOP + 1.0, 0.0)
                        .contains(self.pointer)
                    {
                        self.drag = Drag::RailWidth { side };
                        return;
                    }
                    if !rect.contains(self.pointer) {
                        continue;
                    }
                    let laid = build_rail_layout(rail, &self.theme, &mut self.text, rect);
                    if let Some(sp) = laid
                        .splitters
                        .iter()
                        .find(|s| s.rect.inflate(GRAB_SLOP, GRAB_SLOP).contains(self.pointer))
                    {
                        self.drag = Drag::Splitter {
                            side,
                            path: sp.path.clone(),
                            gap: sp.index,
                        };
                        return;
                    }
                    for area in &laid.areas {
                        if area.tab_strip.contains(self.pointer) {
                            if let Some(tab) =
                                area.tabs.iter().position(|t| t.rect.contains(self.pointer))
                            {
                                let trect = area.tabs[tab].rect;
                                let pid = area.tabs[tab].panel;
                                if chrome::panel_tab_close_rect(trect).contains(self.pointer) {
                                    self.close_panel_tab(pid, None);
                                    return;
                                }
                                self.drag = Drag::PendingTearoff {
                                    side,
                                    panel: pid,
                                    path: area.path.clone(),
                                    tab,
                                    press: self.pointer,
                                };
                            }
                            return;
                        }
                        if area.body.contains(self.pointer) {
                            if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                                let rep = self.representative();
                                let action = {
                                    let ctx = panels::Ctx {
                                        theme: &self.theme,
                                        doc: self.doc.editor.document(),
                                        selection: &self.doc.selection,
                                        active_tool: self.active_tool,
                                        pointer: self.pointer,
                                        representative: rep,
                                        active_slot: self.active_slot,
                                        shape_tool: self.last_shape_tool,
                                        expanded: &self.doc.expanded_groups,
                                        renaming: self
                                            .doc.rename
                                            .as_ref()
                                            .map(|r| (r.target, r.buf.as_str())),
                                        selected_layer: self.doc.selected_layer,
                                        selected_artboard: self.doc.selected_artboard,
                                        text_style: self.active_text_style(),
                                        text_editing: self.text_edit.is_some(),
                                        font_families: &self.font_families,
                                        layer_query: &self.layer_query,
                                        layer_search_focused: self.layer_search_focused,
                                    };
                                    panels::hit(pid, area.body, self.pointer, &ctx)
                                };
                                if action == panels::Action::ShapeSlot {
                                    // Start a press: a hold opens the
                                    // flyout, a quick release re-picks
                                    // the last shape tool.
                                    let anchor =
                                        panels::tools::shape_slot_rect(area.body);
                                    self.shape_press = Some((Instant::now(), anchor));
                                } else {
                                    self.apply_panel_action(action, double);
                                }
                            }
                            return;
                        }
                    }
                    return;
                }
                // Not on a rail.
                if self.space_down && self.cmd_down {
                    // Illustrator scrubby zoom — anchored at the press.
                    self.drag = Drag::ScrubZoom {
                        anchor: self.pointer,
                        last: self.pointer,
                    };
                    self.update_canvas_cursor();
                    return;
                }
                if self.space_down {
                    self.drag = Drag::Pan { last: self.pointer };
                    self.update_canvas_cursor();
                    return;
                }
                let dp = self.doc_point(self.pointer);

                // Track the artboard being worked in (any tool) for
                // artboard-relative Paste in Front / Back. A press on the
                // pasteboard keeps the previous value.
                if let Some(id) = artboard_at(self.doc.editor.document(), dp) {
                    self.doc.current_artboard = Some(id);
                }

                // Type tool.
                if self.active_tool == Tool::Text {
                    if self.text_edit.is_some() {
                        // A press inside the open editor places the caret /
                        // starts a selection drag.
                        if let Some(p) = self.text_editor_point(self.pointer) {
                            if let Some(te) = &mut self.text_edit {
                                let clicks = if double { 2 } else { 1 };
                                te.pointer_down(p, clicks, &mut self.text);
                            }
                        }
                        self.drag = Drag::TextSelect;
                        self.request_main_redraw();
                        return;
                    }
                    let visible = self.visible_doc_rect();
                    if let Some(hit) = select::topmost_selectable_at(
                        self.doc.editor.document(),
                        dp,
                        visible,
                    ) {
                        if let Some(amalith_core::ObjectKind::Text(_)) =
                            self.doc.editor.document().object(hit).map(|o| &o.kind)
                        {
                            let c = self
                                .doc.editor
                                .document()
                                .object(hit)
                                .map(|o| o.transform.as_coeffs())
                                .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                            self.enter_text_edit(hit, Point::new(c[4], c[5]), Some(self.pointer));
                            self.drag = Drag::TextSelect;
                            return;
                        }
                    }
                    self.drag = Drag::DrawText {
                        start_doc: dp,
                        cur_doc: dp,
                    };
                    return;
                }

                // Pen: press to place an anchor, then drag to pull bezier
                // handles out of it. Click the first anchor to close.
                if self.active_tool == Tool::Pen {
                    let close_r = 8.0 / self.doc.view.zoom;
                    if self.pen.len() >= 3
                        && self
                            .pen
                            .first()
                            .is_some_and(|f| (f.point - dp).hypot() <= close_r)
                    {
                        self.commit_pen(true);
                        return;
                    }
                    let p = constrained(self.pen.last().map(|a| a.point), dp, self.shift_down);
                    self.pen.push(PenAnchor {
                        point: p,
                        handle_in: None,
                        handle_out: None,
                        mode: amalith_core::HandleMode::Corner,
                    });
                    self.pen_redo.clear();
                    self.drag = Drag::PenHandle {
                        anchor: self.pen.len() - 1,
                        from: p,
                    };
                    self.request_main_redraw();
                    return;
                }

                // A shape tool rubber-bands a new object.
                if self.active_tool.is_shape() {
                    self.drag = Drag::DrawShape {
                        tool: self.active_tool,
                        start_doc: dp,
                        cur_doc: dp,
                    };
                    return;
                }

                // Artboard tool: a resize handle of the selected artboard,
                // else drag an existing artboard, else rubber-band a new one.
                if self.active_tool == Tool::Artboard {
                    if let Some(id) = self.doc.selected_artboard {
                        if let Some(ab) = self
                            .doc.editor
                            .document()
                            .artboards()
                            .iter()
                            .find(|a| a.id == id)
                        {
                            let quad = handles::rect_quad(convert::rect(ab.rect))
                                .map(|p| self.doc.view.to_screen() * p);
                            if let Some(handle) = handles::hit_handle(self.pointer, quad) {
                                self.drag = Drag::ResizeArtboard {
                                    id,
                                    handle,
                                    start_rect: ab.rect,
                                    start_doc: dp,
                                    cur_doc: dp,
                                };
                                return;
                            }
                        }
                    }
                    match artboard_at(self.doc.editor.document(), dp) {
                        Some(id) => {
                            self.doc.selected_artboard = Some(id);
                            self.drag = Drag::MoveArtboard {
                                id,
                                start_doc: dp,
                                last_doc: dp,
                            };
                        }
                        None => {
                            self.doc.selected_artboard = None;
                            self.drag = Drag::DrawArtboard {
                                start_doc: dp,
                                cur_doc: dp,
                            };
                        }
                    }
                    self.request_main_redraw();
                    return;
                }

                // Direct Selection (Illustrator white arrow): nodes show
                // only for objects you've already picked. A click grabs a
                // node of such an object; otherwise it starts a marquee
                // that either selects the object under the press (if the
                // pointer never moves) or rubber-bands its nodes.
                if self.effective_tool() == Tool::DirectSelect {
                    let hit_r = 6.0 / self.doc.view.zoom;
                    let shown = self.node_paths();

                    // A bezier handle wins over everything — drag it to
                    // bend the curve.
                    if let Some((id, n, side)) =
                        anchors::handle_at(self.doc.editor.document(), &shown, dp, hit_r)
                    {
                        self.doc.anchor_sel = vec![(id, n)];
                        self.drag = Drag::MoveHandle {
                            object: id,
                            anchor: n,
                            side,
                            start_doc: dp,
                            last_doc: dp,
                        };
                        self.request_main_redraw();
                        return;
                    }

                    if let Some(a) =
                        anchors::topmost_anchor_among(self.doc.editor.document(), &shown, dp, hit_r)
                    {
                        // Alt-click an anchor toggles smooth / corner.
                        if self.alt_down {
                            let _ = self.doc.editor.execute(Command::ToggleAnchorSmooth {
                                object: a.0,
                                anchor: a.1,
                            });
                            self.doc.anchor_sel = vec![a];
                            self.request_main_redraw();
                            return;
                        }
                        if self.shift_down {
                            if let Some(i) = self.doc.anchor_sel.iter().position(|x| *x == a) {
                                self.doc.anchor_sel.remove(i);
                            } else {
                                self.doc.anchor_sel.push(a);
                            }
                        } else {
                            if !self.doc.anchor_sel.contains(&a) {
                                self.doc.anchor_sel = vec![a];
                            }
                            self.drag = Drag::MoveAnchors {
                                start_doc: dp,
                                last_doc: dp,
                                moved: false,
                            };
                        }
                        self.request_main_redraw();
                        return;
                    }

                    // Click on a segment inserts an anchor there, then
                    // drags it.
                    if let Some((id, seg, t)) =
                        anchors::segment_at(self.doc.editor.document(), &shown, dp, hit_r)
                    {
                        let _ = self.doc.editor.execute(Command::InsertAnchor {
                            object: id,
                            segment: seg,
                            t,
                        });
                        if let Some(a) = anchors::topmost_anchor_among(
                            self.doc.editor.document(),
                            &[id],
                            dp,
                            hit_r * 2.0,
                        ) {
                            self.doc.anchor_sel = vec![a];
                            self.drag = Drag::MoveAnchors {
                                start_doc: dp,
                                last_doc: dp,
                                moved: false,
                            };
                        }
                        self.request_main_redraw();
                        return;
                    }

                    let visible = self.visible_doc_rect();
                    let candidate =
                        select::topmost_selectable_at(self.doc.editor.document(), dp, visible);
                    self.drag = Drag::AnchorMarquee {
                        start: self.pointer,
                        candidate,
                    };
                    self.request_main_redraw();
                    return;
                }

                // Selection tool (ported from amalith-app's `press`):
                let visible = self.visible_doc_rect();

                // Transform handles / rotation halo win over object hits.
                if !self.doc.selection.is_empty() {
                    if let Some(quad) =
                        select::selection_quad(self.doc.editor.document(), &self.doc.selection)
                    {
                        let to_screen = self.doc.view.to_screen();
                        let scr = quad.map(|p| to_screen * p);
                        let start_xf: HashMap<ObjectId, Affine> = self
                            .doc.selection
                            .iter()
                            .filter_map(|id| {
                                self.doc.editor
                                    .document()
                                    .object(*id)
                                    .map(|o| (*id, convert::affine(o.transform)))
                            })
                            .collect();
                        if !start_xf.is_empty() {
                            if let Some(handle) = handles::hit_handle(self.pointer, scr) {
                                let start_bounds =
                                    select::union_bounds(self.doc.editor.document(), &self.doc.selection)
                                        .unwrap();
                                self.drag = Drag::Scale {
                                    handle,
                                    start_bounds,
                                    preview: start_xf.clone(),
                                    start_xf,
                                };
                                self.request_main_redraw();
                                return;
                            }
                            if handles::hit_rotate_halo(self.pointer, scr) {
                                let center =
                                    select::union_bounds(self.doc.editor.document(), &self.doc.selection)
                                        .unwrap()
                                        .center();
                                self.drag = Drag::Rotate {
                                    center,
                                    start_angle: handles::angle_to(center, dp),
                                    preview: start_xf.clone(),
                                    start_xf,
                                };
                                self.request_main_redraw();
                                return;
                            }
                        }
                    }
                }
                let start_move = |dp: Point| Drag::MoveObjects {
                    start_doc: dp,
                    last_doc: dp,
                    moved: false,
                };
                let doc = self.doc.editor.document();
                if let Some(id) = select::topmost_selectable_at(doc, dp, visible) {
                    // Double-click a text object → edit it (temporary Type tool).
                    if double
                        && matches!(doc.object(id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Text(_)))
                    {
                        let c = doc
                            .object(id)
                            .map(|o| o.transform.as_coeffs())
                            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                        self.enter_text_edit(id, Point::new(c[4], c[5]), Some(self.pointer));
                        return;
                    }
                    if self.shift_down {
                        // Shift-click toggles that object; no drag.
                        if let Some(i) = self.doc.selection.iter().position(|x| *x == id) {
                            self.doc.selection.remove(i);
                        } else {
                            self.doc.selection.push(id);
                        }
                    } else {
                        // Click on an unselected object replaces the
                        // selection before the move; click on one already
                        // selected drags the whole selection.
                        if !self.doc.selection.contains(&id) {
                            self.doc.selection = vec![id];
                        }
                        self.drag = start_move(dp);
                    }
                } else {
                    // Empty space: a press inside the selection box drags
                    // the selection; otherwise it's a marquee.
                    let inside_box = !self.shift_down
                        && select::union_bounds(doc, &self.doc.selection)
                            .is_some_and(|b| b.contains(dp));
                    if inside_box {
                        self.drag = start_move(dp);
                    } else {
                        if !self.shift_down {
                            self.doc.selection.clear();
                        }
                        self.drag = Drag::Marquee {
                            start: self.pointer,
                        };
                    }
                }
                self.request_main_redraw();
            }
            Role::Floating(fid) => {
                let laid = self.floating_layout(fid);
                for area in &laid.areas {
                    if area.tab_strip.contains(self.pointer) {
                        let tab = area
                            .tabs
                            .iter()
                            .position(|t| t.rect.contains(self.pointer))
                            .unwrap_or(0);
                        if let Some(t) = area.tabs.get(tab) {
                            if chrome::panel_tab_close_rect(t.rect).contains(self.pointer) {
                                let pid = t.panel;
                                self.close_panel_tab(pid, Some(fid));
                                return;
                            }
                        }
                        self.drag = Drag::PendingFloatMove {
                            id: fid,
                            tab,
                            press: self.pointer,
                        };
                        return;
                    }
                    if area.body.contains(self.pointer) {
                        if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                            let rep = self.representative();
                            let body = area.body;
                            let action = {
                                let ctx = panels::Ctx {
                                    theme: &self.theme,
                                    doc: self.doc.editor.document(),
                                    selection: &self.doc.selection,
                                    active_tool: self.active_tool,
                                    pointer: self.pointer,
                                    representative: rep,
                                    active_slot: self.active_slot,
                                    shape_tool: self.last_shape_tool,
                                    expanded: &self.doc.expanded_groups,
                                    renaming: self
                                        .doc.rename
                                        .as_ref()
                                        .map(|r| (r.target, r.buf.as_str())),
                                    selected_layer: self.doc.selected_layer,
                                    selected_artboard: self.doc.selected_artboard,
                                    text_style: self.active_text_style(),
                                    text_editing: self.text_edit.is_some(),
                                    font_families: &self.font_families,
                                    layer_query: &self.layer_query,
                                    layer_search_focused: self.layer_search_focused,
                                };
                                panels::hit(pid, body, self.pointer, &ctx)
                            };
                            self.apply_panel_action(action, double);
                        }
                        return;
                    }
                }
            }
        }
    }
}
