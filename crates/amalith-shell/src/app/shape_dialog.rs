//! The exact-size shape dialogs — spawning the floating panel window,
//! closing it (and creating the shape on OK), and its keyboard. The
//! dialog's own layout / fields / geometry live in
//! [`crate::shapedialog`]; this is just the `App`-side glue. Split out of
//! `app/mod.rs`.

use super::*;

impl App {
    /// The float-only panel id standing in for `tool`'s exact-size dialog.
    pub(in crate::app) fn shape_panel_id(tool: Tool) -> PanelId {
        PanelId(match tool {
            Tool::Rectangle => "shapedlg.rect",
            Tool::RoundedRect => "shapedlg.round",
            Tool::Ellipse => "shapedlg.ellipse",
            Tool::Polygon => "shapedlg.polygon",
            Tool::Star => "shapedlg.star",
            _ => "shapedlg.rect",
        })
    }

    /// True when floating group `fid` holds a panel that must never dock
    /// and never shows in the Window menu — the colour picker or a shape
    /// dialog.
    pub(in crate::app) fn is_float_only(&self, fid: u64) -> bool {
        if self.dock.floating_id_of(PanelId("picker")) == Some(fid) {
            return true;
        }
        if self.export.is_some() && self.dock.floating_id_of(export::EXPORT_PID) == Some(fid) {
            return true;
        }
        self.shape_dialog
            .as_ref()
            .is_some_and(|d| self.dock.floating_id_of(Self::shape_panel_id(d.tool)) == Some(fid))
    }

    /// Open the exact-size dialog for `tool` (anchored at document-space
    /// `anchor`) as its own floating window — same plumbing as the colour
    /// picker: movable by the tab strip, not dockable, not in the Window
    /// menu.
    pub(in crate::app) fn spawn_shape_dialog(&mut self, event_loop: &ActiveEventLoop, tool: Tool, anchor: Point) {
        if !tool.is_shape() {
            return;
        }
        self.close_shape_dialog(false);
        self.shape_dialog = Some(shapedialog::ShapeDialog::open(
            tool,
            anchor,
            &self.shape_params,
        ));
        self.text_blink = Instant::now();

        let pid = Self::shape_panel_id(tool);
        let fw = shapedialog::W;
        let fh = shapedialog::body_height(tool) + self.theme.tab_strip_h;
        let (mw, mh) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let o = self.main_inner_origin();
        let pos = Point::new(
            o.x + ((mw - fw) * 0.5).max(4.0),
            o.y + ((mh - fh) * 0.5).max(4.0),
        );
        let rect = [pos.x as f32, pos.y as f32, fw as f32, fh as f32];
        let id = self.dock.float_alone(pid, rect);

        let attrs = Window::default_attributes()
            .with_title(tab_label(pid))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create shape dialog window"),
        );
        let wid = window.id();
        let host = self.make_host(window.clone(), Role::Floating(id));
        self.hosts.insert(wid, host);
        window.request_redraw();
        self.request_main_redraw();
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
    }

    /// Close the shape dialog and its window. `commit` first creates the
    /// shape from the entered values.
    pub(in crate::app) fn close_shape_dialog(&mut self, commit: bool) {
        let Some(mut dlg) = self.shape_dialog.take() else {
            return;
        };
        let pid = Self::shape_panel_id(dlg.tool);
        if commit {
            dlg.commit_all();
            dlg.write_params(&mut self.shape_params);
            let layer = self.ensure_layer();
            let cmd = match dlg.geometry() {
                shapedialog::Geometry::Rect(rect) => Command::CreateRect {
                    layer,
                    rect,
                    name: None,
                },
                shapedialog::Geometry::Ellipse(rect) => Command::CreateEllipse {
                    layer,
                    rect,
                    name: None,
                },
                shapedialog::Geometry::Path(path) => Command::CreatePath {
                    layer,
                    path,
                    name: None,
                },
            };
            if let Ok(CommandOutcome::Object(id)) = self.doc.editor.execute(cmd) {
                self.doc.selection = vec![id];
                self.apply_new_appearance(id);
                self.sync_align_mode();
            }
        }
        // Drop the panel and close the window it lived in.
        self.dock.remove(pid);
        let dead: Vec<WindowId> = self
            .hosts
            .iter()
            .filter_map(|(wid, h)| match h.role {
                Role::Floating(fid) if self.dock.master(fid).is_none() => Some(*wid),
                _ => None,
            })
            .collect();
        for wid in dead {
            self.hosts.remove(&wid);
            self.focused.remove(&wid);
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_window(&self.dock);
        }
        self.request_main_redraw();
    }

    pub(in crate::app) fn shape_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_shape_dialog(false);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_shape_dialog(true);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.shape_dialog.as_mut() else {
            return;
        };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                if self.shift_down {
                    dlg.focus_prev();
                } else {
                    dlg.focus_next();
                }
            }
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::ArrowUp) => dlg.focus_prev(),
            PhysicalKey::Code(KeyCode::ArrowDown) => dlg.focus_next(),
            _ => {
                if let Some(txt) = &event.text {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        dlg.push_char(ch);
                    }
                }
            }
        }
        self.text_blink = Instant::now();
        self.request_main_redraw();
    }
}
