//! The generic Distort & Transform effect dialog — spawning the floating
//! panel window, closing it, and its keyboard. Shares `offsetdlg`'s own
//! panel slot (`Self::offset_panel_id()`) rather than getting a second
//! one — see `panels::Ctx::effect_dialog`'s own doc comment for why.
//! Layout/fields live in [`crate::effectdlg`]; this is the `App`-side
//! glue, mirroring `app/offset_dialog.rs`.

use super::*;

/// Which of the two dialog families `about_to_wait` should spawn for a
/// deferred `Action::OpenEffectDialog`/`AppearanceAddEffect` — resolved
/// once, at action-handling time, from either the target effect's own
/// stored kind (editing) or the fx-menu entry that was clicked (adding).
pub(in crate::app) enum PendingAppearanceEffectDialog {
    Offset { item_index: usize, effect_index: Option<usize> },
    Distort { item_index: usize, effect_index: Option<usize>, kind: effectdlg::EffectKind },
}

pub(in crate::app) enum EffectClose {
    Cancel,
    Ok,
}

impl App {
    /// Spawns `effectdlg::EffectDialog` for `kind`, seeded from the
    /// target item's existing effect at `effect_index` (editing) or that
    /// kind's own defaults (`effect_index: None`, adding) — a no-op if
    /// the panel's target object or item has gone away since the row was
    /// clicked.
    pub(in crate::app) fn spawn_effect_dialog(
        &mut self,
        event_loop: &ActiveEventLoop,
        item_index: usize,
        effect_index: Option<usize>,
        kind: effectdlg::EffectKind,
    ) {
        let Some(object) = self.appearance_target() else { return };
        let Some(item) = self
            .doc
            .editor
            .document()
            .object(object)
            .and_then(|o| o.appearance.items.get(item_index).cloned())
        else {
            return;
        };
        let current = effect_index.and_then(|i| item.effects().get(i).cloned());
        self.close_effect_dialog(EffectClose::Cancel);
        self.effect_dialog = Some(effectdlg::EffectDialog::open_for_item(
            object,
            item_index,
            effect_index,
            kind,
            current.as_ref(),
        ));
        self.spawn_offset_window(event_loop, effectdlg::metric_w(), effectdlg::body_height(kind));
    }

    /// `Ok` pushes or replaces the target item's effect entry (mirroring
    /// `close_offset_dialog`'s own `AppearanceItem` branch exactly, just
    /// for whichever Distort & Transform kind this dialog is editing);
    /// `Cancel`, and simply closing the window, do nothing to the
    /// document at all, since Preview never touched it either.
    pub(in crate::app) fn close_effect_dialog(&mut self, action: EffectClose) {
        // See `close_offset_dialog`'s own matching comment — the two
        // dialog families share one panel slot, so closing either clears
        // both.
        self.offset_dialog = None;
        let Some(dlg) = self.effect_dialog.take() else { return };
        if let EffectClose::Ok = action {
            if let Some(obj) = self.doc.editor.document().object(dlg.object) {
                let mut items = obj.appearance.items.clone();
                if let Some(item) = items.get_mut(dlg.item_index) {
                    let effect = dlg.resolved_effect();
                    let effects = item.effects_mut();
                    match dlg.effect_index.filter(|&i| i < effects.len()) {
                        Some(i) => effects[i] = effect,
                        None => effects.push(effect),
                    }
                    let _ = self.doc.editor.execute(Command::SetAppearanceItems { object: dlg.object, items });
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

    pub(in crate::app) fn effect_dialog_key(&mut self, event: &winit::event::KeyEvent) {
        if !event.state.is_pressed() {
            return;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.close_effect_dialog(EffectClose::Cancel);
                return;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.close_effect_dialog(EffectClose::Ok);
                return;
            }
            _ => {}
        }
        let Some(dlg) = self.effect_dialog.as_mut() else { return };
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Tab) => {
                dlg.focus = (dlg.focus + 1) % dlg.fields.len().max(1);
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
