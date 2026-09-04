//! Gradients: pooled, reusable multi-stop color blends.
//!
//! A [`Gradient`] lives in the document's gradient pool (like swatches and
//! assets) and is referenced from an object's fill or stroke by
//! [`crate::Paint::Gradient`]. Keeping the definition in a pool — rather
//! than inline on `Paint` — means `Paint` and `Appearance` stay `Copy`,
//! gradients can be shared between objects, and "Save gradient as swatch"
//! (Illustrator's workflow) is just "keep this pool entry around".
//!
//! # Geometry
//!
//! [`Gradient::start`] / [`Gradient::end`] are stored in **object
//! bounding-box unit space**: `(0, 0)` is the top-left of the object's
//! local bounds, `(1, 1)` the bottom-right. This is SVG's
//! `gradientUnits="objectBoundingBox"` convention: one stored gradient
//! maps sensibly onto objects of any size, and a non-square bounding box
//! squishes a radial gradient into an ellipse (same as SVG / Illustrator).
//! The renderer converts unit space to the object's local coordinates when
//! it builds the GPU gradient.

use crate::ids::GradientId;
use crate::swatch::Color;
use serde::{Deserialize, Serialize};

/// Linear (blend along a line) or radial (blend out from a point in
/// concentric rings). Freeform gradients are a different data model
/// (a mesh of free-placed color points) and are not represented here yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
}

fn default_opacity() -> f32 {
    1.0
}

fn default_midpoint() -> f32 {
    0.5
}

/// One color stop on the gradient slider.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Location on the slider, `0.0..=1.0`.
    pub offset: f32,
    /// The stop's color. Its own alpha is the base; [`Self::opacity`]
    /// multiplies on top (Illustrator keeps these as two separate fields).
    pub color: Color,
    /// Illustrator's per-stop "Opacity", `0.0..=1.0`, multiplied into the
    /// color's alpha at render time.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// Fractional position of the midpoint diamond between this stop and
    /// the next one, `0.0..=1.0` (`0.5` = the blend's halfway point sits
    /// centered between the two stops). Ignored on the last stop.
    #[serde(default = "default_midpoint")]
    pub midpoint: f32,
}

impl GradientStop {
    pub fn new(offset: f32, color: Color) -> Self {
        Self {
            offset,
            color,
            opacity: 1.0,
            midpoint: 0.5,
        }
    }

    /// The stop's color with per-stop opacity folded into the alpha.
    pub fn effective_color(&self) -> Color {
        Color::rgba(
            self.color.r,
            self.color.g,
            self.color.b,
            self.color.a * self.opacity.clamp(0.0, 1.0),
        )
    }
}

fn default_start() -> [f64; 2] {
    [0.0, 0.5]
}

fn default_end() -> [f64; 2] {
    [1.0, 0.5]
}

fn default_aspect() -> f64 {
    1.0
}

/// A reusable multi-stop blend. See the module docs for the geometry
/// convention (bounding-box unit space).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gradient {
    pub id: GradientId,
    #[serde(default)]
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
    /// Linear: one end of the gradient axis. Radial: the center.
    /// Bounding-box unit space (see module docs).
    #[serde(default = "default_start")]
    pub start: [f64; 2],
    /// Linear: the other end of the axis. Radial: a point on the outer
    /// ring — `distance(start, end)` is the radius. Bounding-box unit space.
    #[serde(default = "default_end")]
    pub end: [f64; 2],
    /// Radial only: ellipse aspect (Illustrator's "Aspect Ratio",
    /// height / width). `1.0` = circle. Ignored for linear gradients.
    #[serde(default = "default_aspect")]
    pub aspect: f64,
    /// Radial only: extra rotation (degrees) of the ellipse's unsquished
    /// axis away from the `start`→`end` direction. `0.0` (the default)
    /// keeps that axis aligned with the axis line, so `end` always marks
    /// a real point on the ellipse; a non-zero rotation turns the ellipse
    /// independently of `end`, matching Illustrator's separate rotate
    /// handle. Ignored for linear gradients.
    #[serde(default)]
    pub rotation: f64,
}

impl Gradient {
    /// The two-stop white→black blend Illustrator gives a fresh gradient.
    pub fn default_stops() -> Vec<GradientStop> {
        vec![
            GradientStop::new(0.0, Color::rgb(1.0, 1.0, 1.0)),
            GradientStop::new(1.0, Color::rgb(0.0, 0.0, 0.0)),
        ]
    }

    /// A fresh left-to-right linear white→black gradient.
    pub fn linear(id: GradientId) -> Self {
        Self {
            id,
            kind: GradientKind::Linear,
            stops: Self::default_stops(),
            start: default_start(),
            end: default_end(),
            aspect: 1.0,
            rotation: 0.0,
        }
    }

    /// A fresh centered radial white→black gradient.
    pub fn radial(id: GradientId) -> Self {
        Self {
            id,
            kind: GradientKind::Radial,
            stops: Self::default_stops(),
            start: [0.5, 0.5],
            end: [1.0, 0.5],
            aspect: 1.0,
            rotation: 0.0,
        }
    }

    /// The ellipse's unsquished-axis angle in **unit space**, radians:
    /// the `start`→`end` direction plus [`Self::rotation`]. Meaningful for
    /// radial gradients.
    pub fn radial_axis_rad(&self) -> f64 {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        dy.atan2(dx) + self.rotation.to_radians()
    }

    /// Linear-axis angle in degrees, measured like Illustrator's Gradient
    /// panel: 0° points right (+x), positive turns counter-clockwise in a
    /// y-down space (so 90° points *up*). Uses the raw `start`/`end`
    /// vector; meaningful for linear gradients.
    pub fn angle_deg(&self) -> f64 {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        (-dy).atan2(dx).to_degrees()
    }

    /// Rotates `end` around `start` so [`Self::angle_deg`] becomes `deg`,
    /// keeping the current axis length.
    pub fn set_angle_deg(&mut self, deg: f64) {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        let len = (dx * dx + dy * dy).sqrt().max(1e-6);
        let rad = deg.to_radians();
        self.end = [
            self.start[0] + rad.cos() * len,
            self.start[1] - rad.sin() * len,
        ];
    }

    /// Radius in unit space (radial gradients).
    pub fn radius(&self) -> f64 {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        (dx * dx + dy * dy).sqrt()
    }

    /// Ensures stops are sorted by offset and clamped to `0..=1`, and that
    /// there are at least two. Call after any edit that can reorder or
    /// remove stops.
    pub fn normalize(&mut self) {
        for stop in &mut self.stops {
            stop.offset = stop.offset.clamp(0.0, 1.0);
            stop.opacity = stop.opacity.clamp(0.0, 1.0);
            stop.midpoint = stop.midpoint.clamp(0.05, 0.95);
        }
        self.stops
            .sort_by(|a, b| a.offset.partial_cmp(&b.offset).unwrap_or(std::cmp::Ordering::Equal));
        while self.stops.len() < 2 {
            let fill = self
                .stops
                .last()
                .copied()
                .unwrap_or_else(|| GradientStop::new(0.0, Color::rgb(0.0, 0.0, 0.0)));
            self.stops.push(GradientStop {
                offset: 1.0,
                ..fill
            });
        }
    }

    /// Sampled color at slider position `t` (`0..=1`), honoring per-stop
    /// opacity but not midpoint skew — a straight lerp between the
    /// bracketing stops. For previews / eyedropper, not the GPU path.
    pub fn sample(&self, t: f32) -> Color {
        if self.stops.is_empty() {
            return Color::rgb(0.0, 0.0, 0.0);
        }
        let t = t.clamp(0.0, 1.0);
        let first = self.stops.first().unwrap();
        if t <= first.offset {
            return first.effective_color();
        }
        let last = self.stops.last().unwrap();
        if t >= last.offset {
            return last.effective_color();
        }
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if t >= a.offset && t <= b.offset {
                let span = (b.offset - a.offset).max(1e-6);
                let k = (t - a.offset) / span;
                let ca = a.effective_color();
                let cb = b.effective_color();
                return Color::rgba(
                    ca.r + (cb.r - ca.r) * k,
                    ca.g + (cb.g - ca.g) * k,
                    ca.b + (cb.b - ca.b) * k,
                    ca.a + (cb.a - ca.a) * k,
                );
            }
        }
        last.effective_color()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_default_is_white_to_black_left_to_right() {
        let g = Gradient::linear(GradientId::new());
        assert_eq!(g.stops.len(), 2);
        assert_eq!(g.stops[0].color, Color::rgb(1.0, 1.0, 1.0));
        assert_eq!(g.stops[1].color, Color::rgb(0.0, 0.0, 0.0));
        assert!((g.angle_deg()).abs() < 1e-6);
    }

    #[test]
    fn set_angle_preserves_length() {
        let mut g = Gradient::linear(GradientId::new());
        let len0 = g.radius();
        g.set_angle_deg(90.0);
        assert!((g.radius() - len0).abs() < 1e-9);
        assert!(g.end[1] < g.start[1], "90 deg should point up (y-down space)");
    }

    #[test]
    fn normalize_sorts_and_pads() {
        let mut g = Gradient {
            id: GradientId::new(),
            kind: GradientKind::Linear,
            stops: vec![GradientStop::new(0.8, Color::rgb(1.0, 0.0, 0.0))],
            start: default_start(),
            end: default_end(),
            aspect: 1.0,
            rotation: 0.0,
        };
        g.normalize();
        assert_eq!(g.stops.len(), 2);
        assert!(g.stops[0].offset <= g.stops[1].offset);
    }

    #[test]
    fn sample_midway_between_two_stops() {
        let g = Gradient::linear(GradientId::new());
        let mid = g.sample(0.5);
        assert!((mid.r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn old_serialized_stop_defaults_opacity_and_midpoint() {
        let mut v = serde_json::to_value(GradientStop::new(0.0, Color::rgb(0.0, 0.0, 0.0))).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("opacity");
        obj.remove("midpoint");
        let stop: GradientStop = serde_json::from_value(v).unwrap();
        assert_eq!(stop.opacity, 1.0);
        assert_eq!(stop.midpoint, 0.5);
    }
}
