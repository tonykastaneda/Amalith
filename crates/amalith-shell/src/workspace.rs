//! Workspace-layout persistence.
//!
//! A snapshot of the dock arrangement (the left / right rails and their
//! widths, which panels are open, how they're split and tabbed) plus a
//! couple of view toggles, written to `layout.json` next to
//! `settings.txt` and restored on launch.
//!
//! Rails only for now — floating panel windows are not part of the
//! snapshot. Detached panels fold back into the right rail on quit, so
//! nothing is lost.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dock::{DockModel, Rail};

/// The persisted layout. `#[serde(default)]` on every field so a file
/// written by an older build (or a hand-edited one) still loads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Layout {
    #[serde(default)]
    pub left: Rail,
    #[serde(default)]
    pub right: Rail,
    /// Canvas rulers (⌘R).
    #[serde(default)]
    pub rulers: bool,
}

impl Layout {
    /// Capture the current shell layout.
    pub fn capture(dock: &DockModel, rulers: bool) -> Self {
        Self {
            left: dock.left.clone(),
            right: dock.right.clone(),
            rulers,
        }
    }

    /// Apply this snapshot to `dock`'s rails (floating groups untouched).
    pub fn apply_to(&self, dock: &mut DockModel) {
        dock.left = self.left.clone();
        dock.right = self.right.clone();
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
