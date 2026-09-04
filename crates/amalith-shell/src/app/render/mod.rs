//! Per-frame rendering for [`App`](super::App): the `redraw` orchestrator
//! (surface acquire → paint → blit → present). The main view, floating
//! windows, and overlays live in the submodules.

mod main_view;
mod overlays;

use super::*;

impl App {
    pub(in crate::app) fn redraw(&mut self, id: WindowId) {
        // Frame-rate estimate: wall-clock between consecutive redraws.
        // Meaningful only while something is animating (pan / drag) —
        // rendering is on demand, so it freezes at the last burst's rate
        // when idle. A big gap (idle → resume) reseeds rather than
        // averaging in a huge dt.
        let now = Instant::now();
        if let Some(prev) = self.last_frame {
            let dt = now.duration_since(prev).as_secs_f32();
            if dt > 1e-4 {
                let inst = 1.0 / dt;
                self.fps = if self.fps <= 0.0 || dt > 0.4 {
                    inst
                } else {
                    self.fps * 0.9 + inst * 0.1
                };
            }
        }
        self.last_frame = Some(now);

        self.warm_images();
        self.prune_isolation();
        let iso_root = self.isolation_root();
        let Some(host) = self.hosts.get_mut(&id) else {
            return;
        };
        let width = host.surface.config.width;
        let height = host.surface.config.height;
        let scale = self.scale;
        let (wl, hl) = (width as f64 / scale, height as f64 / scale);
        let role = host.role;

        // Previewed frame sizes for a live text-box resize (empty for any
        // other drag) — owned here so `DragPreview` can borrow a slice.
        let resize_previews: Vec<TextBoxPreview> = match &self.drag {
            Drag::ResizeTextBox {
                handle,
                start_bounds,
                frames,
                start_doc,
                cur_doc,
            } => {
                let rects = self.text_box_resize_rects(
                    *handle,
                    *start_bounds,
                    frames,
                    *start_doc,
                    *cur_doc,
                );
                frames
                    .iter()
                    .filter_map(|&(id, origin, _, _)| {
                        rects.iter().find(|(rid, _)| *rid == id).map(|(_, r)| {
                            TextBoxPreview {
                                id,
                                width: r.width(),
                                height: r.height(),
                                origin_delta: Vec2::new(r.x0 - origin.x, r.y0 - origin.y),
                            }
                        })
                    })
                    .collect()
            }
            _ => Vec::new(),
        };

        let preview = match &self.drag {
            Drag::MoveObjects {
                start_doc,
                last_doc,
                moved: true,
                ..
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
                text_boxes: &[],
            }),
            Drag::Scale { preview, .. }
            | Drag::Rotate { preview, .. }
            | Drag::RotateTool {
                preview, moved: true, ..
            } => Some(DragPreview {
                ids: &self.doc.selection,
                delta: Vec2::ZERO,
                dup: false,
                xf: Some(preview),
                anchors: None,
                handle: None,
                text_boxes: &[],
            }),
            Drag::ResizeTextBox { .. } => Some(DragPreview {
                ids: &[],
                delta: Vec2::ZERO,
                dup: false,
                xf: None,
                anchors: None,
                handle: None,
                text_boxes: &resize_previews,
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
                text_boxes: &[],
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
                text_boxes: &[],
            }),
            _ => None,
        };
        let draw_shape = match &self.drag {
            // Line preview: raw start → end (Shift snaps to 45°), packed
            // into a Rect the canvas reads as its two endpoints — not a
            // bbox, which would collapse a horizontal / vertical line.
            Drag::DrawShape {
                tool: Tool::Line,
                start_doc,
                cur_doc,
            } => {
                let end = if self.shift_down {
                    constrained(Some(*start_doc), *cur_doc, true)
                } else {
                    *cur_doc
                };
                Some((Tool::Line, Rect::new(start_doc.x, start_doc.y, end.x, end.y)))
            }
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
            // Type tool rubber-band → the area-text box being dragged out.
            Drag::DrawText { start_doc, cur_doc } => Some((
                Tool::Text,
                convert::rect(shape_rect(
                    *start_doc,
                    *cur_doc,
                    self.shift_down,
                    self.alt_down,
                )),
            )),
            // Threading a new frame from a loaded out-port.
            Drag::ThreadNewBox {
                start_doc, cur_doc, ..
            } => Some((
                Tool::Text,
                convert::rect(shape_rect(
                    *start_doc,
                    *cur_doc,
                    self.shift_down,
                    self.alt_down,
                )),
            )),
            // (Resizing an area-text frame draws its own handle box via the
            // text_box drag preview — no rubber-band here.)
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
                fill: self.doc.fill.color().map(convert::color),
                stroke: self.doc.stroke.color().map(convert::color),
                stroke_w: self.doc.stroke_w,
                style: self.doc.stroke_style,
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
        // The Rotate tool does the same — you turn about a reference
        // point, so the 8 scale handles would be misleading; show nodes.
        let pen_nodes = matches!(self.active_tool, Tool::Pen | Tool::Rotate)
            && !self.doc.selection.is_empty();
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
        // Which on-screen node the pointer is over — matches the Direct
        // Selection press hit radius (6 screen px) so the swelling node is
        // exactly the one a click would grab.
        let anchor_hover = if (direct || peek || pen_nodes) && matches!(self.drag, Drag::None) {
            crate::anchors::topmost_anchor_among(
                self.doc.editor.document(),
                &anchor_paths,
                self.doc_point(self.pointer),
                6.0 / self.doc.view.zoom,
            )
        } else {
            None
        };
        let anchor_view = (direct || peek || pen_nodes).then_some(AnchorView {
            selected: &self.doc.anchor_sel,
            paths: &anchor_paths,
            peek,
            hover: anchor_hover,
        });

        self.content.reset();
        let representative = self.representative();
        let (fill_mixed, stroke_mixed) = self.selection_paint_mixed();
        let active_artboard = self.current_artboard();
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
            // Selection tool: badge the cursor when a click would select
            // something.
            let over_selectable = self.active_tool == Tool::Select
                && matches!(self.drag, Drag::None)
                && select::topmost_selectable_at(
                    self.doc.editor.document(),
                    self.doc_point(self.pointer),
                    self.visible_doc_rect(),
                )
                .is_some();
            (self.effective_tool(), hint, over_selectable)
        });
        let stroke_flyout = self.stroke_flyout_layout(wl);
        let ab_bar = self.artboard_bar();
        let ab_edit = self.artboard_edit.clone();
        let (ab_link, ab_fill_menu) = (self.artboard_link, self.artboard_fill_menu);
        let grad_ctx = self.gradient_ctx();
        let stroke_style_shown = self.stroke_style_repr();
        let panel_text_style = self.active_text_style();
        let panel_text_align = self.active_text_align();
        let panel_text_paragraph = self.active_text_paragraph();
        let panel_text_editing = self.text_edit.is_some();
        let rotate_pivot = (self.active_tool == Tool::Rotate)
            .then(|| self.rotate_pivot())
            .flatten();
        // Ruler guides: committed ones (minus any being dragged) plus the
        // live preview line for a create / move drag.
        use main_view::GuideMark;
        let guide_lines: Vec<(amalith_core::GuideOrient, f64, GuideMark)> = if self.guides_hidden {
            Vec::new()
        } else {
            let moving = match &self.drag {
                Drag::MoveGuide { id, .. } => Some(*id),
                _ => None,
            };
            let sel = &self.selected_guides;
            let mut v: Vec<_> = self
                .doc
                .editor
                .document()
                .guides()
                .iter()
                .filter(|g| moving != Some(g.id))
                .map(|g| {
                    let mark = if sel.contains(&g.id) {
                        GuideMark::Selected
                    } else {
                        GuideMark::Idle
                    };
                    (g.orient, g.pos, mark)
                })
                .collect();
            match &self.drag {
                Drag::NewGuide { orient, pos } | Drag::MoveGuide { orient, pos, .. } => {
                    v.push((*orient, *pos, GuideMark::Active));
                }
                _ => {}
            }
            v
        };
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
                fill_mixed,
                stroke_mixed,
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
                active_artboard,
                &tab_labels,
                active_tab,
                cursor_glyph,
                self.doc.anchor_sel.len(),
                zoom_cursor,
                self.cursor_mode,
                self.last_shape_tool,
                self.shape_flyout,
                self.stroke_popover,
                self.text_edit.as_ref().map(|t| t.object),
                panel_text_style,
                panel_text_align,
                panel_text_paragraph,
                panel_text_editing,
                &self.font_families,
                &self.layer_query,
                self.layer_search_focused,
                self.color_mode,
                &self.recent_colors,
                &self.image_cache,
                self.xform_ref,
                self.xform_constrain,
                self.xform_edit.as_ref().map(|(f, s, _)| (*f, s.as_str())),
                self.align_to,
                self.align_to_menu.is_some(),
                self.align_spacing,
                self.align_spacing_edit.as_ref().map(|(s, _)| s.as_str()),
                self.key_object,
                &self.panel_scroll,
                self.settings.cull_inset,
                self.settings.show_cull_outline,
                self.rulers,
                rotate_pivot,
                &guide_lines,
                self.outline_mode,
                iso_root,
                self.layer_drop.map(|(_, _, row, into)| (row, into)),
                ab_bar,
                ab_edit.as_ref().map(|(f, s, _)| (*f, s.as_str())),
                ab_link,
                ab_fill_menu,
                grad_ctx.clone(),
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
                        let clip_body = area.body;
                        let (body, scroll) =
                            panels::scrolled_body(pid, area.body, self.panel_scroll_of(pid));
                        let caret_blink = self.text_blink_on();
                        let ctx = panels::Ctx {
                            theme: &self.theme,
                            doc: self.doc.editor.document(),
                            selection: &self.doc.selection,
                            active_tool: self.active_tool,
                            pointer: self.pointer,
                            representative,
                            fill_mixed,
                            stroke_mixed,
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
                            text_align: panel_text_align,
                            text_paragraph: panel_text_paragraph,
                            text_editing: panel_text_editing,
                            font_families: &self.font_families,
                            layer_query: &self.layer_query,
                            layer_search_focused: self.layer_search_focused,
                            layer_scroll: self.panel_scroll_of(PanelId("layers")),
                            layer_drop: if pid == PanelId("layers") {
                                self.layer_drop.map(|(_, _, row, into)| (row, into))
                            } else {
                                None
                            },
                            color_mode: self.color_mode,
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
                            shape_dialog: self
                                .shape_dialog
                                .as_ref()
                                .map(|d| (d, caret_blink)),
                            export: self.export.as_ref().map(|d| (d, caret_blink)),
                            gradient: self.gradient_ctx(),
                        };
                        self.content.push_clip_layer(Fill::NonZero, ID, &clip_body);
                        panels::paint(&mut self.content, &mut self.text, pid, body, &ctx);
                        panels::paint_scrollbar(
                            &mut self.content,
                            clip_body,
                            pid,
                            scroll,
                            &self.theme,
                        );
                        self.content.pop_layer();
                    }
                }
            }
        }
        if matches!(role, Role::Main) {
            if self.rulers {
                self.paint_rulers();
            }
            // The Stroke flyout — drawn after the rulers so a dropped-down
            // popover isn't hidden by the ruler strip.
            if self.stroke_popover {
                let shown_weight = representative
                    .map(|a| a.stroke_width)
                    .unwrap_or(self.doc.stroke_w);
                stroke_panel::paint(
                    &mut self.content,
                    &mut self.text,
                    &self.theme,
                    &stroke_flyout,
                    &stroke_style_shown,
                    shown_weight,
                );
            }
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
            self.paint_align_to_menu();
            self.paint_isolation_bar();
            self.paint_ruler_menu();
            self.paint_ctx_menu();
            // The Home screen covers the canvas; the New Document modal and
            // the About panel each sit on top of that (and of the canvas).
            if let Some(hm) = &mut self.home {
                hm.paint(&mut self.content, &mut self.text, &self.theme, wl, hl);
            }
            if self.newdoc.is_some() {
                let caret = self.text_blink_on();
                newdoc::paint(
                    &mut self.content,
                    &mut self.text,
                    &self.theme,
                    Rect::new(0.0, 0.0, wl, hl),
                    self.newdoc.as_mut().unwrap(),
                    caret,
                );
            }
            if let Some(a) = &mut self.about {
                a.paint(&mut self.content, &mut self.text, wl, hl);
            }
            if let Some(pr) = &mut self.prefs {
                pr.paint(&mut self.content, &mut self.text, &self.theme, wl, hl);
            }
            if let Some(pal) = &mut self.palette {
                pal.paint(&mut self.content, &mut self.text, &self.theme, wl, hl);
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
        // FPS counter (Preferences ▸ Debug ▸ Show FPS Counter) — bottom
        // centre, over everything.
        if matches!(role, Role::Main) && self.settings.show_fps {
            let s = format!("{:.0} fps", self.fps.max(0.0));
            let tw = self.text.measure(&s, 11.0);
            let pill = Rect::from_center_size(Point::new(wl * 0.5, hl - 16.0), (tw + 20.0, 18.0));
            self.content.fill(
                Fill::NonZero,
                ID,
                Color::from_rgba8(0, 0, 0, 150),
                None,
                &pill.to_rounded_rect(4.0),
            );
            self.text.draw(
                &mut self.content,
                &s,
                11.0,
                Color::from_rgb8(0x66, 0xff, 0x99),
                pill.x0 + 10.0,
                pill.center().y + 4.0,
            );
        }
        self.scene.reset();
        self.scene.append(&self.content, Some(Affine::scale(scale)));

        {
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
            let mut encoder =
                device
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
        }

        // A frame reached the screen. Rendering is on demand from here —
        // `about_to_wait` no longer pumps a redraw every loop — so record
        // the two bits it needs to decide whether to wake us again: the
        // first frame is now guaranteed, and the caret-blink phase this
        // frame showed.
        self.first_frame_done = true;
        self.last_caret_drawn = self.text_blink_on();
    }

    /// Append the canvas rulers to `self.content`. The static layer
    /// (strips, ticks, labels — the parley-heavy part) is cached and only
    /// rebuilt when the view or canvas region changes; the pointer marker
    /// is redrawn every frame.
    fn paint_rulers(&mut self) {
        let region = self.canvas_region();
        let v = self.doc.view;
        // Ruler `0` sits at the active artboard's top-left (Illustrator
        // default): the artboard last worked in, else the first one, else
        // the document origin.
        let active = self.current_artboard();
        let origin = {
            let doc = self.doc.editor.document();
            active
                .and_then(|id| doc.artboard(id))
                .or_else(|| doc.artboards().first())
                .map(|a| Point::new(a.rect.x0, a.rect.y0))
                .unwrap_or(Point::ZERO)
        };
        let unit = self.doc.editor.document().settings.default_unit;
        let key = (v.pan.x, v.pan.y, v.zoom, region, origin.x, origin.y, unit);
        let fresh = self
            .ruler_cache
            .as_ref()
            .is_some_and(|(px, py, z, r, ox, oy, u, _)| {
                (*px, *py, *z, *r, *ox, *oy, *u) == key
            });
        if !fresh {
            let mut layer = Scene::new();
            rulers::build(&mut layer, &mut self.text, &self.theme, region, &v, origin, unit);
            self.ruler_cache = Some((
                v.pan.x, v.pan.y, v.zoom, region, origin.x, origin.y, unit, layer,
            ));
        }
        if let Some((.., layer)) = &self.ruler_cache {
            self.content.append(layer, None);
        }
        rulers::marker(&mut self.content, &self.theme, region, self.pointer);
    }
}
