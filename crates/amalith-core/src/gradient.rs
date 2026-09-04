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
//! it builds the GPU gradient. [`FreeformPoint::pos`] uses the same
//! convention, so a freeform gradient's points scale with the object too.

use crate::ids::GradientId;
use crate::swatch::Color;
use serde::{Deserialize, Serialize};

/// Linear (blend along a line), radial (blend out from a point in
/// concentric rings), or freeform (Illustrator's "Points" mode: an
/// arbitrary scatter of colored points blending into each other, no axis
/// at all — see [`FreeformPoint`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GradientKind {
    #[default]
    Linear,
    Radial,
    Freeform,
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

fn default_spread() -> f32 {
    0.35
}

/// One color point in a **freeform** gradient (Illustrator's "Points"
/// mode). Unlike a [`GradientStop`], a point has no slider position or
/// neighbors it's implicitly ordered against — it's placed anywhere in
/// the shape and blends with whichever other points are nearby.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FreeformPoint {
    /// Position in bounding-box unit space (`0..1`), same convention as
    /// every other gradient coordinate in this module.
    pub pos: [f64; 2],
    pub color: Color,
    /// Illustrator's per-point "Opacity", `0.0..=1.0`.
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// How far this point's color reaches at (near-)full strength before
    /// blending into its neighbors, as a fraction of the object's
    /// bounding-box diagonal. Illustrator's per-point "Spread" (the
    /// soft-edged circle shown around a selected point).
    #[serde(default = "default_spread")]
    pub spread: f32,
}

impl FreeformPoint {
    pub fn new(pos: [f64; 2], color: Color) -> Self {
        Self {
            pos,
            color,
            opacity: 1.0,
            spread: default_spread(),
        }
    }

    /// The point's color with its opacity folded into the alpha.
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
    /// Freeform only: the scattered color points (`stops`/`start`/`end`/
    /// `aspect` are unused and left at their defaults for this kind).
    #[serde(default)]
    pub points: Vec<FreeformPoint>,
}

impl Gradient {
    /// The two-stop white→black blend Illustrator gives a fresh gradient.
    pub fn default_stops() -> Vec<GradientStop> {
        vec![
            GradientStop::new(0.0, Color::rgb(1.0, 1.0, 1.0)),
            GradientStop::new(1.0, Color::rgb(0.0, 0.0, 0.0)),
        ]
    }

    /// The three-point white / gray / black scatter Illustrator gives a
    /// fresh freeform gradient, spread wide enough to cover most shapes
    /// with no gaps at the default spread.
    pub fn default_points() -> Vec<FreeformPoint> {
        vec![
            FreeformPoint::new([0.2, 0.2], Color::rgb(1.0, 1.0, 1.0)),
            FreeformPoint::new([0.8, 0.35], Color::rgb(0.5, 0.5, 0.5)),
            FreeformPoint::new([0.4, 0.8], Color::rgb(0.0, 0.0, 0.0)),
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
            points: Vec::new(),
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
            points: Vec::new(),
        }
    }

    /// A fresh freeform gradient (Illustrator's "Points" mode) with a
    /// white / gray / black scatter.
    pub fn freeform(id: GradientId) -> Self {
        Self {
            id,
            kind: GradientKind::Freeform,
            stops: Self::default_stops(),
            start: default_start(),
            end: default_end(),
            aspect: 1.0,
            points: Self::default_points(),
        }
    }

    /// The ellipse's unsquished-axis angle in **unit space**, radians: the
    /// `start`→`end` direction. `end` always marks a real point on that
    /// axis (at `radius()` from the centre), so rotating the ellipse (the
    /// Gradient tool's rotate handle) means rotating `end` itself around
    /// `start`, not a separate stored angle. Meaningful for radial
    /// gradients.
    pub fn radial_axis_rad(&self) -> f64 {
        let dx = self.end[0] - self.start[0];
        let dy = self.end[1] - self.start[1];
        dy.atan2(dx)
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
    /// there are at least two; also clamps every freeform point's opacity
    /// and spread. Call after any edit that can reorder or remove stops
    /// or points.
    pub fn normalize(&mut self) {
        for stop in &mut self.stops {
            stop.offset = stop.offset.clamp(0.0, 1.0);
            stop.opacity = stop.opacity.clamp(0.0, 1.0);
            stop.midpoint = stop.midpoint.clamp(0.05, 0.95);
        }
        for point in &mut self.points {
            point.opacity = point.opacity.clamp(0.0, 1.0);
            point.spread = point.spread.clamp(0.02, 3.0);
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
    /// opacity *and* midpoint skew (see [`Self::render_stops`] for why a
    /// renderer that only understands straight-line interpolation between
    /// stops — SVG, our own GPU gradients — needs a different, baked
    /// representation to show the same skew).
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
                let k = biased_t((t - a.offset) / span, a.midpoint);
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

    /// This gradient's stops, "baked" for a renderer that can only do
    /// straight-line interpolation between consecutive stops (SVG,
    /// vello/peniko's GPU gradients — [`Self::sample`] can evaluate the
    /// midpoint-skewed curve directly, but neither of those can). Any gap
    /// between two adjacent stops whose `midpoint` skews the 50% blend
    /// point away from the centre gets subdivided into extra samples that
    /// closely approximate the skewed curve as a polyline; a gap already
    /// at the default `0.5` is left as its original two stops (exact
    /// either way, so no point paying for extra stops). Every returned
    /// stop already has per-stop opacity folded into its colour alpha
    /// (`opacity` is always `1.0`) and `midpoint` reset to `0.5` — it's
    /// flat output, not a gradient definition to edit further.
    pub fn render_stops(&self) -> Vec<GradientStop> {
        const SUBDIVISIONS: usize = 24;
        const FLAT: f32 = 0.5;
        let baked = |offset: f32, color: Color| GradientStop {
            offset,
            color,
            opacity: 1.0,
            midpoint: FLAT,
        };
        if self.stops.len() < 2 {
            return self
                .stops
                .iter()
                .map(|s| baked(s.offset, s.effective_color()))
                .collect();
        }
        let mut out = vec![baked(self.stops[0].offset, self.stops[0].effective_color())];
        for pair in self.stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if (a.midpoint - FLAT).abs() < 0.01 {
                out.push(baked(b.offset, b.effective_color()));
                continue;
            }
            let span = (b.offset - a.offset).max(0.0);
            let (ca, cb) = (a.effective_color(), b.effective_color());
            for i in 1..=SUBDIVISIONS {
                let k = i as f32 / SUBDIVISIONS as f32;
                let bk = biased_t(k, a.midpoint);
                let c = Color::rgba(
                    ca.r + (cb.r - ca.r) * bk,
                    ca.g + (cb.g - ca.g) * bk,
                    ca.b + (cb.b - ca.b) * bk,
                    ca.a + (cb.a - ca.a) * bk,
                );
                out.push(baked(a.offset + span * k, c));
            }
        }
        out
    }
}

/// Remaps a raw `0..=1` blend fraction so the 50% blend point lands at
/// `midpoint` instead of the centre — Illustrator's gradient-stop
/// midpoint diamond. `midpoint == 0.5` is the identity (`k` unchanged).
fn biased_t(k: f32, midpoint: f32) -> f32 {
    let m = midpoint.clamp(0.02, 0.98);
    if k <= m {
        0.5 * (k / m)
    } else {
        0.5 + 0.5 * (k - m) / (1.0 - m)
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
    fn freeform_default_has_three_points_and_no_axis_dependency() {
        let g = Gradient::freeform(GradientId::new());
        assert_eq!(g.kind, GradientKind::Freeform);
        assert_eq!(g.points.len(), 3);
        for p in &g.points {
            assert_eq!(p.opacity, 1.0);
            assert!(p.spread > 0.0);
        }
    }

    #[test]
    fn freeform_point_folds_opacity_into_alpha() {
        let mut p = FreeformPoint::new([0.3, 0.6], Color::rgb(1.0, 0.0, 0.0));
        p.opacity = 0.5;
        let c = p.effective_color();
        assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0));
        assert!((c.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn normalize_clamps_freeform_points() {
        let mut g = Gradient::freeform(GradientId::new());
        g.points[0].opacity = 5.0;
        g.points[0].spread = -1.0;
        g.points[1].spread = 100.0;
        g.normalize();
        assert_eq!(g.points[0].opacity, 1.0);
        assert!(g.points[0].spread >= 0.02);
        assert!(g.points[1].spread <= 3.0);
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
            points: Vec::new(),
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
    fn midpoint_skews_the_sampled_blend() {
        let mut g = Gradient::linear(GradientId::new());
        // Pull the midpoint toward the white (offset 0.0) stop: the 50%
        // blend should now land at t=0.2, not t=0.5, and the straight
        // t=0.5 sample should read closer to black than an even lerp.
        g.stops[0].midpoint = 0.2;
        let at_midpoint = g.sample(0.2);
        assert!(
            (at_midpoint.r - 0.5).abs() < 1e-5,
            "the 50% blend should sit at the midpoint, not the centre: {at_midpoint:?}"
        );
        let at_half = g.sample(0.5);
        assert!(
            at_half.r < 0.5,
            "past a midpoint pulled toward the start, t=0.5 should already be past 50%: {at_half:?}"
        );
        // A midpoint of exactly 0.5 is a no-op (plain lerp).
        let mut flat = Gradient::linear(GradientId::new());
        flat.stops[0].midpoint = 0.5;
        assert!((flat.sample(0.5).r - 0.5).abs() < 1e-6);
    }

    #[test]
    fn render_stops_tracks_sample_and_flattens_midpoint() {
        let mut g = Gradient::linear(GradientId::new());
        g.stops[0].midpoint = 0.25;
        let baked = g.render_stops();
        // Every baked stop has a flat (no-op) midpoint and full opacity.
        assert!(baked.iter().all(|s| s.midpoint == 0.5 && s.opacity == 1.0));
        // The baked polyline should closely approximate `sample()` at the
        // same slider position, at every subdivision point.
        for s in &baked {
            let expected = g.sample(s.offset);
            assert!((s.color.r - expected.r).abs() < 1e-4, "{s:?} vs {expected:?}");
        }
        // A flat (0.5) midpoint needs no subdividing: exactly the 2 stops.
        let flat = Gradient::linear(GradientId::new());
        assert_eq!(flat.render_stops().len(), 2);
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
