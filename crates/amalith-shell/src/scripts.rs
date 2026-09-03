//! User scripts: a folder the end user points at (File ▸ Scripts ▸ Add
//! Scripts Folder…, or Preferences ▸ Scripts), plus an optional key
//! binding per script. Nothing is bundled in the app — the folder lives
//! wherever the user keeps it, so an app update can't wipe it.
//!
//! Config persists to `scripts.txt` in the same config dir as
//! `settings.txt`:
//!
//! ```text
//! dir = /Users/me/AmalithScripts
//! key.cleanup.sh = Cmd+Shift+K
//! ```

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::prefs::KeyChord;

/// Extensions we treat as runnable scripts.
const EXTS: &[&str] = &[
    "sh", "command", "bash", "zsh", "py", "js", "mjs", "rb", "pl", "lua", "ps1", "bat", "cmd",
];

/// The user's scripts folder and any per-script key bindings.
#[derive(Clone, Default, PartialEq)]
pub struct ScriptsConfig {
    /// The folder, if one has been chosen.
    pub dir: Option<PathBuf>,
    /// `(file name, chord)` for each script that has a binding. Keyed by
    /// file name so it survives the folder being moved.
    pub keys: Vec<(String, KeyChord)>,
}

impl ScriptsConfig {
    /// The binding for `name`, if any.
    pub fn chord_for(&self, name: &str) -> Option<KeyChord> {
        self.keys.iter().find(|(n, _)| n == name).map(|(_, c)| *c)
    }

    /// Set (or, with `None`, clear) the binding for `name`.
    pub fn set_chord(&mut self, name: &str, chord: Option<KeyChord>) {
        self.keys.retain(|(n, _)| n != name);
        if let Some(c) = chord {
            self.keys.push((name.to_string(), c));
        }
    }

    /// Drop the binding held by `chord`, wherever it sits.
    pub fn clear_chord(&mut self, chord: KeyChord) {
        self.keys.retain(|(_, c)| *c != chord);
    }

    /// The script path bound to `chord`, if the folder is set and the file
    /// still exists.
    pub fn script_for_chord(&self, chord: KeyChord) -> Option<PathBuf> {
        let dir = self.dir.as_ref()?;
        let (name, _) = self.keys.iter().find(|(_, c)| *c == chord)?;
        let p = dir.join(name);
        p.is_file().then_some(p)
    }
}

/// The runnable scripts in `dir`, sorted by file name.
pub fn list(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| EXTS.contains(&e.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

/// The display label for a script menu / prefs row (its file name).
pub fn label(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("script")
        .to_string()
}

/// Run `path` detached, with its folder as the working directory. The
/// interpreter is picked from the extension; anything unrecognised is
/// executed directly (shebang / +x).
pub fn run(path: &Path) {
    use std::process::Command;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let mut cmd = match ext.as_str() {
        "py" => {
            let mut c = Command::new("python3");
            c.arg(path);
            c
        }
        "js" | "mjs" => {
            let mut c = Command::new("node");
            c.arg(path);
            c
        }
        "rb" => {
            let mut c = Command::new("ruby");
            c.arg(path);
            c
        }
        "pl" => {
            let mut c = Command::new("perl");
            c.arg(path);
            c
        }
        "lua" => {
            let mut c = Command::new("lua");
            c.arg(path);
            c
        }
        #[cfg(target_os = "windows")]
        "ps1" => {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"]);
            c.arg(path);
            c
        }
        #[cfg(target_os = "windows")]
        "bat" | "cmd" => {
            let mut c = Command::new("cmd");
            c.arg("/C");
            c.arg(path);
            c
        }
        #[cfg(not(target_os = "windows"))]
        "sh" | "command" | "bash" => {
            let mut c = Command::new("bash");
            c.arg(path);
            c
        }
        #[cfg(not(target_os = "windows"))]
        "zsh" => {
            let mut c = Command::new("zsh");
            c.arg(path);
            c
        }
        _ => Command::new(path),
    };
    if let Some(dir) = path.parent() {
        cmd.current_dir(dir);
    }
    let _ = cmd.spawn();
}

/// Reveal `path` in the OS file manager.
pub fn reveal(path: &Path) {
    use std::process::Command;
    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "windows")]
    let _ = Command::new("explorer").arg("/select,").arg(path).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open")
        .arg(path.parent().unwrap_or(path))
        .spawn();
}

fn store_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("scripts.txt"))
}

/// Load the scripts config (empty if the file is missing / unreadable).
pub fn load() -> ScriptsConfig {
    let mut cfg = ScriptsConfig::default();
    let Some(path) = store_path() else {
        return cfg;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return cfg;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if k == "dir" {
            if !v.is_empty() {
                cfg.dir = Some(PathBuf::from(v));
            }
        } else if let Some(name) = k.strip_prefix("key.") {
            if let Ok(chord) = KeyChord::from_str(v) {
                cfg.set_chord(name, Some(chord));
            }
        }
    }
    cfg
}

/// Rewrite `scripts.txt` from `cfg`.
pub fn save(cfg: &ScriptsConfig) {
    let Some(path) = store_path() else {
        return;
    };
    let mut body = String::new();
    if let Some(dir) = &cfg.dir {
        body.push_str(&format!("dir = {}\n", dir.display()));
    }
    for (name, chord) in &cfg.keys {
        body.push_str(&format!("key.{name} = {chord}\n"));
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, body);
}
