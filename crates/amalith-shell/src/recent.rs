//! Recently-opened documents, persisted across launches.
//!
//! A plain newline-delimited list of absolute paths in the platform's config
//! directory. Most-recent first, de-duplicated, capped at [`MAX`]. Paths that
//! no longer exist are dropped on load.

use std::path::{Path, PathBuf};

/// How many entries to keep.
pub const MAX: usize = 18;

/// `~/Library/Application Support/Amalith/recents.txt` (macOS),
/// `$XDG_CONFIG_HOME/amalith/recents.txt` or `~/.config/amalith/…` (Linux),
/// `%APPDATA%\Amalith\recents.txt` (Windows).
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
    Some(dir.join("recents.txt"))
}

/// The stored list, filtered to paths that still exist.
pub fn load() -> Vec<PathBuf> {
    let Some(path) = store_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .take(MAX)
        .collect()
}

/// Move `path` to the front of the list and write it back.
pub fn push(path: &Path) {
    let Some(store) = store_path() else {
        return;
    };
    let mut list = load();
    list.retain(|p| p != path);
    list.insert(0, path.to_path_buf());
    list.truncate(MAX);

    if let Some(parent) = store.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body: String = list
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(&store, body);
}
