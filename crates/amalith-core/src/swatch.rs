//! Swatches: named, reusable colors in the document's color palette.
use serde::{Deserialize, Serialize};

/// A color value. Canonical storage is RGBA (matching the screen and every
/// renderer this app targets); CMYK is a *conversion*, not a second stored
/// representation — [`Self::to_cmyk`]/[`Self::from_cmyk`] use the standard
/// naive device formula (no ICC profile / press calibration), the same one
/// most vector tools fall back to without a color-management engine. Spot
/// colors (named inks) aren't modeled at all yet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Builds an RGB color from CMYK components (`0.0..=1.0` each), via the
    /// naive device formula `rgb = (1-ink) * (1-k)`. Alpha is always `1.0`
    /// — CMYK has no transparency channel of its own.
    pub fn from_cmyk(c: f32, m: f32, y: f32, k: f32) -> Self {
        Self::rgb((1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k))
    }

    /// This color's CMYK approximation as `[c, m, y, k]` (`0.0..=1.0`
    /// each), via the naive device formula: `k = 1 - max(r,g,b)`, each ink
    /// `= (1 - channel - k) / (1 - k)`. Alpha is dropped (CMYK has no
    /// transparency channel). The inverse of [`Self::from_cmyk`], modulo
    /// the usual RGB↔CMYK round-trip's rank deficiency (many CMYK
    /// combinations, e.g. any "rich black", map to the same RGB and so
    /// can't all be recovered from it).
    pub fn to_cmyk(&self) -> [f32; 4] {
        let k = 1.0 - self.r.max(self.g).max(self.b);
        if k >= 1.0 - f32::EPSILON {
            [0.0, 0.0, 0.0, 1.0]
        } else {
            [
                (1.0 - self.r - k) / (1.0 - k),
                (1.0 - self.g - k) / (1.0 - k),
                (1.0 - self.b - k) / (1.0 - k),
                k,
            ]
        }
    }
}

/// A named color stored in the document's swatch palette.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub name: String,
    pub color: Color,
}

impl Swatch {
    pub fn new(name: impl Into<String>, color: Color) -> Self {
        Self {
            name: name.into(),
            color,
        }
    }
}
