//! The app's version, as shown in the UI and used by the update check.
//! Set at build time by `build.rs`: the release tag for release builds,
//! Cargo.toml's version otherwise. Never hardcode a version string elsewhere.

/// Bare version number, e.g. `0.0.4`.
pub const VERSION: &str = env!("AMALITH_VERSION");
/// Short commit hash of a CI build; empty for local builds.
pub const COMMIT: &str = env!("AMALITH_COMMIT");
/// `v0.0.4` — the new-tab screen's version line.
pub const LABEL: &str = concat!("v", env!("AMALITH_VERSION"));
/// `Amalith v0.0.4` — window title, macOS title strip, workspace chooser.
pub const TITLE: &str = concat!("Amalith v", env!("AMALITH_VERSION"));
