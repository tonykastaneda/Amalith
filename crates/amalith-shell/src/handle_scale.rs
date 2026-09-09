//! Selection & anchor handle size — Illustrator's own dedicated "Selection
//! & Anchor Display" preference (Edit ▸ Preferences there), independent of
//! `Settings.ui_scale` / [`crate::metrics`]. A user who wants bigger grab
//! targets for a stylus or accessibility reason shouldn't be forced to
//! also scale every panel and button — see
//! `Sys-Refactor/DONE-10-selection-anchor-size-preference-medium.md`.
//!
//! Read by [`crate::canvas`] (drawn handle size) and [`crate::handles`]
//! (grab radius, rotation halo band) — pure document-canvas concerns that
//! `crate::metrics` itself deliberately excludes.

use std::cell::Cell;

/// Illustrator's own slider is continuous (Default to Max); three presets
/// cover the same range with far less UI to build and keeps every drawn
/// size a round number.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum HandleSize {
    Small,
    #[default]
    Default,
    Large,
}

impl HandleSize {
    pub const ALL: [HandleSize; 3] = [HandleSize::Small, HandleSize::Default, HandleSize::Large];

    pub fn multiplier(self) -> f64 {
        match self {
            HandleSize::Small => 0.75,
            HandleSize::Default => 1.0,
            HandleSize::Large => 1.35,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            HandleSize::Small => "Small",
            HandleSize::Default => "Default",
            HandleSize::Large => "Large",
        }
    }

    /// The stable on-disk key `settings.rs` reads/writes — independent of
    /// `label()` so a future label wording change can't silently break an
    /// existing `settings.txt`.
    pub fn id_str(self) -> &'static str {
        match self {
            HandleSize::Small => "small",
            HandleSize::Default => "default",
            HandleSize::Large => "large",
        }
    }

    pub fn from_id_str(s: &str) -> Option<Self> {
        Some(match s {
            "small" => HandleSize::Small,
            "default" => HandleSize::Default,
            "large" => HandleSize::Large,
            _ => return None,
        })
    }
}

thread_local! {
    static CURRENT: Cell<HandleSize> = Cell::new(HandleSize::Default);
}

/// Every UI-thread read (`multiplier()`) observes the same applied
/// preference, mirroring `crate::metrics::apply`.
pub fn apply(size: HandleSize) {
    CURRENT.with(|c| c.set(size));
}

/// The live multiplier for handle squares, grab radii and the rotation
/// halo band.
pub fn multiplier() -> f64 {
    CURRENT.with(|c| c.get().multiplier())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_str_round_trips_every_variant() {
        for size in HandleSize::ALL {
            assert_eq!(HandleSize::from_id_str(size.id_str()), Some(size));
        }
        assert_eq!(HandleSize::from_id_str("bogus"), None);
    }

    #[test]
    fn default_multiplier_is_neutral() {
        assert_eq!(HandleSize::default().multiplier(), 1.0);
    }
}
