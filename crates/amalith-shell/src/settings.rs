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
         show_fps = {}\nshow_cull_outline = {}\ncull_inset = {}\nhandle_size = {}\n",
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
        s.handle_size.id_str(),
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
            handle_size: crate::handle_scale::HandleSize::Large,
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
            handle_size,
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
        assert_eq!(handle_size, original.handle_size, "handle_size did not round-trip");
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
