//! Named workspace layouts (Windows ▸ Workspace).
//!
//! A workspace is a named [`Layout`] snapshot — same shape as the
//! transient last-session state in `workspace.rs`, just kept under a name
//! the user picked instead of silently overwritten on every launch.
//! "Essentials Classic" is the one built-in: it isn't stored on disk, its
//! layout is baked into [`essentials_classic`] so it can't be corrupted or
//! deleted. Everything else the user saves (Window ▸ Workspace ▸ New
//! Workspace…) persists to `workspaces.json`, beside `layout.json`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::dock::{DockModel, PanelId, Side};
use crate::workspace::Layout;

pub const ESSENTIALS_CLASSIC: &str = "Essentials Classic";

/// The one built-in workspace's fixed layout — the same arrangement a
/// brand-new install seeds itself with (see `App::new`).
pub fn essentials_classic() -> Layout {
    let mut dock = DockModel::new();
    let tools = dock.spawn_tools_master([40.0, 40.0, 80.0, 400.0]);
    dock.dock_master(tools, Side::Left, 0);
    let right = dock.spawn_master(
        vec![
            vec![PanelId("pathfinder"), PanelId("transform"), PanelId("align"), PanelId("color")],
            vec![PanelId("character"), PanelId("paragraph")],
            vec![PanelId("layers"), PanelId("artboards")],
        ],
        [0.0, 40.0, 320.0, 600.0],
    );
    dock.dock_master(right, Side::Right, 0);
    Layout::capture(&dock, false, false, false, None)
}

/// One user-saved workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    pub layout: Layout,
}

/// Every saved workspace, plus which one is currently active (for the
/// Workspace submenu's checkmark and for "Reset <name>").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Store {
    pub active: String,
    #[serde(default)]
    pub custom: Vec<Named>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            active: ESSENTIALS_CLASSIC.to_string(),
            custom: Vec::new(),
        }
    }
}

impl Store {
    /// Names for the Workspace submenu: the built-in first, then user
    /// ones in save order.
    pub fn names(&self) -> Vec<String> {
        let mut v = vec![ESSENTIALS_CLASSIC.to_string()];
        v.extend(self.custom.iter().map(|w| w.name.clone()));
        v
    }

    /// The saved layout for `name` (the built-in's fixed layout if it
    /// doesn't match any custom entry, so a stale/deleted active name
    /// still resolves to something sane).
    pub fn layout_of(&self, name: &str) -> Layout {
        self.custom
            .iter()
            .find(|w| w.name == name)
            .map(|w| w.layout.clone())
            .unwrap_or_else(essentials_classic)
    }

    /// Save (or overwrite, by name) a user workspace and make it active.
    /// A blank name, or the reserved built-in name, is ignored — there's
    /// nothing sensible to save it as.
    pub fn upsert(&mut self, name: String, layout: Layout) {
        let name = name.trim().to_string();
        if name.is_empty() || name == ESSENTIALS_CLASSIC {
            return;
        }
        self.custom.retain(|w| w.name != name);
        self.custom.push(Named { name: name.clone(), layout });
        self.active = name;
    }

    /// Delete a user workspace. Falls back to the built-in if it was the
    /// active one. No-op for the built-in name itself (nothing to
    /// delete).
    pub fn remove(&mut self, name: &str) {
        self.custom.retain(|w| w.name != name);
        if self.active == name {
            self.active = ESSENTIALS_CLASSIC.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_store_has_only_the_built_in_active() {
        let store = Store::default();
        assert_eq!(store.names(), vec![ESSENTIALS_CLASSIC.to_string()]);
        assert_eq!(store.active, ESSENTIALS_CLASSIC);
    }

    #[test]
    fn upsert_adds_it_lists_it_and_makes_it_active() {
        let mut store = Store::default();
        let layout = essentials_classic();
        store.upsert("Painting".to_string(), layout.clone());
        assert_eq!(store.names(), vec![ESSENTIALS_CLASSIC.to_string(), "Painting".to_string()]);
        assert_eq!(store.active, "Painting");
        assert_eq!(store.layout_of("Painting").masters.len(), layout.masters.len());
    }

    #[test]
    fn upsert_by_the_same_name_replaces_rather_than_duplicates() {
        let mut store = Store::default();
        store.upsert("Painting".to_string(), essentials_classic());
        store.upsert("Painting".to_string(), Layout::default());
        assert_eq!(store.custom.len(), 1);
        assert!(store.layout_of("Painting").masters.is_empty());
    }

    #[test]
    fn upsert_ignores_a_blank_or_reserved_name() {
        let mut store = Store::default();
        store.upsert("  ".to_string(), essentials_classic());
        store.upsert(ESSENTIALS_CLASSIC.to_string(), essentials_classic());
        assert!(store.custom.is_empty());
        assert_eq!(store.active, ESSENTIALS_CLASSIC);
    }

    #[test]
    fn removing_the_active_one_falls_back_to_the_built_in() {
        let mut store = Store::default();
        store.upsert("Painting".to_string(), essentials_classic());
        store.remove("Painting");
        assert!(store.custom.is_empty());
        assert_eq!(store.active, ESSENTIALS_CLASSIC);
    }

    #[test]
    fn a_stale_or_deleted_name_falls_back_to_the_built_in_layout() {
        let store = Store::default();
        assert_eq!(store.layout_of("Nonexistent").masters.len(), essentials_classic().masters.len());
    }
}

fn store_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("workspaces.json"))
}

/// The saved workspace store (built-in-only default if there's no file /
/// it won't parse).
pub fn load() -> Store {
    let Some(path) = store_path() else {
        return Store::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Store::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write `store` to disk, creating the config directory if needed.
pub fn save(store: &Store) {
    let Some(path) = store_path() else {
        return;
    };
    let Ok(json) = serde_json::to_string_pretty(store) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, json);
}
