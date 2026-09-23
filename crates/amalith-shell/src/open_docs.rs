//! The documents Amalith currently has open, published to a small file so the
//! `ama` shell command can list them (see `crate::cli`).
//!
//! Same stopgap as the terminal's environment variables: there's no live
//! channel into the running app, so the app publishes what it has and an
//! outside process reads it. The file is rewritten whenever the list changes
//! and deleted on quit, so `ama` can't offer documents from a dead app.
use std::path::PathBuf;

/// `<config dir>/open-documents.tsv` — the running app's process id on the
/// first line, then one document per line, tab-separated: dirty flag
/// (`1`/`0`), title, saved path. `ama.zsh` builds the same path from `$HOME`
/// and parses the same shape, so the two must stay in step.
pub fn store_path() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("open-documents.tsv"))
}

/// One open document, as `ama` sees it.
pub struct Entry {
    pub title: String,
    /// `None` for a document that has never been saved. `ama` lists it but
    /// has no folder to change into, and says so rather than guessing.
    pub path: Option<PathBuf>,
    pub dirty: bool,
}

/// The file's body. A title is user-supplied text, so the tabs and newlines
/// that would break the row format are flattened to spaces.
///
/// `pid` leads the file so `ama` can tell a live list from one left behind by
/// a crash: [`clear`] only runs on a clean quit, and a stale list would have
/// it offering documents from a session that no longer exists.
pub fn encode(pid: u32, entries: &[Entry]) -> String {
    let mut out = format!("{pid}\n");
    for entry in entries {
        let title = entry.title.replace(['\t', '\n', '\r'], " ");
        let title = title.trim();
        let path = entry.path.as_ref().map(|p| p.to_string_lossy()).unwrap_or_default();
        let dirty = u8::from(entry.dirty);
        out.push_str(&format!("{dirty}\t{title}\t{path}\n"));
    }
    out
}

/// Write `body` (from [`encode`]) over whatever is there. Best-effort: a
/// failure here only costs `ama` an up-to-date list.
pub fn write(body: &str) {
    let Some(path) = store_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, body);
}

/// Remove the file on quit, so `ama` reports "is Amalith running?" instead of
/// offering documents from a session that's over.
pub fn clear() {
    if let Some(path) = store_path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_the_pid_then_one_row_per_document() {
        let body = encode(
            4242,
            &[
                Entry {
                    title: "Logo".into(),
                    path: Some(PathBuf::from("/work/logo.amalith")),
                    dirty: false,
                },
                Entry { title: "Poster".into(), path: None, dirty: true },
            ],
        );
        assert_eq!(body, "4242\n0\tLogo\t/work/logo.amalith\n1\tPoster\t\n");
    }

    /// A tab in a title would be read as a field break by `ama.zsh`, which
    /// would silently shift the path column.
    #[test]
    fn flattens_separators_in_titles() {
        let body = encode(
            1,
            &[Entry { title: "Two\tParts\nAnd more".into(), path: None, dirty: false }],
        );
        assert_eq!(body, "1\n0\tTwo Parts And more\t\n");
        assert_eq!(body.lines().count(), 2, "the pid line plus one row");
        assert_eq!(body.matches('\t').count(), 2, "exactly three fields");
    }

    /// With no documents the file still carries the pid, so `ama` can tell
    /// "running, nothing open" from "not running".
    #[test]
    fn empty_list_still_reports_the_pid() {
        assert_eq!(encode(7, &[]), "7\n");
    }
}
