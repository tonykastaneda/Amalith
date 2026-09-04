//! Export for Screens — the `App`-side glue: spawning the floating panel
//! window, closing it, its keyboard, the folder picker, and kicking off
//! the export. The panel UI lives in [`crate::export`]; the render-and-
//! write backend is [`App::run_export`]. Split out of `app/mod.rs`.

use super::*;

/// The float-only panel id for the Export for Screens dialog.
pub(in crate::app) const EXPORT_PID: PanelId = PanelId("export-screens");

impl App {
    /// Queue the dialog to open on the next `about_to_wait` (the menu /
    /// shortcut paths have no `event_loop` to spawn a window with).
    pub(in crate::app) fn request_export_dialog(&mut self) {
        if self.doc.editor.document().artboards().is_empty() {
            self.doc.io_error = Some("Nothing to export — the document has no artboards.".into());
            self.request_main_redraw();
            return;
        }
        self.pending_export = true;
    }

    pub(in crate::app) fn spawn_export_dialog(&mut self, event_loop: &ActiveEventLoop) {
        self.close_export(false);

        let items: Vec<crate::export::Item> = self
            .doc
            .editor
            .document()
            .artboards()
            .iter()
            .map(|a| crate::export::Item {
                name: a.name.clone(),
                rect: a.rect,
                checked: true,
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let dest = self
            .doc
            .file_path
            .as_ref()
            .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
            .or_else(|| dirs_desktop())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        self.export = Some(crate::export::ExportForScreens::new(items, dest));
        self.text_blink = Instant::now();

        let fw = crate::export::W;
        let fh = crate::export::H + self.theme.tab_strip_h;
        let (mw, mh) = self.main_logical_size().unwrap_or((1280.0, 800.0));
        let o = self.main_inner_origin();
        let pos = Point::new(
            o.x + ((mw - fw) * 0.5).max(4.0),
            o.y + ((mh - fh) * 0.5).max(4.0),
        );
        let rect = [pos.x as f32, pos.y as f32, fw as f32, fh as f32];
        let id = self.dock.float_alone(EXPORT_PID, rect);

        let attrs = Window::default_attributes()
            .with_title(tab_label(EXPORT_PID))
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            .with_inner_size(LogicalSize::new(fw, fh))
            .with_position(LogicalPosition::new(pos.x, pos.y));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("create export window"),
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

    /// Close the dialog and its window. `run` first performs the export.
    pub(in crate::app) fn close_export(&mut self, run: bool) {
        let Some(dlg) = self.export.take() else {
            return;
        };
        if run {
            self.run_export(&dlg);
        }
        self.dock.remove(EXPORT_PID);
        let dead: Vec<WindowId> = self
            .hosts
            .iter()
            .filter_map(|(wid, h)| match h.role {
                Role::Floating(fid) if self.dock.floating(fid).is_none() => Some(*wid),
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

    /// Pick the export destination folder.
    pub(in crate::app) fn export_pick_folder(&mut self) {
        let start = self.export.as_ref().map(|d| d.dest.clone());
        let mut dlg = rfd::FileDialog::new();
        if let Some(s) = start.filter(|p| p.is_dir()) {
            dlg = dlg.set_directory(s);
        }
        if let Some(dir) = dlg.pick_folder() {
            if let Some(d) = self.export.as_mut() {
                d.dest = dir;
            }
        }
        self.request_main_redraw();
    }

    pub(in crate::app) fn export_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_export(false);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_export(true);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.export.as_mut() else {
            return;
        };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Backspace) => dlg.backspace(),
            PhysicalKey::Code(KeyCode::Tab) => dlg.defocus(),
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

    /// Perform the export described by `dlg`. Backend lands next; for now
    /// it reports what it would have written.
    pub(in crate::app) fn run_export(&mut self, dlg: &crate::export::ExportForScreens) {
        let sel = dlg.selected();
        if sel.is_empty() {
            self.doc.io_error = Some("Nothing selected to export.".into());
            self.request_main_redraw();
            return;
        }
        self.doc.io_error = Some(format!(
            "Export backend not wired yet — would write {} file(s) to {}",
            dlg.total_exports(),
            dlg.dest.display()
        ));
        self.request_main_redraw();
    }
}

fn dirs_desktop() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Desktop"))
}
