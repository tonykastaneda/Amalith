//! The native menu bar — an `NSMenu` on macOS, an `HMENU` on Windows,
//! driven by `muda`. Compiled only on those platforms (the `mod`
//! declaration in `app/mod.rs` carries the `cfg`). Split out of
//! `app/mod.rs`; still an `impl` on [`super::App`]'s companion type.

use super::*;

/// The native menu bar: an `NSMenu` on macOS, an `HMENU` attached to the
/// main window on Windows. Items carry the same accelerators as the in-app
/// keyboard shortcuts; clicks arrive on `muda`'s global channel, drained
/// each loop in `about_to_wait`. (On Windows `muda` subclasses the window
/// to catch clicks; accelerator keystrokes still go through the app's own
/// keyboard handler, so the menu text is a label only there.)
pub(in crate::app) struct NativeMenu {
    items: Vec<(muda::MenuId, MenuAction)>,
    /// Panels-menu checkmarks, keyed by panel id, updated as panels
    /// open/close.
    window_checks: Vec<(&'static str, muda::CheckMenuItem)>,
    /// View ▸ Guides checkmarks — (show-guides, lock-guides).
    guide_checks: (muda::CheckMenuItem, muda::CheckMenuItem),
    /// View ▸ Outline checkmark.
    outline_check: muda::CheckMenuItem,
    /// Type ▸ Convert to Area/Point Type — label + enabled tracks the
    /// selection.
    convert_text_i: muda::MenuItem,
    /// Object ▸ Clipping Mask ▸ (Make, Release) — enabled tracks the
    /// selection.
    clip_items: (muda::MenuItem, muda::MenuItem),
    // Kept alive for the process; dropping it tears the menu down.
    _menu: muda::Menu,
}

impl NativeMenu {
    pub(in crate::app) fn build(
        window: &Window,
        scripts: &crate::scripts::ScriptsConfig,
        guides_hidden: bool,
        guides_locked: bool,
        outline: bool,
    ) -> Self {
        use muda::{
            accelerator::{Accelerator, Code, Modifiers},
            CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
        };
        // macOS puts modifier symbols on the Cmd key; Windows on Ctrl.
        #[cfg(target_os = "macos")]
        let prim = Modifiers::SUPER;
        #[cfg(not(target_os = "macos"))]
        let prim = Modifiers::CONTROL;
        #[cfg(target_os = "macos")]
        let _ = window;
        let sup = Some(prim);
        let sup_shift = Some(prim | Modifiers::SHIFT);
        let sup_alt = Some(prim | Modifiers::ALT);
        let mk = |label: &str, mods, code| MenuItem::new(label, true, Some(Accelerator::new(mods, code)));

        let new_i = mk("New", sup, Code::KeyN);
        let open_i = mk("Open…", sup, Code::KeyO);
        let save_i = mk("Save", sup, Code::KeyS);
        let save_as_i = mk("Save As…", sup_shift, Code::KeyS);
        let import_i = mk("Import SVG…", sup_shift, Code::KeyI);
        let place_i = mk("Place…", sup_shift, Code::KeyP);
        let undo_i = mk("Undo", sup, Code::KeyZ);
        let redo_i = mk("Redo", sup_shift, Code::KeyZ);
        let cut_i = mk("Cut", sup, Code::KeyX);
        let copy_i = mk("Copy", sup, Code::KeyC);
        let paste_i = mk("Paste", sup, Code::KeyV);
        let dup_i = mk("Duplicate", sup, Code::KeyD);
        let all_i = mk("All", sup, Code::KeyA);
        let sel_artboard_i = mk("All on Active Artboard", sup_alt, Code::KeyA);
        let deselect_i = mk("Deselect", sup_shift, Code::KeyA);
        let next_above_i = mk("Next Object Above", sup_alt, Code::BracketRight);
        let next_below_i = mk("Next Object Below", sup_alt, Code::BracketLeft);
        let same_fillstroke_i = MenuItem::new("Fill & Stroke", true, None);
        let same_fill_i = MenuItem::new("Fill Color", true, None);
        let same_opacity_i = MenuItem::new("Opacity", true, None);
        let same_stroke_i = MenuItem::new("Stroke Color", true, None);
        let same_weight_i = MenuItem::new("Stroke Weight", true, None);
        let same_font_i = MenuItem::new("Font Family", true, None);
        let same_size_i = MenuItem::new("Font Size", true, None);
        let clip_make_i = MenuItem::new("Make", false, Some(Accelerator::new(sup, Code::Digit7)));
        let clip_release_i =
            MenuItem::new("Release", false, Some(Accelerator::new(sup_alt, Code::Digit7)));
        let forward_i = mk("Bring Forward", sup, Code::BracketRight);
        let front_i = mk("Bring to Front", sup_shift, Code::BracketRight);
        let backward_i = mk("Send Backward", sup, Code::BracketLeft);
        let back_i = mk("Send to Back", sup_shift, Code::BracketLeft);
        let zoom_in_i = mk("Zoom In", sup, Code::Equal);
        let zoom_out_i = mk("Zoom Out", sup, Code::Minus);
        let fit_artboard_i = mk("Fit Artboard in Window", sup, Code::Digit0);
        let fit_all_i = mk("Fit All in Window", sup_alt, Code::Digit0);
        let outline_i = CheckMenuItem::new(
            "Outline",
            true,
            outline,
            Some(Accelerator::new(sup, Code::KeyY)),
        );
        let guides_show_i = CheckMenuItem::new(
            "Show Guides",
            true,
            !guides_hidden,
            Some(Accelerator::new(sup, Code::Semicolon)),
        );
        let guides_lock_i = CheckMenuItem::new(
            "Lock Guides",
            true,
            guides_locked,
            Some(Accelerator::new(sup_alt, Code::Semicolon)),
        );
        let clear_guides_i = MenuItem::new("Clear Guides", true, None);

        let sep = PredefinedMenuItem::separator;
        let about_i = MenuItem::new("About Amalith", true, None);
        let prefs_i = MenuItem::new(
            "Preferences…",
            true,
            Some(Accelerator::new(sup, Code::Comma)),
        );
        // macOS has a real "Quit" that ends the process cleanly. Windows
        // has no app menu convention and `PostQuitMessage` doesn't stop
        // winit's loop, so use a plain item routed to `event_loop.exit()`.
        // Route Quit through our own dispatcher on every platform so
        // `App::exiting` gets a chance to save the layout — the macOS
        // predefined Quit terminates without unwinding winit's loop.
        #[cfg(target_os = "macos")]
        let quit_i = mk("Quit Amalith", sup, Code::KeyQ);
        #[cfg(not(target_os = "macos"))]
        let quit_i = MenuItem::new("Exit", true, None);
        let app = Submenu::with_items(
            "Amalith",
            true,
            &[&about_i, &sep(), &prefs_i, &sep(), &quit_i],
        )
        .expect("app menu");
        // File ▸ Scripts — a user-pointed folder, its scripts listed here.
        let add_scripts_i = MenuItem::new("Add Scripts Folder…", true, None);
        let reveal_scripts_i = MenuItem::new("Reveal Scripts Folder", true, None);
        let remove_scripts_i = MenuItem::new("Remove Scripts Folder", true, None);
        let script_items: Vec<(MenuItem, std::path::PathBuf)> = scripts
            .dir
            .as_deref()
            .map(crate::scripts::list)
            .unwrap_or_default()
            .into_iter()
            .map(|p| (MenuItem::new(crate::scripts::label(&p), true, None), p))
            .collect();
        let scripts_sep = sep();
        let scripts_menu = {
            let mut refs: Vec<&dyn muda::IsMenuItem> = vec![&add_scripts_i];
            if scripts.dir.is_some() {
                refs.push(&reveal_scripts_i);
                refs.push(&remove_scripts_i);
                if !script_items.is_empty() {
                    refs.push(&scripts_sep);
                }
                for (item, _) in &script_items {
                    refs.push(item);
                }
            }
            Submenu::with_items("Scripts", true, &refs).expect("scripts menu")
        };

        let file = Submenu::with_items(
            "File",
            true,
            &[
                &new_i, &open_i, &sep(), &save_i, &save_as_i, &sep(), &import_i, &place_i, &sep(),
                &scripts_menu,
            ],
        )
        .expect("file menu");
        let edit = Submenu::with_items(
            "Edit",
            true,
            &[
                &undo_i, &redo_i, &sep(), &cut_i, &copy_i, &paste_i, &dup_i,
                &sep(), &forward_i, &front_i, &backward_i, &back_i,
            ],
        )
        .expect("edit menu");
        let same_menu = Submenu::with_items(
            "Same",
            true,
            &[
                &same_fillstroke_i,
                &same_fill_i,
                &same_opacity_i,
                &same_stroke_i,
                &same_weight_i,
                &sep(),
                &same_font_i,
                &same_size_i,
            ],
        )
        .expect("same menu");
        let clip_menu = Submenu::with_items("Clipping Mask", true, &[&clip_make_i, &clip_release_i])
            .expect("clip menu");
        let object_menu = Submenu::with_items("Object", true, &[&clip_menu]).expect("object menu");
        let select_menu = Submenu::with_items(
            "Select",
            true,
            &[
                &all_i,
                &sel_artboard_i,
                &deselect_i,
                &sep(),
                &next_above_i,
                &next_below_i,
                &sep(),
                &same_menu,
            ],
        )
        .expect("select menu");
        // Type menu — the convert item's label + enabled state track the
        // selection (see `NativeMenu::sync_type`).
        let convert_text_i = MenuItem::new("Convert to Area Type", false, None);
        let type_menu = Submenu::with_items("Type", true, &[&convert_text_i]).expect("type menu");
        let view = Submenu::with_items(
            "View",
            true,
            &[
                &zoom_in_i,
                &zoom_out_i,
                &sep(),
                &fit_artboard_i,
                &fit_all_i,
                &sep(),
                &outline_i,
                &sep(),
                &guides_show_i,
                &guides_lock_i,
                &clear_guides_i,
            ],
        )
        .expect("view menu");

        let window_checks: Vec<(&'static str, CheckMenuItem)> = WINDOW_PANELS
            .iter()
            .map(|(id, label)| (*id, CheckMenuItem::new(*label, true, false, None)))
            .collect();
        let window_refs: Vec<&dyn muda::IsMenuItem> = window_checks
            .iter()
            .map(|(_, i)| i as &dyn muda::IsMenuItem)
            .collect();
        // "Window" is a name AppKit reserves for its own window menu, so
        // call ours "Panels".
        let panels_menu = Submenu::with_items("Panels", true, &window_refs).expect("panels menu");

        // A menu literally titled "Help" gets AppKit's search field for
        // free on macOS; on Windows it's just the one link.
        let help_docs_i = MenuItem::new("Amalith Help", true, None);
        let help_menu = Submenu::with_items("Help", true, &[&help_docs_i]).expect("help menu");

        let menu = Menu::new();
        menu.append(&app).expect("append app menu");
        menu.append(&file).expect("append file menu");
        menu.append(&edit).expect("append edit menu");
        menu.append(&object_menu).expect("append object menu");
        menu.append(&select_menu).expect("append select menu");
        menu.append(&type_menu).expect("append type menu");
        menu.append(&view).expect("append view menu");
        menu.append(&panels_menu).expect("append panels menu");
        menu.append(&help_menu).expect("append help menu");
        #[cfg(target_os = "macos")]
        menu.init_for_nsapp();
        #[cfg(target_os = "windows")]
        {
            use muda::MenuTheme;
            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(w) = handle.as_raw() {
                    // Safe: `w.hwnd` is the live main window's handle.
                    // Dark theme so the bar and its dropdowns read as one
                    // piece with the rest of the app rather than a white
                    // Win32 strip (muda draws RGB(43,43,43) / white text,
                    // which matches `theme.panel_bg`).
                    unsafe {
                        let _ = menu.init_for_hwnd_with_theme(w.hwnd.get(), MenuTheme::Dark);
                    }
                }
            }
        }

        let items = vec![
            (about_i.id().clone(), MenuAction::About),
            (prefs_i.id().clone(), MenuAction::Preferences),
            (new_i.id().clone(), MenuAction::New),
            (open_i.id().clone(), MenuAction::Open),
            (save_i.id().clone(), MenuAction::Save),
            (save_as_i.id().clone(), MenuAction::SaveAs),
            (import_i.id().clone(), MenuAction::ImportSvg),
            (place_i.id().clone(), MenuAction::Place),
            (undo_i.id().clone(), MenuAction::Undo),
            (redo_i.id().clone(), MenuAction::Redo),
            (cut_i.id().clone(), MenuAction::Cut),
            (copy_i.id().clone(), MenuAction::Copy),
            (paste_i.id().clone(), MenuAction::Paste),
            (dup_i.id().clone(), MenuAction::Duplicate),
            (all_i.id().clone(), MenuAction::SelectAll),
            (sel_artboard_i.id().clone(), MenuAction::SelectAllArtboard),
            (deselect_i.id().clone(), MenuAction::Deselect),
            (next_above_i.id().clone(), MenuAction::SelectNextAbove),
            (next_below_i.id().clone(), MenuAction::SelectNextBelow),
            (same_fillstroke_i.id().clone(), MenuAction::SelectSame(SameKind::FillStroke)),
            (same_fill_i.id().clone(), MenuAction::SelectSame(SameKind::FillColor)),
            (same_opacity_i.id().clone(), MenuAction::SelectSame(SameKind::Opacity)),
            (same_stroke_i.id().clone(), MenuAction::SelectSame(SameKind::StrokeColor)),
            (same_weight_i.id().clone(), MenuAction::SelectSame(SameKind::StrokeWeight)),
            (same_font_i.id().clone(), MenuAction::SelectSame(SameKind::FontFamily)),
            (same_size_i.id().clone(), MenuAction::SelectSame(SameKind::FontSize)),
            (forward_i.id().clone(), MenuAction::BringForward),
            (front_i.id().clone(), MenuAction::BringToFront),
            (backward_i.id().clone(), MenuAction::SendBackward),
            (back_i.id().clone(), MenuAction::SendToBack),
            (zoom_in_i.id().clone(), MenuAction::ZoomIn),
            (zoom_out_i.id().clone(), MenuAction::ZoomOut),
            (fit_artboard_i.id().clone(), MenuAction::FitArtboard),
            (fit_all_i.id().clone(), MenuAction::FitAll),
            (outline_i.id().clone(), MenuAction::ToggleOutline),
            (convert_text_i.id().clone(), MenuAction::ConvertTextKind),
            (help_docs_i.id().clone(), MenuAction::HelpDocs),
            (clip_make_i.id().clone(), MenuAction::ClipMake),
            (clip_release_i.id().clone(), MenuAction::ClipRelease),
            (guides_show_i.id().clone(), MenuAction::ToggleGuides),
            (guides_lock_i.id().clone(), MenuAction::ToggleGuideLock),
            (clear_guides_i.id().clone(), MenuAction::ClearGuides),
            (add_scripts_i.id().clone(), MenuAction::AddScriptsFolder),
            (reveal_scripts_i.id().clone(), MenuAction::RevealScriptsFolder),
            (remove_scripts_i.id().clone(), MenuAction::RemoveScriptsFolder),
        ];
        let mut items = items;
        for (item, path) in &script_items {
            items.push((item.id().clone(), MenuAction::RunScript(path.clone())));
        }
        for (id, item) in &window_checks {
            items.push((item.id().clone(), MenuAction::TogglePanel(id)));
        }
        items.push((quit_i.id().clone(), MenuAction::Quit));
        Self {
            items,
            window_checks,
            guide_checks: (guides_show_i, guides_lock_i),
            outline_check: outline_i,
            convert_text_i,
            clip_items: (clip_make_i, clip_release_i),
            _menu: menu,
        }
    }

    /// Enable/disable the Clipping Mask items to match the selection.
    pub(in crate::app) fn sync_clip(&self, (can_make, can_release): (bool, bool)) {
        self.clip_items.0.set_enabled(can_make);
        self.clip_items.1.set_enabled(can_release);
    }

    /// Match the View ▸ Outline checkmark to the live toggle.
    pub(in crate::app) fn sync_outline(&self, on: bool) {
        self.outline_check.set_checked(on);
    }

    /// Point/area convert item: `Some(true)` = an area-text object is
    /// selected (offer "Convert to Point Type"), `Some(false)` = point
    /// text (offer "Convert to Area Type"), `None` = nothing convertible.
    pub(in crate::app) fn sync_type(&self, area_selected: Option<bool>) {
        match area_selected {
            Some(true) => {
                self.convert_text_i.set_text("Convert to Point Type");
                self.convert_text_i.set_enabled(true);
            }
            Some(false) => {
                self.convert_text_i.set_text("Convert to Area Type");
                self.convert_text_i.set_enabled(true);
            }
            None => {
                self.convert_text_i.set_text("Convert to Area Type");
                self.convert_text_i.set_enabled(false);
            }
        }
    }

    /// Tick / untick each Window-menu entry to match the live dock.
    pub(in crate::app) fn sync_window(&self, dock: &DockModel) {
        for (id, item) in &self.window_checks {
            item.set_checked(dock.contains(PanelId(id)));
        }
    }

    /// Match the View ▸ Guides checkmarks to the live toggles.
    pub(in crate::app) fn sync_guides(&self, hidden: bool, locked: bool) {
        self.guide_checks.0.set_checked(!hidden);
        self.guide_checks.1.set_checked(locked);
    }

    /// Every menu click queued since the last call.
    pub(in crate::app) fn drain(&self) -> Vec<MenuAction> {
        let mut out = Vec::new();
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            if let Some((_, action)) = self.items.iter().find(|(id, _)| *id == event.id) {
                out.push(action.clone());
            }
        }
        out
    }
}
