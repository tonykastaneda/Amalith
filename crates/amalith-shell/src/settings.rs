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

/// `~/Library/Application Support/Amalith/settings.txt` (macOS),
/// `%APPDATA%\Amalith\settings.txt` (Windows),
/// `$XDG_CONFIG_HOME/amalith/settings.txt` or `~/.config/…` (Linux).
fn store_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    let dir = {
        let home = std::env::var_os("HOME")?;
        PathBuf::from(home).join("Library/Application Support/Amalith")
    };
    #[cfg(target_os = "windows")]
    let dir = {
        let base = std::env::var_os("APPDATA")?;
        PathBuf::from(base).join("Amalith")
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let dir = {
        if let Some(x) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(x).join("amalith")
        } else {
            let home = std::env::var_os("HOME")?;
            PathBuf::from(home).join(".config/amalith")
        }
    };
    Some(dir.join("settings.txt"))
}

/// Load settings, starting from [`Settings::default`] and overriding with
/// whatever the file recognises.
pub fn load() -> Settings {
    let mut s = Settings::default();
    let Some(path) = store_path() else {
        return s;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return s;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "nudge_step" => {
                if let Ok(n) = v.parse::<f64>() {
                    s.nudge_step = n.clamp(0.5, 100.0);
                }
            }
            "show_tooltips" => s.show_tooltips = v == "true",
            "home_on_last_close" => s.home_on_last_close = v == "true",
            "accent" => {
                if let Some(rgb) = parse_hex(v) {
                    s.accent = rgb;
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
    let mut body = format!(
        "nudge_step = {}\nshow_tooltips = {}\nhome_on_last_close = {}\naccent = {:02x}{:02x}{:02x}\n",
        s.nudge_step, s.show_tooltips, s.home_on_last_close, s.accent[0], s.accent[1], s.accent[2],
    );
    for (i, tool) in Tool::ALL.iter().enumerate() {
        let v = s.tool_keys[i].map_or_else(String::new, |c| c.to_string());
        body.push_str(&format!("tool.{} = {}\n", tool_name(*tool), v));
    }
    for (i, act) in PrefAction::ALL.iter().enumerate() {
        let v = s.action_keys[i].map_or_else(String::new, |c| c.to_string());
        body.push_str(&format!("action.{} = {}\n", action_name(*act), v));
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, body);
}

/// A stable file key for a tool (independent of its display label).
fn tool_name(tool: Tool) -> &'static str {
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
    }
}

/// A stable file key for a bindable command.
fn action_name(a: PrefAction) -> &'static str {
    match a {
        PrefAction::SwapPaints => "SwapPaints",
        PrefAction::DefaultPaints => "DefaultPaints",
        PrefAction::Place => "Place",
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
