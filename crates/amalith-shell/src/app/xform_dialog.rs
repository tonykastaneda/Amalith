//! The Reflect / Shear dialogs — spawning the floating panel window,
//! closing it (Cancel reverts any live preview, OK commits, Copy commits
//! onto a duplicate and leaves the original untouched), and its keyboard.
//! The dialog's own layout / fields live in [`crate::xformdlg`]; this is
//! just the `App`-side glue, mirroring `app/shape_dialog.rs`.

use super::*;

/// What closing the dialog should do with the transform.
pub(in crate::app) enum XformClose {
    Cancel,
    Ok,
    Copy,
}

impl App {
    /// The float-only panel id standing in for `kind`'s dialog.
    pub(in crate::app) fn xform_panel_id(kind: xformdlg::Kind) -> PanelId {
        PanelId(match kind {
            xformdlg::Kind::Reflect => "xformdlg.reflect",
            xformdlg::Kind::Shear => "xformdlg.shear",
        })
    }

    /// Open the Reflect or Shear dialog for the current selection, pivoting
    /// around its bounding-box centre — the canvas right-click menu's
    /// entry point. (A future Reflect/Shear *tool*, where the user clicks
    /// to place that pivot instead, would call `xformdlg::TransformDialog::
    /// open` the same way, just with a different pivot source.)
    pub(in crate::app) fn spawn_xform_dialog(&mut self, event_loop: &ActiveEventLoop, kind: xformdlg::Kind) {
        if self.doc.selection.is_empty() {
            return;
        }
        self.close_xform_dialog(XformClose::Cancel);
        let doc = self.doc.editor.document();
        let Some(bounds) = self
            .doc
            .selection
            .iter()
            .filter_map(|&id| doc.bounds_of(id))
            .reduce(|a, b| a.union(b))
        else {
            return;
        };
        let pivot = bounds.center();
        let originals: Vec<(ObjectId, amalith_core::Affine)> = self
            .doc
            .selection
            .iter()
            .filter_map(|&id| doc.object(id).map(|o| (id, o.transform)))
            .collect();
        let default_axis = match kind {
            xformdlg::Kind::Reflect => xformdlg::Axis::Vertical,
            xformdlg::Kind::Shear => xformdlg::Axis::Horizontal,
        };
        self.xform_dialog = Some(xformdlg::TransformDialog::open(kind, default_axis, pivot, originals));
        self.text_blink = Instant::now();
        // Illustrator's own Reflect/Shear dialogs preview their default
        // state the instant they open (Reflect defaults to a real
        // Vertical flip) — match that instead of waiting for a first edit.
        self.apply_xform_preview();

        let pid = Self::xform_panel_id(kind);
        let fw = xformdlg::W;
        let fh = xformdlg::body_height(kind) + self.theme.tab_strip_h;
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
                .expect("create xform dialog window"),
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

    /// The dialog's own body rect (window-local, `(0, 0)`-based) — its
    /// window is fixed-size and never resized, so this is just the same
    /// formula `spawn_xform_dialog` used to size it, run back through the
    /// real frame layout for an exact match with what's on screen.
    fn xform_dialog_body(&mut self) -> Option<Rect> {
        let kind = self.xform_dialog.as_ref()?.kind;
        let pid = Self::xform_panel_id(kind);
        let fid = self.dock.floating_id_of(pid)?;
        let h = self.theme.tab_strip_h + xformdlg::body_height(kind);
        let bounds = Rect::new(0.0, 0.0, xformdlg::W, h);
        Some(self.build_master_frame(fid, bounds).body)
    }

    /// Scroll wheel over a field nudges it — no click needed, matching
    /// every other numeric field in the app. Returns whether it was
    /// consumed.
    pub(in crate::app) fn xform_wheel(&mut self, dy: f64) -> bool {
        if self.xform_dialog.is_none() || dy.abs() < 0.5 {
            return false;
        }
        let Some(body) = self.xform_dialog_body() else { return false };
        let p = self.pointer;
        let focus = self.xform_dialog.as_ref().and_then(|d| match xformdlg::hit(d, body, p) {
            xformdlg::Hit::FocusField(f) => Some(f),
            _ => None,
        });
        let Some(focus) = focus else { return false };
        let dir = if dy > 0.0 { 1.0 } else { -1.0 };
        if let Some(dlg) = self.xform_dialog.as_mut() {
            dlg.nudge(focus, dir);
        }
        self.apply_xform_preview();
        self.request_main_redraw();
        true
    }

    /// This dialog's resolved `(id, new local transform)` pairs for its
    /// originally-captured objects, using each one's *current* parent
    /// world transform (objects don't reparent while the dialog is open,
    /// but nothing here assumes the parent transform itself is frozen).
    fn resolve_xform_items(&self, dlg: &xformdlg::TransformDialog) -> Vec<(ObjectId, amalith_core::Affine)> {
        let doc = self.doc.editor.document();
        dlg.originals
            .iter()
            .filter_map(|&(id, local)| {
                let obj = doc.object(id)?;
                let parent = match obj.parent {
                    amalith_core::ObjectParent::Layer(_) => amalith_core::Affine::IDENTITY,
                    amalith_core::ObjectParent::Group(g) => doc.world_transform(g),
                };
                Some((id, dlg.resolve(local, parent)))
            })
            .collect()
    }

    /// Re-run the live preview against the selection, if it's on. Every
    /// call recomputes from `dlg.originals` (never cumulatively), so
    /// repeated edits can't drift and this is safe to call on every
    /// keystroke / dial tick.
    pub(in crate::app) fn apply_xform_preview(&mut self) {
        let Some(dlg) = &self.xform_dialog else { return };
        if !dlg.preview {
            return;
        }
        let items = self.resolve_xform_items(dlg);
        if !items.is_empty() {
            let _ = self.doc.editor.execute(Command::SetTransforms { items });
            self.request_main_redraw();
        }
    }

    /// Close the dialog and its window, per `action`.
    pub(in crate::app) fn close_xform_dialog(&mut self, action: XformClose) {
        let Some(dlg) = self.xform_dialog.take() else { return };
        let pid = Self::xform_panel_id(dlg.kind);
        match action {
            XformClose::Cancel => {
                // Undo whatever the live preview already applied.
                if dlg.preview && !dlg.originals.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetTransforms { items: dlg.originals.clone() });
                }
            }
            XformClose::Ok => {
                let items = self.resolve_xform_items(&dlg);
                if !items.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetTransforms { items });
                }
            }
            XformClose::Copy => {
                // Put the originals back first, so the duplicate is cut
                // from the untouched selection, not an already-previewed one.
                if dlg.preview && !dlg.originals.is_empty() {
                    let _ = self.doc.editor.execute(Command::SetTransforms { items: dlg.originals.clone() });
                }
                let ids: Vec<ObjectId> = dlg.originals.iter().map(|&(id, _)| id).collect();
                if let Ok(new_ids) = self.doc.editor.duplicate_objects(&ids, amalith_core::Vec2::ZERO) {
                    let doc = self.doc.editor.document();
                    let items: Vec<(ObjectId, amalith_core::Affine)> = new_ids
                        .iter()
                        .zip(dlg.originals.iter())
                        .filter_map(|(&nid, &(_, local))| {
                            let obj = doc.object(nid)?;
                            let parent = match obj.parent {
                                amalith_core::ObjectParent::Layer(_) => amalith_core::Affine::IDENTITY,
                                amalith_core::ObjectParent::Group(g) => doc.world_transform(g),
                            };
                            Some((nid, dlg.resolve(local, parent)))
                        })
                        .collect();
                    if !items.is_empty() {
                        let _ = self.doc.editor.execute(Command::SetTransforms { items });
                    }
                    self.doc.selection = new_ids;
                    self.sync_align_mode();
                }
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

    pub(in crate::app) fn xform_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_xform_dialog(XformClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_xform_dialog(XformClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.xform_dialog.as_mut() else { return };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                if self.shift_down {
                    dlg.focus_prev();
                } else {
                    dlg.focus_next();
                }
            }
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::ArrowUp) => {
                dlg.nudge_focused(if self.cmd_down { 0.1 } else if self.shift_down { 10.0 } else { 1.0 });
            }
            PhysicalKey::Code(KeyCode::ArrowDown) => {
                dlg.nudge_focused(if self.cmd_down { -0.1 } else if self.shift_down { -10.0 } else { -1.0 });
            }
            _ => {
                if let Some(txt) = &event.text {
                    for ch in txt.chars().filter(|c| !c.is_control()) {
                        dlg.push_char(ch);
                    }
                }
            }
        }
        self.text_blink = Instant::now();
        self.apply_xform_preview();
        self.request_main_redraw();
    }
}
