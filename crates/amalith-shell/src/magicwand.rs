//! Magic Wand: flood-fill a raster image by color similarity, then trace
//! the resulting mask into closed polygons for the "marching ants"
//! selection overlay (and, later, an actual pixel-mask/cutout operation).
//! Deliberately pure and UI-framework-agnostic — no `vello`/`winit`
//! dependency beyond `kurbo::Point` for the output shape, so it can be
//! unit-tested against plain bitmaps.

use std::collections::HashMap;

use image::RgbaImage;
use vello::kurbo::Point;

/// A boolean selection mask over an image's pixel grid.
pub struct Mask {
    width: u32,
    height: u32,
    bits: Vec<bool>,
}

impl Mask {
    fn new(width: u32, height: u32) -> Self {
        Self { width, height, bits: vec![false; width as usize * height as usize] }
    }

    /// Out-of-bounds always reads as unselected — lets boundary code treat
    /// the mask's own edge the same as an interior edge, with no special
    /// casing.
    pub(crate) fn get(&self, x: i64, y: i64) -> bool {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            false
        } else {
            self.bits[y as usize * self.width as usize + x as usize]
        }
    }

    fn set(&mut self, x: u32, y: u32) {
        self.bits[y as usize * self.width as usize + x as usize] = true;
    }
}

/// Euclidean RGB distance between two pixels (alpha handled separately —
/// see [`flood_fill`]).
fn color_distance(a: [u8; 4], b: [u8; 4]) -> f64 {
    let dr = a[0] as f64 - b[0] as f64;
    let dg = a[1] as f64 - b[1] as f64;
    let db = a[2] as f64 - b[2] as f64;
    (dr * dr + dg * dg + db * db).sqrt()
}

/// 4-connected flood fill from `seed`, matching every reachable pixel
/// within `tolerance` (Euclidean RGB distance, `0.0..=441.67`) of the
/// seed pixel's own color. A fully transparent seed only matches other
/// fully transparent pixels (color is meaningless once alpha is zero,
/// and treating them as a color-distance-0 group keeps a click on empty
/// canvas area from bleeding into unrelated fully-opaque content); a
/// fully transparent pixel never matches a non-transparent seed either
/// way. Returns an all-`false` mask if `seed` is outside the image.
pub fn flood_fill(img: &RgbaImage, seed: (u32, u32), tolerance: f64) -> Mask {
    flood_fill_masked(img, seed, tolerance, |_, _| true)
}

/// Selection boundaries stop propagation, rather than just hiding the
/// result of a flood that already crossed unselected pixels.
pub(crate) fn flood_fill_masked(img: &RgbaImage, seed: (u32, u32), tolerance: f64, allowed: impl Fn(u32, u32) -> bool) -> Mask {
    let (w, h) = img.dimensions();
    let mut mask = Mask::new(w, h);
    if seed.0 >= w || seed.1 >= h || !allowed(seed.0, seed.1) {
        return mask;
    }
    let seed_px = img.get_pixel(seed.0, seed.1).0;
    let seed_transparent = seed_px[3] == 0;
    let matches = |px: [u8; 4]| -> bool {
        let transparent = px[3] == 0;
        if transparent != seed_transparent {
            return false;
        }
        transparent || color_distance(px, seed_px) <= tolerance
    };

    mask.set(seed.0, seed.1);
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        let neighbors = [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ];
        for (nx, ny) in neighbors {
            if nx >= w || ny >= h || mask.get(nx as i64, ny as i64) || !allowed(nx, ny) {
                continue;
            }
            if matches(img.get_pixel(nx, ny).0) {
                mask.set(nx, ny);
                stack.push((nx, ny));
            }
        }
    }
    mask
}

/// Trace `mask`'s boundary into one or more closed polygons, in pixel
/// **corner** coordinates (a single selected pixel at `(0, 0)` produces
/// the unit square `(0,0)-(1,0)-(1,1)-(0,1)`). This is a direct boundary
/// trace, not an interpolated marching-squares mesh — every selected
/// pixel contributes an edge wherever its neighbor across that side
/// isn't also selected (including the mask's own border), so contours
/// are always axis-aligned, the same blocky look a real magic-wand
/// selection has before any smoothing pass. Disjoint regions and
/// interior holes each come back as their own closed loop (rendered by
/// stroking, so a hole's opposite winding doesn't need even-odd fill
/// handling the way it would for filling).
pub fn mask_to_contours(mask: &Mask) -> Vec<Vec<Point>> {
    let mut edges: HashMap<(i64, i64), (i64, i64)> = HashMap::new();
    for y in 0..mask.height as i64 {
        for x in 0..mask.width as i64 {
            if !mask.get(x, y) {
                continue;
            }
            // Wound so the selected region is always on each edge's left.
            if !mask.get(x, y - 1) {
                edges.insert((x, y), (x + 1, y)); // top
            }
            if !mask.get(x + 1, y) {
                edges.insert((x + 1, y), (x + 1, y + 1)); // right
            }
            if !mask.get(x, y + 1) {
                edges.insert((x + 1, y + 1), (x, y + 1)); // bottom
            }
            if !mask.get(x - 1, y) {
                edges.insert((x, y + 1), (x, y)); // left
            }
        }
    }

    let mut contours = Vec::new();
    while let Some(&start) = edges.keys().next() {
        let mut loop_pts = Vec::new();
        let mut cur = start;
        loop {
            loop_pts.push(Point::new(cur.0 as f64, cur.1 as f64));
            let Some(next) = edges.remove(&cur) else { break };
            cur = next;
            if cur == start {
                break;
            }
        }
        if loop_pts.len() >= 3 {
            contours.push(simplify_collinear(loop_pts));
        }
    }
    contours
}

/// Drops points that sit exactly between two collinear neighbors — turns
/// e.g. a straight run of 1px-pixel edges into a single long segment,
/// without changing the polygon's actual shape at all.
fn simplify_collinear(pts: Vec<Point>) -> Vec<Point> {
    let n = pts.len();
    if n < 3 {
        return pts;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let d1 = (cur.x - prev.x, cur.y - prev.y);
        let d2 = (next.x - cur.x, next.y - cur.y);
        let cross = d1.0 * d2.1 - d1.1 * d2.0;
        if cross.abs() > f64::EPSILON {
            out.push(cur);
        }
    }
    if out.is_empty() {
        pts
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgba(px: &[[u8; 4]], w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for (i, p) in px.iter().enumerate() {
            img.put_pixel(i as u32 % w, i as u32 / w, image::Rgba(*p));
        }
        img
    }

    fn selected(mask: &Mask) -> Vec<(i64, i64)> {
        let mut out = Vec::new();
        for y in 0..mask.height as i64 {
            for x in 0..mask.width as i64 {
                if mask.get(x, y) {
                    out.push((x, y));
                }
            }
        }
        out
    }

    #[test]
    fn a_uniform_image_floods_entirely() {
        let img = rgba(&[[10, 10, 10, 255]; 9], 3, 3);
        let mask = flood_fill(&img, (1, 1), 10.0);
        assert_eq!(selected(&mask).len(), 9);
    }

    #[test]
    fn a_tight_tolerance_excludes_a_dissimilar_neighbor() {
        let mut px = [[10u8, 10, 10, 255]; 9];
        px[5] = [250, 10, 10, 255]; // middle-right pixel, very different
        let img = rgba(&px, 3, 3);
        let mask = flood_fill(&img, (1, 1), 5.0);
        assert!(!selected(&mask).contains(&(2, 1)));
        assert_eq!(selected(&mask).len(), 8);
    }

    #[test]
    fn a_generous_tolerance_reaches_across_a_dissimilar_neighbor() {
        let mut px = [[10u8, 10, 10, 255]; 9];
        px[5] = [250, 10, 10, 255];
        let img = rgba(&px, 3, 3);
        let mask = flood_fill(&img, (1, 1), 300.0);
        assert_eq!(selected(&mask).len(), 9);
    }

    #[test]
    fn a_transparent_seed_only_matches_other_transparent_pixels() {
        let mut px = [[0u8, 0, 0, 0]; 9];
        px[4] = [200, 200, 200, 255]; // opaque center, everything else transparent
        let img = rgba(&px, 3, 3);
        let mask = flood_fill(&img, (0, 0), 300.0);
        assert!(!selected(&mask).contains(&(1, 1)));
        assert_eq!(selected(&mask).len(), 8);
    }

    #[test]
    fn a_single_pixel_mask_traces_a_unit_square() {
        let mut mask = Mask::new(1, 1);
        mask.set(0, 0);
        let contours = mask_to_contours(&mask);
        assert_eq!(contours.len(), 1);
        let pts = &contours[0];
        assert_eq!(pts.len(), 4);
        for corner in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            assert!(pts.iter().any(|p| (p.x, p.y) == corner), "missing corner {corner:?}");
        }
    }

    #[test]
    fn two_adjacent_pixels_simplify_to_one_rectangle_not_six_points() {
        let mut mask = Mask::new(2, 1);
        mask.set(0, 0);
        mask.set(1, 0);
        let contours = mask_to_contours(&mask);
        assert_eq!(contours.len(), 1);
        assert_eq!(contours[0].len(), 4);
    }

    #[test]
    fn a_ring_with_a_hole_produces_two_separate_contours() {
        // 3x3, every pixel selected except the center — an outer boundary
        // and one interior hole boundary.
        let mut mask = Mask::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                if (x, y) != (1, 1) {
                    mask.set(x, y);
                }
            }
        }
        let contours = mask_to_contours(&mask);
        assert_eq!(contours.len(), 2);
    }

    #[test]
    fn an_empty_mask_has_no_contours() {
        let mask = Mask::new(4, 4);
        assert!(mask_to_contours(&mask).is_empty());
    }
}
