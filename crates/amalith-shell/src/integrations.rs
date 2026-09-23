//! Preferences ▸ Integrations — the `ama` shell command (`crate::cli`) and the
//! agent skill (`crate::agent`), flattened into the one list the page draws.
//!
//! CLI comes first: it's the row that isn't per-agent, and the one that
//! changes how the user's own shell behaves.
//!
//! Nothing here installs itself. Every row writes into something that belongs
//! to another tool — a shell rc file, an agent's config folder — so the user
//! ticks what they want and presses Install. Launch only *checks* whether an
//! existing install has drifted from this build ([`needs_update`]), which is
//! what raises the corner notice.

/// Which integration a row stands for.
pub enum Kind {
    Cli,
    /// Index into [`crate::agent::statuses`].
    Agent(usize),
}

/// One row on the page.
pub struct Row {
    pub name: &'static str,
    /// Dim hint after the name — what this row actually installs.
    pub detail: &'static str,
    /// The state, in the words the page shows.
    pub summary: &'static str,
    /// False when there's nothing here to install into (no `~/.zshrc`, or an
    /// agent that isn't set up on this machine), which makes the row inert.
    pub present: bool,
    /// Installed, but from a different build.
    pub stale: bool,
    pub kind: Kind,
}

/// Every integration, CLI first. Reads the filesystem, so the UI caches the
/// result rather than calling it while painting.
pub fn rows() -> Vec<Row> {
    let cli = crate::cli::status();
    let mut rows = vec![Row {
        name: "CLI",
        detail: "ama — jump to an open document's folder",
        summary: cli.summary(),
        present: cli.present,
        stale: cli.stale,
        kind: Kind::Cli,
    }];
    rows.extend(crate::agent::statuses().into_iter().enumerate().map(|(i, s)| Row {
        name: s.name,
        detail: "Amalith skill",
        summary: s.summary(),
        present: s.present,
        stale: s.stale,
        kind: Kind::Agent(i),
    }));
    rows
}

/// Install every selected row, `selected` indexed alongside [`rows`]. Returns
/// one human-readable message per failure, empty on success.
pub fn install(selected: &[bool]) -> Vec<String> {
    let rows = rows();
    let mut errors = Vec::new();
    // The agent installer takes its own selection, so fold the agent rows
    // back into one call rather than writing each file separately.
    let mut agents = vec![false; rows.iter().filter(|r| matches!(r.kind, Kind::Agent(_))).count()];

    for (i, row) in rows.iter().enumerate() {
        if !selected.get(i).copied().unwrap_or(false) {
            continue;
        }
        match row.kind {
            Kind::Cli => {
                if let Err(e) = crate::cli::install() {
                    errors.push(format!("CLI: {e}"));
                }
            }
            Kind::Agent(a) => {
                if let Some(on) = agents.get_mut(a) {
                    *on = true;
                }
            }
        }
    }

    if agents.iter().any(|on| *on) {
        errors.extend(crate::agent::install(&agents));
    }
    errors
}

/// Whether any install has drifted from this build — the corner notice's
/// trigger. Checked once at startup.
pub fn needs_update() -> bool {
    rows().iter().any(|row| row.stale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_is_the_first_row_and_agents_follow() {
        let rows = rows();
        assert!(matches!(rows[0].kind, Kind::Cli), "CLI sits at the top");
        assert_eq!(rows[0].name, "CLI");
        for (offset, row) in rows[1..].iter().enumerate() {
            assert!(
                matches!(row.kind, Kind::Agent(i) if i == offset),
                "agent rows keep their own index, offset by the CLI row"
            );
        }
    }

    /// Selecting nothing writes nothing — in particular it must not reach the
    /// agent installer, whose empty-selection path reports a home-folder
    /// error that would surface as a spurious failure.
    #[test]
    fn installing_nothing_reports_nothing() {
        let rows = rows();
        assert!(install(&vec![false; rows.len()]).is_empty());
    }
}
