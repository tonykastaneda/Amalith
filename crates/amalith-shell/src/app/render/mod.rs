//! Per-frame rendering for [`App`](super::App): the `redraw` orchestrator
//! (surface acquire → paint → blit → present). The main view, floating
//! windows, and overlays live in the submodules.

mod main_view;
mod overlays;

use super::*;

impl App {
    pub(in crate::app) fn redraw(&mut self, id: WindowId) {
        self.warm_images();
        let Some(host) = self.hosts.get_mut(&id) else {
            return;
        };
        let width = host.surface.config.width;
        let height = host.surface.config.height;
        let scale = self.scale;
        let (wl, hl) = (width as f64 / scale, height as f64 / scale);
        let role = host.role;

        let preview = match &self.drag {
            Drag::MoveObjects {
                start_doc,
                last_doc,
                moved: true,
            } => Some(DragPreview {
                ids: &self.doc.selection,
                delta: if self.shift_down {
                    snap8(*last_doc - *start_doc)
                } else {
                    *last_doc - *start_doc
                },
                dup: self.alt_down,
                xf: None,
                anchors: None,
                handle: None,
            }),
            Drag::Scale { preview, .. } | Drag::Rotate { preview, .. } => Some(DragPreview {
                ids: &self.doc.selection,
                delta: Vec2::ZERO,
                dup: false,
                xf: Some(preview),
                anchors: None,
                handle: None,
            }),
            Drag::MoveAnchors {
                start_doc,
                last_doc,
                moved: true,
            } => Some(DragPreview {
                ids: &[],
                delta: Vec2::ZERO,
                dup: false,
                xf: None,
                anchors: Some((
                    self.doc.anchor_sel.as_slice(),
                    convert::vec2_to_core(*last_doc - *start_doc),
                )),
                handle: None,
            }),
            Drag::MoveHandle {
                object,
                anchor,
                side,
                start_doc,
                last_doc,
            } => Some(DragPreview {
                ids: &[],
                delta: Vec2::ZERO,
                dup: false,
                xf: None,
                anchors: None,
                handle: Some((
                    *object,
                    *anchor,
                    *side,
                    convert::vec2_to_core(*last_doc - *start_doc),
                )),
            }),
            _ => None,
        };
        let draw_shape = match &self.drag {
            Drag::DrawShape {
                tool,
                start_doc,
                cur_doc,
            } => Some((
                *tool,
                convert::rect(shape_rect(
                    *start_doc,
                    *cur_doc,
                    self.shift_down,
                    self.alt_down,
                )),
            )),
            _ => None,
        };
        // Live outline for the Artboard tool (new-artboard rubber-band, or
        // a ghost of an artboard being dragged). Document-space Rect;
        // canvas applies the view transform.
        let artboard_ghost: Option<Rect> = match &self.drag {
            Drag::DrawArtboard { start_doc, cur_doc } => Some(convert::rect(shape_rect(
                *start_doc,
                *cur_doc,
                self.shift_down,
                self.alt_down,
            ))),
            Drag::MoveArtboard {
                id,
                start_doc,
                last_doc,
            } => {
                let mut d = *last_doc - *start_doc;
                if self.shift_down {
                    d = snap8(d);
                }
                self.doc.editor
                    .document()
                    .artboards()
                    .iter()
                    .find(|a| a.id == *id)
                    .map(|a| convert::rect(a.rect) + d)
            }
            Drag::ResizeArtboard {
                handle,
                start_rect,
                start_doc,
                cur_doc,
                ..
            } => Some(convert::rect(resize_rect(
                *start_rect,
                *handle,
                convert::vec2_to_core(*cur_doc - *start_doc),
            ))),
            _ => None,
        };
        // Resize handles around the selected artboard (or its live rect
        // mid-drag), screen-space. Only while the Artboard tool is active.
        let artboard_handles: Option<[Point; 4]> = self
            .doc.selected_artboard
            .filter(|_| self.active_tool == Tool::Artboard)
            .and_then(|id| {
                let committed = self
                    .doc.editor
                    .document()
                    .artboards()
                    .iter()
                    .find(|a| a.id == id)
                    .map(|a| convert::rect(a.rect))?;
                // A Move/Resize drag of this artboard drives the ghost rect.
                let rect = match &self.drag {
                    Drag::MoveArtboard { .. } | Drag::ResizeArtboard { .. } => {
                        artboard_ghost.unwrap_or(committed)
                    }
                    _ => committed,
                };
                let vt = self.doc.view.to_screen();
                Some(handles::rect_quad(rect).map(|p| vt * p))
            });
        let pen_preview = if self.active_tool == Tool::Pen && !self.pen.is_empty() {
            let hover = self.doc_point(self.pointer);
            let near_close = self.pen.len() >= 3
                && self
                    .pen
                    .first()
                    .is_some_and(|f| (f.point - hover).hypot() <= 8.0 / self.doc.view.zoom);
            Some(PenPreview {
                anchors: &self.pen,
                hover,
                near_close,
            })
        } else {
            None
        };
        // Direct Selection shows anchors only for objects that have been
        // selected (Illustrator's white arrow), not every path. A live
        // anchor selection keeps this view (and suppresses the Selection
        // tool's bounding box) even after a ⌘-marquee releases ⌘.
        let direct =
            self.effective_tool() == Tool::DirectSelect || !self.doc.anchor_sel.is_empty();
        // The Pen tool also shows a selected path's nodes (Illustrator:
        // switch V -> P with an object selected and its anchors appear).
        let pen_nodes = self.active_tool == Tool::Pen && !self.doc.selection.is_empty();
        // Hold Space with the Selection tool to peek at every node (read-only;
        // the bounding box stays). Direct Selection proper takes precedence.
        let peek = !direct && !pen_nodes && self.space_peek();
        let anchor_paths: Vec<ObjectId> = if peek {
            self.peek_paths()
        } else if direct || pen_nodes {
            self.node_paths()
        } else {
            Vec::new()
        };
        let anchor_view = (direct || peek || pen_nodes).then_some(AnchorView {
            selected: &self.doc.anchor_sel,
            paths: &anchor_paths,
            peek,
        });

        self.content.reset();
        let representative = self.representative();
        // App-bar status: a file error wins, else the current file name.
        let status_text: Option<String> = self.doc.io_error.clone().or_else(|| {
            self.doc.file_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
        });
        let tab_labels: Vec<String> = (0..self.tabs.len()).map(|i| self.tab_label(i)).collect();
        let active_tab = self.active;
        let zoom_cursor =
            (self.cursor_mode == CanvasCursor::Zoom).then_some(self.zoom_sign >= 0);
        let cursor_glyph = (self.cursor_mode == CanvasCursor::Glyph).then(|| {
            let hint = if self.active_tool == Tool::Pen {
                let hover = self.doc_point(self.pointer);
                let closing = self.pen.len() >= 3
                    && self
                        .pen
                        .first()
                        .is_some_and(|f| (f.point - hover).hypot() <= 8.0 / self.doc.view.zoom);
                if closing {
                    PenHint::Closing
                } else if self.pen_insert_target().is_some() {
                    PenHint::AddPoint
                } else {
                    PenHint::Draw
                }
            } else {
                PenHint::Draw
            };
            (self.effective_tool(), hint)
        });
        let stroke_flyout = self.stroke_flyout_layout(wl);
        let stroke_style_shown = self.stroke_style_repr();
        let panel_text_style = self.active_text_style();
        let panel_text_editing = self.text_edit.is_some();
        match role {
            Role::Main => main_view::paint_main(
                &mut self.content,
                &mut self.text,
                &self.dock,
                self.doc.editor.document(),
                &self.doc.view,
                &self.theme,
                &self.doc.selection,
                self.active_tool,
                self.active_slot,
                self.picker,
                representative,
                self.doc.fill,
                self.doc.stroke,
                self.pointer,
                preview,
                draw_shape,
                artboard_ghost,
                artboard_handles,
                pen_preview,
                anchor_view,
                self.marquee,
                wl,
                hl,
                self.redock_preview.as_ref(),
                status_text.as_deref(),
                &self.doc.expanded_groups,
                self.doc.stroke_w,
                self.doc.opacity,
                self.doc.rename.as_ref().map(|r| (r.target, r.buf.as_str())),
                self.doc.selected_layer,
                self.doc.selected_artboard,
                // On Home, the modal is drawn in the overlay pass below so it
                // lands on top of the Home screen.
                self.newdoc.as_ref().filter(|_| self.home.is_none()),
                &tab_labels,
                active_tab,
                cursor_glyph,
                self.doc.anchor_sel.len(),
                zoom_cursor,
                self.cursor_mode,
                self.last_shape_tool,
                self.shape_flyout,
                self.stroke_popover,
                stroke_style_shown,
                stroke_flyout,
                self.text_edit.as_ref().map(|t| t.object),
                panel_text_style,
                panel_text_editing,
                &self.font_families,
                &self.layer_query,
                self.layer_search_focused,
                self.color_mode,
                &self.recent_colors,
                &self.image_cache,
            ),
            Role::Floating(fid) => {
                let laid = self.floating_layout(fid);
                let exists = self.dock.floating(fid).is_some();
                if exists {
                    // Same tab strip (with ×) and frame as a docked panel.
                    self.content
                        .fill(Fill::NonZero, ID, self.theme.panel_bg, None, &Rect::new(0.0, 0.0, wl, hl));
                    chrome::paint(&mut self.content, &laid, &self.theme, &mut self.text, &tab_label);
                    let area = laid.areas.first();
                    let pid = area.and_then(|a| a.tabs.get(a.active).map(|t| t.panel));
                    if let (Some(area), Some(pid)) = (area, pid) {
                        let body = area.body;
                        let ctx = panels::Ctx {
                            theme: &self.theme,
                            doc: self.doc.editor.document(),
                            selection: &self.doc.selection,
                            active_tool: self.active_tool,
                            pointer: self.pointer,
                            representative,
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
                            text_style: panel_text_style.clone(),
                            text_editing: panel_text_editing,
                            font_families: &self.font_families,
                            layer_query: &self.layer_query,
                            layer_search_focused: self.layer_search_focused,
                            color_mode: self.color_mode,
                            recent: &self.recent_colors,
                        };
                        self.content.push_clip_layer(Fill::NonZero, ID, &body);
                        panels::paint(&mut self.content, &mut self.text, pid, body, &ctx);
                        self.content.pop_layer();
                    }
                }
            }
        }
        if matches!(role, Role::Main) {
            // Live text edit — drawn over the canvas, clipped to the viewport.
            if let Some(obj) = self.text_edit.as_ref().map(|t| t.object) {
                let vp = self.canvas_viewport();
                let world = self.doc.editor.document().world_transform(obj);
                let xf = self.doc.view.to_screen() * convert::affine(world);
                let color = self
                    .doc.editor
                    .document()
                    .object(obj)
                    .and_then(|o| o.appearance.fill.color())
                    .map(convert::color)
                    .unwrap_or(vello::peniko::Color::from_rgb8(0, 0, 0));
                let caret_on = self.text_blink_on();
                let blue = self.theme.accent;
                self.content
                    .push_clip_layer(vello::peniko::Fill::NonZero, ID, &vp);
                if let Some(te) = &mut self.text_edit {
                    te.render(&mut self.content, &mut self.text, xf, color, caret_on, blue);
                }
                self.content.pop_layer();
            }
            if let Some(pk) = self.picker.filter(|_| !self.dock.contains(PanelId("picker"))) {
                picker::paint(
                    &mut self.content,
                    &pk,
                    self.theme.text,
                    &self.theme,
                    &mut self.text,
                );
            }
            self.paint_font_menu();
            // The Home screen covers the canvas; the New Document modal and
            // the About panel each sit on top of that.
            if let Some(hm) = &mut self.home {
                hm.paint(&mut self.content, &mut self.text, &self.theme, wl, hl);
                if let Some(form) = &self.newdoc {
                    newdoc::paint(
                        &mut self.content,
                        &mut self.text,
                        &self.theme,
                        Rect::new(0.0, 0.0, wl, hl),
                        form,
                    );
                }
            }
            if let Some(a) = &mut self.about {
                a.paint(&mut self.content, &mut self.text, wl, hl);
            }
            if let Some(pr) = &mut self.prefs {
                pr.paint(&mut self.content, &mut self.text, &self.theme, wl, hl);
            }
        }
        if self.panel_menu.as_ref().is_some_and(|m| m.win == id) {
            self.paint_panel_menu(wl, hl);
        }
        // Hover tooltip — topmost, in whichever window the pointer is over.
        if let Some(tt) = &self.tooltip {
            if self.pointer_win == Some(id) && tt.since.elapsed().as_millis() >= 350 {
                overlays::draw_tooltip(
                    &mut self.content,
                    &mut self.text,
                    &self.theme,
                    &tt.text,
                    tt.anchor,
                    wl,
                    hl,
                );
            }
        }
        self.scene.reset();
        self.scene.append(&self.content, Some(Affine::scale(scale)));

        let host = self.hosts.get_mut(&id).unwrap();
        let device = &self.context.devices[host.surface.dev_id];
        let surface_texture = match host.surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };
        host.renderer
            .render_to_texture(
                &device.device,
                &device.queue,
                &self.scene,
                &host.surface.target_view,
                &vello::RenderParams {
                    base_color: palette::css::BLACK,
                    width,
                    height,
                    antialiasing_method: AaConfig::Area,
                },
            )
            .expect("render");
        let mut encoder = device
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("surface blit"),
            });
        host.surface.blitter.copy(
            &device.device,
            &mut encoder,
            &host.surface.target_view,
            &surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default()),
        );
        device.queue.submit([encoder.finish()]);
        surface_texture.present();

        // Keep the window pumping frames. Cheap under AutoVsync and it
        // sidesteps a class of "first RedrawRequested was dropped" bugs
        // where the window opens blank until an event happens to arrive.
        host.window.request_redraw();
    }
}
