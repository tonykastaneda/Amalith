//! The Layer Options dialog — spawning the floating panel window, closing
//! it, and its keyboard. Layout/fields live in [`crate::layerdlg`]; this
//! is the `App`-side glue, mirroring `app/offset_dialog.rs`.

use super::*;

pub(in crate::app) enum LayerDialogClose {
    Cancel,
    Ok,
}

impl App {
    pub(in crate::app) fn layer_dialog_panel_id() -> PanelId {
        PanelId(PanelKind::LayerOptionsDlg)
    }

    /// Open Layer Options for `id` — a no-op if the layer no longer
    /// exists (the row that triggered this could have been deleted
    /// between the double-click and `about_to_wait` picking it up).
    pub(in crate::app) fn spawn_layer_dialog(&mut self, event_loop: &ActiveEventLoop, id: amalith_core::LayerId) {
        let Some(layer) = self.doc.editor.document().layer(id).cloned() else {
            return;
        };
        self.close_layer_dialog(LayerDialogClose::Cancel);
        self.layer_dialog = Some(layerdlg::LayerOptionsDialog::open(&layer));
        self.text_blink = Instant::now();

        let pid = Self::layer_dialog_panel_id();
        let fw = layerdlg::metric_w();
        let fh = layerdlg::body_height() + self.theme.tab_strip_h;
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
                .expect("create layer options dialog window"),
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

    /// `Ok` commits every field as one `Command::SetLayerOptions`;
    /// `Cancel`, and simply closing the window, do nothing to the
    /// document at all — nothing here ever touches the real layer until
    /// this point.
    pub(in crate::app) fn close_layer_dialog(&mut self, action: LayerDialogClose) {
        let Some(dlg) = self.layer_dialog.take() else { return };
        if let LayerDialogClose::Ok = action {
            let _ = self.doc.editor.execute(Command::SetLayerOptions {
                id: dlg.id,
                options: dlg.options(),
            });
            self.request_main_redraw();
        }
        let pid = Self::layer_dialog_panel_id();
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

    pub(in crate::app) fn layer_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_layer_dialog(LayerDialogClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_layer_dialog(LayerDialogClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.layer_dialog.as_mut() else { return };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
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
