//! Adjustment layers: non-destructive color and filter operations that sit
//! in a layer's stack and change everything beneath them *in that layer
//! only* (a raster layer is a self-contained unit). Photoshop calls these
//! adjustment layers; here an adjustment is an ordinary object
//! ([`crate::ObjectKind::Adjustment`]), so visibility, name, opacity
//! (`Object::appearance.opacity`), ordering and undo all come from the one
//! object model.
//!
//! Parameter shapes and valid ranges follow Compositor's
//! `Document/LayerAdjustment.swift`, `Levels.swift`, `Curves.swift`,
//! `HueSaturation.swift` and `ImageAdjustments.swift` at
//! `430620694ab001d80448e0dad44342f108ebbfde` (MIT; see
//! `third_party/compositor/NOTICE.md`). The pixel math itself lives in the
//! `amalith-adjust` crate.
//!
//! Every field is `#[serde(default)]` and every default is Photoshop's own
//! starting value, so a file written by a newer build (with fields this one
//! doesn't know) or an older one (missing fields) still loads.

use serde::{Deserialize, Serialize};

use crate::appearance::BlendMode;
use crate::geom::Rect;
use crate::object::ImageMask;

/// An adjustment object's payload.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct AdjustmentData {
    #[serde(default)]
    pub op: AdjustmentOp,
    /// How the adjusted result combines with what's beneath it. Adjustments
    /// carry their own blend mode because Amalith objects otherwise only
    /// have per-appearance-item ones.
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// Where the adjustment applies: 255 applies fully, 0 not at all.
    /// `None` applies everywhere. Same shape as an image's layer mask.
    #[serde(default)]
    pub mask: Option<ImageMask>,
    /// The rectangle, in the object's local space, the mask image is
    /// stretched over. Outside it the adjustment applies fully (as if the
    /// mask were white there). Meaningless while `mask` is `None`.
    #[serde(default)]
    pub mask_bounds: Rect,
}

impl AdjustmentData {
    pub fn new(op: AdjustmentOp) -> Self {
        Self { op, ..Self::default() }
    }

    /// Whether every parameter is finite and in range. Commands refuse
    /// invalid data rather than clamping it silently.
    pub fn validate(&self) -> bool {
        self.op.validate() && self.mask_bounds.is_finite()
    }
}

/// The operation an adjustment performs, with its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AdjustmentOp {
    Levels(LevelsParams),
    Curves(CurvesParams),
    HueSaturation(HueSaturationParams),
    ColorBalance(ColorBalanceParams),
    Exposure(ExposureParams),
    BlackWhite(BlackWhiteParams),
    Invert,
    GaussianBlur(GaussianBlurParams),
    MotionBlur(MotionBlurParams),
    AddNoise(AddNoiseParams),
}

impl Default for AdjustmentOp {
    fn default() -> Self {
        AdjustmentOp::Levels(LevelsParams::default())
    }
}

/// The kinds of adjustment, without parameters — for menus, icons and
/// default names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdjustmentKind {
    Levels,
    Curves,
    HueSaturation,
    ColorBalance,
    Exposure,
    BlackWhite,
    Invert,
    GaussianBlur,
    MotionBlur,
    AddNoise,
}

impl AdjustmentKind {
    /// Every kind, in menu order.
    pub const ALL: [AdjustmentKind; 10] = [
        AdjustmentKind::Levels,
        AdjustmentKind::Curves,
        AdjustmentKind::Exposure,
        AdjustmentKind::HueSaturation,
        AdjustmentKind::ColorBalance,
        AdjustmentKind::BlackWhite,
        AdjustmentKind::Invert,
        AdjustmentKind::GaussianBlur,
        AdjustmentKind::MotionBlur,
        AdjustmentKind::AddNoise,
    ];

    pub fn label(self) -> &'static str {
        match self {
            AdjustmentKind::Levels => "Levels",
            AdjustmentKind::Curves => "Curves",
            AdjustmentKind::HueSaturation => "Hue/Saturation",
            AdjustmentKind::ColorBalance => "Color Balance",
            AdjustmentKind::Exposure => "Exposure",
            AdjustmentKind::BlackWhite => "Black & White",
            AdjustmentKind::Invert => "Invert",
            AdjustmentKind::GaussianBlur => "Gaussian Blur",
            AdjustmentKind::MotionBlur => "Motion Blur",
            AdjustmentKind::AddNoise => "Add Noise",
        }
    }

    /// Photoshop's starting parameters for a new adjustment of this kind.
    pub fn default_op(self) -> AdjustmentOp {
        match self {
            AdjustmentKind::Levels => AdjustmentOp::Levels(LevelsParams::default()),
            AdjustmentKind::Curves => AdjustmentOp::Curves(CurvesParams::default()),
            AdjustmentKind::HueSaturation => AdjustmentOp::HueSaturation(HueSaturationParams::default()),
            AdjustmentKind::ColorBalance => AdjustmentOp::ColorBalance(ColorBalanceParams::default()),
            AdjustmentKind::Exposure => AdjustmentOp::Exposure(ExposureParams::default()),
            AdjustmentKind::BlackWhite => AdjustmentOp::BlackWhite(BlackWhiteParams::default()),
            AdjustmentKind::Invert => AdjustmentOp::Invert,
            AdjustmentKind::GaussianBlur => AdjustmentOp::GaussianBlur(GaussianBlurParams::default()),
            AdjustmentKind::MotionBlur => AdjustmentOp::MotionBlur(MotionBlurParams::default()),
            AdjustmentKind::AddNoise => AdjustmentOp::AddNoise(AddNoiseParams::default()),
        }
    }
}

impl AdjustmentOp {
    pub fn kind(&self) -> AdjustmentKind {
        match self {
            AdjustmentOp::Levels(_) => AdjustmentKind::Levels,
            AdjustmentOp::Curves(_) => AdjustmentKind::Curves,
            AdjustmentOp::HueSaturation(_) => AdjustmentKind::HueSaturation,
            AdjustmentOp::ColorBalance(_) => AdjustmentKind::ColorBalance,
            AdjustmentOp::Exposure(_) => AdjustmentKind::Exposure,
            AdjustmentOp::BlackWhite(_) => AdjustmentKind::BlackWhite,
            AdjustmentOp::Invert => AdjustmentKind::Invert,
            AdjustmentOp::GaussianBlur(_) => AdjustmentKind::GaussianBlur,
            AdjustmentOp::MotionBlur(_) => AdjustmentKind::MotionBlur,
            AdjustmentOp::AddNoise(_) => AdjustmentKind::AddNoise,
        }
    }

    /// Whether this op changes nothing, so rendering can skip it. Black &
    /// White, Invert and the filters always change something.
    pub fn is_identity(&self) -> bool {
        match self {
            AdjustmentOp::Levels(p) => p.is_identity(),
            AdjustmentOp::Curves(p) => p.is_identity(),
            AdjustmentOp::HueSaturation(p) => p.is_identity(),
            AdjustmentOp::ColorBalance(p) => p.is_identity(),
            AdjustmentOp::Exposure(p) => *p == ExposureParams::default(),
            AdjustmentOp::BlackWhite(_) | AdjustmentOp::Invert => false,
            AdjustmentOp::GaussianBlur(_) | AdjustmentOp::MotionBlur(_) | AdjustmentOp::AddNoise(_) => false,
        }
    }

    /// Whether the op reads neighboring pixels (or, for noise, depends on
    /// position) rather than mapping each color on its own.
    pub fn is_spatial(&self) -> bool {
        matches!(self, AdjustmentOp::GaussianBlur(_) | AdjustmentOp::MotionBlur(_) | AdjustmentOp::AddNoise(_))
    }

    /// How far beyond a region, in document units, the op reads — so a
    /// partial render still has the pixels a blur pulls in from outside.
    /// Upstream's `samplingMargin`.
    pub fn margin_doc(&self) -> f64 {
        match self {
            AdjustmentOp::GaussianBlur(p) => p.radius * 3.0 + 2.0,
            AdjustmentOp::MotionBlur(p) => p.distance / 2.0 + 2.0,
            _ => 0.0,
        }
    }

    pub fn validate(&self) -> bool {
        match self {
            AdjustmentOp::Levels(p) => p.validate(),
            AdjustmentOp::Curves(p) => p.validate(),
            AdjustmentOp::HueSaturation(p) => p.validate(),
            AdjustmentOp::ColorBalance(p) => p.validate(),
            AdjustmentOp::Exposure(p) => p.validate(),
            AdjustmentOp::BlackWhite(p) => p.validate(),
            AdjustmentOp::Invert => true,
            AdjustmentOp::GaussianBlur(p) => p.validate(),
            AdjustmentOp::MotionBlur(p) => p.validate(),
            AdjustmentOp::AddNoise(p) => p.validate(),
        }
    }
}

fn within(v: f64, lo: f64, hi: f64) -> bool {
    v.is_finite() && v >= lo && v <= hi
}

// --- Levels ----------------------------------------------------------------

/// Channel order shared by Levels and Curves: the composite first, then
/// red, green, blue (upstream's `LevelsChannel`).
pub const CHANNEL_COMPOSITE: usize = 0;

/// One channel's Levels: input black/white points and gamma, then the
/// output range, all on the 0–255 scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelRange {
    pub black: f64,
    pub gamma: f64,
    pub white: f64,
    pub output_black: f64,
    pub output_white: f64,
}

impl Default for LevelRange {
    fn default() -> Self {
        Self { black: 0.0, gamma: 1.0, white: 255.0, output_black: 0.0, output_white: 255.0 }
    }
}

impl LevelRange {
    /// Upstream's valid ranges: black 0–254, white above black, gamma
    /// 0.1–9.99, outputs 0–255.
    pub fn validate(&self) -> bool {
        within(self.black, 0.0, 254.0)
            && within(self.white, self.black + 1.0, 255.0)
            && within(self.gamma, 0.1, 9.99)
            && within(self.output_black, 0.0, 255.0)
            && within(self.output_white, 0.0, 255.0)
    }
}

/// Levels: `ranges[0]` is the composite, then red, green, blue.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelsParams {
    pub ranges: [LevelRange; 4],
}

impl LevelsParams {
    pub fn is_identity(&self) -> bool {
        self.ranges.iter().all(|r| *r == LevelRange::default())
    }
    pub fn validate(&self) -> bool {
        self.ranges.iter().all(LevelRange::validate)
    }
}

// --- Curves ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub x: f64,
    pub y: f64,
}

/// Curves: `channels[0]` is the composite, then red, green, blue. Each is
/// 2–32 points on the 0–255 scale, strictly increasing in `x`, from x = 0
/// to x = 255.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CurvesParams {
    pub channels: [Vec<CurvePoint>; 4],
}

impl CurvesParams {
    pub fn identity_channel() -> Vec<CurvePoint> {
        vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 255.0, y: 255.0 }]
    }
    pub fn is_identity(&self) -> bool {
        self.channels.iter().all(|c| c.iter().all(|p| p.x == p.y))
    }
    pub fn validate(&self) -> bool {
        self.channels.iter().all(|points| {
            (2..=32).contains(&points.len())
                && points.first().is_some_and(|p| p.x == 0.0)
                && points.last().is_some_and(|p| p.x == 255.0)
                && points.iter().all(|p| within(p.x, 0.0, 255.0) && within(p.y, 0.0, 255.0))
                && points.windows(2).all(|w| w[0].x < w[1].x)
        })
    }
}

impl Default for CurvesParams {
    fn default() -> Self {
        Self { channels: std::array::from_fn(|_| Self::identity_channel()) }
    }
}

// --- Hue/Saturation --------------------------------------------------------

/// Hue/Saturation's ranges, in order: Master, then the six color families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ColorRange {
    #[default]
    Master,
    Reds,
    Yellows,
    Greens,
    Cyans,
    Blues,
    Magentas,
}

impl ColorRange {
    pub const ALL: [ColorRange; 7] = [
        ColorRange::Master,
        ColorRange::Reds,
        ColorRange::Yellows,
        ColorRange::Greens,
        ColorRange::Cyans,
        ColorRange::Blues,
        ColorRange::Magentas,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Photoshop's starting hue band for this range, in degrees.
    pub fn default_band(self) -> HueBand {
        let b = |a, b, c, d| HueBand { falloff_start: a, range_start: b, range_end: c, falloff_end: d };
        match self {
            ColorRange::Master => b(0.0, 0.0, 360.0, 360.0),
            ColorRange::Reds => b(315.0, 345.0, 15.0, 45.0),
            ColorRange::Yellows => b(15.0, 45.0, 75.0, 105.0),
            ColorRange::Greens => b(75.0, 105.0, 135.0, 165.0),
            ColorRange::Cyans => b(135.0, 165.0, 195.0, 225.0),
            ColorRange::Blues => b(195.0, 225.0, 255.0, 285.0),
            ColorRange::Magentas => b(255.0, 285.0, 315.0, 345.0),
        }
    }
}

/// A hue band in degrees, wrapping at 360: full strength from
/// `range_start` to `range_end`, fading to nothing at the falloff ends.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HueBand {
    pub falloff_start: f64,
    pub range_start: f64,
    pub range_end: f64,
    pub falloff_end: f64,
}

/// One range's hue shift (−180–180, or 0–360 when colorizing), saturation
/// (−100–100, or 0–100 colorizing) and lightness (−100–100).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RangeAdjustment {
    pub hue: f64,
    pub saturation: f64,
    pub lightness: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HueSaturationParams {
    pub colorize: bool,
    /// The range `invert_range` applies to — and, in the panel, the range
    /// the sliders edit.
    pub range: ColorRange,
    /// Apply `range`'s adjustment to everything *outside* its band.
    pub invert_range: bool,
    /// Indexed by [`ColorRange::index`].
    pub adjustments: [RangeAdjustment; 7],
    /// Indexed by [`ColorRange::index`].
    pub bands: [HueBand; 7],
}

impl Default for HueSaturationParams {
    fn default() -> Self {
        Self {
            colorize: false,
            range: ColorRange::Master,
            invert_range: false,
            adjustments: [RangeAdjustment::default(); 7],
            bands: ColorRange::ALL.map(ColorRange::default_band),
        }
    }
}

impl HueSaturationParams {
    pub fn is_identity(&self) -> bool {
        !self.colorize && self.adjustments.iter().all(|a| *a == RangeAdjustment::default())
    }
    pub fn validate(&self) -> bool {
        self.adjustments.iter().all(|a| {
            within(a.hue, -360.0, 360.0) && within(a.saturation, -100.0, 100.0) && within(a.lightness, -100.0, 100.0)
        }) && self.bands.iter().all(|b| {
            [b.falloff_start, b.range_start, b.range_end, b.falloff_end].iter().all(|v| v.is_finite())
        })
    }
}

// --- Color Balance ---------------------------------------------------------

/// Color Balance: each tonal range is `[cyan–red, magenta–green,
/// yellow–blue]`, −100–100.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColorBalanceParams {
    pub shadows: [f64; 3],
    pub midtones: [f64; 3],
    pub highlights: [f64; 3],
    pub preserve_luminosity: bool,
}

impl Default for ColorBalanceParams {
    fn default() -> Self {
        Self { shadows: [0.0; 3], midtones: [0.0; 3], highlights: [0.0; 3], preserve_luminosity: true }
    }
}

impl ColorBalanceParams {
    pub fn is_identity(&self) -> bool {
        [self.shadows, self.midtones, self.highlights].iter().flatten().all(|v| *v == 0.0)
    }
    pub fn validate(&self) -> bool {
        [self.shadows, self.midtones, self.highlights].iter().flatten().all(|v| within(*v, -100.0, 100.0))
    }
}

// --- Exposure --------------------------------------------------------------

/// Exposure in stops (−20–20), offset (−0.5–0.5) and gamma (0.01–9.99).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExposureParams {
    pub exposure: f64,
    pub offset: f64,
    pub gamma: f64,
}

impl Default for ExposureParams {
    fn default() -> Self {
        Self { exposure: 0.0, offset: 0.0, gamma: 1.0 }
    }
}

impl ExposureParams {
    pub fn validate(&self) -> bool {
        within(self.exposure, -20.0, 20.0) && within(self.offset, -0.5, 0.5) && within(self.gamma, 0.01, 9.99)
    }
}

// --- Black & White ---------------------------------------------------------

/// Black & White: how bright each color family becomes in gray (percent,
/// −200–300), with an optional tint.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlackWhiteParams {
    pub reds: f64,
    pub yellows: f64,
    pub greens: f64,
    pub cyans: f64,
    pub blues: f64,
    pub magentas: f64,
    pub tint: bool,
    /// Degrees, 0–360.
    pub tint_hue: f64,
    /// Percent, 0–100.
    pub tint_saturation: f64,
}

impl Default for BlackWhiteParams {
    /// Photoshop's defaults.
    fn default() -> Self {
        Self {
            reds: 40.0,
            yellows: 60.0,
            greens: 40.0,
            cyans: 60.0,
            blues: 20.0,
            magentas: 80.0,
            tint: false,
            tint_hue: 40.0,
            tint_saturation: 20.0,
        }
    }
}

impl BlackWhiteParams {
    /// Weights in the order red, yellow, green, cyan, blue, magenta.
    pub fn weights(&self) -> [f64; 6] {
        [self.reds, self.yellows, self.greens, self.cyans, self.blues, self.magentas]
    }
    pub fn validate(&self) -> bool {
        self.weights().iter().all(|w| within(*w, -200.0, 300.0))
            && within(self.tint_hue, 0.0, 360.0)
            && within(self.tint_saturation, 0.0, 100.0)
    }
}

// --- Filters ---------------------------------------------------------------

/// Gaussian blur radius (the Gaussian's sigma) in document units, 0.1–250.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GaussianBlurParams {
    pub radius: f64,
}

impl Default for GaussianBlurParams {
    fn default() -> Self {
        Self { radius: 10.0 }
    }
}

impl GaussianBlurParams {
    pub fn validate(&self) -> bool {
        within(self.radius, 0.1, 250.0)
    }
}

/// Motion blur: streak angle in degrees (−90–90) and length in document
/// units (1–2000).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionBlurParams {
    pub angle: f64,
    pub distance: f64,
}

impl Default for MotionBlurParams {
    fn default() -> Self {
        Self { angle: 0.0, distance: 10.0 }
    }
}

impl MotionBlurParams {
    pub fn validate(&self) -> bool {
        within(self.angle, -90.0, 90.0) && within(self.distance, 1.0, 2000.0)
    }
}

/// Add Noise: amount in percent (0.1–400), uniform or Gaussian,
/// per-channel or monochromatic, with a seed so the grain is repeatable.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AddNoiseParams {
    pub amount: f64,
    pub gaussian: bool,
    pub monochromatic: bool,
    pub seed: u32,
}

impl Default for AddNoiseParams {
    fn default() -> Self {
        Self { amount: 10.0, gaussian: false, monochromatic: false, seed: 0 }
    }
}

impl AddNoiseParams {
    pub fn validate(&self) -> bool {
        within(self.amount, 0.1, 400.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_op_is_valid() {
        for kind in AdjustmentKind::ALL {
            let op = kind.default_op();
            assert!(op.validate(), "{kind:?} default is invalid");
            assert_eq!(op.kind(), kind);
        }
    }

    #[test]
    fn tonal_defaults_change_nothing_but_effects_do() {
        for kind in [
            AdjustmentKind::Levels,
            AdjustmentKind::Curves,
            AdjustmentKind::HueSaturation,
            AdjustmentKind::ColorBalance,
            AdjustmentKind::Exposure,
        ] {
            assert!(kind.default_op().is_identity(), "{kind:?}");
        }
        for kind in [AdjustmentKind::BlackWhite, AdjustmentKind::Invert, AdjustmentKind::GaussianBlur] {
            assert!(!kind.default_op().is_identity(), "{kind:?}");
        }
    }

    #[test]
    fn out_of_range_values_are_invalid() {
        let mut levels = LevelsParams::default();
        levels.ranges[0].white = 0.0; // below black + 1
        assert!(!AdjustmentOp::Levels(levels).validate());

        let mut curves = CurvesParams::default();
        curves.channels[2] = vec![CurvePoint { x: 0.0, y: 0.0 }, CurvePoint { x: 200.0, y: 255.0 }];
        assert!(!AdjustmentOp::Curves(curves).validate(), "a curve must end at x = 255");

        assert!(!AdjustmentOp::Exposure(ExposureParams { gamma: f64::NAN, ..Default::default() }).validate());
        assert!(!AdjustmentOp::GaussianBlur(GaussianBlurParams { radius: 0.0 }).validate());
    }

    #[test]
    fn missing_fields_load_as_defaults() {
        let data: AdjustmentData = serde_json::from_str(r#"{"op":{"Levels":{}}}"#).unwrap();
        assert_eq!(data, AdjustmentData::new(AdjustmentOp::Levels(LevelsParams::default())));

        let bw: AdjustmentOp = serde_json::from_str(r#"{"BlackWhite":{"reds":55}}"#).unwrap();
        let AdjustmentOp::BlackWhite(bw) = bw else { panic!() };
        assert_eq!(bw.reds, 55.0);
        assert_eq!(bw.yellows, 60.0, "unspecified weights keep Photoshop's defaults");
    }

    #[test]
    fn every_op_round_trips() {
        for kind in AdjustmentKind::ALL {
            let data = AdjustmentData::new(kind.default_op());
            let json = serde_json::to_string(&data).unwrap();
            assert_eq!(serde_json::from_str::<AdjustmentData>(&json).unwrap(), data, "{json}");
        }
    }

    #[test]
    fn blur_margins_match_upstream() {
        assert_eq!(AdjustmentOp::GaussianBlur(GaussianBlurParams { radius: 4.0 }).margin_doc(), 14.0);
        assert_eq!(AdjustmentOp::MotionBlur(MotionBlurParams { angle: 0.0, distance: 20.0 }).margin_doc(), 12.0);
        assert_eq!(AdjustmentKind::Levels.default_op().margin_doc(), 0.0);
    }
}
