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
}

/// An object's fill and stroke. `stroke_width` exists so a stroke has a
/// visible size the moment it's turned on, but isn't user-adjustable yet —
/// every object gets [`Appearance::DEFAULT_STROKE_WIDTH`], full stop; a
/// dedicated stroke-weight control is future work.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f64,
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
    }

    #[test]
    fn none_paint_has_no_color() {
        assert_eq!(Paint::None.color(), None);
    }
}
