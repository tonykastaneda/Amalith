//! `apply_panel_action` — the single sink for `panels::Action` values
//! emitted by panel bodies and context-bar segments. `input/press.rs`
//! routes hits here. Grouped roughly: tool, selection / layers /
//! artboards, appearance, then the Character cluster and context-bar
//! steppers.

use super::*;

impl App {
    pub(in crate::app) fn apply_panel_action(&mut self, action: panels::Action, double: bool) {
        match action {
            panels::Action::None => {}
            panels::Action::SetTool(t) => self.set_tool(t),
            panels::Action::Select(id) => {
                self.doc.selection = vec![id];
                self.sync_align_mode();
                if double {
                    self.begin_rename(panels::RenameId::Object(id));
                }
            }
            panels::Action::SelectLayer(id) => {
                // Selecting a layer deselects any objects, so the row can
                // show its plain blue highlight.
                self.doc.selection.clear();
                self.doc.anchor_sel.clear();
                self.doc.selected_layer = Some(id);
                if double {
                    self.begin_rename(panels::RenameId::Layer(id));
                }
            }
            panels::Action::SelectArtboard(id) => {
                self.doc.selected_artboard = Some(id);
                if double {
                    self.begin_rename(panels::RenameId::Artboard(id));
                }
            }
            panels::Action::FocusArtboard(id) => {
                self.doc.selected_artboard = Some(id);
                if double {
                    self.focus_artboard(id);
                }
            }
            panels::Action::FocusLayerSearch => {
                self.doc.rename = None;
                self.layer_search_focused = true;
                self.request_main_redraw();
            }
            panels::Action::SetActiveSlot(s) => self.active_slot = s,
            // Single click just picks the slot; double click opens the
            // colour picker (Illustrator behaviour).
            panels::Action::OpenPicker(slot) if !double => self.active_slot = slot,
            panels::Action::OpenPicker(slot) => {
                self.active_slot = slot;
                let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                let paint = self
                    .representative()
                    .map(|a| match slot {
                        panels::PaintSlot::Fill => a.fill,
                        panels::PaintSlot::Stroke => a.stroke,
                    })
                    .unwrap_or(match slot {
                        panels::PaintSlot::Fill => self.doc.fill,
                        panels::PaintSlot::Stroke => self.doc.stroke,
                    });
                let origin = Point::new(
                    ((w - picker::W) * 0.5).max(4.0),
                    ((h - picker::H) * 0.5).max(4.0),
                );
                self.picker = Some(picker::Picker::from_color(slot, origin, paint.color()));
            }
            panels::Action::PickerSv(s, v) => {
                if let Some(pk) = &mut self.picker {
                    pk.s = s;
                    pk.v = v;
                    if self.dock.contains(PanelId("picker")) {
                        // Pointer is in the host window; reconstruct the
                        // panel-body origin from the SV inset so a drag
                        // keeps using the same hit math.
                        pk.origin = Point::new(
                            self.pointer.x - 19.0 - s as f64 * 308.0,
                            self.pointer.y - 26.0 - (1.0 - v as f64) * 308.0,
                        );
                    }
                }
                self.drag = Drag::PickColor { in_hue: false };
            }
            panels::Action::PickerHue(h) => {
                if let Some(pk) = &mut self.picker {
                    pk.h = h;
                    if self.dock.contains(PanelId("picker")) {
                        pk.origin = Point::new(
                            self.pointer.x - 350.0,
                            self.pointer.y - 23.0 - (1.0 - h as f64) * 308.0,
                        );
                    }
                }
                self.drag = Drag::PickColor { in_hue: true };
            }
            panels::Action::PickerCancel => self.dismiss_picker(false),
            panels::Action::PickerOk => self.dismiss_picker(true),
            panels::Action::ShapeField(i) => {
                if let Some(d) = self.shape_dialog.as_mut() {
                    d.focus_field(i);
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::ShapeStep(i, delta) => {
                if let Some(d) = self.shape_dialog.as_mut() {
                    d.step(i, delta as f64);
                }
                self.request_main_redraw();
            }
            panels::Action::ShapeLink => {
                if let Some(d) = self.shape_dialog.as_mut() {
                    d.toggle_link();
                }
                self.request_main_redraw();
            }
            panels::Action::ShapeOption(tag) => {
                if let Some(d) = self.shape_dialog.as_mut() {
                    d.apply_option(tag);
                }
                self.request_main_redraw();
            }
            panels::Action::ShapeCancel => self.close_shape_dialog(false),
            panels::Action::ShapeOk => self.close_shape_dialog(true),
            panels::Action::SetPaint(paint) => {
                self.set_paint(self.active_slot, paint);
                if let Some(c) = paint.color() {
                    self.push_recent(c);
                }
            }
            panels::Action::SwapPaints => {
                std::mem::swap(&mut self.doc.fill, &mut self.doc.stroke);
                if !self.doc.selection.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetPaints {
                        objects: self.doc.selection.clone(),
                        fill: Some(self.doc.fill),
                        stroke: Some(self.doc.stroke),
                    });
                }
            }
            panels::Action::DefaultPaints => {
                self.doc.fill = amalith_core::Paint::Solid(amalith_core::Color::rgb(1.0, 1.0, 1.0));
                self.doc.stroke =
                    amalith_core::Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0));
                if !self.doc.selection.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetPaints {
                        objects: self.doc.selection.clone(),
                        fill: Some(self.doc.fill),
                        stroke: Some(self.doc.stroke),
                    });
                }
            }
            panels::Action::SetStrokeWidth(width) => {
                if !self.doc.selection.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                        objects: self.doc.selection.clone(),
                        width,
                    });
                }
            }
            panels::Action::ToggleVisible(id) => {
                if let Some(cur) = self.doc.editor.document().object(id).map(|o| o.visible) {
                    let _ = self.doc.editor.execute(Command::SetVisible {
                        objects: vec![id],
                        visible: !cur,
                    });
                }
            }
            panels::Action::ToggleLocked(id) => {
                if let Some(cur) = self.doc.editor.document().object(id).map(|o| o.locked) {
                    let _ = self.doc.editor.execute(Command::SetLocked {
                        objects: vec![id],
                        locked: !cur,
                    });
                    if !cur {
                        self.doc.selection.retain(|s| *s != id);
                    }
                }
            }
            panels::Action::ToggleExpand(id) => {
                if !self.doc.expanded_groups.remove(&id) {
                    self.doc.expanded_groups.insert(id);
                }
            }
            panels::Action::NewLayer => {
                let n = self.doc.editor.document().layers().len() + 1;
                let _ = self.doc.editor.execute(Command::CreateLayer {
                    name: format!("Layer {n}"),
                    index: None,
                });
            }
            panels::Action::LayerRestack(dir) => self.restack(dir),
            panels::Action::DeleteObjects => {
                if !self.doc.selection.is_empty() {
                    let ids = std::mem::take(&mut self.doc.selection);
                    self.purge_threads(&ids);
                    let _ = self.doc.editor.execute(Command::DeleteObjects { ids });
                }
            }
            panels::Action::DeleteArtboard => {
                // A document always keeps at least one artboard.
                if self.doc.editor.document().artboards().len() > 1 {
                    if let Some(id) = self.doc.selected_artboard.take() {
                        let _ = self.doc.editor.execute(Command::DeleteArtboard { id });
                    }
                }
            }
            panels::Action::NewArtboard => {
                let boards = self.doc.editor.document().artboards();
                let n = boards.len() + 1;
                // Sit the new board to the right of the rightmost one,
                // same size; default 1200×800 when there are none.
                let rect = boards
                    .iter()
                    .map(|a| a.rect)
                    .reduce(|acc, r| if r.x1 > acc.x1 { r } else { acc })
                    .map(|r| {
                        let (w, h) = (r.width(), r.height());
                        amalith_core::Rect::new(r.x1 + 40.0, r.y0, r.x1 + 40.0 + w, r.y0 + h)
                    })
                    .unwrap_or_else(|| amalith_core::Rect::new(-600.0, -400.0, 600.0, 400.0));
                if let Ok(CommandOutcome::Artboard(id)) = self.doc.editor.execute(Command::CreateArtboard {
                    name: format!("Artboard {n}"),
                    rect,
                    index: None,
                }) {
                    self.doc.selected_artboard = Some(id);
                    self.set_tool(Tool::Artboard);
                }
            }
            // Intercepted in on_press (press-and-hold logic).
            panels::Action::ShapeSlot => {}
            // --- Character panel ---
            panels::Action::SetFontFamily(name) => {
                self.edit_text_style(move |s| s.family = name.clone());
            }
            panels::Action::SetFontFace { weight, italic } => {
                self.edit_text_style(move |s| {
                    s.weight = weight;
                    s.italic = italic;
                });
            }
            panels::Action::SetFontSize(v) => {
                self.edit_text_style(move |s| s.size = v);
            }
            panels::Action::SetLeading(v) => {
                self.edit_text_style(move |s| s.leading = v);
            }
            panels::Action::SetTracking(v) => {
                self.edit_text_style(move |s| s.tracking = v);
            }
            panels::Action::ToggleTextFlag(f) => {
                use amalith_core::TextPosition;
                use panels::TextFlag;
                self.edit_text_style(move |s| match f {
                    TextFlag::Underline => s.underline = !s.underline,
                    TextFlag::Strikethrough => s.strikethrough = !s.strikethrough,
                    TextFlag::SmallCaps => s.small_caps = !s.small_caps,
                    TextFlag::Superscript => {
                        s.position = if s.position == TextPosition::Superscript {
                            TextPosition::Normal
                        } else {
                            TextPosition::Superscript
                        }
                    }
                    TextFlag::Subscript => {
                        s.position = if s.position == TextPosition::Subscript {
                            TextPosition::Normal
                        } else {
                            TextPosition::Subscript
                        }
                    }
                    TextFlag::AllCaps => {} // not modelled yet
                });
            }
            panels::Action::SetTextAlign(a) => {
                self.edit_text_align(a);
            }
            panels::Action::SetParagraphMetric(field, v) => {
                use panels::ParaField;
                self.edit_paragraph(move |p| {
                    let slot = match field {
                        ParaField::IndentStart => &mut p.indent_start,
                        ParaField::IndentEnd => &mut p.indent_end,
                        ParaField::IndentFirst => &mut p.indent_first,
                        ParaField::SpaceBefore => &mut p.space_before,
                        ParaField::SpaceAfter => &mut p.space_after,
                    };
                    *slot = v;
                });
            }
            panels::Action::ToggleHyphenate => {
                self.edit_paragraph(|p| p.hyphenate = !p.hyphenate);
            }
            panels::Action::OpenFontMenu(kind, anchor) => {
                self.open_font_menu(kind, anchor);
            }
            panels::Action::OpenAlignToMenu(anchor) => {
                self.font_menu = None;
                self.panel_menu = None;
                if self.align_to_menu.is_some() {
                    self.align_to_menu = None;
                } else {
                    self.align_to_menu = Some(anchor);
                }
            }
            // --- context bar ---
            panels::Action::StepWeight(d) => self.step_weight(d),
            panels::Action::StepOpacity(d) => self.step_opacity(d),
            panels::Action::StepFontSize(d) => self.step_font_size(d),
            panels::Action::ToggleStrokeFlyout => {
                self.stroke_popover = !self.stroke_popover;
            }
            panels::Action::ConvertAnchor { smooth } => {
                for (object, anchor) in self.doc.anchor_sel.clone() {
                    let _ = self.doc.editor.execute(Command::SetAnchorSmooth {
                        object,
                        anchor,
                        smooth,
                    });
                }
            }
            panels::Action::PanelMenu { panel, id } => {
                if panel.0 == "color" {
                    match id {
                        "rgb" => self.color_mode = panels::ColorSpace::Rgb,
                        "hsb" => self.color_mode = panels::ColorSpace::Hsb,
                        "cmyk" => self.color_mode = panels::ColorSpace::Cmyk,
                        "invert" => {
                            let (r, g, b) = self
                                .active_paint()
                                .color()
                                .map(|c| (c.r, c.g, c.b))
                                .unwrap_or((0.0, 0.0, 0.0));
                            let (r, g, b) = panels::color::invert_rgb(r, g, b);
                            self.apply_solid_rgb(r, g, b);
                            self.push_recent(amalith_core::Color::rgb(r, g, b));
                        }
                        "complement" => {
                            let (r, g, b) = self
                                .active_paint()
                                .color()
                                .map(|c| (c.r, c.g, c.b))
                                .unwrap_or((0.0, 0.0, 0.0));
                            let (r, g, b) = panels::color::complement_rgb(r, g, b);
                            self.apply_solid_rgb(r, g, b);
                            self.push_recent(amalith_core::Color::rgb(r, g, b));
                        }
                        _ => {}
                    }
                } else if panel.0 == "transform" {
                    match id {
                        "flip-h" => self.flip_xform(true),
                        "flip-v" => self.flip_xform(false),
                        _ => {}
                    }
                } else if panel.0 == "align" {
                    if id == "cancel-key" {
                        self.key_object = None;
                        if self.align_to == amalith_commands::AlignTo::KeyObject {
                            self.align_to = if self.doc.selection.len() <= 1 {
                                amalith_commands::AlignTo::Artboard
                            } else {
                                amalith_commands::AlignTo::Selection
                            };
                        }
                    }
                }
            }
            panels::Action::SetXformRef(rp) => {
                self.xform_ref = rp;
            }
            panels::Action::ToggleXformConstrain => {
                self.xform_constrain = !self.xform_constrain;
            }
            panels::Action::BeginXformEdit(field) => {
                if let Some((old, buf, _)) = self.xform_edit.take() {
                    if old != field {
                        self.commit_xform_buf(old, buf);
                    }
                }
                self.begin_xform_edit(field);
            }
            panels::Action::NudgeXform { field, delta } => {
                self.nudge_xform(field, delta);
            }
            panels::Action::Pathfinder(op) => {
                let objects = self.doc.selection.clone();
                match self.doc.editor.execute(Command::Pathfinder { op, objects }) {
                    Ok(CommandOutcome::Object(id)) => {
                        self.doc.selection = vec![id];
                        self.doc.anchor_sel.clear();
                    }
                    Ok(_) => {
                        self.doc.selection.clear();
                        self.doc.anchor_sel.clear();
                    }
                    Err(err) => self.doc.io_error = Some(err.to_string()),
                }
                self.sync_align_mode();
            }
            panels::Action::ExpandStroke => {
                let objects = self.doc.selection.clone();
                match self.doc.editor.execute(Command::ExpandStroke { objects }) {
                    Ok(_) => self.doc.anchor_sel.clear(),
                    Err(err) => self.doc.io_error = Some(err.to_string()),
                }
            }
            panels::Action::Align(kind) => {
                let objects = self.doc.selection.clone();
                let artboard = self.doc.current_artboard.or_else(|| {
                    self.doc.editor.document().artboards().first().map(|a| a.id)
                });
                // One object can't align to itself — fall through to the artboard,
                // matching Illustrator's Control bar.
                let to = if self.align_to == amalith_commands::AlignTo::Selection
                    && objects.len() <= 1
                {
                    amalith_commands::AlignTo::Artboard
                } else {
                    self.align_to
                };
                match self.doc.editor.execute(Command::Align {
                    objects,
                    kind,
                    to,
                    key: self.key_object,
                    artboard,
                    spacing: self.align_spacing,
                }) {
                    Ok(_) => {}
                    Err(err) => self.doc.io_error = Some(err.to_string()),
                }
            }
            panels::Action::SetAlignTo(to) => {
                if to == amalith_commands::AlignTo::KeyObject {
                    if self.doc.selection.len() >= 2 {
                        self.align_to = to;
                        if self.key_object.is_none()
                            || self
                                .key_object
                                .is_some_and(|k| !self.doc.selection.contains(&k))
                        {
                            self.key_object = self.frontmost_selected();
                        }
                    }
                } else {
                    self.align_to = to;
                    self.key_object = None;
                }
            }
            panels::Action::BeginAlignSpacingEdit => {
                if self.align_spacing_edit.is_none() {
                    let seed = self
                        .align_spacing
                        .map(trim_num)
                        .unwrap_or_else(|| "Auto".into());
                    self.align_spacing_edit = Some((seed, true));
                }
            }
            panels::Action::ColorScrub { channel, t, track } => {
                self.set_color_channel(channel, t);
                self.drag = Drag::ColorScrub { channel, track };
            }
            panels::Action::ColorSpectrum { t, track } => {
                self.set_color_spectrum(t);
                self.drag = Drag::ColorSpectrum { track };
            }
        }
        self.request_main_redraw();
    }
}

impl App {
    fn begin_xform_edit(&mut self, field: panels::transform::XformField) {
        let seed = self
            .xform_current(field)
            .map(|v| trim_num(v))
            .unwrap_or_default();
        self.xform_edit = Some((field, seed, true));
    }

    fn xform_current(&self, field: panels::transform::XformField) -> Option<f64> {
        use amalith_core::xform;
        use panels::transform::XformField as F;
        let id = *self.doc.selection.first()?;
        let doc = self.doc.editor.document();
        let b = doc.local_bounds_of(id)?;
        let v = xform::values(doc.world_transform(id), b, self.xform_ref);
        Some(match field {
            F::X => v.x,
            F::Y => v.y,
            F::W => v.w,
            F::H => v.h,
            F::Rotation => v.rotation_deg,
            F::Shear => v.shear_deg,
        })
    }

    pub(in crate::app) fn nudge_xform(&mut self, field: panels::transform::XformField, dir: f64) {
        use panels::transform::XformField as F;
        let step = match field {
            F::Rotation | F::Shear => {
                if self.shift_down {
                    15.0
                } else {
                    1.0
                }
            }
            _ => {
                if self.shift_down {
                    10.0
                } else {
                    1.0
                }
            }
        };
        let Some(cur) = self.xform_current(field) else {
            return;
        };
        self.apply_xform_value(field, cur + step * dir);
        // Keep a live field-edit buffer in sync so a later canvas click
        // doesn't re-apply the pre-nudge seed and snap the object back.
        let new = self.xform_current(field);
        if let (Some((f, buf, _)), Some(v)) = (self.xform_edit.as_mut(), new) {
            if *f == field {
                *buf = trim_num(v);
            }
        }
        self.request_main_redraw();
    }

    fn apply_xform_value(&mut self, field: panels::transform::XformField, value: f64) {
        use amalith_core::xform;
        use amalith_core::ObjectParent;
        use panels::transform::XformField as F;
        if !value.is_finite() {
            return;
        }
        let ids = self.doc.selection.clone();
        let rp = self.xform_ref;
        let constrain = self.xform_constrain;
        let mut items = Vec::new();
        {
            let doc = self.doc.editor.document();
            for &id in &ids {
                let Some(obj) = doc.object(id) else { continue };
                let Some(bounds) = doc.local_bounds_of(id) else {
                    continue;
                };
                let parent = match obj.parent {
                    ObjectParent::Group(g) => doc.world_transform(g),
                    ObjectParent::Layer(_) => amalith_core::Affine::IDENTITY,
                };
                let local = obj.transform;
                let next = match field {
                    F::X => xform::set_x(local, parent, bounds, rp, value),
                    F::Y => xform::set_y(local, parent, bounds, rp, value),
                    F::W => xform::set_w(local, parent, bounds, rp, value.max(0.01), constrain),
                    F::H => xform::set_h(local, parent, bounds, rp, value.max(0.01), constrain),
                    F::Rotation => xform::set_rotation(local, parent, bounds, rp, value),
                    F::Shear => {
                        xform::set_shear(local, parent, bounds, rp, value.clamp(-89.0, 89.0))
                    }
                };
                if next.as_coeffs().iter().all(|c| c.is_finite()) {
                    items.push((id, next));
                }
            }
        }
        if !items.is_empty() {
            let _ = self.doc.editor.execute(Command::SetTransforms { items });
            self.request_main_redraw();
        }
    }

    fn flip_xform(&mut self, horizontal: bool) {
        use amalith_core::xform;
        let ids = self.doc.selection.clone();
        let rp = self.xform_ref;
        let mut items = Vec::new();
        {
            let doc = self.doc.editor.document();
            for &id in &ids {
                let Some(obj) = doc.object(id) else { continue };
                let Some(bounds) = doc.local_bounds_of(id) else {
                    continue;
                };
                let next = if horizontal {
                    xform::flip_h(obj.transform, bounds, rp)
                } else {
                    xform::flip_v(obj.transform, bounds, rp)
                };
                items.push((id, next));
            }
        }
        if !items.is_empty() {
            let _ = self.doc.editor.execute(Command::SetTransforms { items });
        }
    }

    fn commit_xform_buf(&mut self, field: panels::transform::XformField, buf: String) {
        if let Some(v) = parse_num(&buf) {
            self.apply_xform_value(field, v);
        }
    }

    pub(in crate::app) fn commit_xform_edit(&mut self) {
        if let Some((field, buf, fresh)) = self.xform_edit.take() {
            // `fresh` means the user never typed — scroll/handle edits are
            // already in the document. Re-applying the seed would reset them.
            if !fresh {
                self.commit_xform_buf(field, buf);
            }
        }
        self.request_main_redraw();
    }

    pub(in crate::app) fn xform_field_at_pointer(&mut self) -> Option<panels::transform::XformField> {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return None;
        }
        if self.pointer_win == self.main_id
            && self.pointer.y >= APP_BAR_H
            && self.pointer.y < APP_BAR_H + OPT_BAR_H
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let bar = opt_bar_rect(w);
            let cx = self.context_bar_ctx();
            if let Some(f) = context_bar::xform_field_at(bar, &cx, self.pointer) {
                return Some(f);
            }
        }
        let areas: Vec<crate::layout::PanelArea> = if self.pointer_win == self.main_id {
            [RailSide::Left, RailSide::Right]
                .iter()
                .flat_map(|&side| {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        return Vec::new();
                    }
                    let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    build_rail_layout(rail, &self.theme, &mut self.text, rect).areas
                })
                .collect()
        } else if let Some(fid) = self.pointer_win.and_then(|wid| {
            self.hosts.get(&wid).and_then(|h| match h.role {
                Role::Floating(f) => Some(f),
                _ => None,
            })
        }) {
            self.floating_layout(fid).areas
        } else {
            return None;
        };
        for area in &areas {
            if !area.body.contains(self.pointer) {
                continue;
            }
            let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) else {
                continue;
            };
            if pid.0 != "transform" {
                continue;
            }
            let pbody = panels::scrolled_body(pid, area.body, self.panel_scroll_of(pid)).0;
            return panels::transform::field_at(pbody, self.pointer);
        }
        None
    }

    /// Digit / Enter / Esc stay in the field. Anything else (Space, V, ⌘Z)
    /// commits and returns false so the rest of `on_key` can run.
    pub(in crate::app) fn xform_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.xform_edit.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.commit_xform_edit();
                true
            }
            PhysicalKey::Code(KeyCode::Escape) => {
                self.xform_edit = None;
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some((_, buf, fresh)) = &mut self.xform_edit {
                    *fresh = false;
                    buf.pop();
                }
                self.request_main_redraw();
                true
            }
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    self.commit_xform_edit();
                    return false;
                };
                let numeric = txt.chars().all(|c| {
                    c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == ','
                });
                if !numeric {
                    self.commit_xform_edit();
                    return false;
                }
                if let Some((_, buf, fresh)) = &mut self.xform_edit {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if *fresh {
                            buf.clear();
                            *fresh = false;
                        }
                        buf.push(ch);
                    }
                }
                self.request_main_redraw();
                true
            }
        }
    }

    pub(in crate::app) fn commit_align_spacing_edit(&mut self) {
        if let Some((buf, fresh)) = self.align_spacing_edit.take() {
            if !fresh {
                let t = buf.trim();
                if t.is_empty() || t.eq_ignore_ascii_case("auto") {
                    self.align_spacing = None;
                } else if let Some(v) = parse_num(&buf) {
                    self.align_spacing = Some(v.max(0.0));
                }
            }
        }
        self.request_main_redraw();
    }

    pub(in crate::app) fn align_spacing_field_at_pointer(&mut self) -> bool {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return false;
        }
        let areas: Vec<crate::layout::PanelArea> = if self.pointer_win == self.main_id {
            [RailSide::Left, RailSide::Right]
                .iter()
                .flat_map(|&side| {
                    let rail = self.dock.rail(side);
                    if rail.is_empty() {
                        return Vec::new();
                    }
                    let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                    let rect = rail_rect_for(side, rail.width as f64, w, h);
                    build_rail_layout(rail, &self.theme, &mut self.text, rect).areas
                })
                .collect()
        } else if let Some(fid) = self.pointer_win.and_then(|wid| {
            self.hosts.get(&wid).and_then(|h| match h.role {
                Role::Floating(f) => Some(f),
                _ => None,
            })
        }) {
            self.floating_layout(fid).areas
        } else {
            return false;
        };
        for area in &areas {
            if !area.body.contains(self.pointer) {
                continue;
            }
            let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) else {
                continue;
            };
            if pid.0 != "align" {
                continue;
            }
            let pbody = panels::scrolled_body(pid, area.body, self.panel_scroll_of(pid)).0;
            return panels::align::spacing_field_at(pbody, self.pointer);
        }
        false
    }

    /// Digit / Enter / Esc stay in the Align spacing field.
    pub(in crate::app) fn align_spacing_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.align_spacing_edit.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.commit_align_spacing_edit();
                true
            }
            PhysicalKey::Code(KeyCode::Escape) => {
                self.align_spacing_edit = None;
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some((buf, fresh)) = &mut self.align_spacing_edit {
                    *fresh = false;
                    buf.pop();
                }
                self.request_main_redraw();
                true
            }
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    self.commit_align_spacing_edit();
                    return false;
                };
                let numeric = txt.chars().all(|c| {
                    c.is_ascii_digit()
                        || c == '.'
                        || c == '-'
                        || c == '+'
                        || c == ','
                        || c.is_ascii_alphabetic()
                        || c.is_whitespace()
                });
                if !numeric {
                    self.commit_align_spacing_edit();
                    return false;
                }
                if let Some((buf, fresh)) = &mut self.align_spacing_edit {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if *fresh {
                            buf.clear();
                            *fresh = false;
                        }
                        buf.push(ch);
                    }
                }
                self.request_main_redraw();
                true
            }
        }
    }
}

fn parse_num(s: &str) -> Option<f64> {
    let t = s
        .trim()
        .trim_end_matches("px")
        .trim_end_matches('°')
        .trim();
    t.parse().ok()
}

fn trim_num(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 5e-5 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
