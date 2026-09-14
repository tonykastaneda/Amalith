use super::*;
impl App {
    pub(in crate::app) fn spawn_symbol_name_dialog(&mut self, event_loop: &ActiveEventLoop) {
        if self.symbol_name_dialog.is_none() {
            return;
        }
        self.text_blink = Instant::now();

        let pid = PanelId(PanelKind::SymbolNameDlg);
        let fw = crate::symbol_name_dialog::width();
        let fh = crate::symbol_name_dialog::height() + self.theme.tab_strip_h;
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
                .expect("create symbol naming dialog window"),
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

    pub(in crate::app) fn close_symbol_name_dialog(&mut self, commit: bool) {
        self.pending_symbol_name_dialog = false;
        let Some(dlg) = self.symbol_name_dialog.take() else {
            return;
        };
        if commit {
            if let Ok(CommandOutcome::Object(instance)) =
                self.doc.editor.execute(Command::DefineSymbol {
                    ids: dlg.ids,
                    name: Some(if dlg.buf.trim().is_empty() {
                        "New Symbol".to_string()
                    } else {
                        dlg.buf.trim().to_string()
                    }),
                })
            {
                self.doc.selection = vec![instance];
                if let Some(amalith_core::ObjectKind::Symbol(data)) =
                    self.doc.editor.document().object(instance).map(|o| &o.kind)
                {
                    self.doc.selected_symbol = Some(data.definition);
                }
            }
        }
        let pid = PanelId(PanelKind::SymbolNameDlg);
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

    pub(in crate::app) fn symbol_name_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => self.close_symbol_name_dialog(false),
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_symbol_name_dialog(true)
            }
            PhysicalKey::Code(KeyCode::Backspace) => {
                if let Some(d) = &mut self.symbol_name_dialog {
                    d.fresh = false;
                    d.buf.pop();
                }
            }
            _ => {
                if let (Some(d), Some(txt)) = (&mut self.symbol_name_dialog, &event.text) {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        if d.fresh {
                            d.buf.clear();
                            d.fresh = false;
                        }
                        d.buf.push(ch);
                    }
                }
            }
        }
        self.request_main_redraw();
    }
}
