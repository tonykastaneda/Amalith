//! Keyboard-shortcut presets (Preferences ▸ Keyboard ▸ Preset). A preset
//! is a named snapshot of every tool + command shortcut. The built-in
//! "Illustrator-like" set is [`Settings::default`]'s; user presets persist
//! to `keymaps.txt` beside `settings.txt`:
//!
//! ```text
//! active = Vim-ish
//! preset = Vim-ish
//! tool.Select = V
//! action.SwapPaints = X
//! preset = Compact
//! tool.Select = S
//! ```

use std::str::FromStr;

use crate::prefs::{KeyChord, PrefAction, Settings};
use crate::settings::{action_name, tool_name};
use crate::tool::Tool;

/// The always-present base preset.
pub const BUILTIN: &str = "Illustrator-like";

type ToolKeys = [Option<KeyChord>; Tool::ALL.len()];
type ActionKeys = [Option<KeyChord>; PrefAction::ALL.len()];

/// A named shortcut set.
#[derive(Clone, PartialEq)]
pub struct Preset {
    pub name: String,
    pub tool_keys: ToolKeys,
    pub action_keys: ActionKeys,
}

/// Every preset plus which one is selected.
#[derive(Clone)]
pub struct Keymaps {
    /// Name of the selected preset (may be [`BUILTIN`]).
    pub active: String,
    /// User-defined presets, in creation order.
    pub custom: Vec<Preset>,
}

impl Default for Keymaps {
    fn default() -> Self {
        Self {
            active: BUILTIN.to_string(),
            custom: Vec::new(),
        }
    }
}

impl Keymaps {
    /// Preset names for the dropdown: the built-in first, then user ones.
    pub fn names(&self) -> Vec<String> {
        let mut v = vec![BUILTIN.to_string()];
        v.extend(self.custom.iter().map(|p| p.name.clone()));
        v
    }

    /// The shortcut arrays for the preset called `name` (built-in →
    /// factory defaults).
    pub fn keys_of(&self, name: &str) -> (ToolKeys, ActionKeys) {
        if name != BUILTIN {
            if let Some(p) = self.custom.iter().find(|p| p.name == name) {
                return (p.tool_keys, p.action_keys);
            }
        }
        let d = Settings::default();
        (d.tool_keys, d.action_keys)
    }

    /// Add (or replace, by name) a user preset and make it active.
    pub fn upsert(&mut self, name: String, tool_keys: ToolKeys, action_keys: ActionKeys) {
        self.custom.retain(|p| p.name != name);
        self.custom.push(Preset {
            name: name.clone(),
            tool_keys,
            action_keys,
        });
        self.active = name;
    }
}

fn store_path() -> Option<std::path::PathBuf> {
    Some(crate::settings::config_dir()?.join("keymaps.txt"))
}

/// Load the presets (empty / built-in only if the file is missing).
pub fn load() -> Keymaps {
    let mut km = Keymaps::default();
    let Some(path) = store_path() else {
        return km;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return km;
    };
    let default = Settings::default();
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "active" => km.active = v.to_string(),
            "preset" => km.custom.push(Preset {
                name: v.to_string(),
                tool_keys: default.tool_keys,
                action_keys: default.action_keys,
            }),
            _ => {
                let Some(p) = km.custom.last_mut() else { continue };
                if let Some(name) = k.strip_prefix("tool.") {
                    if let Some(i) = Tool::ALL.iter().position(|t| tool_name(*t) == name) {
                        p.tool_keys[i] = KeyChord::from_str(v).ok();
                    }
                } else if let Some(name) = k.strip_prefix("action.") {
                    if let Some(i) = PrefAction::ALL.iter().position(|a| action_name(*a) == name) {
                        p.action_keys[i] = KeyChord::from_str(v).ok();
                    }
                }
            }
        }
    }
    // A stale `active` pointing at a deleted preset falls back to built-in.
    if km.active != BUILTIN && !km.custom.iter().any(|p| p.name == km.active) {
        km.active = BUILTIN.to_string();
    }
    km
}

/// Rewrite `keymaps.txt`.
pub fn save(km: &Keymaps) {
    let Some(path) = store_path() else {
        return;
    };
    let mut body = format!("active = {}\n", km.active);
    for p in &km.custom {
        body.push_str(&format!("preset = {}\n", p.name));
        for (i, tool) in Tool::ALL.iter().enumerate() {
            if let Some(c) = p.tool_keys[i] {
                body.push_str(&format!("tool.{} = {c}\n", tool_name(*tool)));
            }
        }
        for (i, act) in PrefAction::ALL.iter().enumerate() {
            if let Some(c) = p.action_keys[i] {
                body.push_str(&format!("action.{} = {c}\n", action_name(*act)));
            }
        }
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, body);
}
