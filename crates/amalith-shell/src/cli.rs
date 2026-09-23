//! The `ama` shell command — Preferences ▸ Integrations ▸ CLI.
//!
//! `ama` lists the documents Amalith has open (`crate::open_docs`) and changes
//! directory to the folder beside the one you pick. Only a shell can change
//! its own working directory, so this ships as a zsh *function* sourced from
//! the user's rc file rather than a binary on `PATH` — a child process could
//! never move the parent shell.
//!
//! Installing therefore touches `~/.zshrc`, which is why it's an explicit
//! action in Preferences and not something launch does. The rc file gets one
//! guarded `source` line, added only when it isn't already there; the function
//! itself lives in Amalith's own config directory, so updating it never edits
//! the user's rc file again.
use std::io::Write;
use std::path::{Path, PathBuf};

/// The function, compiled in like every other asset in this crate.
pub const AMA_ZSH: &str = include_str!("../assets/cli/ama.zsh");

/// Tags the line we add to the rc file, so a reader can see where it came
/// from and an install can recognise its own work.
const MARKER: &str = "# Amalith CLI (Preferences \u{25b8} Integrations)";

/// `<config dir>/ama.zsh` — where the sourced function lives.
pub fn script_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("ama.zsh"))
}

/// The rc file we install into. zsh only: `ama` is written in zsh, and it's
/// the default shell on every macOS version Amalith supports.
fn zshrc() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    Some(home.join(".zshrc"))
}

/// What Preferences shows for the CLI row.
pub struct Status {
    /// There's a `~/.zshrc` to install into. When false the row is inert —
    /// nothing to hook the function onto.
    pub present: bool,
    /// The rc file sources our script *and* the script matches this build.
    pub current: bool,
    /// Sourced, but the script came from a different build.
    pub stale: bool,
}

impl Status {
    pub fn summary(&self) -> &'static str {
        match (self.present, self.current, self.stale) {
            (false, ..) => "No ~/.zshrc found",
            (_, true, _) => "Installed",
            (_, _, true) => "Needs updating",
            _ => "Not installed",
        }
    }
}

pub fn status() -> Status {
    let Some(rc) = zshrc() else {
        return Status { present: false, current: false, stale: false };
    };
    let Some(script) = script_path() else {
        return Status { present: false, current: false, stale: false };
    };
    let present = rc.is_file();
    let sourced = is_sourced(&rc, &script);
    let installed = std::fs::read_to_string(&script).ok();
    let matches = installed.as_deref() == Some(AMA_ZSH);
    Status {
        present,
        current: sourced && matches,
        // A sourced-but-different script is what raises the corner notice.
        // Sourced with the file missing counts as stale too: reinstalling is
        // exactly the fix.
        stale: sourced && !matches,
    }
}

/// Whether `rc` already pulls in our script. Matched on the script path
/// rather than the marker, so a line the user moved or re-worded still counts
/// and we don't add a second one.
fn is_sourced(rc: &Path, script: &Path) -> bool {
    let needle = script.to_string_lossy();
    std::fs::read_to_string(rc).is_ok_and(|text| {
        text.lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .any(|line| line.contains(needle.as_ref()))
    })
}

/// Write the function and, if needed, add the one line that sources it.
pub fn install() -> Result<(), String> {
    let script = script_path().ok_or("couldn't locate Amalith's config folder")?;
    let rc = zshrc().ok_or("couldn't locate your home folder")?;

    if let Some(parent) = script.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&script, AMA_ZSH).map_err(|e| e.to_string())?;

    // Already hooked up: updating the script was the whole job, and the rc
    // file is left exactly as the user has it.
    if is_sourced(&rc, &script) {
        return Ok(());
    }

    let line = format!("\n{MARKER}\n[ -f \"{}\" ] && source \"{}\"\n", script.display(), script.display());
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&rc)
        .map_err(|e| format!("couldn't open {}: {e}", rc.display()))?;
    file.write_all(line.as_bytes()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_defines_the_ama_function_and_reads_the_published_list() {
        // Same reason as the skill's own LF check: this file is written to
        // disk and sourced by the user's shell, where stray carriage
        // returns are a syntax error rather than a cosmetic problem.
        assert!(!AMA_ZSH.contains('\r'), "ama.zsh must be checked out with LF endings");
        assert!(AMA_ZSH.contains("\nama() {"), "must define `ama`");
        assert!(
            AMA_ZSH.contains("open-documents.tsv"),
            "must read what `open_docs` publishes"
        );
        // The two halves of the contract with `open_docs::encode`.
        assert!(AMA_ZSH.contains("builtin cd"), "must cd the calling shell");
        assert!(AMA_ZSH.contains("hasn't been saved"), "must warn on unsaved documents");
    }

    /// The source check ignores comments, so the marker line itself can't be
    /// mistaken for an install.
    #[test]
    fn sourced_detection_ignores_comments() {
        let dir = tempfile::tempdir().unwrap();
        let rc = dir.path().join(".zshrc");
        let script = dir.path().join("ama.zsh");

        std::fs::write(&rc, "# some notes\n").unwrap();
        assert!(!is_sourced(&rc, &script));

        // A commented-out mention doesn't count...
        std::fs::write(&rc, format!("# source \"{}\"\n", script.display())).unwrap();
        assert!(!is_sourced(&rc, &script));

        // ...a live one does.
        std::fs::write(&rc, format!("source \"{}\"\n", script.display())).unwrap();
        assert!(is_sourced(&rc, &script));
    }
}
