//! The color adjustments as exact per-color functions on sRGB-encoded,
//! straight-alpha values in 0–1. These are the single source of truth: the
//! lookup tables in [`crate::lut`] are sampled from them, and the GPU only
//! ever applies those tables.
//!
//! Ported from Compositor at `430620694ab001d80448e0dad44342f108ebbfde`
//! (Copyright (c) 2026 Wonder Assembly LLC, MIT; see
//! `third_party/compositor/NOTICE.md`):
//! - Levels: `Document/Levels.swift` (`LevelRange.apply`, `LevelsSettings.apply`)
//! - Curves: `Document/Curves.swift` (`CurvesSettings.value`, the channel-then-composite order)
//! - Exposure: `Document/ImageAdjustments.swift` (`ExposureSettings.table`)
//! - Hue/Saturation: `Document/HueSaturation.swift` (`hueResponse`, `adjust`, `toHSL`, `toRGB`, `HueBand.weight`)
//! - Black & White and Color Balance: `Rendering/AdjustPixels.c`
//!   (`adjust_black_white`, `tonal_weights`, `adjust_color_balance`)
//!
//! Upstream works on premultiplied bytes and unpremultiplies per pixel;
//! these take straight colors directly, which is what Amalith's images and
//! vello's render targets hold.

use amalith_core::{
    BlackWhiteParams, ColorBalanceParams, ColorRange, CurvePoint, CurvesParams, ExposureParams, HueBand,
    HueSaturationParams, LevelRange, LevelsParams, RangeAdjustment,
};

pub type Rgb = [f64; 3];

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

// --- Levels ----------------------------------------------------------------

/// One channel's Levels on a 0–1 value.
pub fn level(range: &LevelRange, value: f64) -> f64 {
    let span = (range.white - range.black).max(1.0);
    let input = clamp01((value * 255.0 - range.black) / span);
    let gamma = range.gamma.clamp(0.1, 9.99);
    (range.output_black + input.powf(1.0 / gamma) * (range.output_white - range.output_black)) / 255.0
}

/// Levels: each channel's own range first, then the composite.
pub fn levels(params: &LevelsParams, c: Rgb) -> Rgb {
    std::array::from_fn(|i| clamp01(level(&params.ranges[0], level(&params.ranges[i + 1], c[i]))))
}

// --- Curves ----------------------------------------------------------------

/// A curve's value at `x` (0–255), by shape-preserving cubic Hermite
/// interpolation, so it never overshoots between points.
pub fn curve_value(points: &[CurvePoint], x: f64) -> f64 {
    let p = points;
    if p.len() < 2 {
        return x.clamp(0.0, 255.0);
    }
    let i = p.iter().rposition(|q| q.x <= x).unwrap_or(0).min(p.len() - 2);
    let d: Vec<f64> = p.windows(2).map(|w| (w[1].y - w[0].y) / (w[1].x - w[0].x)).collect();
    let slope = |j: usize| -> f64 {
        if j == 0 {
            d[0]
        } else if j == p.len() - 1 {
            d[d.len() - 1]
        } else if d[j - 1] * d[j] <= 0.0 {
            0.0
        } else {
            2.0 / (1.0 / d[j - 1] + 1.0 / d[j])
        }
    };
    let h = p[i + 1].x - p[i].x;
    let t = ((x - p[i].x) / h).clamp(0.0, 1.0);
    let (t2, t3) = (t * t, t * t * t);
    let y = (2.0 * t3 - 3.0 * t2 + 1.0) * p[i].y
        + (t3 - 2.0 * t2 + t) * h * slope(i)
        + (-2.0 * t3 + 3.0 * t2) * p[i + 1].y
        + (t3 - t2) * h * slope(i + 1);
    y.clamp(0.0, 255.0)
}

/// Curves: each channel's own curve first, then the composite.
pub fn curves(params: &CurvesParams, c: Rgb) -> Rgb {
    std::array::from_fn(|i| curve_value(&params.channels[0], curve_value(&params.channels[i + 1], c[i] * 255.0)) / 255.0)
}

// --- Exposure --------------------------------------------------------------

pub fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

pub fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

/// Exposure on one channel: scale and offset linear light, then gamma.
pub fn exposure_channel(params: &ExposureParams, v: f64) -> f64 {
    let scale = 2f64.powf(params.exposure);
    let linear = (srgb_to_linear(v) * scale + params.offset).max(0.0).powf(1.0 / params.gamma);
    clamp01(linear_to_srgb(linear))
}

pub fn exposure(params: &ExposureParams, c: Rgb) -> Rgb {
    c.map(|v| exposure_channel(params, v))
}

// --- Invert ----------------------------------------------------------------

pub fn invert(c: Rgb) -> Rgb {
    c.map(|v| 1.0 - v)
}

// --- Hue/Saturation --------------------------------------------------------

/// Degrees from `from` forward to `to`, always 0–360.
fn forward(from: f64, to: f64) -> f64 {
    (to - from).rem_euclid(360.0)
}

/// How strongly a band claims a hue: 1 inside the range, ramping through
/// each falloff shoulder, 0 outside.
pub fn band_weight(band: &HueBand, hue: f64) -> f64 {
    let span = forward(band.falloff_start, band.falloff_end);
    if span <= 0.0 {
        return 1.0; // Master covers everything.
    }
    let position = forward(band.falloff_start, hue);
    if position > span {
        return 0.0;
    }
    let ramp_in = forward(band.falloff_start, band.range_start);
    let plateau_end = forward(band.falloff_start, band.range_end);
    if position < ramp_in {
        return if ramp_in > 0.0 { position / ramp_in } else { 1.0 };
    }
    if position <= plateau_end {
        return 1.0;
    }
    let ramp_out = span - plateau_end;
    if ramp_out > 0.0 { (span - position) / ramp_out } else { 1.0 }
}

fn range_weight(params: &HueSaturationParams, range: ColorRange, hue: f64) -> f64 {
    if range == ColorRange::Master {
        return 1.0;
    }
    let weight = band_weight(&params.bands[range.index()], hue);
    if params.invert_range && range == params.range { 1.0 - weight } else { weight }
}

/// Every range's combined (hue shift, saturation, lightness) at each whole
/// degree 0–360. Built once per settings so a whole lookup table doesn't
/// re-evaluate all seven ranges for every entry.
pub fn hue_response(params: &HueSaturationParams) -> Vec<[f64; 3]> {
    (0..=360)
        .map(|degree| {
            let mut r = [0.0; 3];
            for range in ColorRange::ALL {
                let adj = params.adjustments[range.index()];
                if adj == RangeAdjustment::default() {
                    continue;
                }
                let w = range_weight(params, range, degree as f64);
                if w > 0.0 {
                    r[0] += adj.hue * w;
                    r[1] += adj.saturation * w;
                    r[2] += adj.lightness * w;
                }
            }
            r
        })
        .collect()
}

pub fn to_hsl(c: Rgb) -> [f64; 3] {
    let [r, g, b] = c;
    let high = r.max(g).max(b);
    let low = r.min(g).min(b);
    let lightness = (high + low) / 2.0;
    let delta = high - low;
    if delta <= 0.0 {
        return [0.0, 0.0, lightness];
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let mut hue = if high == r {
        (g - b) / delta
    } else if high == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    } * 60.0;
    if hue < 0.0 {
        hue += 360.0;
    }
    [hue, saturation.min(1.0), lightness]
}

pub fn to_rgb(hsl: [f64; 3]) -> Rgb {
    let [hue, saturation, lightness] = hsl;
    if saturation <= 0.0 {
        return [lightness; 3];
    }
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - ((sector % 2.0) - 1.0).abs());
    let base = lightness - chroma / 2.0;
    let (r, g, b) = match sector as i64 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    [clamp01(r + base), clamp01(g + base), clamp01(b + base)]
}

/// Hue/Saturation. `response` is [`hue_response`] for the same params.
pub fn hue_saturation(params: &HueSaturationParams, response: &[[f64; 3]], c: Rgb) -> Rgb {
    let [mut hue, mut saturation, mut lightness] = to_hsl(c);
    let lightness_amount;
    if params.colorize {
        let adj = params.adjustments[params.range.index()];
        hue = adj.hue % 360.0;
        saturation = (adj.saturation / 100.0).clamp(0.0, 1.0);
        lightness_amount = adj.lightness / 100.0;
    } else {
        let sampled = response[(hue.round() as usize).min(response.len() - 1)];
        lightness_amount = sampled[2] / 100.0;
        hue = (hue + sampled[0]).rem_euclid(360.0);
        // Multiplicative, so neutral grays stay neutral.
        saturation = (saturation * (1.0 + sampled[1] / 100.0)).clamp(0.0, 1.0);
    }
    // Lightness pulls toward white above 0 and black below, reaching either at ±100.
    let amount = lightness_amount.clamp(-1.0, 1.0);
    lightness = if amount >= 0.0 { lightness + (1.0 - lightness) * amount } else { lightness * (1.0 + amount) };
    to_rgb([hue, saturation, clamp01(lightness)])
}

// --- Black & White ---------------------------------------------------------

/// Black & White: a color is its gray (min channel), plus its secondary
/// (between its two brightest channels) and primary (its brightest), each
/// weighted by that color family's percentage.
pub fn black_white(params: &BlackWhiteParams, c: Rgb) -> Rgb {
    let w = params.weights().map(|v| v / 100.0);
    let [r, g, b] = c;
    let mx = r.max(g).max(b);
    let mn = r.min(g).min(b);
    let md = r + g + b - mx - mn;
    // weights: 0 red, 1 yellow, 2 green, 3 cyan, 4 blue, 5 magenta
    let (primary, secondary) = if mx == r {
        (0, if g >= b { 1 } else { 5 })
    } else if mx == g {
        (2, if r >= b { 1 } else { 3 })
    } else {
        (4, if g >= r { 3 } else { 5 })
    };
    let gray = clamp01(mn + (md - mn) * w[secondary] + (mx - md) * w[primary]);
    let tint_saturation = params.tint_saturation / 100.0;
    if !params.tint || tint_saturation <= 0.0 {
        return [gray; 3];
    }
    // The gray becomes the lightness of a color at the tint hue.
    let chroma = (1.0 - (2.0 * gray - 1.0).abs()) * tint_saturation;
    let hp = (params.tint_hue % 360.0) / 60.0;
    let x = chroma * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = if hp < 1.0 {
        (chroma, x, 0.0)
    } else if hp < 2.0 {
        (x, chroma, 0.0)
    } else if hp < 3.0 {
        (0.0, chroma, x)
    } else if hp < 4.0 {
        (0.0, x, chroma)
    } else if hp < 5.0 {
        (x, 0.0, chroma)
    } else {
        (chroma, 0.0, x)
    };
    let m = gray - chroma / 2.0;
    [clamp01(r1 + m), clamp01(g1 + m), clamp01(b1 + m)]
}

// --- Color Balance ---------------------------------------------------------

/// How much a tone belongs to the shadows, midtones and highlights:
/// overlapping ramps summing to about one, so a shift fades rather than bands.
fn tonal_weights(v: f64) -> [f64; 3] {
    let (a, b, scale) = (0.25, 0.333, 0.7);
    let s = ((v - b) / -a + 0.5).clamp(0.0, 1.0);
    let h = ((v + b - 1.0) / a + 0.5).clamp(0.0, 1.0);
    let m1 = ((v - b) / a + 0.5).clamp(0.0, 1.0);
    let m2 = ((v + b - 1.0) / -a + 0.5).clamp(0.0, 1.0);
    [s * scale, m1 * m2 * scale, h * scale]
}

fn luma(c: Rgb) -> f64 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

pub fn color_balance(params: &ColorBalanceParams, c: Rgb) -> Rgb {
    let before = luma(c);
    let mut out: Rgb = std::array::from_fn(|i| {
        let [s, m, h] = tonal_weights(c[i]);
        clamp01(c[i] + (params.shadows[i] * s + params.midtones[i] * m + params.highlights[i] * h) / 100.0)
    });
    if params.preserve_luminosity {
        let after = luma(out);
        if after > 0.0001 {
            let ratio = before / after;
            out = out.map(|v| clamp01(v * ratio));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn levels_matches_the_upstream_formula() {
        let mut p = LevelsParams::default();
        p.ranges[0] = LevelRange { black: 20.0, gamma: 1.5, white: 230.0, output_black: 0.0, output_white: 255.0 };
        // (125 − 20) / 210 = 0.5; 0.5^(1/1.5) = 0.629961; × 255 = 160.64
        let out = levels(&p, [125.0 / 255.0; 3]);
        assert!(close(out[0] * 255.0, 160.640, 0.01), "{}", out[0] * 255.0);
        // Below black clips to the output black; above white to the output white.
        assert_eq!(levels(&p, [10.0 / 255.0; 3])[0], 0.0);
        assert_eq!(levels(&p, [240.0 / 255.0; 3])[0], 1.0);
    }

    #[test]
    fn levels_applies_the_channel_range_before_the_composite() {
        let mut p = LevelsParams::default();
        p.ranges[1].output_white = 127.5; // red halves
        p.ranges[0].black = 0.0;
        p.ranges[0].white = 127.5; // composite doubles
        let out = levels(&p, [1.0, 0.5, 0.25]);
        assert!(close(out[0], 1.0, 1e-9), "red: halved then doubled");
        assert!(close(out[1], 1.0, 1e-9), "green: doubled and clipped");
        assert!(close(out[2], 0.5, 1e-9));
    }

    #[test]
    fn curves_pass_through_their_points_and_identity_is_exact() {
        let mut p = CurvesParams::default();
        for i in 0..=255 {
            let v = i as f64 / 255.0;
            assert!(close(curves(&p, [v; 3])[0], v, 1e-12));
        }
        p.channels[0] = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 64.0, y: 32.0 },
            CurvePoint { x: 192.0, y: 224.0 },
            CurvePoint { x: 255.0, y: 255.0 },
        ];
        assert!(close(curve_value(&p.channels[0], 64.0), 32.0, 1e-9));
        assert!(close(curve_value(&p.channels[0], 192.0), 224.0, 1e-9));
        // Monotone points never overshoot between them.
        for x in 0..=255 {
            let y = curve_value(&p.channels[0], x as f64);
            assert!((0.0..=255.0).contains(&y));
        }
    }

    #[test]
    fn exposure_one_stop_brightens_mid_gray_through_linear_light() {
        let p = ExposureParams { exposure: 1.0, ..Default::default() };
        let out = exposure_channel(&p, 128.0 / 255.0) * 255.0;
        // 128 → linear 0.215861 → ×2 = 0.431723 → sRGB → 175.5556
        assert!(close(out, 175.5556, 0.001), "{out}");
        assert!(close(exposure_channel(&ExposureParams::default(), 0.3), 0.3, 1e-12));
    }

    #[test]
    fn hue_shift_of_120_turns_red_green() {
        let mut p = HueSaturationParams::default();
        p.adjustments[0].hue = 120.0;
        let r = hue_response(&p);
        let out = hue_saturation(&p, &r, [1.0, 0.0, 0.0]);
        assert!(close(out[0], 0.0, 1e-9) && close(out[1], 1.0, 1e-9) && close(out[2], 0.0, 1e-9), "{out:?}");
        // Grays have no hue to shift and no saturation to scale.
        assert_eq!(hue_saturation(&p, &r, [0.4; 3]), [0.4; 3]);
    }

    #[test]
    fn hue_ranges_only_touch_their_band() {
        let mut p = HueSaturationParams::default();
        p.adjustments[ColorRange::Blues.index()].saturation = -100.0;
        let r = hue_response(&p);
        let blue = hue_saturation(&p, &r, [0.0, 0.0, 1.0]);
        assert!(close(blue[0], blue[1], 1e-9) && close(blue[1], blue[2], 1e-9), "blue desaturates: {blue:?}");
        assert_eq!(hue_saturation(&p, &r, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0], "red is untouched");
    }

    #[test]
    fn black_white_defaults_match_photoshop() {
        let p = BlackWhiteParams::default();
        assert!(close(black_white(&p, [1.0, 0.0, 0.0])[0], 0.40, 1e-9), "pure red at 40%");
        assert!(close(black_white(&p, [0.0, 0.0, 1.0])[0], 0.20, 1e-9), "pure blue at 20%");
        assert!(close(black_white(&p, [1.0, 1.0, 0.0])[0], 0.60, 1e-9), "pure yellow at 60%");
        assert_eq!(black_white(&p, [0.5; 3]), [0.5; 3], "grays stay put");
    }

    #[test]
    fn color_balance_preserves_luminosity_when_asked() {
        let mut p = ColorBalanceParams::default();
        p.midtones = [50.0, 0.0, 0.0];
        let c = [0.5, 0.5, 0.5];
        let out = color_balance(&p, c);
        assert!(out[0] > out[1], "midtones shift toward red");
        assert!(close(luma(out), luma(c), 1e-9));
        p.preserve_luminosity = false;
        assert!(luma(color_balance(&p, c)) > luma(c));
    }

    #[test]
    fn invert_flips_each_channel() {
        assert_eq!(invert([0.0, 0.25, 1.0]), [1.0, 0.75, 0.0]);
    }
}
