//! Layers Panel Options dialog — spawning the floating panel window and
//! closing it. Layout/fields live in [`crate::layerspaneldlg`]; this is
//! the `App`-side glue, mirroring `app/layer_dialog.rs`. No keyboard
//! handling needed (unlike Layer Options): every field here is a radio
//! button, nothing is typed.

use super::*;

pub(in crate::app) enum LayersPanelOptionsClose {
    Cancel,
    Ok,
}

impl App {
    pub(in crate::app) fn layers_panel_options_panel_id() -> PanelId {
        PanelId(PanelKind::LayersPanelOptionsDlg)
    }

    pub(in crate::app) fn spawn_layers_panel_options_dialog(&mut self, event_loop: &ActiveEventLoop) {
        self.close_layers_panel_options_dialog(LayersPanelOptionsClose::Cancel);
        self.layers_panel_options_dialog = Some(layerspaneldlg::LayersPanelOptionsDialog::open(&self.settings));

        let pid = Self::layers_panel_options_panel_id();
        let fw = layerspaneldlg::metric_w();
        let fh = layerspaneldlg::body_height() + self.theme.tab_strip_h;
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
                .expect("create layers panel options dialog window"),
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

    /// `Ok` commits both fields to `self.settings` and saves immediately
    /// (same "write straight through, no batching" convention the
    /// Symbols panel's own hamburger toggle already uses); `Cancel`, and
    /// simply closing the window, touch nothing.
    pub(in crate::app) fn close_layers_panel_options_dialog(&mut self, action: LayersPanelOptionsClose) {
        let Some(dlg) = self.layers_panel_options_dialog.take() else { return };
        if let LayersPanelOptionsClose::Ok = action {
            self.settings.layer_thumbnail_size = dlg.size;
            self.settings.layer_thumbnail_contents = dlg.contents;
            settings::save(&self.settings);
            self.request_main_redraw();
        }
        let pid = Self::layers_panel_options_panel_id();
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

    pub(in crate::app) fn layers_panel_options_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => self.close_layers_panel_options_dialog(LayersPanelOptionsClose::Cancel),
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => self.close_layers_panel_options_dialog(LayersPanelOptionsClose::Ok),
            _ => {}
        }
    }
}
