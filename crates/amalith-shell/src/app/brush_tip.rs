//! Soft-tip falloff adapted from Compositor's BrushRaster.falloff,
//! Document/BrushStroke.swift at 37dbe59b3cf71184b016e4f2f4aa74eada533aec.
//! Copyright (c) 2026 Wonder Assembly LLC. Licensed under MIT;
//! see third_party/compositor/LICENSE and NOTICE.md at the workspace root.

/// Normalized Gaussian: solid at the hardness radius, zero at the rim.
fn falloff(u: f64) -> f64 {
    let u = u.clamp(0.0, 1.0);
    let edge = (-2.5_f64).exp();
    ((-2.5 * u * u).exp() - edge).max(0.0) / (1.0 - edge)
}

/// Amalith's pixel-center coverage, keeping the existing antialiased hard
/// tip at 100% hardness. Inputs are measured in source-image pixels.
pub(super) fn coverage(distance: f64, radius: f64, hardness: f64) -> f64 {
    if !distance.is_finite() || !radius.is_finite() || !hardness.is_finite() || radius <= 0.0 {
        return 0.0;
    }
    let hardness = hardness.clamp(0.0, 1.0);
    let edge = (radius + 0.5 - distance).clamp(0.0, 1.0);
    if hardness == 1.0 { return edge; }
    let inner = radius * hardness;
    if distance <= inner { return edge; }
    edge * falloff((distance - inner) / (radius - inner))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gaussian_matches_upstream_reference() {
        // exp(-2.5 * .5^2), normalized by the upstream rim value.
        assert!((falloff(0.5) - 0.49370195411961637).abs() < 1e-12);
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(1.0), 0.0);
    }
    #[test]
    fn soft_tip_has_a_solid_center_and_monotone_fade() {
        assert_eq!(coverage(4.0, 10.0, 0.5), 1.0);
        assert!(coverage(7.0, 10.0, 0.5) > coverage(9.0, 10.0, 0.5));
        assert_eq!(coverage(10.0, 10.0, 0.5), 0.0);
        assert_eq!(coverage(0.0, 10.0, 0.0), 1.0);
        assert_eq!(coverage(0.0, 0.0, 0.5), 0.0);
    }
    #[test]
    fn hard_tip_preserves_existing_pixel_coverage() {
        for i in 0..120 {
            let distance = i as f64 / 10.0;
            assert_eq!(coverage(distance, 10.0, 1.0), (10.5 - distance).clamp(0.0, 1.0));
        }
    }
}
