//! The Area Type Options dialog — spawning the floating panel window,
//! closing it, and its keyboard. Preview is a live override read straight
//! off `self.area_type_dialog` by the render path (`canvas.rs`'s
//! `TextBoxPreview`) — the real document is never touched until OK, so
//! Cancel (or just closing the window) needs no cleanup at all. Layout/
//! fields live in [`crate::areatypedlg`]; this is the `App`-side glue,
//! mirroring `app/offset_dialog.rs`.

use super::*;

pub(in crate::app) enum AreaTypeClose {
    Cancel,
    Ok,
}

impl App {
    pub(in crate::app) fn area_type_panel_id() -> PanelId {
        PanelId(PanelKind::AreaTypeDlg)
    }

    /// Open Area Type Options for the current selection — a no-op unless
    /// it's a single Area Type text frame.
    pub(in crate::app) fn spawn_area_type_dialog(&mut self, event_loop: &ActiveEventLoop) {
        // A live Type-tool edit session's uncommitted keystrokes (and
        // resize) aren't reflected in `self.doc` yet — reading the
        // document straight away seeded the dialog from stale content
        // and, worse, let the live edit's own eventual commit clobber
        // whatever the dialog changed. Commit it first, same as Undo/
        // Redo already do before touching the document.
        if self.text_edit.is_some() {
            self.commit_text_edit();
        }
        let target = match self.doc.selection.as_slice() {
            [id] => *id,
            _ => return,
        };
        let Some(amalith_core::ObjectKind::Text(td)) =
            self.doc.editor.document().object(target).map(|o| &o.kind)
        else {
            return;
        };
        let amalith_core::TextKind::Area { width, height } = td.kind else {
            return;
        };
        let vertical = td.vertical;
        let cross_align = td.cross_align;
        let data = td.clone();
        // An auto-size box has no stored height to seed the field with —
        // measure its actual current content instead of falling back to
        // `width`, so unchecking Auto Size doesn't hand the user an
        // arbitrary square frame (see `AreaTypeDialog::open`'s doc comment).
        let seed_height = height.unwrap_or_else(|| textedit::measure_text_data(&data, &mut self.text).height());
        self.close_area_type_dialog(AreaTypeClose::Cancel);
        self.area_type_dialog = Some(areatypedlg::AreaTypeDialog::open(
            target,
            vertical,
            width,
            seed_height,
            height.is_none(),
            cross_align,
        ));
        self.text_blink = Instant::now();

        let pid = Self::area_type_panel_id();
        let fw = areatypedlg::metric_w();
        let fh = areatypedlg::body_height() + self.theme.tab_strip_h;
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
                .expect("create area type options dialog window"),
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

    /// The dialog's own body rect (window-local), for hit-testing.
    fn area_type_dialog_body(&mut self) -> Option<Rect> {
        let pid = Self::area_type_panel_id();
        let fid = self.dock.floating_id_of(pid)?;
        let h = self.theme.tab_strip_h + areatypedlg::body_height();
        let bounds = Rect::new(0.0, 0.0, areatypedlg::metric_w(), h);
        Some(self.build_master_frame(fid, bounds).body)
    }

    /// Scroll wheel over a numeric field nudges it. Returns whether it
    /// was consumed.
    pub(in crate::app) fn area_type_wheel(&mut self, dy: f64) -> bool {
        if self.area_type_dialog.is_none() || dy.abs() < 0.5 {
            return false;
        }
        let Some(body) = self.area_type_dialog_body() else { return false };
        let field = self.area_type_dialog.as_ref().and_then(|d| match areatypedlg::hit(d, body, self.pointer) {
            areatypedlg::Hit::Width => Some(areatypedlg::Field::Width),
            areatypedlg::Hit::Height => Some(areatypedlg::Field::Height),
            _ => None,
        });
        let Some(field) = field else { return false };
        let dir = if dy > 0.0 { 1.0 } else { -1.0 };
        if let Some(dlg) = self.area_type_dialog.as_mut() {
            dlg.focus = field;
            dlg.nudge(dir);
        }
        self.request_main_redraw();
        true
    }

    /// `Ok` commits the width/height/cross-align to the target frame in
    /// one `Command::SetText`; `Cancel`, and simply closing the window,
    /// do nothing to the document at all, since Preview never touched it
    /// either.
    pub(in crate::app) fn close_area_type_dialog(&mut self, action: AreaTypeClose) {
        let Some(dlg) = self.area_type_dialog.take() else { return };
        if let AreaTypeClose::Ok = action {
            if let Some(amalith_core::ObjectKind::Text(t)) =
                self.doc.editor.document().object(dlg.target).map(|o| &o.kind)
            {
                let mut data = t.clone();
                data.kind = amalith_core::TextKind::Area {
                    width: dlg.resolved_width(),
                    height: if dlg.auto_size { None } else { Some(dlg.resolved_height()) },
                };
                // `cross_align` only means anything for vertical text
                // (see `TextData::cross_align`'s doc comment) — the
                // dialog's Align segment is greyed and unclickable for a
                // horizontal box, so its value there is never the user's.
                if dlg.vertical {
                    data.cross_align = dlg.align;
                }
                data.local_bounds = textedit::measure_text_data(&data, &mut self.text);
                let _ = self.doc.editor.execute(Command::SetText { object: dlg.target, data });
            }
        }
        let pid = Self::area_type_panel_id();
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

    pub(in crate::app) fn area_type_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_area_type_dialog(AreaTypeClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_area_type_dialog(AreaTypeClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.area_type_dialog.as_mut() else { return };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                dlg.focus = match dlg.focus {
                    areatypedlg::Field::Width if !dlg.auto_size => areatypedlg::Field::Height,
                    _ => areatypedlg::Field::Width,
                };
            }
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::ArrowUp) => dlg.nudge(if self.shift_down { 5.0 } else { 1.0 }),
            PhysicalKey::Code(KeyCode::ArrowDown) => dlg.nudge(if self.shift_down { -5.0 } else { -1.0 }),
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
