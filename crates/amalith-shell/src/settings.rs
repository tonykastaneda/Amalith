//! Persisted application settings.
//!
//! A plain `key = value` text file in the platform's config directory,
//! next to `recents.txt`. Loaded once at startup, rewritten whole when
//! Preferences is confirmed. Unknown / malformed lines are ignored so an
//! older file still loads.

use std::path::PathBuf;
use std::str::FromStr;

use crate::prefs::{KeyChord, PrefAction, Settings};
use crate::tool::Tool;

/// `~/Library/Application Support/Amalith` (macOS), `%APPDATA%\Amalith`
/// (Windows), `$XDG_CONFIG_HOME/amalith` or `~/.config/amalith` (Linux).
/// Where `settings.txt`, `recents.txt` and `layout.json` live.
pub(crate) fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(PathBuf::from(home).join("Library/Application Support/Amalith"))
    }
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("Amalith"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            Some(PathBuf::from(x).join("amalith"))
        } else {
            let home = std::env::var_os("HOME")?;
            Some(PathBuf::from(home).join(".config/amalith"))
        }
    }
}

fn store_path() -> Option<PathBuf> {
    Some(config_dir()?.join("settings.txt"))
}

/// Load settings, starting from [`Settings::default`] and overriding with
/// whatever the file recognises.
pub fn load() -> Settings {
    let s = Settings::default();
    let Some(path) = store_path() else {
        return s;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return s;
    };
    parse(&text)
}

fn parse(text: &str) -> Settings {
    let mut s = Settings::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "ui_scale" => {
                if let Ok(n) = v.parse::<f64>() {
                    s.ui_scale = crate::metrics::normalize_scale(n);
                }
            }
            "nudge_step" => {
                if let Ok(n) = v.parse::<f64>() {
                    s.nudge_step = n.clamp(0.5, 100.0);
                }
            }
            "show_tooltips" => s.show_tooltips = v == "true",
            "home_on_last_close" => s.home_on_last_close = v == "true",
            "show_fps" => s.show_fps = v == "true",
            "show_cull_outline" => s.show_cull_outline = v == "true",
            "cull_inset" => {
                if let Ok(n) = v.parse::<f64>() {
                    s.cull_inset = n.clamp(0.0, 1000.0);
                }
            }
            "hide_wip_menu_items" => s.hide_wip_menu_items = v == "true",
            "hide_wip_tools" => s.hide_wip_tools = v == "true",
            "accent" => {
                if let Some(rgb) = parse_hex(v) {
                    s.accent = rgb;
                }
            }
            "handle_size" => {
                if let Some(size) = crate::handle_scale::HandleSize::from_id_str(v) {
                    s.handle_size = size;
                }
            }
            "smart_guides_enabled" => s.smart_guides_enabled = v == "true",
            "sg_alignment_guides" => s.sg_alignment_guides = v == "true",
            "sg_anchor_path_labels" => s.sg_anchor_path_labels = v == "true",
            "sg_object_highlighting" => s.sg_object_highlighting = v == "true",
            "sg_measurement_labels" => s.sg_measurement_labels = v == "true",
            "sg_construction_guides" => s.sg_construction_guides = v == "true",
            "sg_transform_tools" => s.sg_transform_tools = v == "true",
            "sg_spacing_guides" => s.sg_spacing_guides = v == "true",
            "sg_tolerance" => {
                if let Ok(n) = v.parse::<f64>() {
                    if n.is_finite() { s.sg_tolerance = n.clamp(0.5, 50.0); }
                }
            }
            "sg_angles" => {
                let parsed: Vec<f64> = v.split(',').filter_map(|p| p.trim().parse::<f64>().ok()).collect();
                if let Ok(angles) = <[f64; 6]>::try_from(parsed) {
                    if angles.iter().all(|a| a.is_finite()) { s.sg_angles = angles; }
                }
            }
            "show_grid" => s.show_grid = v == "true",
            "snap_to_grid" => s.snap_to_grid = v == "true",
            "snap_to_pixel" => s.snap_to_pixel = v == "true",
            "snap_to_point" => s.snap_to_point = v == "true",
            "grid_spacing" => {
                if let Ok(n) = v.parse::<f64>() {
                    if n.is_finite() { s.grid_spacing = n.clamp(1.0, 10_000.0); }
                }
            }
            _ => {
                if let Some(name) = k.strip_prefix("tool.") {
                    if let Some(i) = Tool::ALL.iter().position(|t| tool_name(*t) == name) {
                        s.tool_keys[i] = if v.is_empty() {
                            None
                        } else {
                            KeyChord::from_str(v).ok()
                        };
                    }
                } else if let Some(name) = k.strip_prefix("action.") {
                    if let Some(i) =
                        PrefAction::ALL.iter().position(|a| action_name(*a) == name)
                    {
                        s.action_keys[i] = if v.is_empty() {
                            None
                        } else {
                            KeyChord::from_str(v).ok()
                        };
                    }
                }
            }
        }
    }
    s
}

/// Rewrite the whole file from `s`.
pub fn save(s: &Settings) {
    let Some(path) = store_path() else {
        return;
    };
    let body = serialize(s);

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, body);
}

fn serialize(s: &Settings) -> String {
    let mut body = format!(
        "ui_scale = {}\nnudge_step = {}\nshow_tooltips = {}\nhome_on_last_close = {}\naccent = {:02x}{:02x}{:02x}\n\
         show_fps = {}\nshow_cull_outline = {}\ncull_inset = {}\nhide_wip_menu_items = {}\nhide_wip_tools = {}\nhandle_size = {}\n\
         smart_guides_enabled = {}\nsg_alignment_guides = {}\nsg_anchor_path_labels = {}\n\
         sg_object_highlighting = {}\nsg_measurement_labels = {}\nsg_construction_guides = {}\n\
         sg_transform_tools = {}\nsg_spacing_guides = {}\nsg_tolerance = {}\nsg_angles = {}\n\
         show_grid = {}\nsnap_to_grid = {}\nsnap_to_pixel = {}\nsnap_to_point = {}\ngrid_spacing = {}\n",
        s.ui_scale,
        s.nudge_step,
        s.show_tooltips,
        s.home_on_last_close,
        s.accent[0],
        s.accent[1],
        s.accent[2],
        s.show_fps,
        s.show_cull_outline,
        s.cull_inset,
        s.hide_wip_menu_items,
        s.hide_wip_tools,
        s.handle_size.id_str(),
        s.smart_guides_enabled,
        s.sg_alignment_guides,
        s.sg_anchor_path_labels,
        s.sg_object_highlighting,
        s.sg_measurement_labels,
        s.sg_construction_guides,
        s.sg_transform_tools,
        s.sg_spacing_guides,
        s.sg_tolerance,
        s.sg_angles.map(|a| a.to_string()).join(","),
        s.show_grid,
        s.snap_to_grid,
        s.snap_to_pixel,
        s.snap_to_point,
        s.grid_spacing,
    );
    for (i, tool) in Tool::ALL.iter().enumerate() {
        let v = s.tool_keys[i].map_or_else(String::new, |c| c.to_string());
        body.push_str(&format!("tool.{} = {}\n", tool_name(*tool), v));
    }
    for (i, act) in PrefAction::ALL.iter().enumerate() {
        let v = s.action_keys[i].map_or_else(String::new, |c| c.to_string());
        body.push_str(&format!("action.{} = {}\n", action_name(*act), v));
    }

    body
}

/// A stable file key for a tool (independent of its display label).
pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Select",
        Tool::DirectSelect => "DirectSelect",
        Tool::Pen => "Pen",
        Tool::Line => "Line",
        Tool::Text => "Text",
        Tool::Rectangle => "Rectangle",
        Tool::RoundedRect => "RoundedRect",
        Tool::Ellipse => "Ellipse",
        Tool::Polygon => "Polygon",
        Tool::Star => "Star",
        Tool::Artboard => "Artboard",
        Tool::Hand => "Hand",
        Tool::Zoom => "Zoom",
        Tool::Eyedropper => "Eyedropper",
        Tool::Gradient => "Gradient",
        Tool::Rotate => "Rotate",
        Tool::Reflect => "Reflect",
        Tool::Shear => "Shear",
        Tool::Scale => "Scale",
        Tool::Blend => "Blend",
        Tool::Width => "Width",
        Tool::Arc => "Arc",
        Tool::Spiral => "Spiral",
        Tool::FreeTransform => "FreeTransform",
        Tool::Join => "Join",
        Tool::ShapeBuilder => "ShapeBuilder",
        Tool::Eraser => "Eraser",
        Tool::VerticalText => "VerticalText",
        Tool::AreaType => "AreaType",
        Tool::PathType => "PathType",
        Tool::VerticalAreaType => "VerticalAreaType",
        Tool::VerticalPathType => "VerticalPathType",
    }
}

/// A stable file key for a bindable command.
pub fn action_name(a: PrefAction) -> &'static str {
    match a {
        PrefAction::SwapPaints => "SwapPaints",
        PrefAction::DefaultPaints => "DefaultPaints",
        PrefAction::Place => "Place",
        PrefAction::CommandPalette => "CommandPalette",
        PrefAction::TrackingDecrease => "TrackingDecrease",
        PrefAction::TrackingIncrease => "TrackingIncrease",
        PrefAction::LeadingDecrease => "LeadingDecrease",
        PrefAction::LeadingIncrease => "LeadingIncrease",
        PrefAction::BaselineShiftUp => "BaselineShiftUp",
        PrefAction::BaselineShiftDown => "BaselineShiftDown",
    }
}

fn parse_hex(v: &str) -> Option<[u8; 3]> {
    let v = v.trim_start_matches('#');
    if v.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&v[0..2], 16).ok()?,
        u8::from_str_radix(&v[2..4], 16).ok()?,
        u8::from_str_radix(&v[4..6], 16).ok()?,
    ])
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn smart_guides_reject_nonfinite_preferences() {
        let s=parse("sg_tolerance = NaN\nsg_angles = NaN,45,90,135,0,0\n");
        assert_eq!(s.sg_tolerance,4.0);
        assert_eq!(s.sg_angles,Settings::default().sg_angles);
        let s=parse("sg_tolerance = inf\nsg_angles = 0,45,inf,135,0,0\n");
        assert_eq!(s.sg_tolerance,4.0);
        assert_eq!(s.sg_angles,Settings::default().sg_angles);
    }

    /// Every scalar field of `Settings` must survive `save()` → `load()`
    /// (here, the equivalent `serialize`/`parse` pair) unchanged. Both the
    /// struct literal below and the destructure of `round_tripped` list
    /// every field with no `..` fill-in, so adding a field to `Settings`
    /// without updating this test is a compile error, not a silent gap —
    /// the failure mode `08-settings-persistence-sync-medium.md` calls
    /// out (a forgotten line in `serialize`'s `format!` call means a
    /// setting silently reverts to its default every relaunch, with no
    /// compile error and no log line) now fails right here, at the field
    /// that drifted, instead of as a user bug report.
    #[test]
    fn every_settings_field_round_trips_through_save_and_load() {
        use winit::keyboard::KeyCode;

        // Shift+Cmd+Option-Q — every modifier held, a combo no default
        // binding uses, on a key `key_char`/`key_code` actually encodes
        // (the shortcut system's bindable keyspace is deliberately closed
        // to letters/digits/backslash/arrows; an unencodable key isn't a
        // real input here, so isn't a fair sentinel).
        let sentinel = KeyChord { code: KeyCode::KeyQ, shift: true, cmd: true, alt: true };
        let original = Settings {
            ui_scale: 1.5,
            nudge_step: 7.5,
            show_tooltips: false,
            home_on_last_close: false,
            accent: [0x11, 0x22, 0x33],
            tool_keys: [Some(sentinel); Tool::ALL.len()],
            action_keys: [Some(sentinel); PrefAction::ALL.len()],
            show_fps: false,
            show_cull_outline: true,
            cull_inset: 42.5,
            hide_wip_menu_items: true,
            hide_wip_tools: true,
            handle_size: crate::handle_scale::HandleSize::Large,
            smart_guides_enabled: false,
            sg_alignment_guides: false,
            sg_anchor_path_labels: false,
            sg_object_highlighting: false,
            sg_measurement_labels: false,
            sg_construction_guides: false,
            sg_transform_tools: false,
            sg_spacing_guides: false,
            sg_tolerance: 6.5,
            sg_angles: [1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            show_grid: true,
            snap_to_grid: true,
            snap_to_pixel: true,
            snap_to_point: false,
            grid_spacing: 36.0,
        };
        let round_tripped = parse(&serialize(&original));

        let Settings {
            ui_scale,
            nudge_step,
            show_tooltips,
            home_on_last_close,
            accent,
            tool_keys,
            action_keys,
            show_fps,
            show_cull_outline,
            cull_inset,
            hide_wip_menu_items,
            hide_wip_tools,
            handle_size,
            smart_guides_enabled,
            sg_alignment_guides,
            sg_anchor_path_labels,
            sg_object_highlighting,
            sg_measurement_labels,
            sg_construction_guides,
            sg_transform_tools,
            sg_spacing_guides,
            sg_tolerance,
            sg_angles,
            show_grid,
            snap_to_grid,
            snap_to_pixel,
            snap_to_point,
            grid_spacing,
        } = round_tripped;
        assert_eq!(ui_scale, original.ui_scale, "ui_scale did not round-trip");
        assert_eq!(nudge_step, original.nudge_step, "nudge_step did not round-trip");
        assert_eq!(show_tooltips, original.show_tooltips, "show_tooltips did not round-trip");
        assert_eq!(home_on_last_close, original.home_on_last_close, "home_on_last_close did not round-trip");
        assert_eq!(accent, original.accent, "accent did not round-trip");
        assert!(tool_keys == original.tool_keys, "tool_keys did not round-trip");
        assert!(action_keys == original.action_keys, "action_keys did not round-trip");
        assert_eq!(show_fps, original.show_fps, "show_fps did not round-trip");
        assert_eq!(show_cull_outline, original.show_cull_outline, "show_cull_outline did not round-trip");
        assert_eq!(cull_inset, original.cull_inset, "cull_inset did not round-trip");
        assert_eq!(hide_wip_menu_items, original.hide_wip_menu_items, "hide_wip_menu_items did not round-trip");
        assert_eq!(hide_wip_tools, original.hide_wip_tools, "hide_wip_tools did not round-trip");
        assert_eq!(handle_size, original.handle_size, "handle_size did not round-trip");
        assert_eq!(smart_guides_enabled, original.smart_guides_enabled, "smart_guides_enabled did not round-trip");
        assert_eq!(sg_alignment_guides, original.sg_alignment_guides, "sg_alignment_guides did not round-trip");
        assert_eq!(sg_anchor_path_labels, original.sg_anchor_path_labels, "sg_anchor_path_labels did not round-trip");
        assert_eq!(sg_object_highlighting, original.sg_object_highlighting, "sg_object_highlighting did not round-trip");
        assert_eq!(sg_measurement_labels, original.sg_measurement_labels, "sg_measurement_labels did not round-trip");
        assert_eq!(sg_construction_guides, original.sg_construction_guides, "sg_construction_guides did not round-trip");
        assert_eq!(sg_transform_tools, original.sg_transform_tools, "sg_transform_tools did not round-trip");
        assert_eq!(sg_spacing_guides, original.sg_spacing_guides, "sg_spacing_guides did not round-trip");
        assert_eq!(sg_tolerance, original.sg_tolerance, "sg_tolerance did not round-trip");
        assert_eq!(sg_angles, original.sg_angles, "sg_angles did not round-trip");
        assert_eq!(show_grid, original.show_grid, "show_grid did not round-trip");
        assert_eq!(snap_to_grid, original.snap_to_grid, "snap_to_grid did not round-trip");
        assert_eq!(snap_to_pixel, original.snap_to_pixel, "snap_to_pixel did not round-trip");
        assert_eq!(snap_to_point, original.snap_to_point, "snap_to_point did not round-trip");
        assert_eq!(grid_spacing, original.grid_spacing, "grid_spacing did not round-trip");
    }

    #[test]
    fn ui_scale_is_backward_compatible_and_round_trips() {
        assert_eq!(parse("show_tooltips = false").ui_scale, 1.0);
        for scale in [1.0, 1.25, 1.5] {
            let settings = Settings { ui_scale: scale, ..Settings::default() };
            let loaded = parse(&serialize(&settings));
            assert!(loaded == settings);
        }
        for value in ["NaN", "inf", "-inf", "oops", "-1"] {
            assert_eq!(parse(&format!("ui_scale = {value}")).ui_scale, 1.0);
        }
    }
}
