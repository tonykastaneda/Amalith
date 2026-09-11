//! The Offset Path dialog — spawning the floating panel window, closing
//! it, and its keyboard. Preview is purely a canvas overlay
//! (`render::overlays::paint_offset_preview`) computed fresh every frame
//! from each target's own untouched geometry — the real document is
//! never touched until OK, so Cancel (or just closing the window) needs
//! no cleanup at all. Layout/fields live in [`crate::offsetdlg`]; this is
//! the `App`-side glue, mirroring `app/blend_dialog.rs`.

use super::*;

pub(in crate::app) enum OffsetClose {
    Cancel,
    Ok,
}

impl App {
    pub(in crate::app) fn offset_panel_id() -> PanelId {
        PanelId(PanelKind::Offsetdlg)
    }

    /// Open Offset Path for the current selection's path objects — a
    /// no-op if none of them are (or resolve to) a plain path.
    pub(in crate::app) fn spawn_offset_dialog(&mut self, event_loop: &ActiveEventLoop) {
        let doc = self.doc.editor.document();
        let originals: Vec<(ObjectId, amalith_core::PathData)> = self
            .doc.selection
            .iter()
            .filter_map(|&id| Some((id, doc.object(id)?.kind.path_data()?.clone())))
            .collect();
        if originals.is_empty() {
            return;
        }
        self.close_offset_dialog(OffsetClose::Cancel);
        self.offset_dialog = Some(offsetdlg::OffsetDialog::open(originals));
        self.spawn_offset_window(event_loop);
    }

    /// Same dialog, retargeted at one Appearance-panel item's live
    /// non-destructive effect (see [`offsetdlg::Target::AppearanceItem`])
    /// instead of the current selection — a no-op if the panel's target
    /// object or item has gone away since the row was clicked.
    pub(in crate::app) fn spawn_offset_dialog_for_appearance_item(&mut self, event_loop: &ActiveEventLoop, idx: usize) {
        let Some(object) = self.appearance_target() else { return };
        let Some(item) = self
            .doc
            .editor
            .document()
            .object(object)
            .and_then(|o| o.appearance.items.get(idx).copied())
        else {
            return;
        };
        self.close_offset_dialog(OffsetClose::Cancel);
        self.offset_dialog = Some(offsetdlg::OffsetDialog::open_for_item(object, idx, item.offset()));
        self.spawn_offset_window(event_loop);
    }

    /// The floating-window plumbing shared by both spawn entry points —
    /// identical regardless of `self.offset_dialog`'s target.
    fn spawn_offset_window(&mut self, event_loop: &ActiveEventLoop) {
        self.text_blink = Instant::now();

        let pid = Self::offset_panel_id();
        let fw = offsetdlg::metric_w();
        let fh = offsetdlg::body_height() + self.theme.tab_strip_h;
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
                .expect("create offset path dialog window"),
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
    fn offset_dialog_body(&mut self) -> Option<Rect> {
        let pid = Self::offset_panel_id();
        let fid = self.dock.floating_id_of(pid)?;
        let h = self.theme.tab_strip_h + offsetdlg::body_height();
        let bounds = Rect::new(0.0, 0.0, offsetdlg::metric_w(), h);
        Some(self.build_master_frame(fid, bounds).body)
    }

    /// Scroll wheel over a numeric field nudges it. Returns whether it
    /// was consumed.
    pub(in crate::app) fn offset_wheel(&mut self, dy: f64) -> bool {
        if self.offset_dialog.is_none() || dy.abs() < 0.5 {
            return false;
        }
        let Some(body) = self.offset_dialog_body() else { return false };
        let field = self.offset_dialog.as_ref().and_then(|d| match offsetdlg::hit(d, body, self.pointer) {
            offsetdlg::Hit::Offset => Some(offsetdlg::Field::Offset),
            offsetdlg::Hit::MiterLimit => Some(offsetdlg::Field::MiterLimit),
            _ => None,
        });
        let Some(field) = field else { return false };
        let dir = if dy > 0.0 { 1.0 } else { -1.0 };
        if let Some(dlg) = self.offset_dialog.as_mut() {
            dlg.focus = field;
            dlg.nudge(dir);
        }
        self.request_main_redraw();
        true
    }

    /// `Ok` commits the offset as new sibling objects, behind their
    /// sources (`Editor::offset_path` never touches the sources — see
    /// `compile_offset_path`), and selects every result, matching
    /// Illustrator; `Cancel`, and simply closing the window, do nothing to
    /// the document at all, since Preview never touched it either.
    pub(in crate::app) fn close_offset_dialog(&mut self, action: OffsetClose) {
        let Some(dlg) = self.offset_dialog.take() else { return };
        if let OffsetClose::Ok = action {
            match dlg.target {
                offsetdlg::Target::Objects => {
                    if let Ok(new_ids) = self.doc.editor.offset_path(
                        &dlg.objects(),
                        dlg.resolved_offset(),
                        dlg.join,
                        dlg.resolved_miter_limit(),
                    ) {
                        self.doc.selection = new_ids;
                    }
                }
                offsetdlg::Target::AppearanceItem { object, index } => {
                    if let Some(obj) = self.doc.editor.document().object(object) {
                        let mut items = obj.appearance.items.clone();
                        if let Some(item) = items.get_mut(index) {
                            item.set_offset(Some(dlg.resolved_effect()));
                            let _ = self.doc.editor.execute(Command::SetAppearanceItems { object, items });
                        }
                    }
                }
            }
        }
        let pid = Self::offset_panel_id();
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

    pub(in crate::app) fn offset_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_offset_dialog(OffsetClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_offset_dialog(OffsetClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.offset_dialog.as_mut() else { return };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                dlg.focus = match dlg.focus {
                    offsetdlg::Field::Offset if dlg.join == amalith_core::LineJoin::Miter => {
                        offsetdlg::Field::MiterLimit
                    }
                    _ => offsetdlg::Field::Offset,
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
