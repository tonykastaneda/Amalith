//! Nonzero-winding polygon fill via [`lyon_tessellation`].
//!
//! `egui::Shape::convex_polygon` triangulates as a fan from vertex 0, so it
//! only fills convex, non-self-intersecting polygons correctly. Real vector
//! artwork — imported Illustrator line art, any concave outline, any donut
//! shape — is routinely concave and self-intersecting, and the fan fill
//! renders it as a spiky mess.
//!
//! The fix is a real fill tessellator that also respects the winding
//! relationship *between* a path's subpaths: a subpath enclosed by another
//! with opposite winding is a hole (the counter of an "O"), not a second
//! solid island. So a whole [`amalith_core::PathData`]'s subpaths must be
//! tessellated together in one pass, not stacked one convex fill at a time.
//!
//! Amalith's SVG importer does not parse a `fill-rule` attribute, so
//! nonzero (SVG/Illustrator's default) is the only rule in play; that is
//! hard-coded here. Stroke rendering is unchanged — this is fill only.

use eframe::egui::{Color32, Mesh, Pos2};
use lyon_tessellation::{
    math::point, path::Path, BuffersBuilder, FillOptions, FillRule, FillTessellator, FillVertex,
    VertexBuffers,
};

/// Tessellates the closed polygons in `subpaths` (screen-space points, one
/// inner `Vec` per subpath) into a single filled [`Mesh`] of `color`, using
/// the nonzero winding rule across all subpaths at once.
///
/// Returns `None` when there is nothing to fill: no subpath with at least
/// three finite points, or the tessellator emitted no triangles.
pub fn fill_mesh(subpaths: &[Vec<Pos2>], color: Color32) -> Option<Mesh> {
    let mut builder = Path::builder();
    let mut started_any = false;
    for points in subpaths {
        if points.len() < 3 || !points.iter().all(|p| p.is_finite()) {
            continue;
        }
        let mut points = points.iter();
        let first = points.next().expect("len checked >= 3");
        builder.begin(point(first.x, first.y));
        for p in points {
            builder.line_to(point(p.x, p.y));
        }
        builder.end(true);
        started_any = true;
    }
    if !started_any {
        return None;
    }
    let path = builder.build();

    let mut buffers: VertexBuffers<Pos2, u32> = VertexBuffers::new();
    FillTessellator::new()
        .tessellate_path(
            &path,
            &FillOptions::default().with_fill_rule(FillRule::NonZero),
            &mut BuffersBuilder::new(&mut buffers, |vertex: FillVertex| {
                let p = vertex.position();
                Pos2::new(p.x, p.y)
            }),
        )
        .ok()?;

    if buffers.indices.is_empty() {
        return None;
    }
    let mut mesh = Mesh::default();
    for pos in buffers.vertices {
        mesh.colored_vertex(pos, color);
    }
    mesh.indices = buffers.indices;
    Some(mesh)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed area of a triangle mesh (shoelace over every triangle); its
    /// absolute value is the covered area, ignoring overlap.
    fn mesh_area(mesh: &Mesh) -> f32 {
        mesh.indices
            .chunks_exact(3)
            .map(|tri| {
                let a = mesh.vertices[tri[0] as usize].pos;
                let b = mesh.vertices[tri[1] as usize].pos;
                let c = mesh.vertices[tri[2] as usize].pos;
                ((b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)) * 0.5
            })
            .sum()
    }

    fn assert_all_finite(mesh: &Mesh) {
        for v in &mesh.vertices {
            assert!(v.pos.is_finite(), "vertex {:?} is not finite", v.pos);
        }
        assert_eq!(
            mesh.indices.len() % 3,
            0,
            "indices must form whole triangles"
        );
        for &i in &mesh.indices {
            assert!((i as usize) < mesh.vertices.len(), "index {i} out of range");
        }
    }

    /// A regular five-pointed star, traced as its self-intersecting
    /// pentagram outline (the classic case `convex_polygon` mangles). With
    /// nonzero winding the whole star — points and central pentagon —
    /// fills solid.
    fn pentagram(radius: f32) -> Vec<Pos2> {
        // Visiting every second vertex of a pentagon draws the pentagram in
        // one self-crossing loop.
        (0..5)
            .map(|k| {
                let a =
                    std::f32::consts::TAU * (k as f32) * 2.0 / 5.0 - std::f32::consts::FRAC_PI_2;
                Pos2::new(radius * a.cos(), radius * a.sin())
            })
            .collect()
    }

    #[test]
    fn tessellates_self_intersecting_pentagram_to_a_clean_solid_mesh() {
        let mesh = fill_mesh(&[pentagram(100.0)], Color32::RED).expect("pentagram should fill");
        assert_all_finite(&mesh);
        assert!(
            mesh.indices.len() >= 3 * 3,
            "a star needs several triangles"
        );

        // A unit pentagram's area is ~1.12 * r^2 for the *outer* star
        // outline (5 points + inner pentagon). Nonzero winding fills all of
        // it, so we should land comfortably above the bare inner pentagon
        // (~0.69 * r^2) and below the circumscribed circle (pi * r^2).
        let area = mesh_area(&mesh).abs();
        let r2 = 100.0_f32 * 100.0;
        assert!(
            area > 0.9 * r2 && area < std::f32::consts::PI * r2,
            "pentagram fill area {area} out of expected band for r^2 = {r2}"
        );
    }

    #[test]
    fn concave_bowtie_fills_both_lobes() {
        // A figure-eight / bowtie: two triangular lobes meeting at the
        // origin, drawn as one self-crossing quad. Each lobe is a triangle
        // with a vertical base of 2 and apex 2 units away at the origin
        // (area 2), so the total filled area is ~4.
        let bowtie = vec![
            Pos2::new(-2.0, -1.0),
            Pos2::new(-2.0, 1.0),
            Pos2::new(2.0, -1.0),
            Pos2::new(2.0, 1.0),
        ];
        let mesh = fill_mesh(&[bowtie], Color32::WHITE).expect("bowtie should fill");
        assert_all_finite(&mesh);
        let area = mesh_area(&mesh).abs();
        assert!(
            (area - 4.0).abs() < 1e-3,
            "bowtie area {area}, expected ~4.0"
        );
    }

    #[test]
    fn nested_opposite_wound_subpaths_leave_a_hole() {
        // Outer square CCW, inner square CW: nonzero winding cancels the
        // overlap, so the fill is a ring of area 100 - 36 = 64, not a
        // solid 100. This is the multi-subpath relationship a per-subpath
        // fill throws away.
        let outer = vec![
            Pos2::new(0.0, 0.0),
            Pos2::new(10.0, 0.0),
            Pos2::new(10.0, 10.0),
            Pos2::new(0.0, 10.0),
        ];
        let inner = vec![
            Pos2::new(2.0, 2.0),
            Pos2::new(2.0, 8.0),
            Pos2::new(8.0, 8.0),
            Pos2::new(8.0, 2.0),
        ];
        let mesh = fill_mesh(&[outer, inner], Color32::WHITE).expect("ring should fill");
        assert_all_finite(&mesh);
        let area = mesh_area(&mesh).abs();
        assert!(
            (area - 64.0).abs() < 1e-3,
            "ring area {area}, expected 64.0"
        );
    }

    #[test]
    fn degenerate_input_yields_no_mesh() {
        assert!(fill_mesh(&[], Color32::WHITE).is_none());
        assert!(fill_mesh(&[vec![Pos2::ZERO, Pos2::new(1.0, 1.0)]], Color32::WHITE).is_none());
        let nan = vec![
            Pos2::new(f32::NAN, 0.0),
            Pos2::new(1.0, 0.0),
            Pos2::new(0.0, 1.0),
        ];
        assert!(fill_mesh(&[nan], Color32::WHITE).is_none());
    }

    /// Area of the convex hull of `points` (monotonic-chain hull + shoelace).
    /// A fan triangulation of a concave shape covers essentially the whole
    /// hull, so the real fill landing well under this is the regression
    /// signal that concavity is being respected.
    fn convex_hull_area(points: &[Pos2]) -> f32 {
        let mut pts: Vec<Pos2> = points.to_vec();
        pts.sort_by(|a, b| {
            a.x.partial_cmp(&b.x)
                .unwrap()
                .then(a.y.partial_cmp(&b.y).unwrap())
        });
        pts.dedup();
        if pts.len() < 3 {
            return 0.0;
        }
        let cross =
            |o: Pos2, a: Pos2, b: Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
        let mut hull: Vec<Pos2> = Vec::new();
        for &p in pts.iter().chain(pts.iter().rev()) {
            while hull.len() >= 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 0.0 {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
        let mut area = 0.0;
        for i in 0..hull.len() {
            let a = hull[i];
            let b = hull[(i + 1) % hull.len()];
            area += a.x * b.y - b.x * a.y;
        }
        (area * 0.5).abs()
    }

    /// End-to-end regression for the "heavy Illustrator paste renders as
    /// spiky corruption" report: the real fixture (one multi-thousand-point,
    /// 4-subpath Illustrator path — the file `amalith-io`'s import test also
    /// pins) goes through the exact import → flatten → `fill_mesh` path the
    /// renderer runs, and the resulting mesh must be a clean, contained
    /// solid — never the convex fan-fill's hull-spanning spikes.
    #[test]
    fn heavy_illustrator_path_fills_without_fan_spikes() {
        use amalith_core::ObjectKind;

        let svg = include_str!("../../amalith-io/tests/fixtures/illustrator-heavy-paste.svg");
        let imported = amalith_io::import_svg(svg).expect("fixture imports");
        let ObjectKind::Path(path) = &imported.objects[&imported.roots[0]].kind else {
            panic!("expected a Path");
        };

        // Same flatten the renderer uses; identity transform (no camera) is
        // fine — tessellation correctness does not depend on the mapping.
        let subpaths: Vec<Vec<Pos2>> = path
            .flattened_points(0.5)
            .into_iter()
            .map(|pts| {
                pts.into_iter()
                    .map(|p| Pos2::new(p.x as f32, p.y as f32))
                    .collect()
            })
            .collect();
        assert_eq!(subpaths.len(), 4, "fixture has four subpaths");

        let mesh = fill_mesh(&subpaths, Color32::from_rgb(158, 171, 235))
            .expect("heavy path must produce a fill mesh");
        assert_all_finite(&mesh);

        let triangles = mesh.indices.len() / 3;
        assert!(
            triangles > 1_000,
            "a ~10k-point outline should tessellate into many triangles, got {triangles}"
        );

        // Every triangle stays inside the path's bounding box (a real fill
        // is contained; a blown-up vertex or bad index would escape it).
        let bb = amalith_core::geom::bez_path_bounds(&path.geometry);
        let (pad, x0, y0, x1, y1) = (
            1.0_f32,
            bb.x0 as f32,
            bb.y0 as f32,
            bb.x1 as f32,
            bb.y1 as f32,
        );
        for v in &mesh.vertices {
            assert!(
                v.pos.x >= x0 - pad
                    && v.pos.x <= x1 + pad
                    && v.pos.y >= y0 - pad
                    && v.pos.y <= y1 + pad,
                "vertex {:?} escaped the path bbox {bb:?}",
                v.pos
            );
        }

        // The crux: this artwork is deeply concave, so a correct nonzero
        // fill covers far less than its convex hull. `convex_polygon`'s fan
        // fill (the bug) covers ~the whole hull; anything close to hull area
        // here means the spikes are back.
        let all_points: Vec<Pos2> = subpaths.iter().flatten().copied().collect();
        let hull_area = convex_hull_area(&all_points);
        let fill_area = mesh_area(&mesh).abs();
        assert!(fill_area > 0.0, "degenerate empty fill");
        assert!(
            fill_area < 0.85 * hull_area,
            "fill area {fill_area} is not meaningfully below hull area {hull_area} — \
             concavity is not being respected (fan-fill spikes)"
        );
    }

    // --- visual spot-check (run manually) --------------------------------
    //
    // `cargo test -p amalith-app -- --ignored render_fixture_comparison_bmps`
    // rasterizes the real fixture two ways into 24-bit BMPs: the new
    // `fill_mesh` tessellation vs. the old per-subpath convex fan fill. The
    // fan output is visibly full of hull-spanning spikes; the tessellated
    // output is a clean solid. Kept as an on-demand check, not CI noise.

    fn raster_triangles(
        tris: &[[Pos2; 3]],
        w: usize,
        h: usize,
        view: (f32, f32, f32, f32),
    ) -> Vec<u8> {
        let (vx0, vy0, vx1, vy1) = view;
        let sx = w as f32 / (vx1 - vx0);
        let sy = h as f32 / (vy1 - vy0);
        let to_px = |p: Pos2| Pos2::new((p.x - vx0) * sx, (p.y - vy0) * sy);
        let mut buf = vec![24u8; w * h * 3]; // dark background
        for tri in tris {
            let p = [to_px(tri[0]), to_px(tri[1]), to_px(tri[2])];
            let min_x = p
                .iter()
                .map(|q| q.x)
                .fold(f32::MAX, f32::min)
                .floor()
                .max(0.0) as usize;
            let max_x = (p.iter().map(|q| q.x).fold(f32::MIN, f32::max).ceil() as usize).min(w);
            let min_y = p
                .iter()
                .map(|q| q.y)
                .fold(f32::MAX, f32::min)
                .floor()
                .max(0.0) as usize;
            let max_y = (p.iter().map(|q| q.y).fold(f32::MIN, f32::max).ceil() as usize).min(h);
            let area =
                (p[1].x - p[0].x) * (p[2].y - p[0].y) - (p[2].x - p[0].x) * (p[1].y - p[0].y);
            if area.abs() < 1e-6 {
                continue;
            }
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let q = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 =
                        ((p[1].x - q.x) * (p[2].y - q.y) - (p[2].x - q.x) * (p[1].y - q.y)) / area;
                    let w1 =
                        ((p[2].x - q.x) * (p[0].y - q.y) - (p[0].x - q.x) * (p[2].y - q.y)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 >= -0.001 && w1 >= -0.001 && w2 >= -0.001 {
                        let i = (y * w + x) * 3;
                        // additive so overdraw (a fan's overlap) shows up
                        buf[i] = buf[i].saturating_add(70);
                        buf[i + 1] = buf[i + 1].saturating_add(80);
                        buf[i + 2] = buf[i + 2].saturating_add(120);
                    }
                }
            }
        }
        buf
    }

    fn write_bmp(path: &str, rgb: &[u8], w: usize, h: usize) {
        let row = w * 3;
        let pad = (4 - row % 4) % 4;
        let stride = row + pad;
        let size = 54 + stride * h;
        let mut f = vec![0u8; size];
        f[0..2].copy_from_slice(b"BM");
        f[2..6].copy_from_slice(&(size as u32).to_le_bytes());
        f[10..14].copy_from_slice(&54u32.to_le_bytes());
        f[14..18].copy_from_slice(&40u32.to_le_bytes());
        f[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        f[22..26].copy_from_slice(&(h as i32).to_le_bytes());
        f[26..28].copy_from_slice(&1u16.to_le_bytes());
        f[28..30].copy_from_slice(&24u16.to_le_bytes());
        for y in 0..h {
            let src = (h - 1 - y) * row; // BMP rows are bottom-up
            let dst = 54 + y * stride;
            for x in 0..w {
                f[dst + x * 3] = rgb[src + x * 3 + 2]; // B
                f[dst + x * 3 + 1] = rgb[src + x * 3 + 1]; // G
                f[dst + x * 3 + 2] = rgb[src + x * 3]; // R
            }
        }
        std::fs::write(path, f).unwrap();
    }

    #[test]
    #[ignore = "manual visual check; writes BMPs to the scratchpad"]
    fn render_fixture_comparison_bmps() {
        use amalith_core::ObjectKind;
        let out = std::env::var("FILL_BMP_DIR").unwrap_or_else(|_| "/tmp".into());

        let svg = include_str!("../../amalith-io/tests/fixtures/illustrator-heavy-paste.svg");
        let imported = amalith_io::import_svg(svg).unwrap();
        let ObjectKind::Path(path) = &imported.objects[&imported.roots[0]].kind else {
            panic!()
        };
        let subpaths: Vec<Vec<Pos2>> = path
            .flattened_points(0.5)
            .into_iter()
            .map(|pts| {
                pts.into_iter()
                    .map(|p| Pos2::new(p.x as f32, p.y as f32))
                    .collect()
            })
            .collect();
        let bb = amalith_core::geom::bez_path_bounds(&path.geometry);
        let view = (bb.x0 as f32, bb.y0 as f32, bb.x1 as f32, bb.y1 as f32);
        let (w, h) = (900usize, 900usize);

        // New: real tessellation.
        let mesh = fill_mesh(&subpaths, Color32::WHITE).unwrap();
        let new_tris: Vec<[Pos2; 3]> = mesh
            .indices
            .chunks_exact(3)
            .map(|t| {
                [
                    mesh.vertices[t[0] as usize].pos,
                    mesh.vertices[t[1] as usize].pos,
                    mesh.vertices[t[2] as usize].pos,
                ]
            })
            .collect();
        write_bmp(
            &format!("{out}/fill_new_tessellated.bmp"),
            &raster_triangles(&new_tris, w, h, view),
            w,
            h,
        );

        // Old: per-subpath convex fan from vertex 0 (the bug).
        let mut fan_tris: Vec<[Pos2; 3]> = Vec::new();
        for sp in &subpaths {
            for k in 1..sp.len().saturating_sub(1) {
                fan_tris.push([sp[0], sp[k], sp[k + 1]]);
            }
        }
        write_bmp(
            &format!("{out}/fill_old_convex_fan.bmp"),
            &raster_triangles(&fan_tris, w, h, view),
            w,
            h,
        );

        eprintln!("wrote {out}/fill_new_tessellated.bmp and {out}/fill_old_convex_fan.bmp");
        eprintln!(
            "new triangles: {}, fan triangles: {}",
            new_tris.len(),
            fan_tris.len()
        );
    }
}
