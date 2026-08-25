//! Swatches: named, reusable colors in the document's color palette.
use serde::{Deserialize, Serialize};

/// A color value. Stub color model: RGBA only for now; CMYK/spot colors
/// arrive with the print-color subsystem.
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
