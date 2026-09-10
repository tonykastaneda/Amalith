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
    /// Windows-menu checkmarks, keyed by panel id, updated as panels
    /// open/close.
    window_checks: Vec<(PanelKind, muda::CheckMenuItem)>,
    /// View ▸ Guides checkmarks — (show-guides, lock-guides).
    guide_checks: (muda::CheckMenuItem, muda::CheckMenuItem),
    /// File ▸ Document Color Mode checkmarks — (CMYK, RGB).
    color_mode_checks: (muda::CheckMenuItem, muda::CheckMenuItem),
    /// View ▸ Outline checkmark.
    outline_check: muda::CheckMenuItem,
    /// View ▸ Show Transparency Grid checkmark.
    transparency_grid_check: muda::CheckMenuItem,
    /// View ▸ Smart Guides checkmark.
    smart_guides_check: muda::CheckMenuItem,
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
        workspaces: &crate::workspaces::Store,
        guides_hidden: bool,
        guides_locked: bool,
        outline: bool,
        transparency_grid: bool,
        smart_guides: bool,
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

        // The `MenuId → MenuAction` map, built as each item is created
        // instead of separately re-typed afterward — see
        // `09-native-menu-action-registry-medium.md`. `reg` is the one
        // place a click handler gets wired up; an item that never passes
        // through it can't end up in the map, so it can't end up in the
        // old failure mode this whole file used to risk: a menu item that
        // renders, looks clickable, and silently does nothing.
        let mut items: Vec<(muda::MenuId, MenuAction)> = Vec::new();
        fn reg<T: muda::IsMenuItem>(
            items: &mut Vec<(muda::MenuId, MenuAction)>,
            item: T,
            action: MenuAction,
        ) -> T {
            items.push((item.id().clone(), action));
            item
        }

        let new_i = reg(&mut items, mk("New", sup, Code::KeyN), MenuAction::New);
        let open_i = reg(&mut items, mk("Open…", sup, Code::KeyO), MenuAction::Open);
        let close_i = reg(&mut items, mk("Close", sup, Code::KeyW), MenuAction::Close);
        let close_all_i = reg(&mut items, mk("Close All", sup_alt, Code::KeyW), MenuAction::CloseAll);
        let save_i = reg(&mut items, mk("Save", sup, Code::KeyS), MenuAction::Save);
        let save_as_i = reg(&mut items, mk("Save As…", sup_shift, Code::KeyS), MenuAction::SaveAs);
        let revert_i = reg(&mut items, MenuItem::new("Revert", true, None), MenuAction::Revert);
        let import_i = reg(&mut items, mk("Import SVG…", sup_shift, Code::KeyI), MenuAction::ImportSvg);
        let place_i = reg(&mut items, mk("Place…", sup_shift, Code::KeyP), MenuAction::Place);
        let export_screens_i = reg(
            &mut items,
            mk("Export for Screens…", sup_alt, Code::KeyE),
            MenuAction::ExportForScreens,
        );
        let cmyk_i = reg(
            &mut items,
            CheckMenuItem::new("CMYK Color", true, false, None),
            MenuAction::SetColorMode(amalith_core::ColorMode::Cmyk),
        );
        let rgb_i = reg(
            &mut items,
            CheckMenuItem::new("RGB Color", true, false, None),
            MenuAction::SetColorMode(amalith_core::ColorMode::Rgb),
        );
        let undo_i = reg(&mut items, mk("Undo", sup, Code::KeyZ), MenuAction::Undo);
        let redo_i = reg(&mut items, mk("Redo", sup_shift, Code::KeyZ), MenuAction::Redo);
        let cut_i = reg(&mut items, mk("Cut", sup, Code::KeyX), MenuAction::Cut);
        let copy_i = reg(&mut items, mk("Copy", sup, Code::KeyC), MenuAction::Copy);
        let paste_i = reg(&mut items, mk("Paste", sup, Code::KeyV), MenuAction::Paste);
        // Illustrator gives Duplicate no default shortcut — Cmd+D is
        // Transform Again's (Object menu, below), and Duplicate is
        // reachable via Cmd+C, Cmd+F / Cmd+B (paste in front / behind)
        // just as it is there.
        let dup_i = reg(&mut items, MenuItem::new("Duplicate", true, None), MenuAction::Duplicate);
        let transform_again_i = reg(
            &mut items,
            mk("Transform Again", sup, Code::KeyD),
            MenuAction::TransformAgain,
        );
        let offset_path_i = reg(
            &mut items,
            MenuItem::new("Offset Path…", true, None),
            MenuAction::OffsetPath,
        );
        let lock_selection_i = reg(&mut items, mk("Selection", sup, Code::Digit2), MenuAction::LockSelection);
        let unlock_all_i = reg(&mut items, mk("Unlock All", sup_alt, Code::Digit2), MenuAction::UnlockAll);
        let all_i = reg(&mut items, mk("All", sup, Code::KeyA), MenuAction::SelectAll);
        let sel_artboard_i = reg(
            &mut items,
            mk("All on Active Artboard", sup_alt, Code::KeyA),
            MenuAction::SelectAllArtboard,
        );
        let deselect_i = reg(&mut items, mk("Deselect", sup_shift, Code::KeyA), MenuAction::Deselect);
        let next_above_i = reg(
            &mut items,
            mk("Next Object Above", sup_alt, Code::BracketRight),
            MenuAction::SelectNextAbove,
        );
        let next_below_i = reg(
            &mut items,
            mk("Next Object Below", sup_alt, Code::BracketLeft),
            MenuAction::SelectNextBelow,
        );
        let same_fillstroke_i = reg(
            &mut items,
            MenuItem::new("Fill & Stroke", true, None),
            MenuAction::SelectSame(SameKind::FillStroke),
        );
        let same_fill_i = reg(
            &mut items,
            MenuItem::new("Fill Color", true, None),
            MenuAction::SelectSame(SameKind::FillColor),
        );
        let same_opacity_i = reg(
            &mut items,
            MenuItem::new("Opacity", true, None),
            MenuAction::SelectSame(SameKind::Opacity),
        );
        let same_stroke_i = reg(
            &mut items,
            MenuItem::new("Stroke Color", true, None),
            MenuAction::SelectSame(SameKind::StrokeColor),
        );
        let same_weight_i = reg(
            &mut items,
            MenuItem::new("Stroke Weight", true, None),
            MenuAction::SelectSame(SameKind::StrokeWeight),
        );
        let same_font_i = reg(
            &mut items,
            MenuItem::new("Font Family", true, None),
            MenuAction::SelectSame(SameKind::FontFamily),
        );
        let same_size_i = reg(
            &mut items,
            MenuItem::new("Font Size", true, None),
            MenuAction::SelectSame(SameKind::FontSize),
        );
        let clip_make_i = reg(
            &mut items,
            MenuItem::new("Make", false, Some(Accelerator::new(sup, Code::Digit7))),
            MenuAction::ClipMake,
        );
        let clip_release_i = reg(
            &mut items,
            MenuItem::new("Release", false, Some(Accelerator::new(sup_alt, Code::Digit7))),
            MenuAction::ClipRelease,
        );
        let forward_i = reg(&mut items, mk("Bring Forward", sup, Code::BracketRight), MenuAction::BringForward);
        let front_i = reg(
            &mut items,
            mk("Bring to Front", sup_shift, Code::BracketRight),
            MenuAction::BringToFront,
        );
        let backward_i = reg(&mut items, mk("Send Backward", sup, Code::BracketLeft), MenuAction::SendBackward);
        let back_i = reg(&mut items, mk("Send to Back", sup_shift, Code::BracketLeft), MenuAction::SendToBack);
        let zoom_in_i = reg(&mut items, mk("Zoom In", sup, Code::Equal), MenuAction::ZoomIn);
        let zoom_out_i = reg(&mut items, mk("Zoom Out", sup, Code::Minus), MenuAction::ZoomOut);
        let fit_artboard_i = reg(
            &mut items,
            mk("Fit Artboard in Window", sup, Code::Digit0),
            MenuAction::FitArtboard,
        );
        let fit_all_i = reg(&mut items, mk("Fit All in Window", sup_alt, Code::Digit0), MenuAction::FitAll);
        let outline_i = reg(
            &mut items,
            CheckMenuItem::new("Outline", true, outline, Some(Accelerator::new(sup, Code::KeyY))),
            MenuAction::ToggleOutline,
        );
        let transparency_grid_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Show Transparency Grid",
                true,
                transparency_grid,
                Some(Accelerator::new(sup_shift, Code::KeyD)),
            ),
            MenuAction::ToggleTransparencyGrid,
        );
        let smart_guides_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Smart Guides",
                true,
                smart_guides,
                Some(Accelerator::new(sup, Code::KeyU)),
            ),
            MenuAction::ToggleSmartGuides,
        );
        let guides_show_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Show Guides",
                true,
                !guides_hidden,
                Some(Accelerator::new(sup, Code::Semicolon)),
            ),
            MenuAction::ToggleGuides,
        );
        let guides_lock_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Lock Guides",
                true,
                guides_locked,
                Some(Accelerator::new(sup_alt, Code::Semicolon)),
            ),
            MenuAction::ToggleGuideLock,
        );
        let clear_guides_i = reg(
            &mut items,
            MenuItem::new("Clear Guides", true, None),
            MenuAction::ClearGuides,
        );

        let sep = PredefinedMenuItem::separator;
        let about_i = reg(&mut items, MenuItem::new("About Amalith", true, None), MenuAction::About);
        let prefs_i = reg(
            &mut items,
            MenuItem::new("Preferences…", true, Some(Accelerator::new(sup, Code::Comma))),
            MenuAction::Preferences,
        );
        // macOS has a real "Quit" that ends the process cleanly. Windows
        // has no app menu convention and `PostQuitMessage` doesn't stop
        // winit's loop, so use a plain item routed to `event_loop.exit()`.
        // Route Quit through our own dispatcher on every platform so
        // `App::exiting` gets a chance to save the layout — the macOS
        // predefined Quit terminates without unwinding winit's loop.
        #[cfg(target_os = "macos")]
        let quit_i = reg(&mut items, mk("Quit Amalith", sup, Code::KeyQ), MenuAction::Quit);
        #[cfg(not(target_os = "macos"))]
        let quit_i = reg(&mut items, MenuItem::new("Exit", true, None), MenuAction::Quit);
        let app = Submenu::with_items(
            "Amalith",
            true,
            &[&about_i, &sep(), &prefs_i, &sep(), &quit_i],
        )
        .expect("app menu");
        // File ▸ Scripts — a user-pointed folder, its scripts listed here.
        let add_scripts_i = reg(
            &mut items,
            MenuItem::new("Add Scripts Folder…", true, None),
            MenuAction::AddScriptsFolder,
        );
        let reveal_scripts_i = reg(
            &mut items,
            MenuItem::new("Reveal Scripts Folder", true, None),
            MenuAction::RevealScriptsFolder,
        );
        let remove_scripts_i = reg(
            &mut items,
            MenuItem::new("Remove Scripts Folder", true, None),
            MenuAction::RemoveScriptsFolder,
        );
        let script_items: Vec<(MenuItem, std::path::PathBuf)> = scripts
            .dir
            .as_deref()
            .map(crate::scripts::list)
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let mi = reg(
                    &mut items,
                    MenuItem::new(crate::scripts::label(&p), true, None),
                    MenuAction::RunScript(p.clone()),
                );
                (mi, p)
            })
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

        let export_menu = Submenu::with_items("Export", true, &[&export_screens_i])
            .expect("export menu");
        let color_mode_menu = Submenu::with_items("Document Color Mode", true, &[&cmyk_i, &rgb_i])
            .expect("color mode menu");
        let file = Submenu::with_items(
            "File",
            true,
            &[
                &new_i, &open_i, &sep(), &close_i, &close_all_i, &sep(), &save_i, &save_as_i,
                &revert_i, &sep(), &import_i, &place_i, &export_menu, &sep(), &color_mode_menu,
                &sep(), &scripts_menu,
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
        let path_menu = Submenu::with_items("Path", true, &[&offset_path_i]).expect("path menu");
        let lock_menu = Submenu::with_items("Lock", true, &[&lock_selection_i]).expect("lock menu");
        let object_menu = Submenu::with_items(
            "Object",
            true,
            &[
                &transform_again_i, &sep(), &path_menu, &clip_menu, &sep(), &lock_menu,
                &unlock_all_i,
            ],
        )
        .expect("object menu");
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
        let convert_text_i = reg(
            &mut items,
            MenuItem::new("Convert to Area Type", false, None),
            MenuAction::ConvertTextKind,
        );
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
                &transparency_grid_i,
                &sep(),
                &smart_guides_i,
                &guides_show_i,
                &guides_lock_i,
                &clear_guides_i,
            ],
        )
        .expect("view menu");

        // Windows ▸ Workspace: one checked item per saved workspace (the
        // built-in "Essentials Classic" first), then Reset / New / Manage.
        let workspace_names = workspaces.names();
        let workspace_checks: Vec<CheckMenuItem> = workspace_names
            .iter()
            .map(|name| {
                reg(
                    &mut items,
                    CheckMenuItem::new(name, true, *name == workspaces.active, None),
                    MenuAction::PickWorkspace(name.clone()),
                )
            })
            .collect();
        let reset_workspace_i = reg(
            &mut items,
            MenuItem::new(format!("Reset {}", workspaces.active), true, None),
            MenuAction::ResetWorkspace,
        );
        let new_workspace_i = reg(
            &mut items,
            MenuItem::new("New Workspace…", true, None),
            MenuAction::NewWorkspace,
        );
        let manage_workspaces_i = reg(
            &mut items,
            MenuItem::new("Manage Workspaces…", true, None),
            MenuAction::ManageWorkspaces,
        );
        let workspace_sep = sep();
        let mut workspace_refs: Vec<&dyn muda::IsMenuItem> =
            workspace_checks.iter().map(|i| i as &dyn muda::IsMenuItem).collect();
        workspace_refs.push(&workspace_sep);
        workspace_refs.push(&reset_workspace_i);
        workspace_refs.push(&new_workspace_i);
        workspace_refs.push(&manage_workspaces_i);
        let workspace_menu = Submenu::with_items("Workspace", true, &workspace_refs).expect("workspace menu");

        let window_checks: Vec<(PanelKind, CheckMenuItem)> = WINDOW_PANELS
            .iter()
            .map(|kind| {
                let mi = reg(
                    &mut items,
                    CheckMenuItem::new(kind.label(), true, false, None),
                    MenuAction::TogglePanel(*kind),
                );
                (*kind, mi)
            })
            .collect();
        let windows_sep = sep();
        let mut window_refs: Vec<&dyn muda::IsMenuItem> = vec![&workspace_menu, &windows_sep];
        window_refs.extend(window_checks.iter().map(|(_, i)| i as &dyn muda::IsMenuItem));
        // "Window" (singular) is a name AppKit reserves for its own window
        // menu; "Windows" (plural) sidesteps that collision.
        let panels_menu = Submenu::with_items("Windows", true, &window_refs).expect("windows menu");

        // A menu literally titled "Help" gets AppKit's search field for
        // free on macOS; on Windows it's just the one link.
        let help_docs_i = reg(&mut items, MenuItem::new("Amalith Help", true, None), MenuAction::HelpDocs);
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

        Self {
            items,
            window_checks,
            guide_checks: (guides_show_i, guides_lock_i),
            color_mode_checks: (cmyk_i, rgb_i),
            outline_check: outline_i,
            transparency_grid_check: transparency_grid_i,
            smart_guides_check: smart_guides_i,
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

    /// Match the View ▸ Show Transparency Grid checkmark to the live toggle.
    pub(in crate::app) fn sync_transparency_grid(&self, on: bool) {
        self.transparency_grid_check.set_checked(on);
    }

    /// Match the View ▸ Smart Guides checkmark to the live toggle.
    pub(in crate::app) fn sync_smart_guides(&self, on: bool) {
        self.smart_guides_check.set_checked(on);
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
            item.set_checked(dock.contains(PanelId(*id)));
        }
    }

    /// Match the View ▸ Guides checkmarks to the live toggles.
    pub(in crate::app) fn sync_guides(&self, hidden: bool, locked: bool) {
        self.guide_checks.0.set_checked(!hidden);
        self.guide_checks.1.set_checked(locked);
    }

    /// Tick the current Document Color Mode.
    pub(in crate::app) fn sync_color_mode(&self, mode: amalith_core::ColorMode) {
        let rgb = matches!(mode, amalith_core::ColorMode::Rgb);
        self.color_mode_checks.0.set_checked(!rgb);
        self.color_mode_checks.1.set_checked(rgb);
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
