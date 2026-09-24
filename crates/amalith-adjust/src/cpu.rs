//! The CPU reference of the GPU color pass: exactly what the shell's
//! `color.wgsl` computes, used to check the shader and for tests.

use amalith_core::appearance::BlendMode;

use crate::blend::blend;
use crate::lut::ColorLut;

/// Applies one color adjustment to straight-alpha RGBA8 pixels in place:
/// `out = mix(beneath, blend(beneath, lut(beneath)), opacity × mask)`, with
/// each pixel's alpha kept — Photoshop's adjustment-layer semantics.
///
/// `mask` has one coverage byte per pixel (255 applies fully), or `None`
/// for everywhere. Fully transparent pixels are left alone.
pub fn apply(pixels: &mut [u8], lut: &ColorLut, blend_mode: BlendMode, opacity: f32, mask: Option<&[u8]>) {
    for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
        if px[3] == 0 {
            continue;
        }
        let t = opacity.clamp(0.0, 1.0) * mask.map_or(1.0, |m| m[i] as f32 / 255.0);
        if t <= 0.0 {
            continue;
        }
        let beneath = [px[0], px[1], px[2]].map(|v| v as f32 / 255.0);
        let adjusted = lut.sample([px[0], px[1], px[2]]);
        let blended = blend(blend_mode, beneath, adjusted);
        for k in 0..3 {
            let v = beneath[k] + (blended[k] - beneath[k]) * t;
            px[k] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::AdjustmentOp;

    fn invert() -> ColorLut {
        crate::lut::compile(&AdjustmentOp::Invert).unwrap()
    }

    #[test]
    fn full_strength_inverts_and_keeps_alpha() {
        let mut px = [10, 100, 200, 128, 0, 0, 0, 0];
        apply(&mut px, &invert(), BlendMode::Normal, 1.0, None);
        assert_eq!(px, [245, 155, 55, 128, 0, 0, 0, 0], "transparent pixels untouched");
    }

    #[test]
    fn opacity_and_mask_mix_with_the_color_beneath() {
        let mut px = [0, 0, 0, 255, 0, 0, 0, 255];
        apply(&mut px, &invert(), BlendMode::Normal, 0.5, Some(&[255, 0]));
        assert_eq!(&px[..4], &[128, 128, 128, 255], "half opacity, fully masked in");
        assert_eq!(&px[4..], &[0, 0, 0, 255], "masked out");
    }

    #[test]
    fn blend_mode_combines_before_the_mix() {
        // Invert with Multiply: c × (1 − c); at 0.5 that's 0.25.
        let mut px = [128, 128, 128, 255];
        apply(&mut px, &invert(), BlendMode::Multiply, 1.0, None);
        assert!((px[0] as i32 - 64).abs() <= 1, "{px:?}");
    }
}
