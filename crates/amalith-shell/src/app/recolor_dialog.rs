//! Recolor's float-only host and one-commit lifecycle, following primitive dialogs.
use super::*;
use crate::recolordlg::{self, Hit};

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (App, ObjectId) {
        let mut app = App::new();
        let mut document = Document::new("Recolor");
        let layer = LayerId::new();
        document.insert_layer(amalith_core::Layer::new(layer, "Artwork"), 0);
        let id = ObjectId::new();
        let mut object = amalith_core::Object::rectangle(
            id,
            amalith_core::ObjectParent::Layer(layer),
            amalith_core::Rect::new(0., 0., 10., 10.),
        );
        object
            .appearance
            .set_fill(amalith_core::Paint::Solid(amalith_core::Color::rgb(
                1., 0., 0.,
            )));
        document.insert_object(object, 0).unwrap();
        app.doc = Doc::new(Editor::new(document));
        app.doc.selection = vec![id];
        app.recolor_dialog = recolordlg::RecolorDialog::open(
            app.doc.id,
            app.doc.editor.revision(),
            app.doc.editor.document().clone(),
            vec![id],
        );
        (app, id)
    }

    #[test]
    fn recolor_preview_cancel_and_commit_are_transactional() {
        let (mut app, id) = fixture();
        let original = app
            .doc
            .editor
            .document()
            .object(id)
            .unwrap()
            .appearance
            .fill();
        let revision = app.doc.editor.revision();
        let d = app.recolor_dialog.as_mut().unwrap();
        d.hex = "00FF00".into();
        assert!(d.commit_hex());
        assert_ne!(d.rendered.object(id).unwrap().appearance.fill(), original);
        assert_eq!(app.doc.editor.revision(), revision);
        assert_eq!(
            app.doc
                .editor
                .document()
                .object(id)
                .unwrap()
                .appearance
                .fill(),
            original
        );
        app.close_recolor_dialog(false);
        assert!(app.recolor_dialog.is_none());
        assert_eq!(app.doc.editor.revision(), revision);
        assert_eq!(app.doc.selection, vec![id]);

        app.recolor_dialog = recolordlg::RecolorDialog::open(
            app.doc.id,
            revision,
            app.doc.editor.document().clone(),
            vec![id],
        );
        let d = app.recolor_dialog.as_mut().unwrap();
        d.hex = "00FF00".into();
        d.commit_hex();
        app.close_recolor_dialog(true);
        assert!(app.recolor_dialog.is_none());
        assert_eq!(
            app.doc
                .editor
                .document()
                .object(id)
                .unwrap()
                .appearance
                .fill(),
            amalith_core::Paint::Solid(amalith_core::Color::rgb(0., 1., 0.))
        );
        app.doc.editor.undo().unwrap();
        assert_eq!(
            app.doc
                .editor
                .document()
                .object(id)
                .unwrap()
                .appearance
                .fill(),
            original
        );
    }

    #[test]
    fn recolor_excluded_colors_and_invalid_hex_do_not_change_preview() {
        let (mut app, id) = fixture();
        let original = app
            .doc
            .editor
            .document()
            .object(id)
            .unwrap()
            .appearance
            .fill();
        let d = app.recolor_dialog.as_mut().unwrap();
        d.hex = "00".into();
        assert!(!d.commit_hex());
        assert_eq!(d.rendered.object(id).unwrap().appearance.fill(), original);
        d.hex = "00FF00".into();
        assert!(d.commit_hex());
        app.recolor_action(Hit::Toggle(0));
        let d = app.recolor_dialog.as_ref().unwrap();
        assert!(d.mapping().is_empty());
        assert_eq!(d.rendered.object(id).unwrap().appearance.fill(), original);
    }
}

impl App {
    pub(super) fn spawn_recolor_dialog(&mut self, event_loop: &ActiveEventLoop) {
        if self.recolor_dialog.is_some() {
            return;
        }
        if self.picker.is_some() {
            self.dismiss_picker(false);
        }
        let Some(d) = recolordlg::RecolorDialog::open(
            self.doc.id,
            self.doc.editor.revision(),
            self.doc.editor.document().clone(),
            self.doc.selection.clone(),
        ) else {
            self.doc.io_error =
                Some("Select vector artwork with editable fill or stroke colors.".into());
            self.request_main_redraw();
            return;
        };
        self.recolor_dialog = Some(d);
        let (mw, mh) = self.main_logical_size().unwrap_or((1280., 800.));
        let (fw, fh) = (
            recolordlg::width(),
            recolordlg::height() + self.theme.tab_strip_h,
        );
        let o = self.main_inner_origin();
        let pos = Point::new(
            o.x + ((mw - fw) * 0.5).max(4.),
            o.y + ((mh - fh) * 0.5).max(4.),
        );
        let fid = self.dock.float_alone(
            PanelId(PanelKind::RecolorDlg),
            [pos.x as f32, pos.y as f32, fw as f32, fh as f32],
        );
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Recolor Artwork")
                        .with_decorations(false)
                        .with_resizable(false)
                        .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
                        .with_inner_size(LogicalSize::new(fw, fh))
                        .with_position(LogicalPosition::new(pos.x, pos.y)),
                )
                .expect("create recolor window"),
        );
        let host = self.make_host(window.clone(), Role::Floating(fid));
        self.hosts.insert(window.id(), host);
        window.request_redraw();
        self.request_main_redraw();
    }

    pub(super) fn close_recolor_dialog(&mut self, apply: bool) {
        if apply {
            let Some(d) = self.recolor_dialog.as_mut() else {
                return;
            };
            if d.editing && !d.commit_hex() {
                self.request_main_redraw();
                return;
            }
            // Selection changes must never redirect a pending recolor operation.
            if self.doc.id != d.document || self.doc.editor.revision() != d.revision {
                d.error = Some("Artwork changed. Cancel and reopen Recolor.".into());
                self.request_main_redraw();
                return;
            }
            if !d.mapping().is_empty() {
                if let Err(e) = self.doc.editor.execute(Command::RecolorArtwork {
                    objects: d.ids.clone(),
                    colors: d.mapping(),
                }) {
                    d.error = Some(e.to_string());
                    self.request_main_redraw();
                    return;
                }
            }
        }
        if self.recolor_picker.is_some() {
            self.dismiss_picker(false);
        }
        self.recolor_dialog = None;
        let fid = self.dock.floating_id_of(PanelId(PanelKind::RecolorDlg));
        self.dock.remove(PanelId(PanelKind::RecolorDlg));
        let dead: Vec<_> = self
            .hosts
            .iter()
            .filter_map(|(wid, h)| {
                matches!(h.role, Role::Floating(id) if Some(id)==fid).then_some(*wid)
            })
            .collect();
        for wid in dead {
            self.hosts.remove(&wid);
            self.focused.remove(&wid);
        }
        self.request_main_redraw();
    }

    pub(super) fn recolor_action(&mut self, hit: Hit) {
        match hit {
            Hit::Ok => {
                self.close_recolor_dialog(true);
                return;
            }
            Hit::Cancel => {
                self.close_recolor_dialog(false);
                return;
            }
            _ => {}
        }
        let Some(d) = self.recolor_dialog.as_mut() else {
            return;
        };
        if d.editing && !matches!(hit, Hit::Hex) && !d.commit_hex() {
            self.request_main_redraw();
            return;
        }
        match hit {
            Hit::Row(i) => {
                d.selected = i;
                d.sync_hex();
            }
            Hit::Picker(i) => {
                d.selected = i;
                d.sync_hex();
                self.recolor_picker = Some(i);
                self.picker = Some(picker::Picker::from_color(
                    panels::PaintSlot::Fill,
                    Point::ZERO,
                    Some(d.replacement[i]),
                ));
            }
            Hit::Toggle(i) => {
                d.enabled[i] = !d.enabled[i];
                d.refresh();
            }
            Hit::Hex => {
                d.editing = true;
                d.fresh = true;
            }
            Hit::Step(i, delta) => {
                let c = &mut d.replacement[d.selected];
                let channel = match i {
                    0 => &mut c.r,
                    1 => &mut c.g,
                    _ => &mut c.b,
                };
                *channel = (*channel + delta / 255.).clamp(0., 1.);
                d.refresh();
            }
            Hit::Previous => d.page = d.page.saturating_sub(1),
            Hit::Next => d.page = (d.page + 1).min((d.source.len() - 1) / 8),
            Hit::Reset => {
                d.replacement = d.source.clone();
                d.enabled.fill(true);
                d.refresh();
            }
            Hit::Shuffle => {
                d.replacement.rotate_right(1);
                d.refresh();
            }
            Hit::Preview => d.preview = !d.preview,
            Hit::Ok | Hit::Cancel => {}
        }
        self.request_main_redraw();
    }
    pub(super) fn recolor_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_recolor_dialog(false);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                if self.recolor_dialog.as_ref().is_some_and(|d| d.editing) {
                    self.recolor_dialog.as_mut().unwrap().commit_hex();
                    self.request_main_redraw();
                } else {
                    self.close_recolor_dialog(true);
                }
                return;
            }
            _ => {}
        }
        let Some(d) = self.recolor_dialog.as_mut().filter(|d| d.editing) else {
            return;
        };
        if self.cmd_down {
            if event.physical_key == PhysicalKey::Code(KeyCode::KeyA) {
                d.fresh = true;
            }
            return;
        }
        if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
            if d.fresh {
                d.hex.clear();
                d.fresh = false;
            } else {
                d.hex.pop();
            }
        } else if let Some(s) = &event.text {
            for c in s.chars().filter(|c| c.is_ascii_hexdigit()) {
                if d.fresh {
                    d.hex.clear();
                    d.fresh = false;
                }
                if d.hex.len() < 6 {
                    d.hex.push(c.to_ascii_uppercase());
                }
            }
        }
        self.request_main_redraw();
    }
}
