//! Document-level metadata and settings.
use crate::units::Unit;
use serde::{Deserialize, Serialize};

/// Descriptive, non-geometric information about a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub title: Option<String>,
    pub author: Option<String>,
    /// Free-form string identifying the app/version that created the file,
    /// e.g. `"amalith/0.1.0"`. Not parsed; purely diagnostic.
    pub created_with: Option<String>,
}

/// Document-wide preferences that affect display/input, not geometry.
///
/// Geometry is always stored in canonical px (see `units.rs`); changing
/// `default_unit` only changes what unit rulers/dialogs display by default.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub default_unit: Unit,
    pub color_mode: ColorMode,
    pub bleed: Bleed,
    pub raster_effects: RasterEffects,
    pub preview_mode: PreviewMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorMode {
    #[default]
    Cmyk,
    Rgb,
}

/// Document bleed in canonical px, independent of the display unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Bleed {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RasterEffects {
    Screen72,
    Medium150,
    #[default]
    High300,
}

impl RasterEffects {
    pub const fn ppi(self) -> u16 {
        match self {
            Self::Screen72 => 72,
            Self::Medium150 => 150,
            Self::High300 => 300,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PreviewMode {
    #[default]
    Default,
    Pixel,
    Overprint,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_unit: Unit::Px,
            color_mode: ColorMode::Cmyk,
            bleed: Bleed::default(),
            raster_effects: RasterEffects::High300,
            preview_mode: PreviewMode::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_default_unit_only_settings_deserialize_with_new_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"default_unit":"In"}"#).unwrap();
        assert_eq!(settings.default_unit, Unit::In);
        assert_eq!(settings.color_mode, ColorMode::Cmyk);
        assert_eq!(settings.raster_effects.ppi(), 300);
        assert_eq!(settings.preview_mode, PreviewMode::Default);
        assert_eq!(settings.bleed, Bleed::default());
    }
}
