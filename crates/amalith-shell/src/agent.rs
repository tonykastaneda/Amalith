//! Agent integration: the skill file that teaches a coding agent how to work
//! with an Amalith document, and installing it into the agents the user has.
//!
//! Three layers, each with exactly one job:
//!
//! - **Trigger** — `AMALITH_ENV=1` and friends, set on every shell the app
//!   spawns (see `app::terminal`). That is all the environment carries; you
//!   cannot push knowledge into an agent's memory through it.
//! - **Teaching** — [`SKILL_MD`], embedded in the binary and written out to
//!   one canonical path. Agents load skills from directories of their own,
//!   never by scanning the working directory, so a copy has to be installed
//!   into each one — Preferences ▸ Integrations does that. A hidden file
//!   sitting next to a `.amalith` would simply never be read.
//! - **Per-document context** — deliberately not here. It needs a live
//!   channel into the running editor, which doesn't exist yet.
//!
//! The skill describes the *installed app*, not any one document, so there is
//! exactly one copy per agent and it is rewritten whenever it drifts from the
//! running build. That matters more than it sounds: a stale skill describes a
//! CLI surface that may have moved and the agent follows it confidently, which
//! is worse than having no skill at all.
use std::path::{Path, PathBuf};

/// The skill, compiled in like every other asset in this crate. Embedding it
/// rather than shipping a file in `Contents/Resources` means it can never be
/// separated from the build it describes, and one code path covers both
/// `cargo run` and a packaged `.app`.
pub const SKILL_MD: &str = include_str!("../assets/agent/SKILL.md");

/// The directory Amalith keeps its own copy of the skill in, inside the app's
/// config dir. `AMALITH_SKILL` points at the file, so an agent with no skill
/// system of its own can just be told to read it.
fn own_skill_dir() -> Option<PathBuf> {
    Some(crate::settings::config_dir()?.join("skills/amalith"))
}

/// `<config dir>/skills/amalith/SKILL.md` — always present after
/// [`refresh`], and what `AMALITH_SKILL` is set to.
pub fn skill_path() -> Option<PathBuf> {
    Some(own_skill_dir()?.join("SKILL.md"))
}

/// A coding agent that loads skills from a directory of its own.
struct Agent {
    /// What to call it when reporting back to the user.
    name: &'static str,
    /// Its skills directory, relative to the home directory.
    skills_dir: &'static str,
}

/// The agents we know how to install into, in the order Preferences lists
/// them. An entry is only acted on when its `skills_dir` already exists —
/// that's the signal the agent is actually set up on this machine, and it
/// keeps us from creating directories inside tools the user doesn't use.
///
/// Every path here is a directory the agent itself reads skills from. Tools
/// without that convention are deliberately absent rather than listed as
/// permanently "not found": an instructions file (`AGENTS.md` and friends) is
/// a different mechanism and would need its own handling. Supporting another
/// agent is one line here.
const AGENTS: &[Agent] = &[
    Agent { name: "Claude Code", skills_dir: ".claude/skills" },
    Agent { name: "Codex", skills_dir: ".codex/skills" },
    Agent { name: "Cursor", skills_dir: ".cursor/skills" },
    Agent { name: "Gemini CLI", skills_dir: ".gemini/skills" },
];

/// One agent's row on Preferences ▸ Integrations.
pub struct Status {
    /// What to call it in the UI.
    pub name: &'static str,
    /// Where an installed copy lives, or would.
    pub dir: PathBuf,
    /// The agent is set up on this machine — its skills directory exists.
    /// When false there's nothing to install into and the row is inert.
    pub present: bool,
    /// A copy of *this* build's skill is installed.
    pub current: bool,
    /// A copy is installed, but it came from a different build. Only
    /// reachable if the app was downgraded or the file was hand-edited:
    /// [`refresh`] repairs this at launch.
    pub stale: bool,
}

impl Status {
    /// The state, in the words the Integrations page shows.
    pub fn summary(&self) -> &'static str {
        match (self.present, self.current, self.stale) {
            (false, ..) => "Not found on this computer",
            (_, true, _) => "Installed",
            (_, _, true) => "Needs updating",
            _ => "Not installed",
        }
    }

    /// What the row's button should say, or `None` when there's nothing to do.
    pub fn action(&self) -> Option<&'static str> {
        match (self.present, self.current, self.stale) {
            (false, ..) => None,
            (_, true, _) => Some("Reinstall"),
            (_, _, true) => Some("Update"),
            _ => Some("Install"),
        }
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from).or_else(|| {
        // Windows has no HOME; USERPROFILE is the equivalent.
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    })
}

/// Writes [`SKILL_MD`] to `path` unless an identical copy is already there,
/// creating parent directories as needed. Returns whether it wrote.
fn write_if_changed(path: &Path) -> std::io::Result<bool> {
    if std::fs::read_to_string(path).is_ok_and(|current| current == SKILL_MD) {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, SKILL_MD)?;
    Ok(true)
}

/// One [`Status`] per known agent, in [`AGENTS`] order — which is the order
/// the Integrations page lists them and the index `selected` in [`install`]
/// refers to.
///
/// This reads the filesystem, so the UI caches the result rather than calling
/// it while painting.
pub fn statuses() -> Vec<Status> {
    let Some(home) = home() else { return Vec::new() };
    AGENTS.iter().map(|agent| status_in(&home, agent)).collect()
}

fn status_in(home: &Path, agent: &'static Agent) -> Status {
    let skills_dir = home.join(agent.skills_dir);
    let dir = skills_dir.join("amalith");
    let installed = std::fs::read_to_string(dir.join("SKILL.md")).ok();
    Status {
        name: agent.name,
        present: skills_dir.is_dir(),
        current: installed.as_deref() == Some(SKILL_MD),
        stale: installed.is_some_and(|existing| existing != SKILL_MD),
        dir,
    }
}

/// Preferences ▸ Integrations ▸ Install. Writes the skill for every agent
/// whose row is selected, and returns one human-readable message per failure
/// (empty on success) so the page can show what went wrong.
///
/// Only the `amalith/` subdirectory inside each agent's skills folder is ever
/// touched, so nothing the user (or another tool) put there is at risk.
pub fn install(selected: &[bool]) -> Vec<String> {
    let rows = statuses();
    if rows.is_empty() {
        return vec!["Couldn't locate your home folder.".to_string()];
    }
    let mut errors = Vec::new();
    // `statuses` is the single place agent paths get built, so an install
    // can't drift from the row the user actually ticked.
    for (index, row) in rows.into_iter().enumerate() {
        if !selected.get(index).copied().unwrap_or(false) {
            continue;
        }
        if let Err(e) = write_if_changed(&row.dir.join("SKILL.md")) {
            errors.push(format!("{}: {e}", row.name));
        }
    }
    errors
}

/// Whether any installed copy has drifted from this build's skill — which is
/// what raises the bottom-right toast (see `crate::skill_notice`).
///
/// Amalith does *not* quietly rewrite those copies: they live inside another
/// tool's configuration, so the user gets told and clicks Install. That also
/// means the toast has something to report in the first place.
pub fn needs_update() -> bool {
    statuses().iter().any(|s| s.stale)
}

/// Called once at app launch: writes Amalith's own copy of the skill, which
/// is what `AMALITH_SKILL` points at. Agent copies are left alone — see
/// [`needs_update`].
pub fn refresh() {
    if let Some(path) = skill_path() {
        let _ = write_if_changed(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The skill has to parse as a skill: agents key off the frontmatter's
    /// `name`/`description`, and the `AMALITH_ENV` gate is what keeps it from
    /// firing in unrelated sessions.
    #[test]
    fn skill_has_frontmatter_and_env_gate() {
        assert!(SKILL_MD.starts_with("---\n"), "skill needs YAML frontmatter");
        let (frontmatter, body) = SKILL_MD[4..]
            .split_once("\n---\n")
            .expect("frontmatter terminator");
        assert!(frontmatter.contains("name: amalith"), "skill needs a name");
        assert!(frontmatter.contains("AMALITH_ENV=1"), "description must state the trigger");
        assert!(body.contains("AMALITH_ENV"), "body must tell the agent to check the gate");
    }

    #[test]
    fn write_if_changed_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/SKILL.md");

        assert!(write_if_changed(&path).unwrap(), "first write creates the file");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SKILL_MD);
        assert!(!write_if_changed(&path).unwrap(), "identical content is left alone");

        std::fs::write(&path, "stale from an older build").unwrap();
        assert!(write_if_changed(&path).unwrap(), "drifted content is rewritten");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SKILL_MD);
    }

    /// Every listed agent points at a `skills` directory it actually reads,
    /// under a dot-directory in the home folder.
    #[test]
    fn agent_table_is_well_formed() {
        for agent in AGENTS {
            assert!(agent.skills_dir.starts_with('.'), "{} needs a dotted path", agent.name);
            assert!(agent.skills_dir.ends_with("skills"), "{} must be a skills dir", agent.name);
            assert!(!agent.name.is_empty());
        }
    }

    /// What the Integrations page reads to label each row.
    #[test]
    fn status_tracks_presence_and_drift() {
        let home = tempfile::tempdir().unwrap();
        let agent = &AGENTS[0];

        // No skills directory: the agent isn't set up, so the row is inert.
        let s = status_in(home.path(), agent);
        assert!(!s.present);
        assert_eq!(s.action(), None);
        assert_eq!(s.summary(), "Not found on this computer");

        // Present but nothing installed yet.
        std::fs::create_dir_all(home.path().join(agent.skills_dir)).unwrap();
        let s = status_in(home.path(), agent);
        assert!(s.present && !s.current && !s.stale);
        assert_eq!(s.action(), Some("Install"));
        assert_eq!(s.summary(), "Not installed");

        // Installed and matching this build.
        write_if_changed(&s.dir.join("SKILL.md")).unwrap();
        let s = status_in(home.path(), agent);
        assert!(s.current && !s.stale);
        assert_eq!(s.action(), Some("Reinstall"));
        assert_eq!(s.summary(), "Installed");

        // Installed, but from some other build.
        std::fs::write(s.dir.join("SKILL.md"), "older build").unwrap();
        let s = status_in(home.path(), agent);
        assert!(s.stale && !s.current);
        assert_eq!(s.action(), Some("Update"));
        assert_eq!(s.summary(), "Needs updating");
    }
}
