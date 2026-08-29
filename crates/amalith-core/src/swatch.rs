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

    /// Maps this RGB color into a typical process-CMYK gamut and back to
    /// display RGB, approximating Illustrator's RGB → CMYK document
    /// conversion without an ICC profile. Saturated RGB greens and blues
    /// go duller; alpha is unchanged.
    pub fn to_cmyk_limited(self) -> Self {
        let r = srgb_to_linear(self.r.clamp(0.0, 1.0));
        let g = srgb_to_linear(self.g.clamp(0.0, 1.0));
        let b = srgb_to_linear(self.b.clamp(0.0, 1.0));
        let k = 1.0 - r.max(g).max(b);
        let (c, m, y) = if k >= 0.999 {
            (0.0, 0.0, 0.0)
        } else {
            let ink = 1.0 - k;
            (
                (1.0 - r - k) / ink,
                (1.0 - g - k) / ink,
                (1.0 - b - k) / ink,
            )
        };
        let (r, g, b) = process_cmyk_to_linear_rgb(c, m, y, k);
        Self {
            r: linear_to_srgb(r),
            g: linear_to_srgb(g),
            b: linear_to_srgb(b),
            a: self.a,
        }
    }
}

fn srgb_to_linear(u: f32) -> f32 {
    if u <= 0.04045 {
        u / 12.92
    } else {
        ((u + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(u: f32) -> f32 {
    let u = u.clamp(0.0, 1.0);
    if u <= 0.0031308 {
        12.92 * u
    } else {
        1.055 * u.powf(1.0 / 2.4) - 0.055
    }
}

/// Subtractive mix of SWOP-like process inks over paper white, in linear light.
/// Cyan/magenta/yellow are impure (not RGB complements), which is what
/// pulls out-of-gamut RGB into a printable range.
fn process_cmyk_to_linear_rgb(c: f32, m: f32, y: f32, k: f32) -> (f32, f32, f32) {
    const CYAN: (f32, f32, f32) = (0.0, 0.631, 0.847);
    const MAGENTA: (f32, f32, f32) = (0.847, 0.039, 0.447);
    const YELLOW: (f32, f32, f32) = (1.0, 0.910, 0.0);
    const BLACK: (f32, f32, f32) = (0.0, 0.0, 0.0);
    let mut rgb = (1.0, 1.0, 1.0);
    rgb = lay_ink(rgb, c, CYAN);
    rgb = lay_ink(rgb, m, MAGENTA);
    rgb = lay_ink(rgb, y, YELLOW);
    rgb = lay_ink(rgb, k, BLACK);
    rgb
}

fn lay_ink(rgb: (f32, f32, f32), amount: f32, ink: (f32, f32, f32)) -> (f32, f32, f32) {
    (
        rgb.0 * (1.0 - amount * (1.0 - ink.0)),
        rgb.1 * (1.0 - amount * (1.0 - ink.1)),
        rgb.2 * (1.0 - amount * (1.0 - ink.2)),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmyk_limit_dulls_pure_rgb_green_and_preserves_alpha() {
        let green = Color::rgba(0.0, 1.0, 0.0, 0.75);
        let limited = green.to_cmyk_limited();
        assert!(
            limited.g < 0.85,
            "process CMYK cannot hold RGB green; got g={}",
            limited.g
        );
        assert_ne!((limited.r, limited.g, limited.b), (0.0, 1.0, 0.0));
        assert!((limited.a - 0.75).abs() < f32::EPSILON);
    }

    #[test]
    fn cmyk_limit_keeps_white_and_black_near_neutral() {
        let white = Color::rgb(1.0, 1.0, 1.0).to_cmyk_limited();
        assert!(white.r > 0.9 && white.g > 0.9 && white.b > 0.9);
        let black = Color::rgb(0.0, 0.0, 0.0).to_cmyk_limited();
        assert!(black.r < 0.2 && black.g < 0.2 && black.b < 0.2);
    }
}
