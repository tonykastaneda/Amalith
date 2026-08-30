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

/// How a stroke's outline sits relative to the path: centred on it
/// (the classic default), tucked entirely inside a closed shape, or
/// pushed entirely outside it. Illustrator's Stroke panel "Align Stroke".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeAlign {
    #[default]
    Center,
    Inside,
    Outside,
}

/// The shape drawn at an open path's endpoints. Mirrors `kurbo::Cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    /// "Projecting" in Illustrator's panel — a square that overshoots the
    /// endpoint by half the stroke weight.
    Square,
}

/// How a stroke turns a corner. Mirrors `kurbo::Join`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Everything about a stroke except its paint and weight: the corner /
/// endpoint treatment, the miter cutoff, which side of the path it hugs,
/// and an optional dash pattern. Split out of [`Appearance`] so the whole
/// bundle round-trips through one command.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    #[serde(default)]
    pub cap: LineCap,
    #[serde(default)]
    pub join: LineJoin,
    /// When a mitered corner's spike grows longer than `miter_limit`
    /// times the stroke weight, the join falls back to a bevel. This is
    /// Illustrator's "Limit: N x". Default 10, useful range 1..=500.
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f64,
    #[serde(default)]
    pub align: StrokeAlign,
    /// `false` = solid. `true` = use `dash` below.
    #[serde(default)]
    pub dashed: bool,
    /// Three dash/gap pairs, matching the six boxes in Illustrator's
    /// Stroke panel. A pair of `0.0` is skipped; if every entry is `0.0`
    /// the stroke renders solid even with `dashed` set.
    #[serde(default)]
    pub dash: [f64; 6],
    #[serde(default)]
    pub dash_offset: f64,
}

fn default_miter_limit() -> f64 {
    10.0
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: default_miter_limit(),
            align: StrokeAlign::Center,
            dashed: false,
            dash: [0.0; 6],
            dash_offset: 0.0,
        }
    }
}

impl StrokeStyle {
    /// The dash/gap run with empty pairs dropped, or `None` when the
    /// stroke is effectively solid (not dashed, or every box empty).
    pub fn dash_pattern(&self) -> Option<Vec<f64>> {
        if !self.dashed {
            return None;
        }
        let pat: Vec<f64> = self
            .dash
            .chunks(2)
            .filter(|p| p[0] > 0.0 || p[1] > 0.0)
            .flat_map(|p| [p[0].max(0.0), p[1].max(0.0)])
            .collect();
        if pat.is_empty() || pat.iter().all(|v| *v == 0.0) {
            None
        } else {
            Some(pat)
        }
    }
}

/// An object's fill, stroke, and compositing opacity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    pub fill: Paint,
    pub stroke: Paint,
    pub stroke_width: f64,
    /// Corner / cap / dash / alignment of the stroke. Defaults when
    /// reading documents written before the Stroke panel existed.
    #[serde(default)]
    pub stroke_style: StrokeStyle,
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
            stroke_style: StrokeStyle::default(),
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

    #[test]
    fn old_serialized_appearance_defaults_stroke_style() {
        let mut value = serde_json::to_value(Appearance::default()).unwrap();
        value.as_object_mut().unwrap().remove("stroke_style");
        let appearance: Appearance = serde_json::from_value(value).unwrap();
        assert_eq!(appearance.stroke_style, StrokeStyle::default());
        assert_eq!(appearance.stroke_style.miter_limit, 10.0);
        assert_eq!(appearance.stroke_style.align, StrokeAlign::Center);
    }

    #[test]
    fn dash_pattern_drops_empty_pairs_and_solid_stays_none() {
        let solid = StrokeStyle::default();
        assert_eq!(solid.dash_pattern(), None);

        let dashed = StrokeStyle {
            dashed: true,
            dash: [6.0, 3.0, 0.0, 0.0, 0.0, 0.0],
            ..StrokeStyle::default()
        };
        assert_eq!(dashed.dash_pattern(), Some(vec![6.0, 3.0]));

        let empty = StrokeStyle {
            dashed: true,
            ..StrokeStyle::default()
        };
        assert_eq!(empty.dash_pattern(), None);
    }
}
