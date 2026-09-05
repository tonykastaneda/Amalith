//! Workspace-layout persistence.
//!
//! A snapshot of the dock arrangement (every *docked* Master, its groups,
//! panels, and display mode) plus a couple of view toggles, written to
//! `layout.json` next to `settings.txt` and restored on launch.
//!
//! Docked masters only, same as before this rewrite — a floating Master
//! is a real OS window with no launch-time equivalent to spawn it back
//! into, so it isn't part of the snapshot (nothing is lost: closing one
//! folds its groups back into whatever's docked right — see
//! `WindowEvent::CloseRequested` — well before the app would ever quit
//! with one still open).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dock::{DockModel, Master};

/// The persisted layout. `#[serde(default)]` on every field so a file
/// written by an older build (or a hand-edited one) still loads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Layout {
    #[serde(default)]
    pub masters: Vec<Master>,
    /// Canvas rulers (⌘R).
    #[serde(default)]
    pub rulers: bool,
    /// Guides hidden (View ▸ Hide Guides, ⌘;).
    #[serde(default)]
    pub guides_hidden: bool,
    /// Guides locked (View ▸ Lock Guides, ⌘⌥;).
    #[serde(default)]
    pub guides_locked: bool,
}

impl Layout {
    /// Capture the current shell layout.
    pub fn capture(dock: &DockModel, rulers: bool, guides_hidden: bool, guides_locked: bool) -> Self {
        Self {
            masters: dock.masters.iter().filter(|m| m.dock.is_some()).cloned().collect(),
            rulers,
            guides_hidden,
            guides_locked,
        }
    }

    /// Apply this snapshot to `dock`'s docked masters (any floating ones
    /// `dock` already had — there shouldn't be any this early — are left
    /// alone). A layout with no masters at all (an empty/corrupt file)
    /// leaves whatever `dock` already had, rather than wiping it out.
    pub fn apply_to(&self, dock: &mut DockModel) {
        if !self.masters.is_empty() {
            dock.masters.retain(|m| m.dock.is_none());
            dock.masters.extend(self.masters.iter().cloned());
        }
    }
}

fn store_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("layout.json"))
}

/// The saved layout, or `None` if there's no file / it won't parse.
pub fn load() -> Option<Layout> {
    let text = std::fs::read_to_string(store_path()?).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write `layout` to disk, creating the config directory if needed.
pub fn save(layout: &Layout) {
    let Some(path) = store_path() else {
        return;
    };
    let Ok(json) = serde_json::to_string_pretty(layout) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, json);
}
