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
            panels::Action::LayerSwatch(id) => {
                self.doc.selection.clear();
                self.doc.anchor_sel.clear();
                self.doc.selected_layer = Some(id);
                if double {
                    // Menu/shortcut-style actions have no `event_loop`
                    // here; the window spawns next `about_to_wait`, same
                    // as Export/Offset Path.
                    self.pending_layer_dialog = Some(id);
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
                        panels::PaintSlot::Fill => a.fill(),
                        panels::PaintSlot::Stroke => a.stroke(),
                    })
                    .unwrap_or(match slot {
                        panels::PaintSlot::Fill => self.doc.fill,
                        panels::PaintSlot::Stroke => self.doc.stroke,
                    });
                let origin = Point::new(
                    ((w - picker::metric_w()) * 0.5).max(4.0),
                    ((h - picker::metric_h()) * 0.5).max(4.0),
                );
                self.picker = Some(picker::Picker::from_color(slot, origin, paint.color()));
            }
            panels::Action::PickerSv(s, v) => {
                if let Some(pk) = &mut self.picker {
                    pk.s = s;
                    pk.v = v;
                    if self.dock.contains(PanelId(PanelKind::Picker)) {
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
                    if self.dock.contains(PanelId(PanelKind::Picker)) {
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
            panels::Action::ExportHit(h) => {
                let outcome = self
                    .export
                    .as_mut()
                    .map(|d| d.apply(h))
                    .unwrap_or(crate::export::Outcome::None);
                match outcome {
                    crate::export::Outcome::PickFolder => self.export_pick_folder(),
                    crate::export::Outcome::Run => self.close_export(true),
                    crate::export::Outcome::Cancel => self.close_export(false),
                    crate::export::Outcome::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::BlendHit(hit) => {
                match hit {
                    blenddlg::Hit::Row(i) => {
                        if let Some(dlg) = self.blend_dialog.as_mut() {
                            dlg.mode = [blenddlg::Mode::SmoothColor, blenddlg::Mode::Steps, blenddlg::Mode::Distance][i];
                            dlg.focused = dlg.mode != blenddlg::Mode::SmoothColor;
                        }
                        self.apply_blend_preview();
                    }
                    blenddlg::Hit::Field => {
                        if let Some(dlg) = self.blend_dialog.as_mut() {
                            dlg.focused = true;
                        }
                    }
                    blenddlg::Hit::Preview => {
                        if let Some(dlg) = self.blend_dialog.as_mut() {
                            dlg.preview = !dlg.preview;
                        }
                        match self.blend_dialog.as_ref().map(|d| d.preview) {
                            Some(true) => self.apply_blend_preview(),
                            Some(false) => {
                                if let Some(dlg) = &self.blend_dialog {
                                    let (group, spacing, spine) =
                                        (dlg.group, dlg.original_spacing, dlg.spine);
                                    let _ = self.doc.editor.execute(Command::SetBlendOptions {
                                        group,
                                        spacing,
                                        spine,
                                    });
                                }
                            }
                            None => {}
                        }
                    }
                    blenddlg::Hit::Ok => self.close_blend_dialog(blend_dialog::BlendClose::Ok),
                    blenddlg::Hit::Cancel => self.close_blend_dialog(blend_dialog::BlendClose::Cancel),
                    blenddlg::Hit::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::OffsetHit(hit) => {
                match hit {
                    offsetdlg::Hit::Offset => {
                        if let Some(dlg) = self.offset_dialog.as_mut() {
                            dlg.focus = offsetdlg::Field::Offset;
                        }
                    }
                    offsetdlg::Hit::Join(i) => {
                        if let Some(dlg) = self.offset_dialog.as_mut() {
                            dlg.join = [
                                amalith_core::LineJoin::Miter,
                                amalith_core::LineJoin::Round,
                                amalith_core::LineJoin::Bevel,
                            ][i];
                        }
                    }
                    offsetdlg::Hit::MiterLimit => {
                        if let Some(dlg) = self.offset_dialog.as_mut() {
                            dlg.focus = offsetdlg::Field::MiterLimit;
                        }
                    }
                    offsetdlg::Hit::Preview => {
                        if let Some(dlg) = self.offset_dialog.as_mut() {
                            dlg.preview = !dlg.preview;
                        }
                    }
                    offsetdlg::Hit::Ok => self.close_offset_dialog(offset_dialog::OffsetClose::Ok),
                    offsetdlg::Hit::Cancel => self.close_offset_dialog(offset_dialog::OffsetClose::Cancel),
                    offsetdlg::Hit::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::LayerDialogHit(hit) => {
                match hit {
                    layerdlg::Hit::Name => {}
                    layerdlg::Hit::ToggleColorMenu => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.color_menu_open = !dlg.color_menu_open;
                        }
                    }
                    layerdlg::Hit::ColorItem(i) => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            if let Some(c) = amalith_core::LayerColor::ALL.get(i) {
                                dlg.color = *c;
                            }
                            dlg.color_menu_open = false;
                        }
                    }
                    layerdlg::Hit::ToggleTemplate => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.template = !dlg.template;
                        }
                    }
                    layerdlg::Hit::ToggleLock => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.locked = !dlg.locked;
                        }
                    }
                    layerdlg::Hit::ToggleShow => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.visible = !dlg.visible;
                        }
                    }
                    layerdlg::Hit::TogglePrint => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.print = !dlg.print;
                        }
                    }
                    layerdlg::Hit::TogglePreview => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.preview = !dlg.preview;
                        }
                    }
                    layerdlg::Hit::ToggleDimImages => {
                        if let Some(dlg) = self.layer_dialog.as_mut() {
                            dlg.dim_images = if dlg.dim_images.is_some() { None } else { Some("50".to_string()) };
                        }
                    }
                    layerdlg::Hit::DimPct => {}
                    layerdlg::Hit::Ok => self.close_layer_dialog(layer_dialog::LayerDialogClose::Ok),
                    layerdlg::Hit::Cancel => self.close_layer_dialog(layer_dialog::LayerDialogClose::Cancel),
                    layerdlg::Hit::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::AreaTypeHit(hit) => {
                match hit {
                    areatypedlg::Hit::Width => {
                        if let Some(dlg) = self.area_type_dialog.as_mut() {
                            dlg.focus = areatypedlg::Field::Width;
                        }
                    }
                    areatypedlg::Hit::Height => {
                        if let Some(dlg) = self.area_type_dialog.as_mut() {
                            dlg.focus = areatypedlg::Field::Height;
                        }
                    }
                    areatypedlg::Hit::Align(i) => {
                        if let Some(dlg) = self.area_type_dialog.as_mut() {
                            // Start hugs the box's right edge ("Right"),
                            // End hugs the left ("Left") — matches
                            // `areatypedlg::ALIGNS` / `context_bar::area_type::OPTIONS`.
                            dlg.align = [
                                amalith_core::TextAlign::Start,
                                amalith_core::TextAlign::Center,
                                amalith_core::TextAlign::End,
                                amalith_core::TextAlign::JustifyAll,
                            ][i];
                        }
                    }
                    areatypedlg::Hit::AutoSize => {
                        if let Some(dlg) = self.area_type_dialog.as_mut() {
                            dlg.auto_size = !dlg.auto_size;
                        }
                    }
                    areatypedlg::Hit::Preview => {
                        if let Some(dlg) = self.area_type_dialog.as_mut() {
                            dlg.preview = !dlg.preview;
                        }
                    }
                    areatypedlg::Hit::Ok => self.close_area_type_dialog(area_type_dialog::AreaTypeClose::Ok),
                    areatypedlg::Hit::Cancel => self.close_area_type_dialog(area_type_dialog::AreaTypeClose::Cancel),
                    areatypedlg::Hit::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::XformHit(hit) => {
                if let xformdlg::Hit::Dial(field, _, center) = hit {
                    self.drag = Drag::XformDialAngle { field, center };
                }
                let outcome = self
                    .xform_dialog
                    .as_mut()
                    .map(|d| d.apply(hit))
                    .unwrap_or(xformdlg::Outcome::None);
                match outcome {
                    xformdlg::Outcome::Changed => self.apply_xform_preview(),
                    xformdlg::Outcome::Copy => self.close_xform_dialog(xform_dialog::XformClose::Copy),
                    xformdlg::Outcome::Cancel => self.close_xform_dialog(xform_dialog::XformClose::Cancel),
                    xformdlg::Outcome::Ok => self.close_xform_dialog(xform_dialog::XformClose::Ok),
                    xformdlg::Outcome::None => {}
                }
                self.text_blink = Instant::now();
                self.request_main_redraw();
            }
            panels::Action::SetPaint(paint) => {
                self.set_paint(self.active_slot, paint);
                if let Some(c) = paint.color() {
                    self.push_recent(c);
                }
            }
            panels::Action::ApplyGradientPaint => self.apply_gradient_paint(),
            panels::Action::GradientKind(kind) => self.gradient_set_kind(kind),
            panels::Action::GradientAddStop { offset } => self.gradient_add_stop(offset),
            panels::Action::GradientSelectStop { index, .. } => {
                self.gradient_select_stop(index, double)
            }
            panels::Action::GradientStep(field, delta) => self.gradient_step(field, delta),
            panels::Action::GradientBeginEdit(field) => self.begin_gradient_edit(field),
            panels::Action::GradientReverse => self.gradient_reverse(),
            // The drag is armed by the press router; nothing to do on the
            // bare click.
            panels::Action::GradientMidDrag { .. } => {}
            panels::Action::GradientStopPicker => self.gradient_stop_picker(),
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
            panels::Action::ShapeSlot | panels::Action::ToolFlyout(_) => {}
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
                self.area_align_menu = None;
                if self.align_to_menu.is_some() {
                    self.align_to_menu = None;
                } else {
                    self.align_to_menu = Some(anchor);
                }
            }
            panels::Action::OpenWidthProfileMenu(anchor) => {
                self.font_menu = None;
                self.panel_menu = None;
                self.align_to_menu = None;
                self.area_align_menu = None;
                if self.width_profile_menu.is_some() {
                    self.width_profile_menu = None;
                } else {
                    self.width_profile_menu = Some(anchor);
                }
            }
            panels::Action::SetWidthProfile(preset) => {
                self.apply_width_profile(preset);
            }
            panels::Action::OpenAreaAlignMenu(anchor) => {
                self.font_menu = None;
                self.panel_menu = None;
                self.align_to_menu = None;
                self.width_profile_menu = None;
                if self.area_align_menu.is_some() {
                    self.area_align_menu = None;
                } else {
                    self.area_align_menu = Some(anchor);
                }
            }
            panels::Action::SetCrossAlign(align) => {
                self.edit_cross_align(align);
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
                if panel.0 == PanelKind::Color {
                    match id {
                        "rgb" => self.color_mode = panels::ColorSpace::Rgb,
                        "hsb" => self.color_mode = panels::ColorSpace::Hsb,
                        "cmyk" => self.color_mode = panels::ColorSpace::Cmyk,
                        "load-icc-profile" => self.load_cmyk_profile(),
                        "clear-icc-profile" => self.clear_cmyk_profile(),
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
                } else if panel.0 == PanelKind::Transform {
                    match id {
                        "flip-h" => self.flip_xform(true),
                        "flip-v" => self.flip_xform(false),
                        _ => {}
                    }
                } else if panel.0 == PanelKind::Align {
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
                } else if panel.0 == PanelKind::Links {
                    if let Some(asset_id) = self.doc.selected_asset {
                        match id {
                            "embed" => self.embed_asset(asset_id),
                            "unembed" => self.unembed_asset(asset_id),
                            _ => {}
                        }
                    }
                }
            }
            panels::Action::SelectAsset(id) => {
                self.doc.selected_asset = Some(id);
            }
            panels::Action::GoToLinkAsset(id) => self.go_to_link(id),
            panels::Action::RelinkAsset(id) => self.relink_asset(id),
            panels::Action::UpdateLinkAsset(id) => self.update_linked_asset(id),
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
            panels::Action::ArtboardOrient(portrait) => self.set_artboard_orient(portrait),
            panels::Action::ToggleArtboardFillMenu => {
                self.artboard_fill_menu = !self.artboard_fill_menu;
                self.request_main_redraw();
            }
            panels::Action::ArtboardFillPick(i) => {
                self.artboard_fill_menu = false;
                self.pick_artboard_fill(i);
            }
            panels::Action::ToggleArtboardLink => {
                self.artboard_link = !self.artboard_link;
                self.request_main_redraw();
            }
            panels::Action::BeginArtboardEdit(field) => {
                self.commit_artboard_edit();
                self.begin_artboard_edit(field);
            }
            panels::Action::NudgeArtboard(field, delta) => self.nudge_artboard(field, delta),
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
            panels::Action::BeginStrokeWeightEdit => {
                if self.stroke_weight_edit.is_none() {
                    let w = self
                        .doc.selection
                        .first()
                        .and_then(|id| self.doc.editor.document().object(*id))
                        .map(|o| o.appearance.stroke_width())
                        .unwrap_or(self.doc.stroke_w);
                    self.stroke_weight_edit = Some((trim_num(w), true));
                }
            }
            panels::Action::BeginOpacityEdit => {
                if self.opacity_edit.is_none() {
                    let op = self
                        .doc.selection
                        .first()
                        .and_then(|id| self.doc.editor.document().object(*id))
                        .map(|o| o.appearance.opacity)
                        .unwrap_or(self.doc.opacity);
                    self.opacity_edit = Some(widgets::NumEdit::seeded(format!("{:.0}", op * 100.0), amalith_core::MeasureKind::Percent));
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
            panels::Action::EmbedAsset(id) => self.embed_asset(id),
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
                    5.0
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
        if let Some(v) = parse_num(&buf, xform_field_kind(field)) {
            self.apply_xform_value(field, v);
        }
    }

    /// Applies the focused Transform field's current buffer live, without
    /// leaving edit mode — called after every keystroke.
    fn apply_xform_edit_live(&mut self) {
        let Some((field, buf, fresh)) = &self.xform_edit else { return };
        if *fresh {
            return;
        }
        if let Some(v) = parse_num(buf, xform_field_kind(*field)) {
            self.apply_xform_value(*field, v);
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
            && self.pointer.y >= metric_app_bar_h()
            && self.pointer.y < metric_app_bar_h() + metric_opt_bar_h()
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let bar = opt_bar_rect(w);
            let cx = self.context_bar_ctx();
            if let Some(f) = context_bar::xform_field_at(bar, &cx, self.pointer) {
                return Some(f);
            }
        }
        let pbody = self.active_panel_body_at_pointer(PanelKind::Transform)?;
        panels::transform::field_at(pbody, self.pointer)
    }

    /// The Gradient-panel numeric under the pointer, for scroll-to-nudge.
    pub(in crate::app) fn gradient_field_at_pointer(&mut self) -> Option<panels::gradient::GradField> {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return None;
        }
        let kind = self.target_gradient().map(|(_, g)| g.kind);
        let pbody = self.active_panel_body_at_pointer(PanelKind::Gradient)?;
        panels::gradient::field_at(pbody, self.pointer, kind)
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
                self.apply_xform_edit_live();
                self.request_main_redraw();
                true
            }
            // Consumed here, not passed through — otherwise it falls to
            // the canvas's own Up/Down (nudge the selected object) while
            // a field is focused, which is the whole point of catching it.
            PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::ArrowDown) => {
                let dir = if event.physical_key == PhysicalKey::Code(KeyCode::ArrowUp) { 1.0 } else { -1.0 };
                let step = if self.cmd_down { 0.1 } else if self.shift_down { 5.0 } else { 1.0 };
                if let Some((field, buf, fresh)) = &mut self.xform_edit {
                    let cur = parse_num(buf, xform_field_kind(*field)).unwrap_or(0.0);
                    *buf = trim_num(cur + dir * step);
                    *fresh = false;
                }
                self.apply_xform_edit_live();
                self.request_main_redraw();
                true
            }
            // A bare modifier keydown (Shift, held for Shift+Up/Down) has
            // no text — falling to the `_` arm below would treat it as
            // "some other key" and commit/exit right as Shift is pressed,
            // before the arrow key it's modifying even arrives.
            PhysicalKey::Code(
                KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight,
            ) => true,
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    self.commit_xform_edit();
                    return false;
                };
                let numeric = txt.chars().all(widgets::measurement_char);
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
                self.apply_xform_edit_live();
                self.request_main_redraw();
                true
            }
        }
    }

    // --- Artboard options-bar segment ---------------------------------

    fn artboard_current(&self, field: panels::transform::ABField) -> Option<f64> {
        use panels::transform::ABField as F;
        let id = self.doc.selected_artboard?;
        let r = self.doc.editor.document().artboard(id)?.rect;
        Some(match field {
            F::X => r.x0,
            F::Y => r.y0,
            F::W => r.x1 - r.x0,
            F::H => r.y1 - r.y0,
            F::Name => return None,
        })
    }

    fn begin_artboard_edit(&mut self, field: panels::transform::ABField) {
        use panels::transform::ABField as F;
        let seed = if field == F::Name {
            self.doc
                .selected_artboard
                .and_then(|id| self.doc.editor.document().artboard(id).map(|a| a.name.clone()))
                .unwrap_or_default()
        } else {
            self.artboard_current(field).map(trim_num).unwrap_or_default()
        };
        self.artboard_edit = Some((field, seed, true));
        self.request_main_redraw();
    }

    pub(in crate::app) fn commit_artboard_edit(&mut self) {
        self.apply_artboard_edit_live();
        self.artboard_edit = None;
        self.request_main_redraw();
    }

    /// Applies the focused Artboard field's current buffer live, without
    /// leaving edit mode — called after every keystroke.
    fn apply_artboard_edit_live(&mut self) {
        use panels::transform::ABField as F;
        let Some((field, buf, fresh)) = &self.artboard_edit else { return };
        if *fresh {
            return;
        }
        let (field, buf) = (*field, buf.clone());
        let Some(id) = self.doc.selected_artboard else { return };
        if field == F::Name {
            let name = buf.trim();
            if !name.is_empty() {
                let _ = self
                    .doc
                    .editor
                    .execute(Command::RenameArtboard { id, name: name.to_string() });
            }
        } else if let Some(v) = parse_num(&buf, amalith_core::MeasureKind::Length(amalith_core::Unit::Px)) {
            self.apply_artboard_value(field, v);
        }
    }

    fn apply_artboard_value(&mut self, field: panels::transform::ABField, v: f64) {
        use panels::transform::ABField as F;
        if !v.is_finite() {
            return;
        }
        let Some(id) = self.doc.selected_artboard else {
            return;
        };
        let Some(mut r) = self.doc.editor.document().artboard(id).map(|a| a.rect) else {
            return;
        };
        let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
        let ratio = if w.abs() > 1e-6 { h / w } else { 1.0 };
        match field {
            F::X => r = amalith_core::Rect::new(v, r.y0, v + w, r.y1),
            F::Y => r = amalith_core::Rect::new(r.x0, v, r.x1, v + h),
            F::W => {
                let nw = v.max(1.0);
                let nh = if self.artboard_link { nw * ratio } else { h };
                r = amalith_core::Rect::new(r.x0, r.y0, r.x0 + nw, r.y0 + nh);
            }
            F::H => {
                let nh = v.max(1.0);
                let nw = if self.artboard_link && ratio.abs() > 1e-6 {
                    nh / ratio
                } else {
                    w
                };
                r = amalith_core::Rect::new(r.x0, r.y0, r.x0 + nw, r.y0 + nh);
            }
            F::Name => return,
        }
        let _ = self.doc.editor.execute(Command::ResizeArtboard { id, rect: r });
        self.request_main_redraw();
    }

    pub(in crate::app) fn nudge_artboard(&mut self, field: panels::transform::ABField, dir: f64) {
        let Some(cur) = self.artboard_current(field) else {
            return;
        };
        let step = if self.shift_down { 5.0 } else { 1.0 };
        self.apply_artboard_value(field, cur + step * dir);
        let new = self.artboard_current(field);
        if let (Some((f, buf, _)), Some(v)) = (self.artboard_edit.as_mut(), new) {
            if *f == field {
                *buf = trim_num(v);
            }
        }
        self.request_main_redraw();
    }

    fn set_artboard_orient(&mut self, portrait: bool) {
        let Some(id) = self.doc.selected_artboard else {
            return;
        };
        let Some(r) = self.doc.editor.document().artboard(id).map(|a| a.rect) else {
            return;
        };
        let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
        let is_portrait = h >= w;
        if is_portrait != portrait {
            let nr = amalith_core::Rect::new(r.x0, r.y0, r.x0 + h, r.y0 + w);
            let _ = self
                .doc
                .editor
                .execute(Command::ResizeArtboard { id, rect: nr });
            self.request_main_redraw();
        }
    }

    fn pick_artboard_fill(&mut self, item: u8) {
        let Some(id) = self.doc.selected_artboard else {
            return;
        };
        let fill = match item {
            0 => Some(amalith_core::Color::rgb(1.0, 1.0, 1.0)),
            1 => Some(amalith_core::Color::rgb(0.0, 0.0, 0.0)),
            2 => None,
            _ => {
                // "Other…" — open the colour picker, retargeted to this fill.
                let (w, h) = self.main_logical_size().unwrap_or((1280.0, 800.0));
                let start = self
                    .doc
                    .editor
                    .document()
                    .artboard(id)
                    .and_then(|a| a.fill)
                    .map(|c| amalith_core::Color::rgba(c.r, c.g, c.b, c.a));
                let origin = Point::new(
                    ((w - crate::picker::metric_w()) * 0.5).max(4.0),
                    ((h - crate::picker::metric_h()) * 0.5).max(4.0),
                );
                self.picker = Some(crate::picker::Picker::from_color(
                    self.active_slot,
                    origin,
                    start,
                ));
                self.picker_artboard = true;
                self.request_main_redraw();
                return;
            }
        };
        let _ = self.doc.editor.execute(Command::SetArtboardFill { id, fill });
        self.request_main_redraw();
    }

    /// Feed a key to the artboard field being edited. Returns `true` if it
    /// was consumed.
    pub(in crate::app) fn artboard_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        use panels::transform::ABField as F;
        use winit::keyboard::{KeyCode, PhysicalKey};
        let Some((field, _, _)) = self.artboard_edit else {
            return false;
        };
        if !event.state.is_pressed() {
            return true;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.commit_artboard_edit();
                true
            }
            PhysicalKey::Code(KeyCode::Escape) => {
                self.artboard_edit = None;
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some((_, buf, fresh)) = &mut self.artboard_edit {
                    if *fresh {
                        buf.clear();
                    }
                    *fresh = false;
                    buf.pop();
                }
                self.apply_artboard_edit_live();
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::ArrowDown) if field != F::Name => {
                let dir = if event.physical_key == PhysicalKey::Code(KeyCode::ArrowUp) { 1.0 } else { -1.0 };
                let step = if self.cmd_down { 0.1 } else if self.shift_down { 5.0 } else { 1.0 };
                if let Some((_, buf, fresh)) = &mut self.artboard_edit {
                    let cur = parse_num(buf, amalith_core::MeasureKind::Length(amalith_core::Unit::Px)).unwrap_or(0.0);
                    *buf = trim_num(cur + dir * step);
                    *fresh = false;
                }
                self.apply_artboard_edit_live();
                self.request_main_redraw();
                true
            }
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    return true;
                };
                let ok = field == F::Name || txt.chars().all(widgets::measurement_char);
                if !ok {
                    return true;
                }
                if let Some((_, buf, fresh)) = &mut self.artboard_edit {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if *fresh {
                            buf.clear();
                            *fresh = false;
                        }
                        buf.push(ch);
                    }
                }
                self.apply_artboard_edit_live();
                self.request_main_redraw();
                true
            }
        }
    }

    /// Whether the pointer is over the Artboard options-bar segment (used
    /// to decide if a press should commit the current field edit).
    pub(in crate::app) fn over_artboard_segment(&self) -> bool {
        if self.pointer_win != self.main_id
            || self.pointer.y < metric_app_bar_h()
            || self.pointer.y >= metric_app_bar_h() + metric_opt_bar_h()
        {
            return false;
        }
        let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
        let bar = opt_bar_rect(w);
        let cx = self.context_bar_ctx();
        context_bar::segment_rect(bar, &cx, context_bar::SegKind::Artboard)
            .is_some_and(|r| r.contains(self.pointer))
    }

    /// Applies the Align spacing field's current buffer live, without
    /// leaving edit mode — called after every keystroke.
    fn apply_align_spacing_edit_live(&mut self) {
        let Some((buf, fresh)) = &self.align_spacing_edit else { return };
        if *fresh {
            return;
        }
        let t = buf.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("auto") {
            self.align_spacing = None;
        } else if let Some(v) = parse_num(buf, amalith_core::MeasureKind::Length(amalith_core::Unit::Px)) {
            self.align_spacing = Some(v.max(0.0));
        }
    }

    pub(in crate::app) fn commit_align_spacing_edit(&mut self) {
        self.apply_align_spacing_edit_live();
        self.align_spacing_edit = None;
        self.request_main_redraw();
    }

    pub(in crate::app) fn align_spacing_field_at_pointer(&mut self) -> bool {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return false;
        }
        let Some(pbody) = self.active_panel_body_at_pointer(PanelKind::Align) else {
            return false;
        };
        panels::align::spacing_field_at(pbody, self.pointer)
    }

    /// Scroll-wheel nudge of the Align spacing field, no click needed.
    /// `None` ("Auto") starts from 0 on the first nudge.
    pub(in crate::app) fn nudge_align_spacing(&mut self, delta: f64) {
        let next = (self.align_spacing.unwrap_or(0.0) + delta).max(0.0);
        self.align_spacing = Some(next);
        if let Some((buf, fresh)) = &mut self.align_spacing_edit {
            *buf = trim_num(next);
            *fresh = false;
        }
        self.request_main_redraw();
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
                self.apply_align_spacing_edit_live();
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::ArrowDown) => {
                let dir = if event.physical_key == PhysicalKey::Code(KeyCode::ArrowUp) { 1.0 } else { -1.0 };
                let step = if self.cmd_down { 0.1 } else if self.shift_down { 5.0 } else { 1.0 };
                self.nudge_align_spacing(dir * step);
                true
            }
            PhysicalKey::Code(
                KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight,
            ) => true,
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    self.commit_align_spacing_edit();
                    return false;
                };
                let numeric = txt.chars().all(widgets::measurement_char);
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
                self.apply_align_spacing_edit_live();
                self.request_main_redraw();
                true
            }
        }
    }

    /// Applies the Opacity field's current buffer to the document without
    /// leaving edit mode — called after every keystroke so the selection
    /// updates live, the same way the stepper arrows already did.
    fn apply_opacity_edit_live(&mut self) {
        let Some(edit) = &self.opacity_edit else { return };
        if edit.fresh {
            return;
        }
        let Some(v) = parse_num(&edit.buf, amalith_core::MeasureKind::Percent) else { return };
        let opacity = (v as f32 / 100.0).clamp(0.0, 1.0);
        self.doc.opacity = opacity;
        if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::SetOpacity {
                objects: self.doc.selection.clone(),
                opacity,
            });
        }
    }

    pub(in crate::app) fn commit_opacity_edit(&mut self) {
        self.apply_opacity_edit_live();
        self.opacity_edit = None;
        self.request_main_redraw();
    }

    pub(in crate::app) fn opacity_field_at_pointer(&mut self) -> bool {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return false;
        }
        if self.pointer_win == self.main_id
            && self.pointer.y >= metric_app_bar_h()
            && self.pointer.y < metric_app_bar_h() + metric_opt_bar_h()
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let bar = opt_bar_rect(w);
            let cx = self.context_bar_ctx();
            return context_bar::opacity_field_at(bar, &cx, self.pointer);
        }
        false
    }

    /// Digit / Enter / Esc stay in the Opacity field.
    pub(in crate::app) fn opacity_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let Some(edit) = &mut self.opacity_edit else {
            return false;
        };
        match widgets::edit_key(edit, event, self.shift_down, self.cmd_down) {
            widgets::EditOutcome::Consumed => {
                self.apply_opacity_edit_live();
                self.request_main_redraw();
                true
            }
            widgets::EditOutcome::Commit => {
                self.commit_opacity_edit();
                true
            }
            widgets::EditOutcome::CommitAndPassThrough => {
                self.commit_opacity_edit();
                false
            }
            widgets::EditOutcome::Cancel => {
                self.opacity_edit = None;
                self.request_main_redraw();
                true
            }
        }
    }

    /// Applies the Stroke Weight field's current buffer live, without
    /// leaving edit mode — called after every keystroke.
    fn apply_stroke_weight_edit_live(&mut self) {
        let Some((buf, fresh)) = &self.stroke_weight_edit else { return };
        if *fresh {
            return;
        }
        let Some(v) = parse_num(buf, amalith_core::MeasureKind::Length(amalith_core::Unit::Px)) else { return };
        let width = v.max(0.0);
        self.doc.stroke_w = width;
        if !self.doc.selection.is_empty() {
            let _ = self.doc.editor.execute(Command::SetStrokeWidth {
                objects: self.doc.selection.clone(),
                width,
            });
        }
    }

    pub(in crate::app) fn commit_stroke_weight_edit(&mut self) {
        self.apply_stroke_weight_edit_live();
        self.stroke_weight_edit = None;
        self.request_main_redraw();
    }

    pub(in crate::app) fn stroke_weight_field_at_pointer(&mut self) -> bool {
        if self.home.is_some() || self.newdoc.is_some() || self.prefs.is_some() {
            return false;
        }
        if self.pointer_win == self.main_id
            && self.pointer.y >= metric_app_bar_h()
            && self.pointer.y < metric_app_bar_h() + metric_opt_bar_h()
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let bar = opt_bar_rect(w);
            let cx = self.context_bar_ctx();
            return context_bar::stroke_weight_field_at(bar, &cx, self.pointer);
        }
        false
    }

    /// Digit / Enter / Esc stay in the Stroke Weight field.
    pub(in crate::app) fn stroke_weight_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if self.stroke_weight_edit.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        use winit::keyboard::{KeyCode, PhysicalKey};
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.commit_stroke_weight_edit();
                true
            }
            PhysicalKey::Code(KeyCode::Escape) => {
                self.stroke_weight_edit = None;
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some((buf, fresh)) = &mut self.stroke_weight_edit {
                    *fresh = false;
                    buf.pop();
                }
                self.apply_stroke_weight_edit_live();
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::ArrowDown) => {
                let dir = if event.physical_key == PhysicalKey::Code(KeyCode::ArrowUp) { 1.0 } else { -1.0 };
                if let Some((buf, fresh)) = &mut self.stroke_weight_edit {
                    let cur = parse_num(buf, amalith_core::MeasureKind::Length(amalith_core::Unit::Px)).unwrap_or(0.0);
                    // Plain arrows match the options-bar stepper: quarter-pt
                    // steps at/below 1pt (0, .25, .5, .75, 1), whole-pt
                    // steps above it — 1pt → 2pt going up, but 1pt → 0.75pt
                    // coming down, so the boundary depends on direction.
                    let step = if self.cmd_down {
                        0.1
                    } else if self.shift_down {
                        5.0
                    } else if dir < 0.0 {
                        if cur <= 1.0 { 0.25 } else { 1.0 }
                    } else if cur < 1.0 {
                        0.25
                    } else {
                        1.0
                    };
                    *buf = trim_num((cur + dir * step).max(0.0));
                    *fresh = false;
                }
                self.apply_stroke_weight_edit_live();
                self.request_main_redraw();
                true
            }
            PhysicalKey::Code(
                KeyCode::ShiftLeft
                | KeyCode::ShiftRight
                | KeyCode::ControlLeft
                | KeyCode::ControlRight
                | KeyCode::AltLeft
                | KeyCode::AltRight
                | KeyCode::SuperLeft
                | KeyCode::SuperRight,
            ) => true,
            _ => {
                let Some(txt) = event.text.as_ref() else {
                    self.commit_stroke_weight_edit();
                    return false;
                };
                let numeric = txt.chars().all(widgets::measurement_char);
                if !numeric {
                    self.commit_stroke_weight_edit();
                    return false;
                }
                if let Some((buf, fresh)) = &mut self.stroke_weight_edit {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if *fresh {
                            buf.clear();
                            *fresh = false;
                        }
                        buf.push(ch);
                    }
                }
                self.apply_stroke_weight_edit_live();
                self.request_main_redraw();
                true
            }
        }
    }
}

/// Parses a measurement field's buffer — arithmetic, plus any per-literal
/// unit suffix (`5in`, `3pt`) converted into `kind`'s own unit — see
/// [`amalith_core::parse_measurement`]. Every options-bar / dialog field
/// in this file reads its buffer through this rather than a bare
/// `str::parse`, so typing e.g. `5in` into a px field converts it.
pub(in crate::app) fn parse_num(s: &str, kind: amalith_core::MeasureKind) -> Option<f64> {
    amalith_core::parse_measurement(s, kind)
}

/// The [`amalith_core::MeasureKind`] a Transform field's own numbers are
/// in — length fields in document px, Rotation/Shear in degrees.
fn xform_field_kind(field: panels::transform::XformField) -> amalith_core::MeasureKind {
    use panels::transform::XformField as F;
    match field {
        F::Rotation | F::Shear => amalith_core::MeasureKind::Angle,
        F::X | F::Y | F::W | F::H => amalith_core::MeasureKind::Length(amalith_core::Unit::Px),
    }
}

pub(in crate::app) fn trim_num(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 5e-5 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}
