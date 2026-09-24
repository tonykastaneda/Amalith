//! Blend modes for combining an adjusted color with the color beneath it,
//! per the W3C Compositing and Blending spec — the same formulas vello's
//! `blend.wgsl` implements, so an adjustment's blend matches every other
//! blend in Amalith. Mirrored by the shell's `color.wgsl`.

use amalith_core::appearance::BlendMode;

type Rgb = [f32; 3];

fn separable(mode: BlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        BlendMode::Overlay => separable(BlendMode::HardLight, cs, cb),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if cs <= 0.5 {
                cb * 2.0 * cs
            } else {
                let s = 2.0 * cs - 1.0;
                cb + s - cb * s
            }
        }
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => cs,
    }
}

fn lum(c: Rgb) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        out = out.map(|v| l + (v - l) * l / (l - n).max(1e-7));
    }
    if x > 1.0 {
        out = out.map(|v| l + (v - l) * (1.0 - l) / (x - l).max(1e-7));
    }
    out
}

fn set_lum(c: Rgb, l: f32) -> Rgb {
    let d = l - lum(c);
    clip_color(c.map(|v| v + d))
}

fn sat(c: Rgb) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: Rgb, s: f32) -> Rgb {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    if mx > mn {
        c.map(|v| (v - mn) * s / (mx - mn))
    } else {
        [0.0; 3]
    }
}

/// `backdrop` is the color beneath, `source` the adjusted color.
pub fn blend(mode: BlendMode, backdrop: Rgb, source: Rgb) -> Rgb {
    match mode {
        BlendMode::Normal => source,
        BlendMode::Hue => set_lum(set_sat(source, sat(backdrop)), lum(backdrop)),
        BlendMode::Saturation => set_lum(set_sat(backdrop, sat(source)), lum(backdrop)),
        BlendMode::Color => set_lum(source, lum(backdrop)),
        BlendMode::Luminosity => set_lum(backdrop, lum(source)),
        _ => std::array::from_fn(|k| separable(mode, backdrop[k], source[k])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separable_modes_follow_the_spec() {
        let (b, s) = ([0.2, 0.5, 0.8], [0.6, 0.5, 0.1]);
        assert_eq!(blend(BlendMode::Normal, b, s), s);
        let m = blend(BlendMode::Multiply, b, s);
        assert!((m[0] - 0.12).abs() < 1e-6);
        let d = blend(BlendMode::Difference, b, s);
        assert!((d[2] - 0.7).abs() < 1e-6);
        let sc = blend(BlendMode::Screen, b, s);
        assert!((sc[0] - (0.2 + 0.6 - 0.12)).abs() < 1e-6);
    }

    #[test]
    fn luminosity_keeps_the_backdrop_color_and_takes_the_source_lightness() {
        let out = blend(BlendMode::Luminosity, [0.8, 0.2, 0.2], [0.5, 0.5, 0.5]);
        assert!((lum(out) - 0.5).abs() < 1e-5);
        assert!(out[0] > out[1], "still reddish");
    }
}
