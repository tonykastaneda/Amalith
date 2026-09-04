//! Press routing for the main and floating windows. `window_event`
//! delegates its left-mouse-down arm here. The guard order (modals →
//! chrome → rails → canvas) is load-bearing.

use super::super::*;

impl App {
    pub(in crate::app) fn on_press(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        id: WindowId,
        double: bool,
    ) {
        let Some(role) = self.hosts.get(&id).map(|h| h.role) else {
            return;
        };
        // The command palette (⌘K) is topmost while open.
        if self.palette.is_some() {
            let hit = self
                .palette
                .as_ref()
                .map(|p| p.hit(self.pointer))
                .unwrap_or(crate::palette::Hit::Outside);
            match hit {
                crate::palette::Hit::Row(i) => self.run_palette_cmd(i),
                crate::palette::Hit::Outside => self.palette = None,
                crate::palette::Hit::Panel => {}
            }
            self.request_main_redraw();
            return;
        }
        // Any press ends the "⌘Z re-opens the last pen path" window.
        self.last_pen = None;
        self.tooltip = None;
        // A press anywhere blurs the Layers search field; a hit on the
        // field itself re-focuses it later in this handler.
        self.layer_search_focused = false;
        // Same for a Transform-panel numeric edit: click outside the field
        // commits it so the canvas / zoom / Space-pan work on this press.
        if self.xform_edit.is_some() && self.xform_field_at_pointer().is_none() {
            self.commit_xform_edit();
        }
        if self.gradient_edit.is_some() && self.gradient_field_at_pointer().is_none() {
            self.commit_gradient_edit();
        }
        if self.align_spacing_edit.is_some() && !self.align_spacing_field_at_pointer() {
            self.commit_align_spacing_edit();
        }
        if self.artboard_edit.is_some() && !self.over_artboard_segment() {
            self.commit_artboard_edit();
        }
        // An open Character-panel dropdown is topmost — it eats the press.
        if self.font_menu.is_some() && self.font_menu_click(self.pointer) {
            return;
        }
        if self.align_to_menu.is_some() && self.align_to_menu_click(self.pointer) {
            return;
        }
        if self.ruler_menu.is_some() {
            self.ruler_menu_click(self.pointer);
            return;
        }
        if self.ctx_menu.is_some() && self.ctx_menu_click(self.pointer) {
            return;
        }
        // Isolation breadcrumb bar.
        if !self.isolation.is_empty() {
            if let Some(&(_, depth)) =
                self.iso_bar.iter().find(|(r, _)| r.contains(self.pointer))
            {
                self.isolation_to_depth(depth);
                return;
            }
        }
        // An open panel hamburger flyout: clicks inside it are consumed;
        // a click on its own hamburger toggles it shut; anything else
        // closes it and falls through so another hamburger can open.
        if self.panel_menu.is_some() && self.panel_menu_click(id, self.pointer) {
            return;
        }
        // The Preferences modal.
        if self.prefs.is_some() {
            let hit = self.prefs.as_mut().unwrap().on_press(self.pointer);
            match hit {
                prefs::Hit::Backdrop | prefs::Hit::Cancel => self.prefs = None,
                prefs::Hit::Ok => {
                    let mut p = self.prefs.take().unwrap();
                    p.commit_naming();
                    self.settings = p.working;
                    self.scripts = p.working_scripts;
                    self.keymaps = p.working_keymaps;
                    self.apply_theme_accent();
                    settings::save(&self.settings);
                    crate::scripts::save(&self.scripts);
                    crate::keymap::save(&self.keymaps);
                    self.rebuild_native_menu();
                }
                prefs::Hit::TogglePresetMenu => {
                    if let Some(p) = &mut self.prefs {
                        p.preset_menu_open = !p.preset_menu_open;
                    }
                }
                prefs::Hit::PickPreset(i) => {
                    if let Some(p) = &mut self.prefs {
                        let names = p.working_keymaps.names();
                        if let Some(name) = names.get(i).cloned() {
                            let (tk, ak) = p.working_keymaps.keys_of(&name);
                            p.working.tool_keys = tk;
                            p.working.action_keys = ak;
                            p.working_keymaps.active = name;
                        }
                        p.preset_menu_open = false;
                    }
                }
                prefs::Hit::AddPreset => {
                    if let Some(p) = &mut self.prefs {
                        if p.naming.is_some() {
                            p.commit_naming();
                        } else {
                            p.naming = Some(crate::text_field::TextField::new(""));
                            p.preset_menu_open = false;
                        }
                    }
                }
                prefs::Hit::ChooseScriptsFolder => {
                    if let Some(dir) = rfd::FileDialog::new()
                        .set_title("Choose Scripts Folder")
                        .pick_folder()
                    {
                        if let Some(p) = &mut self.prefs {
                            p.working_scripts.dir = Some(dir);
                            p.refresh_scripts();
                        }
                    }
                }
                prefs::Hit::ClearScriptsFolder => {
                    if let Some(p) = &mut self.prefs {
                        p.working_scripts.dir = None;
                        p.working_scripts.keys.clear();
                        p.refresh_scripts();
                    }
                }
                prefs::Hit::SetAccent(rgb) => {
                    if let Some(p) = &mut self.prefs {
                        p.working.accent = rgb;
                    }
                }
                prefs::Hit::StartRecording(t) => {
                    if let Some(p) = &mut self.prefs {
                        p.recording = Some(t);
                    }
                }
                prefs::Hit::ResetKeys => {
                    if let Some(p) = &mut self.prefs {
                        let def = prefs::Settings::default();
                        p.working.tool_keys = def.tool_keys;
                        p.working.action_keys = def.action_keys;
                        p.recording = None;
                    }
                }
                prefs::Hit::Category(i) => {
                    if let Some(p) = &mut self.prefs {
                        p.category = i;
                        p.page_scroll.set_offset(0.0);
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
                prefs::Hit::ToggleFps => {
                    if let Some(p) = &mut self.prefs {
                        p.working.show_fps = !p.working.show_fps;
                    }
                }
                prefs::Hit::ToggleCullOutline => {
                    if let Some(p) = &mut self.prefs {
                        p.working.show_cull_outline = !p.working.show_cull_outline;
                    }
                }
                prefs::Hit::SetCullInset(v) => {
                    if let Some(p) = &mut self.prefs {
                        p.working.cull_inset = v;
                    }
                }
                prefs::Hit::None => {
                    if let Some(p) = &mut self.prefs {
                        p.preset_menu_open = false;
                    }
                }
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
                    if let newdoc::Hit::Field(f) = hit {
                        // Click / double-click / drag select inside the
                        // field, via its TextField (uses the box rect it
                        // cached at paint).
                        let (p, clicks) = (self.pointer, self.click_streak);
                        if let Some(form) = self.newdoc.as_mut() {
                            if form.focused() != Some(f) {
                                form.commit_focus();
                            }
                            form.focus = Some(f);
                            form.field(f).pointer_down(p, clicks, &mut self.text);
                        }
                        self.drag = Drag::NewdocSelect { field: f };
                        self.request_main_redraw();
                        return;
                    }
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
                        Some(home::Hit::News) => about::open_url(home::NEWS_URL),
                        Some(home::Hit::Docs) => about::open_url(home::DOCS_URL),
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
                        let spawn = double && matches!(action, panels::Action::OpenPicker(_));
                        self.apply_panel_action(action, double);
                        if spawn {
                            self.spawn_picker_window(event_loop);
                        }
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
                if let Some(pk) = self.picker.filter(|_| !self.dock.contains(PanelId("picker"))) {
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
                        picker::Hit::Cancel => {
                            self.dismiss_picker(false);
                        }
                        picker::Hit::Ok => {
                            self.dismiss_picker(true);
                        }
                        picker::Hit::Title => {
                            self.drag = Drag::MovePicker {
                                offset: Point::new(
                                    self.pointer.x - pk.origin.x,
                                    self.pointer.y - pk.origin.y,
                                ),
                            };
                        }
                        picker::Hit::Inside | picker::Hit::Outside => {}
                    }
                    self.request_main_redraw();
                    return;
                }

                for side in [RailSide::Left, RailSide::Right] {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        continue;
                    }
                    let rect = rail_rect_for(side, rail, w, h);
                    // The rail's inner edge widens the whole rail — check it
                    // first, since its grab zone spills onto the canvas.
                    // Skipped when every column is iconized: the rail is
                    // pinned to the icon strip's width in that state (see
                    // `rail_effective_width`), so there is no docked tree
                    // to resize — dragging here would silently stash a new
                    // `rail.width` with no visible effect, only to snap the
                    // rail to some size the user never saw while expanding
                    // a column later. Widening a fully-collapsed dock back
                    // out is the separate, not-yet-built icon+label mode.
                    if rail.tree.is_some()
                        && rail_edge_bar(side, rect)
                            .inflate(GRAB_SLOP + 1.0, 0.0)
                            .contains(self.pointer)
                    {
                        self.drag = Drag::RailWidth { side };
                        return;
                    }
                    // A side's open flyout can spill past the rail's own
                    // rect onto the canvas (it sits between the icon strip
                    // and the canvas, not squeezed inside the rail width),
                    // so a pointer outside `rect` still needs this side's
                    // (flyout-aware) layout built before being ruled out.
                    let has_open_flyout = self.flyout_icon.is_some_and(|(s, ..)| s == side);
                    if !rect.contains(self.pointer) && !has_open_flyout {
                        continue;
                    }
                    let laid = build_rail_layout_with_flyout(
                        rail,
                        side,
                        rect,
                        self.flyout_icon,
                        &self.theme,
                        &mut self.text,
                    );
                    if let Some(col) = laid.icon_col {
                        if icon_col_edge_bar(side, col)
                            .inflate(GRAB_SLOP, 0.0)
                            .contains(self.pointer)
                        {
                            self.drag = Drag::IconColWidth { side };
                            return;
                        }
                    }
                    if let Some(ir) = laid.icon_rects.iter().find(|ir| ir.rect.contains(self.pointer)) {
                        // A click opens/switches the flyout; a drag tears
                        // the whole group off into a floating window (see
                        // `Drag::PendingIconTearoff` / `tear_off_icon_group`).
                        self.drag = Drag::PendingIconTearoff {
                            side,
                            column: ir.column,
                            group: ir.group,
                            tab: ir.tab,
                            press: self.pointer,
                        };
                        return;
                    }
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
                        if area.title_bar.contains(self.pointer) {
                            let close = chrome::group_close_rect(area.title_bar, &self.theme);
                            if close.contains(self.pointer) {
                                if area.is_flyout {
                                    if let Some((_, column, group)) = self.flyout_icon {
                                        self.close_icon_group(side, column, group);
                                    }
                                } else {
                                    self.close_group(side, &area.path);
                                }
                                return;
                            }
                            let collapse_shown = area.is_flyout || chrome::is_column_top(area, &laid.areas);
                            if collapse_shown {
                                let collapse = chrome::collapse_rect(area.title_bar, &self.theme);
                                if collapse.contains(self.pointer) {
                                    if area.is_flyout {
                                        if let Some((_, column, _)) = self.flyout_icon {
                                            self.expand_column(side, column);
                                        }
                                    } else {
                                        self.collapse_column(side, &area.path);
                                    }
                                    return;
                                }
                            }
                            // Anything else on the title bar: press-drag
                            // tears the *whole group* off into a floating
                            // window (a flyout has no real tree position
                            // to detach from, so it's skipped here — drag
                            // its icon row instead, same as any other).
                            if !area.is_flyout {
                                self.drag = Drag::PendingGroupTearoff {
                                    side,
                                    path: area.path.clone(),
                                    press: self.pointer,
                                };
                            }
                            return;
                        }
                        if area.tab_strip.contains(self.pointer) {
                            let burger = chrome::panel_menu_rect(area.tab_strip, &self.theme);
                            if area.show_menu && burger.contains(self.pointer) {
                                if let Some(pid) =
                                    area.tabs.get(area.active).map(|t| t.panel)
                                {
                                    self.toggle_panel_menu(pid, burger, id);
                                }
                                return;
                            }
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
                                let pbody = panels::scrolled_body(
                                    pid,
                                    area.body,
                                    self.panel_scroll_of(pid),
                                )
                                .0;
                                let rep = self.representative();
                                let action = {
                                    let ctx = panels::Ctx {
                                        theme: &self.theme,
                                        doc: self.doc.editor.document(),
                                        selection: &self.doc.selection,
                                        active_tool: self.active_tool,
                                        pointer: self.pointer,
                                        representative: rep,
                                        fill_mixed: false,
                                        stroke_mixed: false,
                                        active_slot: self.active_slot,
                                        picker: self.picker,
                                        cur_fill: self.doc.fill,
                                        cur_stroke: self.doc.stroke,
                                        shape_tool: self.last_shape_tool,
                                        expanded: &self.doc.expanded_groups,
                                        renaming: self
                                            .doc.rename
                                            .as_ref()
                                            .map(|r| (r.target, r.buf.as_str())),
                                        selected_layer: self.doc.selected_layer,
                                        selected_artboard: self.doc.selected_artboard,
                                        text_style: self.active_text_style(),
                                        text_align: self.active_text_align(),
                                        text_paragraph: self.active_text_paragraph(),
                                        text_editing: self.text_edit.is_some(),
                                        font_families: &self.font_families,
                                        layer_query: &self.layer_query,
                                        layer_search_focused: self.layer_search_focused,
                        layer_scroll: self.panel_scroll_of(PanelId("layers")),
                        layer_drop: None,
                                        color_mode: self.color_mode,
                                        cmyk_profile: self.cmyk_profile.as_ref(),
                                        recent: &self.recent_colors,
                                        xform_ref: self.xform_ref,
                                        xform_constrain: self.xform_constrain,
                                        xform_edit: self
                                            .xform_edit
                                            .as_ref()
                                            .map(|(f, s, _)| (*f, s.as_str())),
                                        align_to: self.align_to,
                                        align_spacing: self.align_spacing,
                                        align_spacing_edit: self
                                            .align_spacing_edit
                                            .as_ref()
                                            .map(|(s, _)| s.as_str()),
                                        key_object: self.key_object,
                                        shape_dialog: None,
                                        export: None,
                                        gradient: self.gradient_ctx(),
                                        gradient_edit: self.gradient_edit.as_ref().map(|(f, s, _)| (*f, s.as_str())),
                                    };
                                    panels::hit(pid, pbody, self.pointer, &ctx)
                                };
                                if action == panels::Action::ShapeSlot {
                                    // Start a press: a hold opens the
                                    // flyout, a quick release re-picks
                                    // the last shape tool.
                                    let anchor =
                                        panels::tools::shape_slot_rect(pbody);
                                    self.shape_press = Some((Instant::now(), anchor));
                                } else {
                                    let spawn =
                                        double && matches!(action, panels::Action::OpenPicker(_));
                                    // Pressing an object row also arms a
                                    // drag-reorder (the click already
                                    // selected it).
                                    let arm_drag = !double
                                        && pid == PanelId("layers")
                                        && matches!(action, panels::Action::Select(_));
                                    let grad_drag = gradient_drag_for(&action, double);
                                    self.apply_panel_action(action, double);
                                    if spawn {
                                        self.spawn_picker_window(event_loop);
                                    }
                                    if arm_drag {
                                        self.drag = Drag::LayerDrag {
                                            body: pbody,
                                            press: self.pointer,
                                            moved: false,
                                        };
                                    }
                                    if let Some(d) = grad_drag {
                                        self.drag = d;
                                    }
                                }
                            }
                            return;
                        }
                    }
                    // Nothing on this side's rail (or flyout) matched. A
                    // flyout that reached here only because it spills past
                    // the rail's own rect onto the canvas means the click
                    // was really meant for whatever's underneath —
                    // dismiss it and let the click fall through instead of
                    // swallowing it; otherwise keep the usual behaviour of
                    // a rail dead-zone click doing nothing further.
                    if has_open_flyout {
                        self.dismiss_flyout();
                        if !rect.contains(self.pointer) {
                            continue;
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
                // Hand tool = a persistent pan drag.
                if self.active_tool == Tool::Hand {
                    self.drag = Drag::Pan { last: self.pointer };
                    self.update_canvas_cursor();
                    return;
                }
                // Zoom tool: click steps zoom at the cursor; drag is
                // Illustrator's scrubby zoom (same as Space+⌘ drag).
                if self.active_tool == Tool::Zoom {
                    self.drag = Drag::ScrubZoom {
                        anchor: self.pointer,
                        last: self.pointer,
                    };
                    self.update_canvas_cursor();
                    return;
                }
                // Eyedropper: sample the object under the cursor.
                if self.active_tool == Tool::Eyedropper {
                    self.eyedrop_at(self.pointer);
                    return;
                }
                let dp = self.doc_point(self.pointer);
                // Gradient tool: press near an annotator handle edits that
                // handle (drag a stop along the line, or move an endpoint);
                // anywhere else lays down a fresh axis on the object under
                // the cursor.
                if self.active_tool == Tool::Gradient {
                    self.gradient_tool_press(dp, double);
                    // A double-click on a stop opened the colour picker —
                    // give it its floating window.
                    if self.picker.is_some() {
                        self.spawn_picker_window(event_loop);
                    }
                    return;
                }

                // Ruler strip → drag out a new guide.
                if let Some(orient) = self.ruler_strip_at(self.pointer) {
                    if !self.guides_locked {
                        use amalith_core::GuideOrient;
                        let pos = match orient {
                            GuideOrient::Horizontal => dp.y,
                            GuideOrient::Vertical => dp.x,
                        };
                        self.drag = Drag::NewGuide { orient, pos };
                        self.request_main_redraw();
                    }
                    return;
                }
                // Grab / select an existing guide (selection tools only).
                if matches!(self.active_tool, Tool::Select | Tool::DirectSelect) {
                    if let Some(id) = self.guide_at(self.pointer) {
                        if self.shift_down {
                            if let Some(i) = self.selected_guides.iter().position(|g| *g == id) {
                                self.selected_guides.remove(i);
                            } else {
                                self.selected_guides.push(id);
                            }
                        } else if !self.selected_guides.contains(&id) {
                            self.selected_guides = vec![id];
                        }
                        if let Some(g) = self.doc.editor.document().guide(id).copied() {
                            use amalith_core::GuideOrient;
                            let axis0 = match g.orient {
                                GuideOrient::Horizontal => dp.y,
                                GuideOrient::Vertical => dp.x,
                            };
                            self.drag = Drag::MoveGuide {
                                id,
                                orient: g.orient,
                                pos: g.pos,
                                orig: g.pos,
                                grab: axis0 - g.pos,
                                press: self.pointer,
                                moved: false,
                            };
                        }
                        self.request_main_redraw();
                        return;
                    }
                    if !self.shift_down && !self.selected_guides.is_empty() {
                        self.selected_guides.clear();
                        self.request_main_redraw();
                    }
                }

                // Rotate tool: a drag turns the selection about the
                // reference point; a plain click (no drag) re-places it.
                if self.active_tool == Tool::Rotate {
                    let start_xf: HashMap<ObjectId, Affine> = self
                        .doc
                        .selection
                        .iter()
                        .filter_map(|id| {
                            self.doc
                                .editor
                                .document()
                                .object(*id)
                                .map(|o| (*id, convert::affine(o.transform)))
                        })
                        .collect();
                    if let Some(pivot) = self.rotate_pivot().filter(|_| !start_xf.is_empty()) {
                        self.drag = Drag::RotateTool {
                            pivot,
                            start_angle: handles::angle_to(pivot, dp),
                            preview: start_xf.clone(),
                            start_xf,
                            copy: self.alt_down,
                            moved: false,
                        };
                        self.request_main_redraw();
                    }
                    return;
                }

                // "Loaded text" cursor: a prior out-port click armed a
                // thread. This press drops its target — an existing frame
                // clicked, or a new one rubber-banded.
                if let Some(from) = self.text_load {
                    let visible = self.visible_doc_rect();
                    let hit = select::topmost_selectable_at(self.doc.editor.document(), dp, visible)
                        .filter(|id| {
                            *id != from
                                && matches!(
                                    self.doc.editor.document().object(*id).map(|o| &o.kind),
                                    Some(amalith_core::ObjectKind::Text(t))
                                        if matches!(t.kind, amalith_core::TextKind::Area { .. })
                                )
                        });
                    if let Some(to) = hit {
                        self.thread_text(from, to);
                    } else {
                        self.drag = Drag::ThreadNewBox {
                            from,
                            start_doc: dp,
                            cur_doc: dp,
                        };
                    }
                    return;
                }

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
                        // starts a selection drag. A triple-click selects
                        // the whole text and must NOT arm the drag — the
                        // next cursor move would collapse it to the pointer.
                        let clicks = self.click_streak.min(3);
                        if let Some(p) = self.text_editor_point(self.pointer) {
                            if let Some(te) = &mut self.text_edit {
                                te.pointer_down(p, clicks, &mut self.text);
                            }
                        }
                        self.drag = if clicks >= 3 {
                            Drag::None
                        } else {
                            Drag::TextSelect
                        };
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
                    // Not drawing yet and over an existing anchor of the
                    // selected path — select it (so the Convert bar shows)
                    // rather than starting a fresh path on top of it.
                    if self.pen.is_empty() {
                        let paths = self.node_paths();
                        if !paths.is_empty() {
                            if let Some(a) = anchors::topmost_anchor_among(
                                self.doc.editor.document(),
                                &paths,
                                dp,
                                6.0 / self.doc.view.zoom,
                            ) {
                                self.doc.anchor_sel = vec![a];
                                self.request_main_redraw();
                                return;
                            }
                        }
                    }
                    // Over a segment of the selected path (and not drawing
                    // yet) — a click inserts an anchor there.
                    if let Some((id, seg, t)) = self.pen_insert_target() {
                        let _ = self.doc.editor.execute(Command::InsertAnchor {
                            object: id,
                            segment: seg,
                            t,
                        });
                        if let Some(a) = anchors::topmost_anchor_among(
                            self.doc.editor.document(),
                            &[id],
                            dp,
                            12.0 / self.doc.view.zoom,
                        ) {
                            self.doc.anchor_sel = vec![a];
                        }
                        self.request_main_redraw();
                        return;
                    }
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
                        space_last: None,
                    };
                    self.request_main_redraw();
                    return;
                }

                // A shape tool rubber-bands a new object; the Line tool
                // rubber-bands a single straight segment (same drag state,
                // committed differently on release).
                if self.active_tool.is_shape() || self.active_tool == Tool::Line {
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

                // Out-port of a single selected area-text frame → arm a
                // thread (next press drops its target).
                if let Some(id) = self.text_out_port_hit() {
                    self.text_load = Some(id);
                    self.update_canvas_cursor();
                    self.request_main_redraw();
                    return;
                }

                // Right-edge convert dot: double-click toggles the text
                // object between point and area type.
                if let Some(id) = self.text_convert_hit() {
                    if double {
                        self.toggle_text_kind(id);
                    }
                    self.request_main_redraw();
                    return;
                }

                // Bottom-centre auto-fit tab of an area-text frame:
                // double-click snaps the height to the text; a plain press
                // drags the height like the S scale handle.
                if let Some(id) = self.text_autofit_hit() {
                    if double {
                        self.fit_text_box_height(id);
                    } else if let Some(start_bounds) =
                        select::union_bounds(self.doc.editor.document(), &self.doc.selection)
                    {
                        let boxes = self.area_text_boxes();
                        if !boxes.is_empty() {
                            self.drag = Drag::ResizeTextBox {
                                handle: handles::Handle::S,
                                start_bounds,
                                frames: boxes,
                                start_doc: dp,
                                cur_doc: dp,
                            };
                        }
                    }
                    self.request_main_redraw();
                    return;
                }

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
                                // If every selected object is an area-text
                                // frame, a handle drag re-sizes the box(es)
                                // and re-wraps — it doesn't scale glyphs.
                                let boxes = self.area_text_boxes();
                                if !boxes.is_empty() {
                                    self.drag = Drag::ResizeTextBox {
                                        handle,
                                        start_bounds,
                                        frames: boxes,
                                        start_doc: dp,
                                        cur_doc: dp,
                                    };
                                    self.request_main_redraw();
                                    return;
                                }
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
                let start_move = |dp: Point, hit: Option<ObjectId>| Drag::MoveObjects {
                    start_doc: dp,
                    last_doc: dp,
                    moved: false,
                    hit,
                };
                let doc = self.doc.editor.document();
                let iso_root = self.isolation_root();
                let hit = match iso_root {
                    Some(root) => {
                        select::topmost_in(doc, root, dp, 4.0 / self.doc.view.zoom)
                    }
                    None => select::topmost_selectable_at(doc, dp, visible),
                };
                if let Some(id) = hit {
                    let kind = doc.object(id).map(|o| &o.kind);
                    // Double-click a text object → edit it (temporary Type tool).
                    if double && matches!(kind, Some(amalith_core::ObjectKind::Text(_))) {
                        let c = doc
                            .object(id)
                            .map(|o| o.transform.as_coeffs())
                            .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
                        self.enter_text_edit(id, Point::new(c[4], c[5]), Some(self.pointer));
                        return;
                    }
                    // Double-click any non-text object → drill into it
                    // (isolation mode): a group opens its contents, a bare
                    // path / shape / image just dims everything else and
                    // scopes selection to itself. Text was handled above.
                    if double {
                        self.enter_isolation(id);
                        return;
                    }
                    if self.shift_down {
                        // Shift-click toggles that object; no drag.
                        if let Some(i) = self.doc.selection.iter().position(|x| *x == id) {
                            self.doc.selection.remove(i);
                        } else {
                            self.doc.selection.push(id);
                        }
                        self.sync_align_mode();
                    } else {
                        // Click on an unselected object replaces the
                        // selection before the move; click on one already
                        // selected drags the whole selection (a click
                        // without a drag designates the Align key object).
                        if !self.doc.selection.contains(&id) {
                            self.doc.selection = vec![id];
                            self.sync_align_mode();
                        }
                        self.drag = start_move(dp, Some(id));
                    }
                } else if iso_root.is_some() && double {
                    // Double-click on empty canvas steps out one level; a
                    // single click just deselects / starts a marquee.
                    self.pop_isolation();
                } else {
                    // Empty space: a press inside the selection box drags
                    // the selection; otherwise it's a marquee.
                    let inside_box = !self.shift_down
                        && select::union_bounds(doc, &self.doc.selection)
                            .is_some_and(|b| b.contains(dp));
                    if inside_box {
                        self.drag = start_move(dp, None);
                    } else {
                        if !self.shift_down {
                            self.doc.selection.clear();
                            self.sync_align_mode();
                        }
                        self.drag = Drag::Marquee {
                            start: self.pointer,
                        };
                    }
                }
                self.request_main_redraw();
            }
            Role::Floating(fid) => {
                // Collapsed to an icon (states 4/6): a persistent header
                // (close × / expand ») plus one row per tab, same
                // geometry the renderer uses (`floating_collapsed_rows`).
                if let Some(node) = self.dock.floating(fid).filter(|f| f.icon_w.is_some()).map(|f| f.node.clone()) {
                    let tab_count = match &node {
                        Node::Tabs { panels, .. } => panels.len(),
                        Node::Split { .. } => 1,
                    };
                    let sz = self.floating_window(fid).map(|w| w.inner_size()).unwrap_or_default();
                    let (wl, hl) = (
                        (sz.width as f64 / self.scale).max(1.0),
                        (sz.height as f64 / self.scale).max(1.0),
                    );
                    let rect = Rect::new(0.0, 0.0, wl, hl);
                    let (header, rows) =
                        layout::floating_collapsed_rows(rect, tab_count, self.theme.group_title_h);
                    if header.contains(self.pointer) {
                        if chrome::group_close_rect(header, &self.theme).contains(self.pointer) {
                            self.close_floating(fid);
                            return;
                        }
                        if chrome::collapse_rect(header, &self.theme).contains(self.pointer) {
                            self.expand_floating(fid);
                            return;
                        }
                        // Anything else on the header drags the still-
                        // collapsed window, same as the open state's title
                        // bar — it stays repositionable while shrunk.
                        self.drag = Drag::PendingFloatMove {
                            id: fid,
                            tab: 0,
                            press: self.pointer,
                        };
                        return;
                    }
                    // A specific tab's row: activate it, then expand —
                    // there's no separate "preview" step worth having for
                    // an already-standalone window, unlike a rail's icon
                    // strip flyout.
                    if let Some(tab) = rows.iter().position(|r| r.contains(self.pointer)) {
                        if let Some(f) = self.dock.floating_mut(fid) {
                            if let Node::Tabs { active, panels } = &mut f.node {
                                if tab < panels.len() {
                                    *active = tab;
                                }
                            }
                        }
                    }
                    self.expand_floating(fid);
                    return;
                }
                let laid = self.floating_layout(fid);
                for area in &laid.areas {
                    if area.title_bar.contains(self.pointer) {
                        let close = chrome::group_close_rect(area.title_bar, &self.theme);
                        if close.contains(self.pointer) {
                            self.close_floating(fid);
                            return;
                        }
                        // A floating window is its own column of exactly
                        // one — it always owns the collapse control,
                        // unlike an attached group which only gets it at
                        // the top of a stack.
                        let collapse = chrome::collapse_rect(area.title_bar, &self.theme);
                        if collapse.contains(self.pointer) {
                            self.collapse_floating(fid);
                            return;
                        }
                        // Anything else on the title bar drags the whole
                        // window — the tab strip below it still does the
                        // same for now too (unchanged), so existing
                        // muscle memory keeps working.
                        self.drag = Drag::PendingFloatMove {
                            id: fid,
                            tab: area.active,
                            press: self.pointer,
                        };
                        return;
                    }
                    if area.tab_strip.contains(self.pointer) {
                        let burger = chrome::panel_menu_rect(area.tab_strip, &self.theme);
                        if area.show_menu && burger.contains(self.pointer) {
                            if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                                self.toggle_panel_menu(pid, burger, id);
                            }
                            return;
                        }
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
                            let body =
                                panels::scrolled_body(pid, area.body, self.panel_scroll_of(pid)).0;
                            let action = {
                                let ctx = panels::Ctx {
                                    theme: &self.theme,
                                    doc: self.doc.editor.document(),
                                    selection: &self.doc.selection,
                                    active_tool: self.active_tool,
                                    pointer: self.pointer,
                                    representative: rep,
                                    fill_mixed: false,
                                    stroke_mixed: false,
                                    active_slot: self.active_slot,
                                    picker: self.picker,
                                    cur_fill: self.doc.fill,
                                    cur_stroke: self.doc.stroke,
                                    shape_tool: self.last_shape_tool,
                                    expanded: &self.doc.expanded_groups,
                                    renaming: self
                                        .doc.rename
                                        .as_ref()
                                        .map(|r| (r.target, r.buf.as_str())),
                                    selected_layer: self.doc.selected_layer,
                                    selected_artboard: self.doc.selected_artboard,
                                    text_style: self.active_text_style(),
                                    text_align: self.active_text_align(),
                                    text_paragraph: self.active_text_paragraph(),
                                    text_editing: self.text_edit.is_some(),
                                    font_families: &self.font_families,
                                    layer_query: &self.layer_query,
                                    layer_search_focused: self.layer_search_focused,
                        layer_scroll: self.panel_scroll_of(PanelId("layers")),
                        layer_drop: None,
                                    color_mode: self.color_mode,
                                    cmyk_profile: self.cmyk_profile.as_ref(),
                                    recent: &self.recent_colors,
                                    xform_ref: self.xform_ref,
                                    xform_constrain: self.xform_constrain,
                                    xform_edit: self
                                        .xform_edit
                                        .as_ref()
                                        .map(|(f, s, _)| (*f, s.as_str())),
                                    align_to: self.align_to,
                                    align_spacing: self.align_spacing,
                                    align_spacing_edit: self
                                        .align_spacing_edit
                                        .as_ref()
                                        .map(|(s, _)| s.as_str()),
                                    key_object: self.key_object,
                                    shape_dialog: self.shape_dialog.as_ref().map(|d| (d, false)),
                                    export: self.export.as_ref().map(|d| (d, false)),
                                    gradient: self.gradient_ctx(),
                                    gradient_edit: self.gradient_edit.as_ref().map(|(f, s, _)| (*f, s.as_str())),
                                };
                                panels::hit(pid, body, self.pointer, &ctx)
                            };
                            let spawn =
                                double && matches!(action, panels::Action::OpenPicker(_));
                            let arm_drag = !double
                                && pid == PanelId("layers")
                                && matches!(action, panels::Action::Select(_));
                            let grad_drag = gradient_drag_for(&action, double);
                            self.apply_panel_action(action, double);
                            if let Some(d) = grad_drag {
                                self.drag = d;
                            }
                            if spawn {
                                self.spawn_picker_window(event_loop);
                            }
                            if arm_drag {
                                self.drag = Drag::LayerDrag {
                                    body,
                                    press: self.pointer,
                                    moved: false,
                                };
                            }
                        }
                        return;
                    }
                }
            }
        }
    }
}

/// If `action` is a Gradient-panel press that should start a drag (moving a
/// stop or a midpoint), the drag to arm — else `None`. A double-click
/// never starts a drag (it opens the picker).
fn gradient_drag_for(action: &panels::Action, double: bool) -> Option<Drag> {
    if double {
        return None;
    }
    match *action {
        panels::Action::GradientSelectStop { index, bar } => {
            Some(Drag::GradientStop { index, bar })
        }
        panels::Action::GradientMidDrag { index, bar } => {
            Some(Drag::GradientMid { index, bar })
        }
        _ => None,
    }
}
