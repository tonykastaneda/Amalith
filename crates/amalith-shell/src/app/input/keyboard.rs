//! Keyboard handling for the main window — modal capture, the Type
//! editor, ⌘-shortcuts, and bare-key tool switches. `window_event`
//! delegates its `KeyboardInput` arm here.

use winit::event::KeyEvent;
use winit::keyboard::{KeyCode, PhysicalKey};

use amalith_commands::{Command, CommandOutcome};

use crate::prefs::{self, KeyChord};
use crate::textedit;
use crate::tool::Tool;

use super::super::{App, PastePlace};

impl App {
    pub(in crate::app) fn on_key(&mut self, event: KeyEvent) {
        // The Preferences modal swallows every key. While a shortcut row
        // is "recording", the next key becomes that tool's binding;
        // otherwise Esc closes the modal.
        if self.prefs.is_some() {
            if !event.state.is_pressed() {
                return;
            }
            let recording = self.prefs.as_ref().and_then(|p| p.recording);
            if let Some(i) = recording {
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
                        };
                        if let Some(p) = self.prefs.as_mut() {
                            // Steal the chord from whichever tool holds it.
                            for k in p.working.tool_keys.iter_mut() {
                                if *k == Some(chord) {
                                    *k = None;
                                }
                            }
                            p.working.tool_keys[i] = Some(chord);
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
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Space) => {
                self.space_down = pressed;
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
                if pressed && !self.doc.selection.is_empty() =>
            {
                let _ = self.doc.editor.execute(Command::DeleteObjects {
                    ids: std::mem::take(&mut self.doc.selection),
                });
                self.request_main_redraw();
            }
            // ⌘ shortcuts (copy / paste / duplicate / group / all).
            PhysicalKey::Code(code) if pressed && self.cmd_down => match code {
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
                KeyCode::KeyA => self.select_all(),
                // File I/O: open, save, save-as, import SVG.
                KeyCode::KeyN => self.open_new_doc(),
                // ⌘⇧O — Type ▸ Create Outlines; plain ⌘O opens a file.
                KeyCode::KeyO if self.shift_down => self.create_outlines(),
                KeyCode::KeyO => self.open_document(),
                KeyCode::KeyS => self.save_document(self.shift_down),
                KeyCode::KeyI if self.shift_down => self.import_svg(),
                KeyCode::KeyW => self.close_tab(self.active),
                // Z-order: ⌘] / ⌘[ step one, ⌘⌥] / ⌘⌥[ to the ends.
                KeyCode::BracketRight => {
                    if self.alt_down {
                        self.restack_extreme(true);
                    } else {
                        self.restack(1);
                    }
                }
                KeyCode::BracketLeft => {
                    if self.alt_down {
                        self.restack_extreme(false);
                    } else {
                        self.restack(-1);
                    }
                }
                _ => {}
            },
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
                    KeyCode::Escape => {
                        if self.picker.take().is_some() {
                            // just dismissed the picker
                        } else if self.active_tool == Tool::Artboard {
                            // Exit the Artboard tool back to the
                            // tool that was active before it.
                            self.set_tool(self.pre_artboard_tool);
                        } else {
                            self.pen.clear();
                            self.pen_redo.clear();
                            self.doc.anchor_sel.clear();
                            self.doc.selection.clear();
                        }
                        self.request_main_redraw();
                    }
                    // Tool shortcuts — user-remappable (Preferences ▸
                    // Keyboard). `settings.tool_keys` is indexed by
                    // `Tool::ALL` position.
                    _ => {
                        let chord = KeyChord {
                            code,
                            shift: self.shift_down,
                        };
                        if let Some(i) = self
                            .settings
                            .tool_keys
                            .iter()
                            .position(|k| *k == Some(chord))
                        {
                            self.set_tool(Tool::ALL[i]);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}
