//! ICC-profile-based color management (Little CMS, via the `lcms2`
//! crate) for print-accurate CMYK.
//!
//! [`amalith_core::Color::to_cmyk`]/`from_cmyk` — the naive device
//! formula (`k = 1 - max(r,g,b)`, etc.) — stays the always-available
//! fallback everywhere in this app, not something this module replaces:
//! without an actual destination profile there is no way to know what
//! press, paper stock, or total-ink limit a conversion should target, so
//! pretending otherwise would be less honest than a clearly-labeled
//! approximation. What this module adds is the *real* conversion once a
//! user loads an actual ICC profile — a print shop hands you one
//! directly, or one ships with a printer's driver — swapping Little CMS's
//! standards-compliant transform in for both the Color panel's live
//! preview and the PDF exporter's output (see `pdfexport.rs`).
//!
//! Little CMS is the open-source, industry-standard CMM (color
//! management module): the engine behind GIMP, Krita, Scribus, and
//! ImageMagick's color management, and the reference most ICC compliance
//! testing is measured against — not a newer or narrower reimplementation
//! that hasn't seen the same real-world mileage. `Cargo.toml` builds it
//! with the `static` feature so the shipped app links its own copy
//! rather than depending on the user's machine having one installed.
use amalith_core::Color;
use lcms2::{ColorSpaceSignature, InfoType, Intent, Locale, PixelFormat, Profile, Transform};

/// A loaded destination CMYK ICC profile, plus the two transforms
/// through it to/from this app's on-screen working space — every
/// [`Color`] in this app is authored and displayed as sRGB, so that's
/// the fixed source/target on the RGB side of both transforms.
#[derive(Debug)]
pub struct CmykProfile {
    icc_bytes: Vec<u8>,
    name: String,
    to_cmyk: Transform<[f32; 3], [f32; 4]>,
    to_rgb: Transform<[f32; 4], [f32; 3]>,
}

impl CmykProfile {
    /// Loads and validates an ICC profile from raw bytes (a `.icc`/`.icm`
    /// file the user picked). Errors — with a message fit to surface to
    /// the user directly — if the bytes aren't a valid ICC profile, or
    /// the profile isn't a CMYK one (an RGB or Lab profile can't be used
    /// as a *destination* here; picking the wrong file is an easy
    /// mistake, so this is checked explicitly rather than failing later
    /// with an opaque transform error).
    pub fn load(bytes: Vec<u8>) -> Result<Self, String> {
        let dst = Profile::new_icc(&bytes).map_err(|e| format!("Not a valid ICC profile: {e}"))?;
        if dst.color_space() != ColorSpaceSignature::CmykData {
            return Err(format!(
                "That profile's color space is {:?}, not CMYK — pick a CMYK output/printer profile.",
                dst.color_space()
            ));
        }
        let name = dst
            .info(InfoType::Description, Locale::none())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "CMYK Profile".to_string());
        let srgb = Profile::new_srgb();
        let to_cmyk = Transform::new(
            &srgb,
            PixelFormat::RGB_FLT,
            &dst,
            PixelFormat::CMYK_FLT,
            Intent::RelativeColorimetric,
        )
        .map_err(|e| format!("Couldn't build the RGB\u{2192}CMYK transform: {e}"))?;
        let to_rgb = Transform::new(
            &dst,
            PixelFormat::CMYK_FLT,
            &srgb,
            PixelFormat::RGB_FLT,
            Intent::RelativeColorimetric,
        )
        .map_err(|e| format!("Couldn't build the CMYK\u{2192}RGB transform: {e}"))?;
        Ok(Self {
            icc_bytes: bytes,
            name,
            to_cmyk,
            to_rgb,
        })
    }

    /// Reads and loads the profile at `path`.
    pub fn from_path(path: &std::path::Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
        Self::load(bytes)
    }

    /// The profile's own embedded description (e.g. "U.S. Web Coated
    /// (SWOP) v2") — shown wherever the active profile needs to be named
    /// back to the user, so it's never a mystery which one is in effect.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The raw ICC bytes, for embedding as the PDF exporter's own
    /// `ICCBased` color space — an exported file then carries the exact
    /// profile its numbers are relative to, rather than leaving a RIP or
    /// viewer to guess via its own default CMYK assumption.
    pub fn icc_bytes(&self) -> &[u8] {
        &self.icc_bytes
    }

    /// `c`'s CMYK equivalent under this profile, `[c, m, y, k]` in
    /// `0.0..=1.0`. Clamped defensively: Little CMS can return values
    /// slightly outside that range for an out-of-gamut input, but every
    /// consumer here expects a clean percentage.
    ///
    /// Little CMS's `CMYK_FLT` pixel format is, perhaps surprisingly,
    /// scaled `0..100` (a percentage) rather than `0..1` — every other
    /// float format it has, RGB included, *is* `0..1`; ink channels are
    /// the one deliberate exception (see lcms2's `cmspack.c`,
    /// `IsInkSpace(...) ? 100.0 : 1.0`). Converting to/from this app's
    /// `0..1` convention on the way in and out of the transform is what
    /// the `/ 100.0` and `* 100.0` below are for — dropping them doesn't
    /// error, it just silently produces numbers off by 100x, which is
    /// exactly the kind of bug that would quietly defeat the entire
    /// point of using a real CMM instead of the naive formula.
    pub fn rgb_to_cmyk(&self, c: Color) -> [f32; 4] {
        let src = [[c.r, c.g, c.b]];
        let mut dst = [[0.0f32; 4]];
        self.to_cmyk.transform_pixels(&src, &mut dst);
        dst[0].map(|v| (v / 100.0).clamp(0.0, 1.0))
    }

    /// The inverse of [`Self::rgb_to_cmyk`] — not a perfect round trip in
    /// general (multiple CMYK combinations, e.g. any "rich black", can
    /// map to the same RGB, so recovering the original exactly isn't
    /// always possible), but accurate for whatever RGB this profile's
    /// rendering intent would actually produce from that ink mix. See
    /// `rgb_to_cmyk`'s doc comment for why `* 100.0` is needed here too.
    pub fn cmyk_to_rgb(&self, cmyk: [f32; 4]) -> Color {
        let src = [cmyk.map(|v| v * 100.0)];
        let mut dst = [[0.0f32; 3]];
        self.to_rgb.transform_pixels(&src, &mut dst);
        Color::rgb(dst[0][0].clamp(0.0, 1.0), dst[0][1].clamp(0.0, 1.0), dst[0][2].clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real end-to-end check against an actual ICC profile already on
    /// this machine (shipped with macOS) — proves Little CMS is really
    /// linked, initialized, and producing sane output, not just that the
    /// Rust plumbing compiles. Skips (doesn't fail) off macOS, where that
    /// path doesn't exist.
    #[test]
    fn loads_a_real_cmyk_profile_and_round_trips_a_neutral_gray() {
        let path = std::path::Path::new("/System/Library/ColorSync/Profiles/Generic CMYK Profile.icc");
        if !path.exists() {
            eprintln!("skipping: no system CMYK profile at {path:?} (not on macOS?)");
            return;
        }
        let profile = CmykProfile::from_path(path).expect("load the system CMYK profile");
        assert!(!profile.name().is_empty());

        let mid_gray = Color::rgb(0.5, 0.5, 0.5);
        let cmyk = profile.rgb_to_cmyk(mid_gray);
        for v in cmyk {
            assert!((0.0..=1.0).contains(&v), "{cmyk:?} out of range");
        }
        // K should dominate for a neutral gray (this is what tells a real
        // profile-based conversion apart from doing nothing at all).
        assert!(cmyk[3] > 0.0, "a mid gray should carry some black: {cmyk:?}");

        let back = profile.cmyk_to_rgb(cmyk);
        // Not an exact round trip (gamut mapping / rendering intent), but
        // an in-gamut neutral gray should stay in the ballpark.
        assert!((back.r - mid_gray.r).abs() < 0.25, "{back:?}");
        assert!((back.g - mid_gray.g).abs() < 0.25, "{back:?}");
        assert!((back.b - mid_gray.b).abs() < 0.25, "{back:?}");
    }

    /// No external file needed: an sRGB profile re-serialized to bytes is
    /// always available, and is exactly the "wrong kind of profile" case
    /// (picked an RGB profile instead of a CMYK one) the loader should
    /// reject with a clear reason rather than a confusing later failure.
    #[test]
    fn rejects_a_non_cmyk_profile() {
        let bytes = Profile::new_srgb().icc().expect("serialize the synthetic sRGB profile");
        let err = CmykProfile::load(bytes).unwrap_err();
        assert!(err.contains("CMYK"), "{err}");
    }
}
