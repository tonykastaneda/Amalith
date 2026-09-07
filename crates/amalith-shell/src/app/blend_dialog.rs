//! The Blend Options dialog — spawning the floating panel window, closing
//! it (OK commits the chosen spacing; Cancel reverts whatever Preview
//! already applied, back to the spacing the group had when the dialog
//! opened), and its keyboard. Layout/fields live in [`crate::blenddlg`];
//! this is the `App`-side glue, mirroring `app/xform_dialog.rs`.

use super::*;

pub(in crate::app) enum BlendClose {
    Cancel,
    Ok,
}

impl App {
    pub(in crate::app) fn blend_panel_id() -> PanelId {
        PanelId("blenddlg")
    }

    /// Open Blend Options for `group` — a no-op if it isn't a blend group.
    pub(in crate::app) fn spawn_blend_dialog(&mut self, event_loop: &ActiveEventLoop, group: ObjectId) {
        let blend = match self.doc.editor.document().object(group).map(|o| &o.kind) {
            Some(amalith_core::ObjectKind::Group(g)) => g.blend,
            _ => None,
        };
        let Some(blend) = blend else { return };
        self.close_blend_dialog(BlendClose::Cancel);
        self.blend_dialog = Some(blenddlg::BlendDialog::open(group, blend.spacing, blend.spine));
        self.text_blink = Instant::now();

        let pid = Self::blend_panel_id();
        let fw = blenddlg::W;
        let fh = blenddlg::body_height() + self.theme.tab_strip_h;
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
                .expect("create blend dialog window"),
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
    fn blend_dialog_body(&mut self) -> Option<Rect> {
        let pid = Self::blend_panel_id();
        let fid = self.dock.floating_id_of(pid)?;
        let h = self.theme.tab_strip_h + blenddlg::body_height();
        let bounds = Rect::new(0.0, 0.0, blenddlg::W, h);
        Some(self.build_master_frame(fid, bounds).body)
    }

    /// Scroll wheel over the numeric field nudges it. Returns whether it
    /// was consumed.
    pub(in crate::app) fn blend_wheel(&mut self, dy: f64) -> bool {
        if self.blend_dialog.is_none() || dy.abs() < 0.5 {
            return false;
        }
        let Some(body) = self.blend_dialog_body() else { return false };
        if !matches!(blenddlg::hit(body, self.pointer), blenddlg::Hit::Field) {
            return false;
        }
        let dir = if dy > 0.0 { 1.0 } else { -1.0 };
        if let Some(dlg) = self.blend_dialog.as_mut() {
            dlg.nudge(dir);
        }
        self.apply_blend_preview();
        self.request_main_redraw();
        true
    }

    /// Re-runs the blend with whatever the dialog currently shows, if
    /// Preview is on. Safe to call after every edit (row pick, field
    /// keystroke, nudge) — a no-op while Preview is off.
    pub(in crate::app) fn apply_blend_preview(&mut self) {
        let Some(dlg) = &self.blend_dialog else { return };
        if !dlg.preview {
            return;
        }
        let (group, spacing, spine) = (dlg.group, dlg.resolved_spacing(), dlg.spine);
        let _ = self.doc.editor.execute(Command::SetBlendOptions { group, spacing, spine });
    }

    pub(in crate::app) fn close_blend_dialog(&mut self, action: BlendClose) {
        let Some(dlg) = self.blend_dialog.take() else { return };
        match action {
            BlendClose::Ok => {
                let spacing = dlg.resolved_spacing();
                let _ = self.doc.editor.execute(Command::SetBlendOptions {
                    group: dlg.group,
                    spacing,
                    spine: dlg.spine,
                });
            }
            BlendClose::Cancel => {
                // Only Preview ever touched the real document — undo just
                // that, back to what the group had on open.
                if dlg.preview {
                    let _ = self.doc.editor.execute(Command::SetBlendOptions {
                        group: dlg.group,
                        spacing: dlg.original_spacing,
                        spine: dlg.spine,
                    });
                }
            }
        }
        let pid = Self::blend_panel_id();
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

    pub(in crate::app) fn blend_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_blend_dialog(BlendClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_blend_dialog(BlendClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.blend_dialog.as_mut() else { return };
        if !dlg.focused {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::ArrowUp) => dlg.nudge(if self.shift_down { 10.0 } else { 1.0 }),
            PhysicalKey::Code(KeyCode::ArrowDown) => dlg.nudge(if self.shift_down { -10.0 } else { -1.0 }),
            _ => {
                if let Some(txt) = &event.text {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        dlg.push_char(ch);
                    }
                }
            }
        }
        self.apply_blend_preview();
        self.text_blink = Instant::now();
        self.request_main_redraw();
    }
}
