//! Physical units and conversion to/from the document's canonical unit.
//!
//! All geometry stored on a [`crate::Document`] (artboard rects, object
//! transforms, path coordinates) is in **canonical pixels**: CSS/SVG-style
//! px at 96 px/inch. This matches the units designers already think in for
//! screen work ("1920 x 1080 px") and matches SVG's user-unit convention,
//! so interchange with SVG needs no rescale by default.
//!
//! [`Unit`] and [`Length`] exist for *display and input* conversion only —
//! e.g. a document set to "mm" in its [`crate::Settings`] still stores
//! geometry in px internally; the UI converts at the edges. This mirrors
//! Inkscape's `SPDocument` display-unit vs. internal user-unit split, and
//! avoids ever having to know "what unit is this number in" when reading
//! geometry out of the document model.
use serde::{Deserialize, Serialize};

/// A physical or digital unit of length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unit {
    /// CSS/SVG pixel: 1/96 inch. The document's canonical internal unit.
    Px,
    /// PostScript/Illustrator point: 1/72 inch.
    Pt,
    /// Inch.
    In,
    /// Millimeter.
    Mm,
    /// Centimeter.
    Cm,
}

impl Unit {
    /// How many of this unit fit in one inch.
    pub const fn per_inch(self) -> f64 {
        match self {
            Unit::Px => 96.0,
            Unit::Pt => 72.0,
            Unit::In => 1.0,
            Unit::Mm => 25.4,
            Unit::Cm => 2.54,
        }
    }

    /// Converts a value in this unit to canonical px.
    pub fn to_px(self, value: f64) -> f64 {
        value * (Unit::Px.per_inch() / self.per_inch())
    }

    /// Converts a value in canonical px to this unit.
    pub fn from_px(self, px: f64) -> f64 {
        px * (self.per_inch() / Unit::Px.per_inch())
    }
}

/// A length paired with the unit it was expressed in, for display/input.
///
/// Internally this always round-trips through canonical px; construct one
/// with [`Length::new`] and read it back with [`Length::px`] or
/// [`Length::in_unit`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Length {
    px: f64,
}

impl Length {
    /// Builds a length from a value expressed in `unit`.
    pub fn new(value: f64, unit: Unit) -> Self {
        Self {
            px: unit.to_px(value),
        }
    }

    /// Builds a length directly from canonical px.
    pub fn from_px(px: f64) -> Self {
        Self { px }
    }

    /// Returns the value in canonical px.
    pub fn px(self) -> f64 {
        self.px
    }

    /// Returns the value converted into `unit`.
    pub fn in_unit(self, unit: Unit) -> f64 {
        unit.from_px(self.px)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} != {b}");
    }

    #[test]
    fn inch_to_px_is_96() {
        approx_eq(Unit::In.to_px(1.0), 96.0);
    }

    #[test]
    fn pt_to_px_matches_illustrator_convention() {
        // 72pt == 1in == 96px
        approx_eq(Unit::Pt.to_px(72.0), 96.0);
    }

    #[test]
    fn mm_and_cm_are_consistent() {
        approx_eq(Unit::Mm.to_px(10.0), Unit::Cm.to_px(1.0));
    }

    #[test]
    fn roundtrip_through_every_unit_is_identity() {
        let original_px = 1234.5;
        for unit in [Unit::Px, Unit::Pt, Unit::In, Unit::Mm, Unit::Cm] {
            let length = Length::from_px(original_px);
            let in_unit = length.in_unit(unit);
            let back = Length::new(in_unit, unit);
            approx_eq(back.px(), original_px);
        }
    }

    #[test]
    fn px_is_identity() {
        approx_eq(Unit::Px.to_px(42.0), 42.0);
        approx_eq(Unit::Px.from_px(42.0), 42.0);
    }
}
