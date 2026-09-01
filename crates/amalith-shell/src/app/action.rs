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
                    let _ = self.doc.editor.execute(Command::DeleteObjects {
                        ids: std::mem::take(&mut self.doc.selection),
                    });
                }
            }
            panels::Action::DeleteArtboard => {
                if let Some(id) = self.doc.selected_artboard.take() {
                    let _ = self.doc.editor.execute(Command::DeleteArtboard { id });
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
                    .unwrap_or_else(|| amalith_core::Rect::new(0.0, 0.0, 1200.0, 800.0));
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
            panels::Action::OpenFontMenu(kind, anchor) => {
                self.open_font_menu(kind, anchor);
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
