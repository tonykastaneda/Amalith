//! Compiling a color adjustment into a lookup table, and sampling it.
//!
//! Every color op becomes one table so a single GPU shader can apply any of
//! them; the color math never has to be written twice.
//! - Per-channel ops (Levels, Curves, Exposure, Invert) become three
//!   256-entry tables. Inputs are 8-bit, so these are exact.
//! - Ops that mix channels (Hue/Saturation, Color Balance, Black & White)
//!   become an `n`³ color cube, sampled with tetrahedral interpolation —
//!   the same code in Rust here and in WGSL, so CPU and GPU agree.

use amalith_core::AdjustmentOp;

use crate::color::{self, Rgb};

/// Points per axis of a color cube. Upstream's Hue/Saturation uses 33,
/// but at 33 a hue shift is off by up to 12.7/255 on some colors (1.7% of
/// them past 2/255); 65 brings that to about 5/255. A cube is 65³ × 16
/// bytes ≈ 4.4 MB, built once per settings change.
pub const CUBE_N: usize = 65;

#[derive(Debug, Clone, PartialEq)]
pub enum ColorLut {
    /// `table[channel][input byte]`, output in 0–1.
    OneD(Box<[[f32; 256]; 3]>),
    /// `data[(b * n + g) * n + r]` = the adjusted color at that grid point
    /// (RGB in 0–1; the fourth lane is padding for GPU alignment).
    Cube { n: usize, data: Vec<[f32; 4]> },
}

/// Compiles a color op. `None` for spatial ops (blur, noise), which are
/// not per-color.
pub fn compile(op: &AdjustmentOp) -> Option<ColorLut> {
    let one_d = |f: &dyn Fn(Rgb) -> Rgb| {
        let mut table = Box::new([[0f32; 256]; 3]);
        for i in 0..256 {
            let v = i as f64 / 255.0;
            let out = f([v; 3]);
            for c in 0..3 {
                table[c][i] = out[c] as f32;
            }
        }
        ColorLut::OneD(table)
    };
    let cube = |f: &dyn Fn(Rgb) -> Rgb| {
        let n = CUBE_N;
        let step = (n - 1) as f64;
        let mut data = Vec::with_capacity(n * n * n);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    let out = f([r as f64 / step, g as f64 / step, b as f64 / step]);
                    data.push([out[0] as f32, out[1] as f32, out[2] as f32, 1.0]);
                }
            }
        }
        ColorLut::Cube { n, data }
    };
    Some(match op {
        AdjustmentOp::Levels(p) => one_d(&|c| color::levels(p, c)),
        AdjustmentOp::Curves(p) => one_d(&|c| color::curves(p, c)),
        AdjustmentOp::Exposure(p) => one_d(&|c| color::exposure(p, c)),
        AdjustmentOp::Invert => one_d(&color::invert),
        AdjustmentOp::HueSaturation(p) => {
            let response = color::hue_response(p);
            cube(&|c| color::hue_saturation(p, &response, c))
        }
        AdjustmentOp::ColorBalance(p) => cube(&|c| color::color_balance(p, c)),
        AdjustmentOp::BlackWhite(p) => cube(&|c| color::black_white(p, c)),
        AdjustmentOp::GaussianBlur(_) | AdjustmentOp::MotionBlur(_) | AdjustmentOp::AddNoise(_) => return None,
    })
}

/// The exact function a color op's table approximates — what [`compile`]
/// samples. `None` for spatial ops.
pub fn exact(op: &AdjustmentOp, c: Rgb) -> Option<Rgb> {
    Some(match op {
        AdjustmentOp::Levels(p) => color::levels(p, c),
        AdjustmentOp::Curves(p) => color::curves(p, c),
        AdjustmentOp::Exposure(p) => color::exposure(p, c),
        AdjustmentOp::Invert => color::invert(c),
        AdjustmentOp::HueSaturation(p) => color::hue_saturation(p, &color::hue_response(p), c),
        AdjustmentOp::ColorBalance(p) => color::color_balance(p, c),
        AdjustmentOp::BlackWhite(p) => color::black_white(p, c),
        _ => return None,
    })
}

impl ColorLut {
    /// Looks up a straight 8-bit color. Output channels are 0–1.
    pub fn sample(&self, rgb: [u8; 3]) -> [f32; 3] {
        match self {
            ColorLut::OneD(t) => [t[0][rgb[0] as usize], t[1][rgb[1] as usize], t[2][rgb[2] as usize]],
            ColorLut::Cube { n, data } => {
                tetrahedral(*n, data, rgb.map(|v| v as f32 / 255.0))
            }
        }
    }
}

/// Tetrahedral interpolation in an `n`³ cube. Mirrored exactly by
/// `sample_cube` in the shell's `color.wgsl`.
pub fn tetrahedral(n: usize, data: &[[f32; 4]], c: [f32; 3]) -> [f32; 3] {
    let max = (n - 1) as f32;
    let mut i0 = [0usize; 3];
    let mut f = [0f32; 3];
    for k in 0..3 {
        let x = c[k].clamp(0.0, 1.0) * max;
        let i = (x.floor() as usize).min(n - 2);
        i0[k] = i;
        f[k] = x - i as f32;
    }
    let at = |dr: usize, dg: usize, db: usize| -> [f32; 3] {
        let v = data[((i0[2] + db) * n + (i0[1] + dg)) * n + (i0[0] + dr)];
        [v[0], v[1], v[2]]
    };
    let (fr, fg, fb) = (f[0], f[1], f[2]);
    let c000 = at(0, 0, 0);
    let c111 = at(1, 1, 1);
    // Walk from c000 to c111 along the three edges in decreasing order of
    // the fractional parts; each case is one of the cube's six tetrahedra.
    let (a, b, wa, wb, wc) = if fr > fg {
        if fg > fb {
            (at(1, 0, 0), at(1, 1, 0), fr, fg, fb)
        } else if fr > fb {
            (at(1, 0, 0), at(1, 0, 1), fr, fb, fg)
        } else {
            (at(0, 0, 1), at(1, 0, 1), fb, fr, fg)
        }
    } else if fb > fg {
        (at(0, 0, 1), at(0, 1, 1), fb, fg, fr)
    } else if fb > fr {
        (at(0, 1, 0), at(0, 1, 1), fg, fb, fr)
    } else {
        (at(0, 1, 0), at(1, 1, 0), fg, fr, fb)
    };
    std::array::from_fn(|k| c000[k] + wa * (a[k] - c000[k]) + wb * (b[k] - a[k]) + wc * (c111[k] - b[k]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{
        AdjustmentKind, BlackWhiteParams, ColorBalanceParams, ColorRange, CurvePoint, CurvesParams, ExposureParams,
        HueSaturationParams, LevelRange, LevelsParams,
    };

    /// A tiny deterministic generator so the tests need no dependency.
    struct Rng(u64);
    impl Rng {
        fn byte(&mut self) -> u8 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 24) as u8
        }
    }

    fn one_d_ops() -> Vec<AdjustmentOp> {
        let mut levels = LevelsParams::default();
        levels.ranges[0] = LevelRange { black: 12.0, gamma: 0.8, white: 240.0, output_black: 5.0, output_white: 250.0 };
        levels.ranges[2].gamma = 1.7;
        let mut curves = CurvesParams::default();
        curves.channels[0] = vec![
            CurvePoint { x: 0.0, y: 0.0 },
            CurvePoint { x: 90.0, y: 60.0 },
            CurvePoint { x: 170.0, y: 200.0 },
            CurvePoint { x: 255.0, y: 255.0 },
        ];
        vec![
            AdjustmentOp::Levels(levels),
            AdjustmentOp::Curves(curves),
            AdjustmentOp::Exposure(ExposureParams { exposure: 0.7, offset: -0.02, gamma: 1.2 }),
            AdjustmentOp::Invert,
        ]
    }

    fn cube_ops() -> Vec<AdjustmentOp> {
        let mut hs = HueSaturationParams::default();
        hs.adjustments[0] = amalith_core::RangeAdjustment { hue: 25.0, saturation: 30.0, lightness: -10.0 };
        let mut reds = HueSaturationParams::default();
        reds.adjustments[ColorRange::Reds.index()].hue = -40.0;
        reds.adjustments[ColorRange::Reds.index()].saturation = -60.0;
        let mut colorize = HueSaturationParams::default();
        colorize.colorize = true;
        colorize.adjustments[0] = amalith_core::RangeAdjustment { hue: 30.0, saturation: 25.0, lightness: 0.0 };
        let mut cb = ColorBalanceParams::default();
        cb.shadows = [20.0, -10.0, 30.0];
        cb.midtones = [-25.0, 15.0, 0.0];
        cb.highlights = [10.0, 0.0, -35.0];
        let mut bw = BlackWhiteParams::default();
        bw.tint = true;
        vec![
            AdjustmentOp::HueSaturation(hs),
            AdjustmentOp::HueSaturation(reds),
            AdjustmentOp::HueSaturation(colorize),
            AdjustmentOp::ColorBalance(cb),
            AdjustmentOp::BlackWhite(BlackWhiteParams::default()),
            AdjustmentOp::BlackWhite(bw),
        ]
    }

    #[test]
    fn one_d_tables_match_the_exact_function_to_float_precision() {
        for op in one_d_ops() {
            let lut = compile(&op).unwrap();
            assert!(matches!(lut, ColorLut::OneD(_)));
            for v in 0..=255u8 {
                for rgb in [[v, 0, 0], [0, v, 0], [0, 0, v], [v, v, v]] {
                    let got = lut.sample(rgb);
                    let want = exact(&op, rgb.map(|x| x as f64 / 255.0)).unwrap();
                    for k in 0..3 {
                        assert!((got[k] as f64 - want[k]).abs() < 1e-6, "{op:?} {rgb:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn identity_params_leave_every_byte_unchanged() {
        for kind in [AdjustmentKind::Levels, AdjustmentKind::Curves, AdjustmentKind::Exposure] {
            let lut = compile(&kind.default_op()).unwrap();
            for v in 0..=255u8 {
                let out = lut.sample([v, v, v]);
                assert_eq!((out[0] * 255.0).round() as u8, v, "{kind:?}");
            }
        }
        for kind in [AdjustmentKind::HueSaturation, AdjustmentKind::ColorBalance] {
            let lut = compile(&kind.default_op()).unwrap();
            let mut rng = Rng(0x9E3779B97F4A7C15);
            for _ in 0..20_000 {
                let rgb = [rng.byte(), rng.byte(), rng.byte()];
                let out = lut.sample(rgb);
                for k in 0..3 {
                    assert!(((out[k] * 255.0).round() as i32 - rgb[k] as i32).abs() <= 1, "{kind:?} {rgb:?} {out:?}");
                }
            }
        }
    }

    #[test]
    fn cube_tables_stay_close_to_the_exact_function() {
        const SAMPLES: usize = 100_000;
        for op in cube_ops() {
            let lut = compile(&op).unwrap();
            let mut rng = Rng(0x2545F4914F6CDD1D);
            let (mut worst, mut over) = (0f64, 0usize);
            for _ in 0..SAMPLES {
                let rgb = [rng.byte(), rng.byte(), rng.byte()];
                let got = lut.sample(rgb);
                let want = exact(&op, rgb.map(|x| x as f64 / 255.0)).unwrap();
                let err = (0..3).map(|k| (got[k] as f64 - want[k]).abs()).fold(0.0, f64::max) * 255.0;
                worst = worst.max(err);
                if err > 2.0 {
                    over += 1;
                }
            }
            eprintln!("{:?}: worst {worst:.2}/255, {over} of {SAMPLES} over 2/255", op.kind());
            // A cube can't follow a hue band's shoulder exactly (the exact
            // function itself steps once per degree there); what matters is
            // that nearly every color is within 2/255 and none is far off.
            assert!(over * 100 <= SAMPLES, "{op:?}: {over} samples over 2/255");
            assert!(worst <= 6.0, "{op:?}: worst {worst}");
        }
    }

    #[test]
    fn tetrahedral_hits_grid_points_exactly() {
        let op = cube_ops().remove(0);
        let ColorLut::Cube { n, data } = compile(&op).unwrap() else { panic!() };
        let step = (n - 1) as f32;
        for (b, g, r) in [(0, 0, 0), (5, 17, 31), (32, 32, 32), (32, 0, 16)] {
            let got = tetrahedral(n, &data, [r as f32 / step, g as f32 / step, b as f32 / step]);
            let want = data[(b * n + g) * n + r];
            for k in 0..3 {
                assert!((got[k] - want[k]).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn spatial_ops_have_no_table() {
        for kind in [AdjustmentKind::GaussianBlur, AdjustmentKind::MotionBlur, AdjustmentKind::AddNoise] {
            assert!(compile(&kind.default_op()).is_none());
        }
    }
}
