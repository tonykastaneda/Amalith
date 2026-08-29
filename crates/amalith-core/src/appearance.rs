//! An object's fill and stroke paint.
use crate::swatch::Color;
use serde::{Deserialize, Serialize};

/// What paints a fill or a stroke: nothing, or a flat color. Gradients and
/// patterns aren't modeled yet — every real paint today is `Solid`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    None,
    Solid(Color),
}

impl Paint {
    pub fn color(self) -> Option<Color> {
        match self {
            Paint::None => None,
            Paint::Solid(color) => Some(color),
        }
    }

    /// See [`Color::to_cmyk_limited`]. `None` is unchanged.
    pub fn to_cmyk_limited(self) -> Self {
        match self {
            Paint::None => Paint::None,
            Paint::Solid(color) => Paint::Solid(color.to_cmyk_limited()),
        }
    }
}

/// An object's fill, stroke, and compositing opacity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f64,
    /// Per-object compositing multiplier applied after each paint's own
    /// alpha. Defaults when reading documents written before opacity existed.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
}

fn default_opacity() -> f32 {
    1.0
}

impl Appearance {
    pub const DEFAULT_STROKE_WIDTH: f64 = 10.0;
}

impl Default for Appearance {
    /// Every new object's starting appearance: a light fill and a dark
    /// 10px stroke, both visible immediately — not Illustrator's actual
    /// default (black fill, no stroke), because every primitive tool here
    /// is meant to draw with a visible stroke out of the box for now.
    fn default() -> Self {
        Self {
            fill: Paint::Solid(Color::rgb(0.87, 0.87, 0.87)),
            stroke: Paint::Solid(Color::rgb(0.18, 0.18, 0.18)),
            stroke_width: Self::DEFAULT_STROKE_WIDTH,
            opacity: default_opacity(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_appearance_has_a_visible_stroke() {
        let appearance = Appearance::default();
        assert_eq!(
            appearance.stroke.color(),
            Some(Color::rgb(0.18, 0.18, 0.18))
        );
        assert_eq!(appearance.stroke_width, 10.0);
        assert_eq!(appearance.opacity, 1.0);
    }

    #[test]
    fn none_paint_has_no_color() {
        assert_eq!(Paint::None.color(), None);
    }

    #[test]
    fn old_serialized_appearance_defaults_opacity_to_one() {
        let mut value = serde_json::to_value(Appearance::default()).unwrap();
        value.as_object_mut().unwrap().remove("opacity");
        let appearance: Appearance = serde_json::from_value(value).unwrap();
        assert_eq!(appearance.opacity, 1.0);
    }
}
