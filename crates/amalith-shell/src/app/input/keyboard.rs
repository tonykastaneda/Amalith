//! Keyboard handling for the main window — modal capture, the Type
//! editor, ⌘-shortcuts, and bare-key tool switches. `window_event`
//! delegates its `KeyboardInput` arm here.

use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use amalith_commands::{Command, CommandOutcome};

use crate::prefs::{self, KeyChord};
use crate::dock::PanelId;
use crate::textedit;
use crate::tool::Tool;

use super::super::{App, Drag, PastePlace};

impl App {
    pub(in crate::app) fn on_key(&mut self, event: KeyEvent) {
        // The command palette (⌘K) swallows every key while open.
        if self.palette.is_some() {
            if !event.state.is_pressed() {
                return;
            }
            use winit::keyboard::{Key, NamedKey};
            // Vertical nav — the single-line search field doesn't use these.
            match &event.logical_key {
                Key::Named(NamedKey::ArrowDown) => {
                    if let Some(p) = &mut self.palette {
                        p.move_sel(1);
                    }
                    self.request_main_redraw();
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    if let Some(p) = &mut self.palette {
                        p.move_sel(-1);
                    }
                    self.request_main_redraw();
                    return;
                }
                _ => {}
            }
            let mods = textedit::Mods {
                shift: self.shift_down,
                alt: self.alt_down,
                meta: self.cmd_down,
            };
            let logical = event.logical_key.clone();
            let typed = event.text.clone();
            if self.clipboard.is_none() {
                self.clipboard = arboard::Clipboard::new().ok();
            }
            let clip = self.clipboard.as_mut();
            let resp = self
                .palette
                .as_mut()
                .map(|p| p.field.key(&logical, mods, typed.as_deref(), clip, &mut self.text));
            match resp {
                Some(crate::text_field::Resp::Cancel) => self.palette = None,
                Some(crate::text_field::Resp::Submit) => {
                    let sel = self.palette.as_ref().and_then(|p| p.selected());
                    match sel {
                        Some(i) => self.run_palette_cmd(i),
                        None => self.palette = None,
                    }
                }
                Some(crate::text_field::Resp::Changed) => {
                    if let Some(p) = &mut self.palette {
                        p.refilter();
                    }
                }
                _ => {}
            }
            self.request_main_redraw();
            return;
        }
        // The Preferences modal swallows every key. While a shortcut row
        // is "recording", the next key becomes that tool's binding;
        // otherwise Esc closes the modal.
        if self.prefs.is_some() {
            if !event.state.is_pressed() {
                return;
            }
            // Typing a name for a new shortcut preset.
            if self.prefs.as_ref().is_some_and(|p| p.naming.is_some()) {
                let mods = textedit::Mods {
                    shift: self.shift_down,
                    alt: self.alt_down,
                    meta: self.cmd_down,
                };
                let logical = event.logical_key.clone();
                let typed = event.text.clone();
                if self.clipboard.is_none() {
                    self.clipboard = arboard::Clipboard::new().ok();
                }
                let clip = self.clipboard.as_mut();
                let resp = self
                    .prefs
                    .as_mut()
                    .and_then(|p| p.naming.as_mut())
                    .map(|f| f.key(&logical, mods, typed.as_deref(), clip, &mut self.text));
                match resp {
                    Some(crate::text_field::Resp::Cancel) => {
                        if let Some(p) = self.prefs.as_mut() {
                            p.naming = None;
                        }
                    }
                    Some(crate::text_field::Resp::Submit | crate::text_field::Resp::Tab(_)) => {
                        if let Some(p) = self.prefs.as_mut() {
                            p.commit_naming();
                        }
                    }
                    _ => {}
                }
                self.request_main_redraw();
                return;
            }
            let recording = self.prefs.as_ref().and_then(|p| p.recording);
            if let Some(target) = recording {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        if let Some(p) = self.prefs.as_mut() {
                            p.recording = None;
                        }
                    }
                    PhysicalKey::Code(code) if prefs::key_char(code).is_some() => {
                        let chord = KeyChord {
                            code,
                            shift: self.shift_down,
                            cmd: self.cmd_down,
                        };
                        if let Some(p) = self.prefs.as_mut() {
                            // Steal the chord from whichever binding holds it.
                            for k in p.working.tool_keys.iter_mut() {
                                if *k == Some(chord) {
                                    *k = None;
                                }
                            }
                            for k in p.working.action_keys.iter_mut() {
                                if *k == Some(chord) {
                                    *k = None;
                                }
                            }
                            p.working_scripts.clear_chord(chord);
                            match target {
                                prefs::BindTarget::Tool(i) => p.working.tool_keys[i] = Some(chord),
                                prefs::BindTarget::Action(i) => {
                                    p.working.action_keys[i] = Some(chord)
                                }
                                prefs::BindTarget::Script(i) => {
                                    if let Some(path) = p.script_paths.get(i) {
                                        let name = crate::scripts::label(path);
                                        p.working_scripts.set_chord(&name, Some(chord));
                                    }
                                }
                            }
                            p.recording = None;
                        }
                    }
                    // A non-letter/digit key: ignore, stay recording.
                    _ => {}
                }
                self.request_main_redraw();
                return;
            }
            if matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape)) {
                self.prefs = None;
                self.request_main_redraw();
            }
            return;
        }
        // The About panel is modal: it swallows every key, handling
        // only Esc (dismiss) and ⌘/Ctrl+C (copy the selection).
        if self.about.is_some() {
            if event.state.is_pressed() {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => self.close_about(),
                    PhysicalKey::Code(KeyCode::KeyC) if self.cmd_down => {
                        let text =
                            self.about.as_ref().and_then(|a| a.selected_text());
                        if let (Some(text), Some(cb)) =
                            (text, self.clipboard.as_mut())
                        {
                            let _ = cb.set_text(text);
                        }
                    }
                    _ => {}
                }
            }
            return;
        }
        // Escape cancels; Enter accepts. An overlay swallows every other
        // key; a floating picker panel does not — the rest of the app
        // keeps working while it is open.
        if self.picker.is_some() {
            if event.state.is_pressed() {
                match event.physical_key {
                    PhysicalKey::Code(KeyCode::Escape) => {
                        self.dismiss_picker(false);
                        return;
                    }
                    PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                        self.dismiss_picker(true);
                        return;
                    }
                    _ => {}
                }
            }
            if !self.dock.contains(PanelId("picker")) {
                return;
            }
        }
        // The New Document modal, then an inline rename, each
        // swallow all keyboard input while active.
        if self.newdoc.is_some() {
            self.newdoc_key(&event);
            return;
        }
        // The Home screen swallows tool keys, but lets ⌘-shortcuts
        // (⌘N, ⌘O, …) through to their handlers below.
        if self.home.is_some() && !self.cmd_down {
            return;
        }
        // An open font dropdown takes keys to type-to-filter.
        if self.font_menu.is_some() {
            self.font_menu_key(&event);
            return;
        }
        if self.align_to_menu.is_some() {
            if event.state.is_pressed()
                && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            {
                self.align_to_menu = None;
                self.request_main_redraw();
            }
            return;
        }
        if self.ruler_menu.is_some() {
            if event.state.is_pressed()
                && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            {
                self.ruler_menu = None;
                self.request_main_redraw();
            }
            return;
        }
        if self.ctx_menu.is_some() {
            if event.state.is_pressed()
                && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            {
                self.ctx_menu = None;
                self.request_main_redraw();
            }
            return;
        }
        if self.panel_menu.is_some() {
            if event.state.is_pressed()
                && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
            {
                self.panel_menu = None;
                self.request_main_redraw();
            }
            return;
        }
        if self.xform_edit.is_some() && self.xform_key(&event) {
            return;
        }
        if self.align_spacing_edit.is_some() && self.align_spacing_key(&event) {
            return;
        }
        if self.doc.rename.is_some() {
            self.rename_key(&event);
            return;
        }
        if self.layer_search_focused {
            self.layer_search_key(&event);
            return;
        }
        // The Type tool's live editor gets first crack at every key.
        if self.text_edit.is_some() {
            match self.text_edit_key(&event) {
                textedit::KeyResult::Handled => return,
                textedit::KeyResult::Commit => {
                    let obj = self.text_edit.as_ref().map(|t| t.object);
                    self.commit_text_edit();
                    // Illustrator: Esc / ⌘Return out of the Type
                    // tool drops to Selection with the text object
                    // just made selected.
                    self.set_tool(Tool::Select);
                    if let Some(id) = obj
                        .filter(|id| self.doc.editor.document().object(*id).is_some())
                    {
                        self.doc.selection = vec![id];
                    }
                    self.request_main_redraw();
                    return;
                }
                // ⌘Z / ⌘S / … — commit, then let the shell handle it.
                textedit::KeyResult::PassThrough => {
                    self.commit_text_edit();
                }
            }
        }
        // Escape closes the Stroke flyout before anything else acts.
        if self.stroke_popover
            && event.state.is_pressed()
            && matches!(event.physical_key, PhysicalKey::Code(KeyCode::Escape))
        {
            self.stroke_popover = false;
            self.request_main_redraw();
            return;
        }
        let pressed = event.state.is_pressed();
        // Any key other than ⌘Z ends the pen re-open window.
        if pressed && !matches!(event.physical_key, PhysicalKey::Code(KeyCode::KeyZ)) {
            self.last_pen = None;
        }
        // A user script bound to this chord runs and consumes the key.
        if pressed {
            if let PhysicalKey::Code(code) = event.physical_key {
                if prefs::key_char(code).is_some() {
                    let chord = KeyChord {
                        code,
                        shift: self.shift_down,
                        cmd: self.cmd_down,
                    };
                    if let Some(path) = self.scripts.script_for_chord(chord) {
                        crate::scripts::run(&path);
                        return;
                    }
                }
            }
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Space) => {
                self.space_down = pressed;
                // A pen handle mid-pull arms / disarms the Space-to-move-
                // anchor mode the instant Space changes, no cursor nudge.
                if matches!(self.drag, Drag::PenHandle { .. }) {
                    self.drag_pen_handle();
                }
                self.update_canvas_cursor();
                // Toggle the hold-Space node peek.
                self.request_main_redraw();
            }
            PhysicalKey::Code(KeyCode::KeyZ) if pressed && self.cmd_down => {
                let redo = self.shift_down;
                if redo && self.active_tool == Tool::Pen && !self.pen_redo.is_empty() {
                    if let Some(p) = self.pen_redo.pop() {
                        self.pen.push(p);
                    }
                    self.request_main_redraw();
                } else if !redo && self.pen_undo_step() {
                    // handled
                } else {
                    let _ = if redo {
                        self.doc.editor.redo()
                    } else {
                        self.doc.editor.undo()
                    };
                    self.prune_selection();
                    self.request_main_redraw();
                }
            }
            // Delete with an anchor selection removes those anchors, not
            // the whole object. Highest ordinal first so earlier removals
            // don't shift the rest.
            PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                if pressed && !self.doc.anchor_sel.is_empty() =>
            {
                let mut sel = std::mem::take(&mut self.doc.anchor_sel);
                sel.sort_by(|a, b| b.1.cmp(&a.1));
                for (object, anchor) in sel {
                    let _ = self
                        .doc
                        .editor
                        .execute(Command::DeleteAnchor { object, anchor });
                }
                self.prune_selection();
                self.request_main_redraw();
            }
            PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                if pressed && !self.selected_guides.is_empty() =>
            {
                for id in std::mem::take(&mut self.selected_guides) {
                    let _ = self.doc.editor.execute(Command::DeleteGuide { id });
                }
                self.request_main_redraw();
            }
            PhysicalKey::Code(KeyCode::Backspace | KeyCode::Delete)
                if pressed && !self.doc.selection.is_empty() =>
            {
                let ids = std::mem::take(&mut self.doc.selection);
                self.purge_threads(&ids);
                let _ = self.doc.editor.execute(Command::DeleteObjects { ids });
                self.request_main_redraw();
            }
            // ⌘ shortcuts (copy / paste / duplicate / group / all).
            PhysicalKey::Code(code) if pressed && self.cmd_down => {
                let chord = KeyChord {
                    code,
                    shift: self.shift_down,
                    cmd: true,
                };
                if let Some(i) = self
                    .settings
                    .action_keys
                    .iter()
                    .position(|k| *k == Some(chord))
                {
                    self.run_pref_action(prefs::PrefAction::ALL[i]);
                    return;
                }
                match code {
                    KeyCode::KeyC if !self.doc.selection.is_empty() => {
                        let ids = self.doc.selection.clone();
                        self.copy_selection(&ids);
                    }
                    KeyCode::KeyX if !self.doc.selection.is_empty() => {
                        let ids = self.doc.selection.clone();
                        self.copy_selection(&ids);
                        let _ = self.doc.editor.execute(Command::DeleteObjects {
                            ids: std::mem::take(&mut self.doc.selection),
                        });
                        self.request_main_redraw();
                    }
                    // Plain paste recentres on the view; ⌘F / ⌘B keep
                    // the source coordinates. Each first checks the OS
                    // clipboard for SVG (paste from Illustrator etc.).
                    KeyCode::KeyV => self.paste_clipboard(PastePlace::Plain),
                    KeyCode::KeyF => self.paste_clipboard(PastePlace::InFront),
                    KeyCode::KeyB => self.paste_clipboard(PastePlace::Behind),
                    KeyCode::KeyD if !self.doc.selection.is_empty() => {
                        if let Ok(ids) = self.doc.editor.duplicate_objects(
                            &self.doc.selection,
                            amalith_core::Vec2::new(16.0, 16.0),
                        ) {
                            self.doc.selection = ids;
                        }
                        self.request_main_redraw();
                    }
                    KeyCode::KeyG if self.shift_down => {
                        if let Ok(freed) = self.doc.editor.ungroup(&self.doc.selection) {
                            if !freed.is_empty() {
                                self.doc.selection = freed;
                            }
                        }
                        self.request_main_redraw();
                    }
                    KeyCode::KeyG if self.doc.selection.len() > 1 => {
                        if let Ok(CommandOutcome::Object(g)) =
                            self.doc.editor.execute(Command::Group {
                                ids: self.doc.selection.clone(),
                                name: None,
                            })
                        {
                            self.doc.selection = vec![g];
                        }
                        self.request_main_redraw();
                    }
                    // Select: ⌘A all, ⌥⌘A active artboard, ⇧⌘A deselect.
                    KeyCode::KeyA if self.shift_down => self.deselect(),
                    KeyCode::KeyA if self.alt_down => self.select_all_artboard(),
                    KeyCode::KeyA => self.select_all(),
                    // File I/O: open, save, save-as, import SVG.
                    KeyCode::KeyN => self.open_new_doc(),
                    // ⌘⇧O — Type ▸ Create Outlines; plain ⌘O opens a file.
                    KeyCode::KeyO if self.shift_down => self.create_outlines(),
                    KeyCode::KeyO => self.open_document(),
                    KeyCode::KeyS => self.save_document(self.shift_down),
                    KeyCode::KeyI if self.shift_down => self.import_svg(),
                    KeyCode::KeyW => self.close_tab(self.active),
                    // ⌘R — show / hide the canvas rulers.
                    KeyCode::KeyR if !self.shift_down => {
                        self.rulers = !self.rulers;
                        self.request_main_redraw();
                    }
                    // ⌘; hide/show guides, ⌘⌥; lock/unlock them.
                    KeyCode::Semicolon if self.alt_down => {
                        self.set_guides_locked(!self.guides_locked);
                    }
                    KeyCode::Semicolon => {
                        self.set_guides_hidden(!self.guides_hidden);
                    }
                    // ⌘Y — toggle Outline (wireframe) view.
                    KeyCode::KeyY if !self.shift_down => {
                        self.toggle_outline_mode();
                    }
                    // View zoom: ⌘+ / ⌘− step, ⌘0 fit, ⌘1 actual size.
                    // `Equal` is the `=`/`+` key; on most layouts ⌘+ needs
                    // Shift, so accept it with or without.
                    KeyCode::Equal => self.zoom_step(1.6),
                    KeyCode::Minus => self.zoom_step(1.0 / 1.6),
                    KeyCode::Digit0 if self.alt_down => self.fit_view(),
                    KeyCode::Digit0 => self.zoom_fit(),
                    KeyCode::Digit1 if !self.shift_down => self.zoom_actual(),
                    // Z-order: ⌘] / ⌘[ step one, ⌘⇧] / ⌘⇧[ to the ends.
                    // ⌘⌥] / ⌘⌥[ step the selection through the stack.
                    KeyCode::BracketRight => {
                        if self.shift_down {
                            self.restack_extreme(true);
                        } else if self.alt_down {
                            self.select_next_z(1);
                        } else {
                            self.restack(1);
                        }
                    }
                    KeyCode::BracketLeft => {
                        if self.shift_down {
                            self.restack_extreme(false);
                        } else if self.alt_down {
                            self.select_next_z(-1);
                        } else {
                            self.restack(-1);
                        }
                    }
                    _ => {}
                }
            }
            // Bare-key: arrow nudge, Escape, tool shortcuts.
            PhysicalKey::Code(code) if pressed && !self.cmd_down && !self.alt_down => {
                match code {
                    KeyCode::ArrowLeft => self.nudge(-1.0, 0.0),
                    KeyCode::ArrowRight => self.nudge(1.0, 0.0),
                    KeyCode::ArrowUp => self.nudge(0.0, -1.0),
                    KeyCode::ArrowDown => self.nudge(0.0, 1.0),
                    KeyCode::Enter | KeyCode::NumpadEnter if !self.pen.is_empty() => {
                        self.commit_pen(false);
                    }
                    // Illustrator's Convert Anchor Point (Shift+C): toggle
                    // every selected anchor between smooth and corner.
                    KeyCode::KeyC if self.shift_down && !self.doc.anchor_sel.is_empty() => {
                        for (object, anchor) in self.doc.anchor_sel.clone() {
                            let _ = self
                                .doc
                                .editor
                                .execute(Command::ToggleAnchorSmooth { object, anchor });
                        }
                        self.request_main_redraw();
                    }
                    KeyCode::Escape => {
                        if self.text_load.is_some() {
                            // Cancel a loaded-text thread cursor.
                            self.text_load = None;
                            self.update_canvas_cursor();
                        } else if self.picker.is_some() {
                            self.dismiss_picker(false);
                        } else if self.active_tool == Tool::Artboard {
                            // Exit the Artboard tool back to the
                            // tool that was active before it.
                            self.set_tool(self.pre_artboard_tool);
                        } else if self.active_tool == Tool::Pen && !self.pen.is_empty() {
                            // Illustrator: Esc ends the path in progress. Two
                            // or more anchors commit as an open path (a line);
                            // a lone anchor is dropped (`commit_pen` no-ops).
                            self.commit_pen(false);
                        } else {
                            self.pen.clear();
                            self.pen_redo.clear();
                            self.doc.anchor_sel.clear();
                            self.doc.selection.clear();
                            self.selected_guides.clear();
                        }
                        self.request_main_redraw();
                    }
                    // Tool + command shortcuts — user-remappable
                    // (Preferences ▸ Keyboard). `settings.tool_keys` is
                    // indexed by `Tool::ALL`, `action_keys` by
                    // `prefs::PrefAction::ALL`.
                    _ => {
                        let chord = KeyChord {
                            code,
                            shift: self.shift_down,
                            cmd: false,
                        };
                        if let Some(i) = self
                            .settings
                            .tool_keys
                            .iter()
                            .position(|k| *k == Some(chord))
                        {
                            self.set_tool(Tool::ALL[i]);
                        } else if let Some(i) = self
                            .settings
                            .action_keys
                            .iter()
                            .position(|k| *k == Some(chord))
                        {
                            self.run_pref_action(prefs::PrefAction::ALL[i]);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub(in crate::app) fn run_pref_action(&mut self, act: prefs::PrefAction) {
        match act {
            prefs::PrefAction::SwapPaints => {
                self.apply_panel_action(crate::panels::Action::SwapPaints, false);
            }
            prefs::PrefAction::DefaultPaints => {
                self.apply_panel_action(crate::panels::Action::DefaultPaints, false);
            }
            prefs::PrefAction::Place => self.place_image_dialog(),
            prefs::PrefAction::CommandPalette => self.open_palette(),
        }
    }
}
