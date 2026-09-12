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
    /// View ▸ Show Grid / Snap to Grid / Snap to Pixel / Snap to Point.
    grid_checks: (muda::CheckMenuItem, muda::CheckMenuItem, muda::CheckMenuItem, muda::CheckMenuItem),
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
        show_grid: bool,
        snap_to_grid: bool,
        snap_to_pixel: bool,
        snap_to_point: bool,
        hide_wip: bool,
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
        // A placeholder for a real Illustrator menu item Amalith can't do
        // yet — permanently disabled (so there's nothing to click, and
        // nothing to `reg()`), labeled so the menu still shows the whole
        // shape of the feature set. `hide_wip` (Preferences ▸ Debug ▸ Hide
        // WIP Menu Items) drops these from the tree entirely instead of
        // just greying them out — see the `if !hide_wip { v.push(...) }`
        // pattern used throughout this function for every submenu that
        // mixes real and WIP items.
        let wip = |label: &str| MenuItem::new(format!("{label} (WIP)"), false, None);

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
        let sup_alt_shift = Some(prim | Modifiers::ALT | Modifiers::SHIFT);
        let blend_make_i = reg(
            &mut items,
            MenuItem::new("Make", true, Some(Accelerator::new(sup_alt, Code::KeyB))),
            MenuAction::BlendMake,
        );
        let blend_release_i = reg(
            &mut items,
            MenuItem::new("Release", true, Some(Accelerator::new(sup_alt_shift, Code::KeyB))),
            MenuAction::BlendRelease,
        );
        let blend_options_i = reg(
            &mut items,
            MenuItem::new("Blend Options…", true, None),
            MenuAction::BlendOptionsMenu,
        );
        let blend_expand_i = reg(&mut items, MenuItem::new("Expand", true, None), MenuAction::BlendExpand);
        let blend_replace_spine_i = reg(
            &mut items,
            MenuItem::new("Replace Spine", true, None),
            MenuAction::BlendReplaceSpine,
        );
        let blend_reverse_spine_i = reg(
            &mut items,
            MenuItem::new("Reverse Spine", true, None),
            MenuAction::BlendReverseSpine,
        );
        let blend_reverse_stacking_i = reg(
            &mut items,
            MenuItem::new("Reverse Front to Back", true, None),
            MenuAction::BlendReverseStacking,
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
        let show_grid_i = reg(
            &mut items,
            CheckMenuItem::new("Show Grid", true, show_grid, Some(Accelerator::new(sup, Code::Quote))),
            MenuAction::ToggleShowGrid,
        );
        let snap_to_grid_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Snap to Grid",
                true,
                snap_to_grid,
                Some(Accelerator::new(sup_shift, Code::Quote)),
            ),
            MenuAction::ToggleSnapToGrid,
        );
        let snap_to_pixel_i = reg(
            &mut items,
            CheckMenuItem::new("Snap to Pixel", true, snap_to_pixel, None),
            MenuAction::ToggleSnapToPixel,
        );
        let snap_to_point_i = reg(
            &mut items,
            CheckMenuItem::new(
                "Snap to Point",
                true,
                snap_to_point,
                Some(Accelerator::new(sup_alt, Code::Quote)),
            ),
            MenuAction::ToggleSnapToPoint,
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

        let export_as_wip = wip("Export As…");
        let save_for_web_wip = wip("Save for Web (Legacy)…");
        let mut export_items: Vec<&dyn muda::IsMenuItem> = vec![&export_screens_i];
        if !hide_wip {
            export_items.push(&export_as_wip);
            export_items.push(&save_for_web_wip);
        }
        let export_menu = Submenu::with_items("Export", true, &export_items).expect("export menu");
        let color_mode_menu = Submenu::with_items("Document Color Mode", true, &[&cmyk_i, &rgb_i])
            .expect("color mode menu");
        let new_from_template_wip = wip("New from Template…");
        let package_wip = wip("Package…");
        let document_setup_wip = wip("Document Setup…");
        let file_info_wip = wip("File Info…");
        let print_wip = wip("Print…");
        let file_sep1 = sep();
        let file_sep2 = sep();
        let file_sep3 = sep();
        let file_sep4 = sep();
        let file_sep5 = sep();
        let file_sep6 = sep();
        let mut file_items: Vec<&dyn muda::IsMenuItem> = vec![&new_i];
        if !hide_wip {
            file_items.push(&new_from_template_wip);
        }
        file_items.push(&open_i);
        file_items.push(&file_sep1);
        file_items.push(&close_i);
        file_items.push(&close_all_i);
        file_items.push(&file_sep2);
        file_items.push(&save_i);
        file_items.push(&save_as_i);
        file_items.push(&revert_i);
        file_items.push(&file_sep3);
        file_items.push(&import_i);
        file_items.push(&place_i);
        file_items.push(&export_menu);
        if !hide_wip {
            file_items.push(&package_wip);
            file_items.push(&file_sep4);
            file_items.push(&document_setup_wip);
        }
        file_items.push(&color_mode_menu);
        if !hide_wip {
            file_items.push(&file_info_wip);
            file_items.push(&file_sep5);
            file_items.push(&print_wip);
        }
        file_items.push(&file_sep6);
        file_items.push(&scripts_menu);
        let file = Submenu::with_items("File", true, &file_items).expect("file menu");
        // Stacking order (Bring/Send) lives under Object ▸ Arrange in real
        // Illustrator, not Edit — see `arrange_menu`, below.
        // Edit ▸ Paste in Front/Back — real backend (`paste_clipboard`,
        // same as the ⌘F/⌘B shortcuts already run).
        let paste_front_i = reg(&mut items, mk("Paste in Front", sup, Code::KeyF), MenuAction::PasteInFront);
        let paste_back_i = reg(&mut items, mk("Paste in Back", sup, Code::KeyB), MenuAction::PasteInBack);
        let paste_in_place_wip = wip("Paste in Place");
        let paste_all_artboards_wip = wip("Paste on All Artboards");
        let paste_without_format_wip = wip("Paste without Formatting");
        let find_replace_wip = wip("Find and Replace…");
        let spelling_wip = wip("Spelling");
        let edit_colors_wip = wip("Edit Colors");
        let color_settings_wip = wip("Color Settings…");
        let keyboard_shortcuts_wip = wip("Keyboard Shortcuts…");
        let edit_sep1 = sep();
        let edit_sep2 = sep();
        let mut edit_items: Vec<&dyn muda::IsMenuItem> = vec![
            &undo_i, &redo_i, &edit_sep1, &cut_i, &copy_i, &paste_i, &paste_front_i, &paste_back_i,
        ];
        if !hide_wip {
            edit_items.push(&paste_in_place_wip);
            edit_items.push(&paste_all_artboards_wip);
            edit_items.push(&paste_without_format_wip);
        }
        edit_items.push(&dup_i);
        if !hide_wip {
            edit_items.push(&edit_sep2);
            edit_items.push(&find_replace_wip);
            edit_items.push(&spelling_wip);
            edit_items.push(&edit_colors_wip);
            edit_items.push(&color_settings_wip);
            edit_items.push(&keyboard_shortcuts_wip);
        }
        let edit = Submenu::with_items("Edit", true, &edit_items).expect("edit menu");
        let same_shapes_text_wip = wip("Shapes & Text");
        let same_appearance_wip = wip("Appearance");
        let same_appearance_attr_wip = wip("Appearance Attribute");
        let same_blending_mode_wip = wip("Blending Mode");
        let same_graphic_style_wip = wip("Graphic Style");
        let same_shape_wip = wip("Shape");
        let same_symbol_wip = wip("Symbol Instance");
        let same_link_block_wip = wip("Link Block Series");
        let same_text_wip = wip("Text");
        let same_font_family_style_wip = wip("Font Family & Style");
        let same_font_family_style_size_wip = wip("Font Family, Style & Size");
        let same_text_fill_wip = wip("Text Fill Color");
        let same_text_stroke_wip = wip("Text Stroke Color");
        let same_text_fill_stroke_wip = wip("Text Fill & Stroke Color");
        let same_sep1 = sep();
        let same_sep2 = sep();
        let same_sep3 = sep();
        let mut same_items: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            same_items.push(&same_shapes_text_wip);
            same_items.push(&same_appearance_wip);
            same_items.push(&same_appearance_attr_wip);
            same_items.push(&same_blending_mode_wip);
        }
        same_items.push(&same_fillstroke_i);
        same_items.push(&same_fill_i);
        same_items.push(&same_opacity_i);
        same_items.push(&same_stroke_i);
        same_items.push(&same_weight_i);
        if !hide_wip {
            same_items.push(&same_graphic_style_wip);
            same_items.push(&same_shape_wip);
            same_items.push(&same_symbol_wip);
            same_items.push(&same_link_block_wip);
        }
        same_items.push(&same_sep1);
        if !hide_wip {
            same_items.push(&same_text_wip);
        }
        same_items.push(&same_font_i);
        if !hide_wip {
            same_items.push(&same_font_family_style_wip);
            same_items.push(&same_sep2);
            same_items.push(&same_font_family_style_size_wip);
        }
        same_items.push(&same_size_i);
        if !hide_wip {
            same_items.push(&same_sep3);
            same_items.push(&same_text_fill_wip);
            same_items.push(&same_text_stroke_wip);
            same_items.push(&same_text_fill_stroke_wip);
        }
        let same_menu = Submenu::with_items("Same", true, &same_items).expect("same menu");
        let clip_menu = Submenu::with_items("Clipping Mask", true, &[&clip_make_i, &clip_release_i])
            .expect("clip menu");
        let blend_menu = Submenu::with_items(
            "Blend",
            true,
            &[
                &blend_make_i,
                &blend_release_i,
                &sep(),
                &blend_options_i,
                &sep(),
                &blend_expand_i,
                &sep(),
                &blend_replace_spine_i,
                &blend_reverse_spine_i,
                &blend_reverse_stacking_i,
            ],
        )
        .expect("blend menu");
        // Object ▸ Transform — only "Transform Again" is real; the rest
        // need dedicated object-menu dialogs that don't exist yet (their
        // tool-driven equivalents — Rotate/Reflect/Scale tools — do).
        let xform_move_wip = wip("Move…");
        let xform_rotate_wip = wip("Rotate…");
        let xform_reflect_wip = wip("Reflect…");
        let xform_scale_wip = wip("Scale…");
        let xform_shear_wip = wip("Shear…");
        let xform_each_wip = wip("Transform Each…");
        let xform_reset_bbox_wip = wip("Reset Bounding Box");
        let tf_sep1 = sep();
        let tf_sep2 = sep();
        let tf_sep3 = sep();
        let mut transform_items: Vec<&dyn muda::IsMenuItem> = vec![&transform_again_i];
        if !hide_wip {
            transform_items.push(&tf_sep1);
            transform_items.push(&xform_move_wip);
            transform_items.push(&xform_rotate_wip);
            transform_items.push(&xform_reflect_wip);
            transform_items.push(&xform_scale_wip);
            transform_items.push(&xform_shear_wip);
            transform_items.push(&tf_sep2);
            transform_items.push(&xform_each_wip);
            transform_items.push(&tf_sep3);
            transform_items.push(&xform_reset_bbox_wip);
        }
        let transform_menu = Submenu::with_items("Transform", true, &transform_items).expect("transform menu");

        // Object ▸ Arrange — real Illustrator's home for stacking order
        // (moved out of Edit, see above).
        let send_current_layer_wip = wip("Send to Current Layer");
        let arrange_sep1 = sep();
        let mut arrange_items: Vec<&dyn muda::IsMenuItem> = vec![&front_i, &forward_i, &backward_i, &back_i];
        if !hide_wip {
            arrange_items.push(&arrange_sep1);
            arrange_items.push(&send_current_layer_wip);
        }
        let arrange_menu = Submenu::with_items("Arrange", true, &arrange_items).expect("arrange menu");

        // Object ▸ Align / Distribute — real backend already exists
        // (`Command::Align`, the same `AlignKind` the Align panel's own
        // buttons dispatch), just needed a menu slot.
        let align_hleft_i = reg(&mut items, MenuItem::new("Horizontal Align Left", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::HLeft));
        let align_hcenter_i = reg(&mut items, MenuItem::new("Horizontal Align Center", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::HCenter));
        let align_hright_i = reg(&mut items, MenuItem::new("Horizontal Align Right", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::HRight));
        let align_vtop_i = reg(&mut items, MenuItem::new("Vertical Align Top", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::VTop));
        let align_vcenter_i = reg(&mut items, MenuItem::new("Vertical Align Center", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::VCenter));
        let align_vbottom_i = reg(&mut items, MenuItem::new("Vertical Align Bottom", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::VBottom));
        let align_menu = Submenu::with_items(
            "Align",
            true,
            &[&align_hleft_i, &align_hcenter_i, &align_hright_i, &align_vtop_i, &align_vcenter_i, &align_vbottom_i],
        )
        .expect("align menu");
        let dist_vtop_i = reg(&mut items, MenuItem::new("Vertical Distribute Top", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistVTop));
        let dist_vcenter_i = reg(&mut items, MenuItem::new("Vertical Distribute Center", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistVCenter));
        let dist_vbottom_i = reg(&mut items, MenuItem::new("Vertical Distribute Bottom", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistVBottom));
        let dist_hleft_i = reg(&mut items, MenuItem::new("Horizontal Distribute Left", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistHLeft));
        let dist_hcenter_i = reg(&mut items, MenuItem::new("Horizontal Distribute Center", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistHCenter));
        let dist_hright_i = reg(&mut items, MenuItem::new("Horizontal Distribute Right", true, None), MenuAction::AlignObjects(amalith_commands::AlignKind::DistHRight));
        let distribute_menu = Submenu::with_items(
            "Distribute",
            true,
            &[&dist_vtop_i, &dist_vcenter_i, &dist_vbottom_i, &dist_hleft_i, &dist_hcenter_i, &dist_hright_i],
        )
        .expect("distribute menu");

        // Object ▸ Group / Ungroup — real backend (`Command::Group`,
        // `Editor::ungroup`), same as the ⌘G / ⌘⇧G shortcuts already run.
        let group_i = reg(&mut items, mk("Group", sup, Code::KeyG), MenuAction::GroupSelection);
        let ungroup_i = reg(&mut items, mk("Ungroup", sup_shift, Code::KeyG), MenuAction::UngroupSelection);
        let ungroup_all_wip = wip("Ungroup All");

        let lock_all_above_wip = wip("All Artwork Above");
        let lock_other_layers_wip = wip("Other Layers");
        let lock_menu = Submenu::with_items(
            "Lock",
            true,
            &[&lock_selection_i, &lock_all_above_wip, &lock_other_layers_wip],
        )
        .expect("lock menu");
        // Hide's three entries (Selection/All Artwork Above/Other Layers)
        // have no backend yet at all — collapse to one placeholder rather
        // than a submenu that would be empty whenever WIP items are hidden.
        let hide_wip_item = wip("Hide");
        let show_all_wip = wip("Show All");

        let expand_wip = wip("Expand…");
        let expand_appearance_wip = wip("Expand Appearance");
        let crop_image_wip = wip("Crop Image");
        let rasterize_wip = wip("Rasterize…");
        let gradient_mesh_wip = wip("Create Gradient Mesh…");
        let object_mosaic_wip = wip("Create Object Mosaic…");
        let trim_marks_wip = wip("Create Trim Marks");
        let flatten_transparency_wip = wip("Flatten Transparency…");
        let pixel_perfect_wip = wip("Make Pixel Perfect");
        let generative_wip = wip("Generative");
        let slice_wip = wip("Slice");

        // Object ▸ Path — Offset Path is real; the rest need their own
        // dedicated implementations (anchor/segment editing beyond what
        // Direct Selection already does).
        let path_join_wip = wip("Join");
        let path_average_wip = wip("Average…");
        let path_outline_stroke_wip = wip("Outline Stroke");
        let path_reverse_wip = wip("Reverse Path Direction");
        let path_simplify_wip = wip("Simplify…");
        let path_smooth_wip = wip("Smooth…");
        let path_add_anchors_wip = wip("Add Anchor Points");
        let path_remove_anchors_wip = wip("Remove Anchor Points");
        let path_divide_below_wip = wip("Divide Objects Below");
        let path_split_grid_wip = wip("Split Into Grid…");
        let path_clean_up_wip = wip("Clean Up…");
        let path_sep1 = sep();
        let path_sep2 = sep();
        let mut path_items: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            path_items.push(&path_join_wip);
            path_items.push(&path_average_wip);
            path_items.push(&path_sep1);
        }
        path_items.push(&offset_path_i);
        if !hide_wip {
            path_items.push(&path_outline_stroke_wip);
            path_items.push(&path_reverse_wip);
            path_items.push(&path_sep2);
            path_items.push(&path_simplify_wip);
            path_items.push(&path_smooth_wip);
            path_items.push(&path_add_anchors_wip);
            path_items.push(&path_remove_anchors_wip);
            path_items.push(&path_divide_below_wip);
            path_items.push(&path_split_grid_wip);
            path_items.push(&path_clean_up_wip);
        }
        let path_menu = Submenu::with_items("Path", true, &path_items).expect("path menu");

        let shape_wip = wip("Shape");
        let pattern_wip = wip("Pattern");
        let intertwine_wip = wip("Intertwine");
        let repeat_wip = wip("Repeat");
        let objects_on_path_wip = wip("Objects on Path");
        let envelope_distort_wip = wip("Envelope Distort");
        let perspective_wip = wip("Perspective");
        let live_paint_wip = wip("Live Paint");
        let mockup_wip = wip("Mockup");
        let image_trace_wip = wip("Image Trace");
        let text_wrap_wip = wip("Text Wrap");
        let compound_path_wip = wip("Compound Path");
        let artboards_wip = wip("Artboards");
        let graph_wip = wip("Graph");
        let collect_export_wip = wip("Collect For Export");

        let obj_sep1 = sep(); // after Distribute, before Group
        let obj_sep2 = sep(); // after Show All, before Expand...
        let obj_sep3 = sep(); // after Flatten Transparency, before Make Pixel Perfect
        let obj_sep4 = sep(); // after Make Pixel Perfect, before Generative
        let obj_sep5 = sep(); // after Generative, before Slice
        let obj_sep6 = sep(); // after Slice, before Path
        let obj_sep7 = sep(); // after Text Wrap, before Clipping Mask
        let obj_sep8 = sep(); // after Graph, before Collect For Export

        let mut object_items: Vec<&dyn muda::IsMenuItem> = vec![
            &transform_menu, &arrange_menu, &align_menu, &distribute_menu, &obj_sep1,
            &group_i, &ungroup_i,
        ];
        if !hide_wip {
            object_items.push(&ungroup_all_wip);
        }
        object_items.push(&lock_menu);
        object_items.push(&unlock_all_i);
        if !hide_wip {
            object_items.push(&hide_wip_item);
            object_items.push(&show_all_wip);
            object_items.push(&obj_sep2);
            object_items.push(&expand_wip);
            object_items.push(&expand_appearance_wip);
            object_items.push(&crop_image_wip);
            object_items.push(&rasterize_wip);
            object_items.push(&gradient_mesh_wip);
            object_items.push(&object_mosaic_wip);
            object_items.push(&trim_marks_wip);
            object_items.push(&flatten_transparency_wip);
            object_items.push(&obj_sep3);
            object_items.push(&pixel_perfect_wip);
            object_items.push(&obj_sep4);
            object_items.push(&generative_wip);
            object_items.push(&obj_sep5);
            object_items.push(&slice_wip);
            object_items.push(&obj_sep6);
        }
        object_items.push(&path_menu);
        if !hide_wip {
            object_items.push(&shape_wip);
            object_items.push(&pattern_wip);
            object_items.push(&intertwine_wip);
            object_items.push(&repeat_wip);
            object_items.push(&objects_on_path_wip);
        }
        object_items.push(&blend_menu);
        if !hide_wip {
            object_items.push(&envelope_distort_wip);
            object_items.push(&perspective_wip);
            object_items.push(&live_paint_wip);
            object_items.push(&mockup_wip);
            object_items.push(&image_trace_wip);
            object_items.push(&text_wrap_wip);
            object_items.push(&obj_sep7);
        }
        object_items.push(&clip_menu);
        if !hide_wip {
            object_items.push(&compound_path_wip);
            object_items.push(&artboards_wip);
            object_items.push(&graph_wip);
            object_items.push(&obj_sep8);
            object_items.push(&collect_export_wip);
        }
        let object_menu = Submenu::with_items("Object", true, &object_items).expect("object menu");
        let reselect_wip = wip("Reselect");
        let inverse_wip = wip("Inverse");
        let select_object_wip = wip("Object");
        let start_global_edit_wip = wip("Start Global Edit");
        let save_selection_wip = wip("Save Selection…");
        let edit_selection_wip = wip("Edit Selection…");
        let update_selection_wip = wip("Update Selection");
        let sel_sep1 = sep();
        let sel_sep2 = sep();
        let sel_sep3 = sep();
        let mut select_items: Vec<&dyn muda::IsMenuItem> = vec![&all_i, &sel_artboard_i, &deselect_i];
        if !hide_wip {
            select_items.push(&reselect_wip);
            select_items.push(&inverse_wip);
        }
        select_items.push(&sel_sep1);
        select_items.push(&next_above_i);
        select_items.push(&next_below_i);
        select_items.push(&sel_sep2);
        select_items.push(&same_menu);
        if !hide_wip {
            select_items.push(&select_object_wip);
            select_items.push(&start_global_edit_wip);
            select_items.push(&sel_sep3);
            select_items.push(&save_selection_wip);
            select_items.push(&edit_selection_wip);
            select_items.push(&update_selection_wip);
        }
        let select_menu = Submenu::with_items("Select", true, &select_items).expect("select menu");
        // Type menu — the convert item's label + enabled state track the
        // selection (see `NativeMenu::sync_type`).
        let convert_text_i = reg(
            &mut items,
            MenuItem::new("Convert to Area Type", false, None),
            MenuAction::ConvertTextKind,
        );
        let area_type_options_i = reg(
            &mut items,
            MenuItem::new("Area Type Options…", true, None),
            MenuAction::AreaTypeOptions,
        );
        let type_font_wip = wip("Font");
        let type_recent_fonts_wip = wip("Recent Fonts");
        let type_size_wip = wip("Size");
        let type_glyphs_wip = wip("Glyphs");
        let type_on_path_wip = wip("Type on a Path");
        let type_threaded_text_wip = wip("Threaded Text");
        let type_fit_headline_wip = wip("Fit Headline");
        let type_resolve_fonts_wip = wip("Resolve Missing Fonts…");
        let type_find_replace_font_wip = wip("Find/Replace Font…");
        let type_change_case_wip = wip("Change Case");
        let type_smart_punct_wip = wip("Smart Punctuation…");
        let type_create_outlines_wip = wip("Create Outlines");
        let type_optical_margin_wip = wip("Optical Margin Alignment");
        let type_retype_wip = wip("Retype");
        let type_bullets_wip = wip("Bullets and Numbering");
        let type_insert_special_wip = wip("Insert Special Character");
        let type_insert_whitespace_wip = wip("Insert Whitespace Character");
        let type_insert_break_wip = wip("Insert Break Character");
        let type_placeholder_text_wip = wip("Fill with Placeholder Text");
        let type_show_hidden_wip = wip("Show Hidden Characters");
        let type_orientation_wip = wip("Type Orientation");
        let type_legacy_wip = wip("Legacy Text");
        let t_sep1 = sep();
        let t_sep2 = sep();
        let t_sep3 = sep();
        let t_sep4 = sep();
        let t_sep5 = sep();
        let t_sep6 = sep();
        let mut type_items: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            type_items.push(&type_font_wip);
            type_items.push(&type_recent_fonts_wip);
            type_items.push(&type_size_wip);
            type_items.push(&t_sep1);
            type_items.push(&type_glyphs_wip);
            type_items.push(&t_sep2);
        }
        type_items.push(&convert_text_i);
        type_items.push(&area_type_options_i);
        if !hide_wip {
            type_items.push(&type_on_path_wip);
            type_items.push(&type_threaded_text_wip);
            type_items.push(&t_sep3);
            type_items.push(&type_fit_headline_wip);
            type_items.push(&type_resolve_fonts_wip);
            type_items.push(&type_find_replace_font_wip);
            type_items.push(&type_change_case_wip);
            type_items.push(&type_smart_punct_wip);
            type_items.push(&type_create_outlines_wip);
            type_items.push(&type_optical_margin_wip);
            type_items.push(&type_retype_wip);
            type_items.push(&t_sep4);
            type_items.push(&type_bullets_wip);
            type_items.push(&t_sep5);
            type_items.push(&type_insert_special_wip);
            type_items.push(&type_insert_whitespace_wip);
            type_items.push(&type_insert_break_wip);
            type_items.push(&type_placeholder_text_wip);
            type_items.push(&t_sep6);
            type_items.push(&type_show_hidden_wip);
            type_items.push(&type_orientation_wip);
            type_items.push(&type_legacy_wip);
        }
        let type_menu = Submenu::with_items("Type", true, &type_items).expect("type menu");
        // Effect menu — Illustrator's own live-effect menu (distinct from
        // Object ▸ Path ▸ Offset Path above, which is the destructive
        // one): adds a live effect to the currently selected Appearance-
        // panel row (or the topmost item in the selection's stack if none
        // is explicitly selected there — see `MenuAction::EffectMenu`'s
        // own doc comment). Free Distort isn't here — it needs an
        // on-canvas corner-drag interaction, not a menu item that opens a
        // numeric dialog.
        let effect_offset_i = reg(
            &mut items,
            MenuItem::new("Offset Path…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Offset),
        );
        let effect_zigzag_i = reg(
            &mut items,
            MenuItem::new("Zig Zag…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::ZigZag)),
        );
        let effect_pucker_bloat_i = reg(
            &mut items,
            MenuItem::new("Pucker & Bloat…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::PuckerBloat)),
        );
        let effect_roughen_i = reg(
            &mut items,
            MenuItem::new("Roughen…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::Roughen)),
        );
        let effect_transform_i = reg(
            &mut items,
            MenuItem::new("Transform…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::Transform)),
        );
        let effect_tweak_i = reg(
            &mut items,
            MenuItem::new("Tweak…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::Tweak)),
        );
        let effect_twist_i = reg(
            &mut items,
            MenuItem::new("Twist…", true, None),
            MenuAction::EffectMenu(panels::EffectMenuChoice::Distort(crate::effectdlg::EffectKind::Twist)),
        );
        let distort_transform_menu = Submenu::with_items(
            "Distort & Transform",
            true,
            &[
                &effect_zigzag_i,
                &effect_pucker_bloat_i,
                &effect_roughen_i,
                &effect_transform_i,
                &effect_tweak_i,
                &effect_twist_i,
            ],
        )
        .expect("distort & transform menu");
        // Effect ▸ Path — real Illustrator nests Offset Path here (not
        // top-level); Outline Object/Stroke need their own implementation.
        let effect_outline_object_wip = wip("Outline Object");
        let effect_outline_stroke_wip = wip("Outline Stroke");
        let mut effect_path_items: Vec<&dyn muda::IsMenuItem> = vec![&effect_offset_i];
        if !hide_wip {
            effect_path_items.push(&effect_outline_object_wip);
            effect_path_items.push(&effect_outline_stroke_wip);
        }
        let effect_path_menu = Submenu::with_items("Path", true, &effect_path_items).expect("effect path menu");

        // Effect ▸ Pathfinder — real backend (`Command::Pathfinder`, the
        // same `PathfinderOp` the Pathfinder panel's own buttons dispatch).
        let pf_add_i = reg(&mut items, MenuItem::new("Add", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Unite));
        let pf_intersect_i = reg(&mut items, MenuItem::new("Intersect", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Intersect));
        let pf_exclude_i = reg(&mut items, MenuItem::new("Exclude", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Exclude));
        let pf_subtract_i = reg(&mut items, MenuItem::new("Subtract", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::MinusFront));
        let pf_minus_back_i = reg(&mut items, MenuItem::new("Minus Back", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::MinusBack));
        let pf_divide_i = reg(&mut items, MenuItem::new("Divide", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Divide));
        let pf_trim_i = reg(&mut items, MenuItem::new("Trim", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Trim));
        let pf_merge_i = reg(&mut items, MenuItem::new("Merge", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Merge));
        let pf_crop_i = reg(&mut items, MenuItem::new("Crop", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Crop));
        let pf_outline_i = reg(&mut items, MenuItem::new("Outline", true, None), MenuAction::PathfinderOp(amalith_commands::PathfinderOp::Outline));
        let pf_hard_mix_wip = wip("Hard Mix");
        let pf_soft_mix_wip = wip("Soft Mix…");
        let pf_trap_wip = wip("Trap…");
        let mut pathfinder_items: Vec<&dyn muda::IsMenuItem> = vec![
            &pf_add_i, &pf_intersect_i, &pf_exclude_i, &pf_subtract_i, &pf_minus_back_i,
            &pf_divide_i, &pf_trim_i, &pf_merge_i, &pf_crop_i, &pf_outline_i,
        ];
        if !hide_wip {
            pathfinder_items.push(&pf_hard_mix_wip);
            pathfinder_items.push(&pf_soft_mix_wip);
            pathfinder_items.push(&pf_trap_wip);
        }
        let pathfinder_menu = Submenu::with_items("Pathfinder", true, &pathfinder_items).expect("pathfinder menu");

        // Everything else in Illustrator's real Effect menu that Amalith's
        // vector pipeline has no backend for yet — collapsed to one WIP
        // placeholder per named category rather than itemizing subtrees
        // with zero functional value (3D and Materials, Convert to Shape,
        // Stylize, and the whole "Photoshop Effects" raster-filter section
        // each have many named children in real Illustrator).
        let effect_3d_wip = wip("3D and Materials");
        let effect_convert_shape_wip = wip("Convert to Shape");
        let effect_crop_marks_wip = wip("Crop Marks");
        let effect_rasterize_wip = wip("Rasterize…");
        let effect_stylize_wip = wip("Stylize");
        let effect_svg_filters_wip = wip("SVG Filters");
        let effect_warp_wip = wip("Warp");
        let effect_doc_raster_settings_wip = wip("Document Raster Effects Settings…");
        let ps_effects_header_wip = MenuItem::new("Photoshop Effects", false, None);
        let ps_gallery_wip = wip("Effect Gallery…");
        let ps_artistic_wip = wip("Artistic");
        let ps_blur_wip = wip("Blur");
        let ps_brush_strokes_wip = wip("Brush Strokes");
        let ps_distort_wip = wip("Distort");
        let ps_pixelate_wip = wip("Pixelate");
        let ps_sketch_wip = wip("Sketch");
        let ps_stylize_wip = wip("Stylize");
        let ps_texture_wip = wip("Texture");
        let ps_video_wip = wip("Video");

        let effect_sep1 = sep(); // after Document Raster Effects Settings
        let effect_sep2 = sep(); // after Warp, before Photoshop Effects
        let mut effect_items: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            effect_items.push(&effect_doc_raster_settings_wip);
            effect_items.push(&effect_sep1);
            effect_items.push(&effect_3d_wip);
            effect_items.push(&effect_convert_shape_wip);
            effect_items.push(&effect_crop_marks_wip);
        }
        effect_items.push(&distort_transform_menu);
        effect_items.push(&effect_path_menu);
        effect_items.push(&pathfinder_menu);
        if !hide_wip {
            effect_items.push(&effect_rasterize_wip);
            effect_items.push(&effect_stylize_wip);
            effect_items.push(&effect_svg_filters_wip);
            effect_items.push(&effect_warp_wip);
            effect_items.push(&effect_sep2);
            effect_items.push(&ps_effects_header_wip);
            effect_items.push(&ps_gallery_wip);
            effect_items.push(&ps_artistic_wip);
            effect_items.push(&ps_blur_wip);
            effect_items.push(&ps_brush_strokes_wip);
            effect_items.push(&ps_distort_wip);
            effect_items.push(&ps_pixelate_wip);
            effect_items.push(&ps_sketch_wip);
            effect_items.push(&ps_stylize_wip);
            effect_items.push(&ps_texture_wip);
            effect_items.push(&ps_video_wip);
        }
        let effect_menu = Submenu::with_items("Effect", true, &effect_items).expect("effect menu");
        let overprint_preview_wip = wip("Overprint Preview");
        let pixel_preview_wip = wip("Pixel Preview");
        let trim_view_wip = wip("Trim View");
        let presentation_mode_wip = wip("Presentation Mode");
        let screen_mode_wip = wip("Screen Mode");
        let proof_setup_wip = wip("Proof Setup");
        let proof_colors_wip = wip("Proof Colors");
        let rotate_view_wip = wip("Rotate View");
        let show_slices_wip = wip("Show Slices");
        let lock_slices_wip = wip("Lock Slices");
        let hide_bbox_wip = wip("Hide Bounding Box");
        let actual_size_wip = wip("Actual Size");
        let live_paint_gaps_wip = wip("Show Live Paint Gaps");
        let hide_gradient_annotator_wip = wip("Hide Gradient Annotator");
        let hide_corner_widget_wip = wip("Hide Corner Widget");
        let hide_edges_wip = wip("Hide Edges");
        let perspective_grid_wip = wip("Perspective Grid");
        let hide_artboards_wip = wip("Hide Artboards");
        let show_print_tiling_wip = wip("Show Print Tiling");
        let rulers_wip = wip("Rulers");
        let hide_text_threads_wip = wip("Hide Text Threads");
        let snap_to_glyph_wip = wip("Snap to Glyph");
        let new_view_wip = wip("New View…");
        let edit_views_wip = wip("Edit Views…");
        let v_sep1 = sep();
        let v_sep2 = sep();
        let v_sep3 = sep();
        let v_sep4 = sep();
        let v_sep5 = sep();
        let v_sep6 = sep();
        let v_sep7 = sep();
        let v_sep8 = sep();
        let v_sep_u1 = sep();
        let v_sep_u2 = sep();
        let v_sep_u3 = sep();
        let v_sep_u4 = sep();
        let v_sep_u5 = sep();
        let v_sep_u6 = sep();
        let v_sep_u7 = sep();
        let mut view_items: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            view_items.push(&overprint_preview_wip);
            view_items.push(&pixel_preview_wip);
            view_items.push(&trim_view_wip);
            view_items.push(&v_sep1);
            view_items.push(&presentation_mode_wip);
            view_items.push(&v_sep2);
            view_items.push(&screen_mode_wip);
            view_items.push(&v_sep3);
            view_items.push(&proof_setup_wip);
            view_items.push(&proof_colors_wip);
            view_items.push(&v_sep4);
        }
        view_items.push(&zoom_in_i);
        view_items.push(&zoom_out_i);
        view_items.push(&v_sep_u1);
        view_items.push(&fit_artboard_i);
        view_items.push(&fit_all_i);
        if !hide_wip {
            view_items.push(&v_sep5);
            view_items.push(&rotate_view_wip);
        }
        view_items.push(&v_sep_u2);
        view_items.push(&outline_i);
        view_items.push(&transparency_grid_i);
        if !hide_wip {
            view_items.push(&show_slices_wip);
            view_items.push(&lock_slices_wip);
            view_items.push(&hide_bbox_wip);
            view_items.push(&actual_size_wip);
            view_items.push(&live_paint_gaps_wip);
            view_items.push(&hide_gradient_annotator_wip);
            view_items.push(&hide_corner_widget_wip);
            view_items.push(&hide_edges_wip);
        }
        view_items.push(&v_sep_u3);
        view_items.push(&smart_guides_i);
        if !hide_wip {
            view_items.push(&snap_to_glyph_wip);
        }
        view_items.push(&v_sep_u4);
        if !hide_wip {
            view_items.push(&perspective_grid_wip);
            view_items.push(&v_sep6);
        }
        view_items.push(&show_grid_i);
        view_items.push(&v_sep_u5);
        view_items.push(&snap_to_grid_i);
        view_items.push(&snap_to_pixel_i);
        view_items.push(&v_sep_u6);
        view_items.push(&snap_to_point_i);
        if !hide_wip {
            view_items.push(&v_sep7);
            view_items.push(&hide_artboards_wip);
            view_items.push(&show_print_tiling_wip);
            view_items.push(&rulers_wip);
            view_items.push(&hide_text_threads_wip);
        }
        view_items.push(&v_sep_u7);
        view_items.push(&guides_show_i);
        view_items.push(&guides_lock_i);
        view_items.push(&clear_guides_i);
        if !hide_wip {
            view_items.push(&v_sep8);
            view_items.push(&new_view_wip);
            view_items.push(&edit_views_wip);
        }
        let view = Submenu::with_items("View", true, &view_items).expect("view menu");

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
        let new_window_wip = wip("New Window");
        let window_arrange_wip = wip("Arrange");
        let find_extensions_wip = wip("Find Extensions on Exchange…");
        let app_frame_wip = wip("Application Frame");
        let app_bar_wip = wip("Application Bar");
        let contextual_task_bar_wip = wip("Contextual Task Bar");
        let control_wip = wip("Control");
        let help_bar_wip = wip("Help Bar");
        let toolbars_wip = wip("Toolbars");
        let brush_libraries_wip = wip("Brush Libraries");
        let graphic_style_libraries_wip = wip("Graphic Style Libraries");
        let swatch_libraries_wip = wip("Swatch Libraries");
        let symbol_libraries_wip = wip("Symbol Libraries");
        let win_sep1 = sep();
        let win_sep2 = sep();
        let win_sep3 = sep();
        let windows_sep = sep();
        let mut window_refs: Vec<&dyn muda::IsMenuItem> = Vec::new();
        if !hide_wip {
            window_refs.push(&new_window_wip);
            window_refs.push(&win_sep1);
            window_refs.push(&window_arrange_wip);
            window_refs.push(&find_extensions_wip);
        }
        window_refs.push(&workspace_menu);
        if !hide_wip {
            window_refs.push(&win_sep2);
            window_refs.push(&app_frame_wip);
            window_refs.push(&app_bar_wip);
            window_refs.push(&contextual_task_bar_wip);
            window_refs.push(&control_wip);
            window_refs.push(&help_bar_wip);
            window_refs.push(&toolbars_wip);
        }
        window_refs.push(&windows_sep);
        window_refs.extend(window_checks.iter().map(|(_, i)| i as &dyn muda::IsMenuItem));
        if !hide_wip {
            window_refs.push(&win_sep3);
            window_refs.push(&brush_libraries_wip);
            window_refs.push(&graphic_style_libraries_wip);
            window_refs.push(&swatch_libraries_wip);
            window_refs.push(&symbol_libraries_wip);
        }
        // "Window" (singular) is a name AppKit reserves for its own window
        // menu; "Windows" (plural) sidesteps that collision.
        let panels_menu = Submenu::with_items("Windows", true, &window_refs).expect("windows menu");

        // A menu literally titled "Help" gets AppKit's search field for
        // free on macOS; on Windows it's just the one link.
        let help_docs_i = reg(&mut items, MenuItem::new("Amalith Help", true, None), MenuAction::HelpDocs);
        let tutorials_wip = wip("Tutorials…");
        let whats_new_wip = wip("What's New…");
        let support_community_wip = wip("Support Community");
        let submit_bug_wip = wip("Submit Bug/Feature Request…");
        let system_info_wip = wip("System Info…");
        let help_sep1 = sep();
        let help_sep2 = sep();
        let mut help_items: Vec<&dyn muda::IsMenuItem> = vec![&help_docs_i];
        if !hide_wip {
            help_items.push(&tutorials_wip);
            help_items.push(&whats_new_wip);
            help_items.push(&help_sep1);
            help_items.push(&support_community_wip);
            help_items.push(&submit_bug_wip);
            help_items.push(&help_sep2);
            help_items.push(&system_info_wip);
        }
        let help_menu = Submenu::with_items("Help", true, &help_items).expect("help menu");

        let menu = Menu::new();
        menu.append(&app).expect("append app menu");
        menu.append(&file).expect("append file menu");
        menu.append(&edit).expect("append edit menu");
        menu.append(&object_menu).expect("append object menu");
        menu.append(&type_menu).expect("append type menu");
        menu.append(&select_menu).expect("append select menu");
        menu.append(&effect_menu).expect("append effect menu");
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
            grid_checks: (show_grid_i, snap_to_grid_i, snap_to_pixel_i, snap_to_point_i),
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

    /// Match the View ▸ Show Grid checkmark to the live toggle.
    pub(in crate::app) fn sync_show_grid(&self, on: bool) {
        self.grid_checks.0.set_checked(on);
    }

    /// Match the View ▸ Snap to Grid checkmark to the live toggle.
    pub(in crate::app) fn sync_snap_to_grid(&self, on: bool) {
        self.grid_checks.1.set_checked(on);
    }

    /// Match the View ▸ Snap to Pixel checkmark to the live toggle.
    pub(in crate::app) fn sync_snap_to_pixel(&self, on: bool) {
        self.grid_checks.2.set_checked(on);
    }

    /// Match the View ▸ Snap to Point checkmark to the live toggle.
    pub(in crate::app) fn sync_snap_to_point(&self, on: bool) {
        self.grid_checks.3.set_checked(on);
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
