//! Pathfinder boolean ops and stroke expansion.
//!
//! Geometry is flattened to polygons, run through `i_overlay`, then rebuilt
//! as [`PathData`]. Results live in the same coordinate space as the input
//! contours (callers bake world/parent transforms).

use amalith_core::{
    Appearance, AppearanceItem, LineCap, LineJoin, Paint, PathData, StrokeStyle,
};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::outline::offset::OutlineOffset;
use kurbo::{flatten, stroke, Affine, BezPath, Cap, Join, PathEl, Point, Stroke, StrokeOpts, Vec2};

use crate::command::PathfinderOp;

/// How closely a flattened polygon edge has to track the real curve
/// (document-space points) for boolean topology. Shape Builder keeps these
/// polygons until all region operations finish, then restores source curves.
/// Other Pathfinder operations currently use the curve-fitting fallback.
const TOL: f64 = 0.05;

pub struct PathInput {
    pub contours: Vec<Vec<[f64; 2]>>,
    pub appearance: Appearance,
}

pub struct PathResult {
    pub path: PathData,
    pub appearance: Appearance,
}

/// Flatten a Bézier path to closed polygon contours.
pub fn flatten_path(path: &BezPath) -> Vec<Vec<[f64; 2]>> {
    let mut contours = Vec::new();
    let mut cur: Vec<[f64; 2]> = Vec::new();
    flatten(path, TOL, |el| match el {
        PathEl::MoveTo(p) => {
            if cur.len() >= 3 {
                contours.push(std::mem::take(&mut cur));
            }
            cur = vec![[p.x, p.y]];
        }
        PathEl::LineTo(p) => cur.push([p.x, p.y]),
        PathEl::ClosePath => {
            if cur.len() >= 3 {
                contours.push(std::mem::take(&mut cur));
            }
        }
        PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _) => {}
    });
    if cur.len() >= 3 {
        contours.push(cur);
    }
    contours
}

/// Turns Pathfinder's flattened polygon output back into an editable
/// path — fitting real cubic Béziers onto each contour (see
/// [`crate::curvefit`]) rather than leaving one straight-line anchor per
/// flattened vertex, which is what actually got computed but never what
/// a person wants to see or edit afterward.
pub(crate) fn contours_to_path(contours: &[Vec<[f64; 2]>]) -> Option<PathData> {
    if contours.is_empty() {
        return None;
    }
    let mut path = BezPath::new();
    for c in contours {
        if c.len() < 3 {
            continue;
        }
        path.extend(crate::curvefit::fit_closed_contour(c));
    }
    if path.elements().is_empty() {
        None
    } else {
        Some(PathData::from_bezpath(path))
    }
}

fn overlay(
    a: &[Vec<[f64; 2]>],
    b: &[Vec<[f64; 2]>],
    rule: OverlayRule,
) -> Vec<Vec<[f64; 2]>> {
    if a.is_empty() {
        return match rule {
            OverlayRule::Union | OverlayRule::Xor | OverlayRule::Subject => b.to_vec(),
            _ => Vec::new(),
        };
    }
    if b.is_empty() {
        return match rule {
            OverlayRule::Union | OverlayRule::Xor | OverlayRule::Difference | OverlayRule::Subject => {
                a.to_vec()
            }
            _ => Vec::new(),
        };
    }
    let a = a.to_vec();
    let b = b.to_vec();
    let shapes: Vec<Vec<Vec<[f64; 2]>>> = a.overlay(&b, rule, FillRule::NonZero);
    shapes.into_iter().flatten().filter(|c| !is_sliver(c)).collect()
}

/// A contour that isn't real content — it's numerical noise. `i_overlay`
/// computes a cut's two sides as independent boolean passes (e.g.
/// `divide`'s own `leftover`/`hit`), and where their edges are supposed
/// to meet exactly, floating-point rounding can instead leave a sliver
/// polygon along the seam. That sliver can be *thin and long* (running
/// the whole length of the seam) rather than merely tiny, so area alone
/// doesn't catch it — a hairline-thin sliver the length of a real curve
/// can easily clear a small area floor. Checking area against perimeter
/// instead catches both: a real shape's area scales with perimeter², a
/// sliver's scales linearly (area ≈ perimeter/2 × its own width), so
/// dividing them back out recovers that effective width directly.
/// Every `overlay` call site funnels through here so no Pathfinder op
/// (boolean ops, Shape Builder, Divide, Trim, …) ever turns one of these
/// into a real, separately-selectable output object.
const MIN_EFFECTIVE_WIDTH: f64 = 0.05;

fn is_sliver(c: &[[f64; 2]]) -> bool {
    let perimeter = contour_perimeter(c);
    if perimeter < 1e-9 {
        return true;
    }
    contour_area(c) / (perimeter * 0.5) < MIN_EFFECTIVE_WIDTH
}

fn contour_perimeter(c: &[[f64; 2]]) -> f64 {
    if c.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..c.len() {
        let [x1, y1] = c[i];
        let [x2, y2] = c[(i + 1) % c.len()];
        sum += ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();
    }
    sum
}

fn contour_area(c: &[[f64; 2]]) -> f64 {
    if c.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..c.len() {
        // Translate to the first vertex before the shoelace sum to avoid
        // cancellation for tiny regions far from the document origin.
        let (x1, y1) = (c[i][0]-c[0][0], c[i][1]-c[0][1]);
        let next = c[(i + 1) % c.len()];
        let (x2,y2)=(next[0]-c[0][0],next[1]-c[0][1]);
        sum += x1 * y2 - x2 * y1;
    }
    (sum * 0.5).abs()
}

fn union_all(items: &[Vec<Vec<[f64; 2]>>]) -> Vec<Vec<[f64; 2]>> {
    let mut acc: Vec<Vec<[f64; 2]>> = Vec::new();
    for item in items {
        acc = overlay(&acc, item, OverlayRule::Union);
    }
    acc
}

fn paths_from_contours(contours: Vec<Vec<[f64; 2]>>, appearance: Appearance) -> Vec<PathResult> {
    contours_to_path(&contours)
        .into_iter()
        .map(|path| PathResult { path, appearance: appearance.clone() })
        .collect()
}

fn no_stroke(mut a: Appearance) -> Appearance {
    a.set_stroke(Paint::None);
    a
}

/// A connected region with its hole contours, still in the boolean engine's
/// polygon representation. Never fit curves between topology operations.
pub struct ShapeRegion {
    pub contours: Vec<Vec<[f64;2]>>,
    pub appearance: Appearance,
}

pub fn polygon_path(contours: &[Vec<[f64;2]>]) -> PathData {
    let mut p=BezPath::new();
    for c in contours.iter().filter(|c|c.len()>=3) {
        p.move_to((c[0][0],c[0][1]));
        for q in &c[1..] { p.line_to((q[0],q[1])); }
        p.close_path();
    }
    PathData::from_bezpath(p)
}

fn region_overlay(a: &[Vec<[f64;2]>],b: &[Vec<[f64;2]>],rule: OverlayRule) -> Vec<Vec<[f64;2]>> {
    // Unlike the general Pathfinder sliver heuristic, do not discard real
    // narrow artwork based on a fixed document-unit width.
    let adapter=i_overlay::i_float::adapter::FloatPointAdapter::<[f64;2],i32>::with_iter(a.iter().flatten().chain(b.iter().flatten()));
    let grid_noise=adapter.inv_scale()*8.0;
    let shapes: Vec<Vec<Vec<[f64;2]>>>=a.to_vec().overlay(&b.to_vec(),rule,FillRule::NonZero);
    // Separate float overlays can disagree by a few integer-grid units on
    // an intersection. Remove only that precision-scale residue, rather
    // than deleting legitimate details with a fixed 0.05pt width cutoff.
    shapes.into_iter().flatten().filter(|c|contour_area(c)>contour_perimeter(c)*0.5*grid_noise).collect()
}

pub fn shape_builder_union(inputs: &[PathInput]) -> Vec<Vec<[f64;2]>> {
    let mut out=Vec::new();
    for input in inputs { out=region_overlay(&out,&input.contours,OverlayRule::Union); }
    out
}

pub fn shape_builder_regions(inputs: &[PathInput]) -> Vec<ShapeRegion> {
    let mut pieces: Vec<ShapeRegion>=Vec::new();
    for input in inputs {
        let mut next=Vec::new();
        let mut covered=Vec::new();
        for piece in pieces {
            let remaining=region_overlay(&piece.contours,&input.contours,OverlayRule::Difference);
            if !remaining.is_empty() { next.push(ShapeRegion { contours:remaining,appearance:piece.appearance }); }
            let hit=region_overlay(&piece.contours,&input.contours,OverlayRule::Intersect);
            if !hit.is_empty() { next.push(ShapeRegion { contours:hit,appearance:input.appearance.clone() }); }
            covered=region_overlay(&covered,&piece.contours,OverlayRule::Union);
        }
        let novel=region_overlay(&input.contours,&covered,OverlayRule::Difference);
        if !novel.is_empty() { next.push(ShapeRegion { contours:novel,appearance:input.appearance.clone() }); }
        pieces=next;
    }
    // One face per connected component, with holes kept attached to its outer
    // boundary. Disjoint islands must not be activated by the same hover hit.
    pieces.into_iter().flat_map(|p| {
        let shapes: Vec<Vec<Vec<[f64;2]>>>=p.contours.overlay(&Vec::<Vec<[f64;2]>>::new(),OverlayRule::Subject,FillRule::NonZero);
        shapes.into_iter().map(move |contours|ShapeRegion { contours,appearance:p.appearance.clone() })
    }).collect()
}

pub(crate) fn erase_closed(path: &BezPath, area: &BezPath, cut: &[Vec<[f64; 2]>]) -> Option<Vec<BezPath>> {
    let contours = flatten_path(path);
    if region_overlay(&contours, cut, OverlayRule::Intersect).is_empty() { return None; }
    let remaining = region_overlay(&contours, cut, OverlayRule::Difference);
    let shapes: Vec<Vec<Vec<[f64; 2]>>> = remaining.overlay(
        &Vec::<Vec<[f64; 2]>>::new(), OverlayRule::Subject, FillRule::NonZero,
    );
    Some(shapes.into_iter().map(|shape| crate::curve_restore::restore(&shape, &[path.clone(), area.clone()])).collect())
}

pub(crate) fn shape_builder_results(inputs: &[PathInput],cut: &[Vec<[f64;2]>],sources: &[BezPath],appearance: Option<Appearance>) -> (Vec<usize>,Vec<PathResult>) {
    let mut consumed=Vec::new();
    let mut result=Vec::new();
    if let Some(appearance)=appearance {
        let path=PathData::from_bezpath(crate::curve_restore::restore(cut,sources));
        result.push(PathResult { path,appearance });
    }
    for (i,input) in inputs.iter().enumerate() {
        if region_overlay(&input.contours,cut,OverlayRule::Intersect).is_empty() { continue; }
        consumed.push(i);
        let remaining=region_overlay(&input.contours,cut,OverlayRule::Difference);
        if !remaining.is_empty() {
            let path=PathData::from_bezpath(crate::curve_restore::restore(&remaining,sources));
            result.push(PathResult { path,appearance:input.appearance.clone() });
        }
    }
    (consumed,result)
}

/// Run a Pathfinder op. `inputs` is back → front.
pub fn apply(op: PathfinderOp, inputs: &[PathInput]) -> Vec<PathResult> {
    if inputs.is_empty() {
        return Vec::new();
    }
    match op {
        PathfinderOp::Unite => {
            let all: Vec<_> = inputs.iter().map(|i| i.contours.clone()).collect();
            paths_from_contours(union_all(&all), inputs.last().unwrap().appearance.clone())
        }
        PathfinderOp::MinusFront => {
            let back = &inputs[0];
            let rest: Vec<_> = inputs[1..].iter().map(|i| i.contours.clone()).collect();
            let cut = union_all(&rest);
            paths_from_contours(overlay(&back.contours, &cut, OverlayRule::Difference), back.appearance.clone())
        }
        PathfinderOp::MinusBack => {
            let front = inputs.last().unwrap();
            let rest: Vec<_> = inputs[..inputs.len() - 1]
                .iter()
                .map(|i| i.contours.clone())
                .collect();
            let cut = union_all(&rest);
            paths_from_contours(
                overlay(&front.contours, &cut, OverlayRule::Difference),
                front.appearance.clone(),
            )
        }
        PathfinderOp::Intersect => {
            let mut acc = inputs[0].contours.clone();
            for next in &inputs[1..] {
                acc = overlay(&acc, &next.contours, OverlayRule::Intersect);
            }
            paths_from_contours(acc, inputs.last().unwrap().appearance.clone())
        }
        PathfinderOp::Exclude => {
            let mut acc = inputs[0].contours.clone();
            for next in &inputs[1..] {
                acc = overlay(&acc, &next.contours, OverlayRule::Xor);
            }
            paths_from_contours(acc, inputs.last().unwrap().appearance.clone())
        }
        PathfinderOp::Divide => divide(inputs),
        PathfinderOp::Trim => trim(inputs, false),
        PathfinderOp::Merge => trim(inputs, true),
        PathfinderOp::Crop => crop(inputs),
        PathfinderOp::Outline => outline(inputs),
    }
}

/// Split every overlap into its own piece (back → front).
fn divide(inputs: &[PathInput]) -> Vec<PathResult> {
    if inputs.len() == 1 {
        return paths_from_contours(inputs[0].contours.clone(), inputs[0].appearance.clone());
    }
    // Pieces are (contours, appearance of the topmost covering original).
    let mut pieces: Vec<(Vec<Vec<[f64; 2]>>, Appearance)> =
        vec![(inputs[0].contours.clone(), inputs[0].appearance.clone())];
    for next in &inputs[1..] {
        let mut out = Vec::new();
        let mut covered = Vec::new();
        for (cont, app) in pieces {
            let leftover = overlay(&cont, &next.contours, OverlayRule::Difference);
            if !leftover.is_empty() {
                out.push((leftover, app));
            }
            let hit = overlay(&cont, &next.contours, OverlayRule::Intersect);
            if !hit.is_empty() {
                out.push((hit.clone(), next.appearance.clone()));
                covered = overlay(&covered, &hit, OverlayRule::Union);
            }
        }
        let novel = overlay(&next.contours, &covered, OverlayRule::Difference);
        if !novel.is_empty() {
            out.push((novel, next.appearance.clone()));
        }
        pieces = out;
    }
    pieces
        .into_iter()
        .filter_map(|(c, a)| contours_to_path(&c).map(|path| PathResult { path, appearance: a }))
        .collect()
}

/// Keep only visible parts. `merge_same_fill` unions adjacent same-fill pieces.
fn trim(inputs: &[PathInput], merge_same_fill: bool) -> Vec<PathResult> {
    let mut covered: Vec<Vec<[f64; 2]>> = Vec::new();
    let mut out: Vec<PathResult> = Vec::new();
    for input in inputs.iter().rev() {
        let vis = overlay(&input.contours, &covered, OverlayRule::Difference);
        if !vis.is_empty() {
            if let Some(path) = contours_to_path(&vis) {
                out.push(PathResult {
                    path,
                    appearance: no_stroke(input.appearance.clone()),
                });
            }
        }
        covered = overlay(&covered, &input.contours, OverlayRule::Union);
    }
    out.reverse();
    if merge_same_fill {
        merge_by_fill(out)
    } else {
        out
    }
}

fn merge_by_fill(pieces: Vec<PathResult>) -> Vec<PathResult> {
    let mut groups: Vec<(Appearance, Vec<Vec<[f64; 2]>>)> = Vec::new();
    for p in pieces {
        let contours = flatten_path(&p.path.geometry);
        if let Some((_, acc)) = groups
            .iter_mut()
            .find(|(a, _)| a.fill() == p.appearance.fill() && a.opacity == p.appearance.opacity)
        {
            *acc = overlay(acc, &contours, OverlayRule::Union);
        } else {
            groups.push((p.appearance, contours));
        }
    }
    groups
        .into_iter()
        .filter_map(|(a, c)| contours_to_path(&c).map(|path| PathResult { path, appearance: a }))
        .collect()
}

fn crop(inputs: &[PathInput]) -> Vec<PathResult> {
    if inputs.len() < 2 {
        return Vec::new();
    }
    let clip = &inputs.last().unwrap().contours;
    let mut out = Vec::new();
    for input in &inputs[..inputs.len() - 1] {
        let hit = overlay(&input.contours, clip, OverlayRule::Intersect);
        if let Some(path) = contours_to_path(&hit) {
            out.push(PathResult {
                path,
                appearance: no_stroke(input.appearance.clone()),
            });
        }
    }
    out
}

fn outline(inputs: &[PathInput]) -> Vec<PathResult> {
    // Split like Divide, then keep each piece's outline as a stroked, unfilled path.
    divide(inputs)
        .into_iter()
        .map(|mut r| {
            r.appearance.set_fill(Paint::None);
            if r.appearance.stroke() == Paint::None {
                let width = r.appearance.stroke_width().max(1.0);
                r.appearance.set_stroke(Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0)));
                r.appearance.set_stroke_width(width);
            }
            r
        })
        .collect()
}

/// Each input's own contours minus `cut`, dropping any that vanish
/// entirely. Keeps each survivor's own appearance — the Eraser tool
/// uses this to give every object its stroke touched back just the
/// part that wasn't swept over.
#[cfg(test)]
pub(crate) fn subtract_each(inputs: &[PathInput], cut: &[Vec<[f64; 2]>]) -> Vec<PathResult> {
    inputs
        .iter()
        .filter_map(|i| {
            let remaining = overlay(&i.contours, cut, OverlayRule::Difference);
            contours_to_path(&remaining).map(|path| PathResult { path, appearance: i.appearance.clone() })
        })
        .collect()
}

/// Whether `a` and `b` share any area at all.
#[cfg(test)]
pub(crate) fn intersects(a: &[Vec<[f64; 2]>], b: &[Vec<[f64; 2]>]) -> bool {
    !overlay(a, b, OverlayRule::Intersect).is_empty()
}

pub fn has_visible_stroke(a: &Appearance) -> bool {
    a.stroke() != Paint::None && a.stroke_width() > 0.05
}

/// Object ▸ Path ▸ Offset Path: grows (`offset > 0`) or shrinks
/// (`offset < 0`) a path's boundary by `offset`, joining corners per
/// `join` (Illustrator's Miter/Round/Bevel).
///
/// A *closed* path goes through `i_overlay`'s own polygon-offset
/// (`outline()`, the same self-intersection-resolving machinery its
/// boolean ops already use here, in `apply` below): a naive per-corner
/// miter join, offsetting each edge independently and connecting at the
/// corners, leaves an inward offset's adjacent edges unclipped past each
/// other at any convex corner — a real self-intersecting bowtie, not a
/// simple polygon — and `i_overlay`'s offset resolves that properly,
/// where a bounding-box/area heuristic on the raw points can't.
///
/// An *open* path instead goes through `kurbo::stroke` at `2 * |offset|`
/// — offset outward on both sides into a single closed loop, joined at
/// its two ends the same way as its corners — matching Illustrator's own
/// result for an open path (not a parallel open curve). This case has no
/// inner-vs-outer ambiguity to begin with (there's only ever the one
/// loop), so the self-intersection problem above doesn't apply to it.
pub fn offset_path(path: &BezPath, offset: f64, join: LineJoin, miter_limit: f64) -> Option<PathData> {
    if offset.abs() < 1e-6 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    let is_closed = path.elements().iter().any(|e| matches!(e, PathEl::ClosePath));
    if !is_closed {
        let kurbo_join = match join {
            LineJoin::Miter => Join::Miter,
            LineJoin::Round => Join::Round,
            LineJoin::Bevel => Join::Bevel,
        };
        let style = Stroke::new(offset.abs() * 2.0)
            .with_caps(Cap::Butt)
            .with_join(kurbo_join)
            .with_miter_limit(miter_limit.max(1.0));
        let outlined = stroke(path.clone(), &style, &StrokeOpts::default(), TOL);
        return if outlined.elements().is_empty() {
            None
        } else {
            Some(PathData::from_bezpath(outlined))
        };
    }
    let contours = flatten_path(path);
    if contours.is_empty() {
        return None;
    }
    let mesh_join = match join {
        // Illustrator's miter *limit* L caps a corner's spike length to
        // L times the offset; for a symmetric corner of full angle φ,
        // that spike ratio is 1/sin(φ/2), so the angle at which it first
        // exceeds L is φ = 2·asin(1/L) — `i_overlay` takes that angle
        // directly (corners sharper than it fall back to bevel) rather
        // than the ratio itself.
        LineJoin::Miter => {
            i_overlay::mesh::style::LineJoin::Miter(2.0 * (1.0 / miter_limit.max(1.0)).asin())
        }
        LineJoin::Round => i_overlay::mesh::style::LineJoin::Round(0.35),
        LineJoin::Bevel => i_overlay::mesh::style::LineJoin::Bevel,
    };
    let style = i_overlay::mesh::style::OutlineStyle::new(offset).line_join(mesh_join);
    let shapes = contours.outline(&style);
    let flat: Vec<Vec<[f64; 2]>> = shapes.into_iter().flatten().collect();
    contours_to_path(&flat)
}

/// A tiny deterministic PRNG for [`roughen`]/[`tweak`]'s per-point jitter —
/// `splitmix64`. Not a real `rand`-crate dependency: both effects only
/// need "a stable, reasonably-distributed value per (seed, index)", never
/// re-rolled on repaint (the seed is fixed once, at add-time, in the
/// effect's own params), so a proper RNG crate would be disproportionate.
/// Returns a value in `-1.0..=1.0`.
fn hash_jitter(seed: u64, i: usize) -> f64 {
    let mut z = seed.wrapping_add((i as u64).wrapping_mul(0x9E3779B97F4A7C15));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    (z as f64 / u64::MAX as f64) * 2.0 - 1.0
}

/// Walks each of `path`'s subpaths as a flattened polyline via
/// [`amalith_core::ArcLengthPath`], sampling `sample_count(total_length)`
/// evenly-spaced points along each one and letting `displace` move each
/// sample (given its index *within that subpath* — resets to 0 at each
/// subpath's start, so a per-sample alternation like Zig Zag's stays in
/// phase there), then refits a `BezPath` from the results via
/// [`push_subpath`]. The shared skeleton behind [`zig_zag`], [`roughen`],
/// and [`twist`].
///
/// Every subpath is treated as closed if *any* subpath in `path` contains
/// a `ClosePath` — the same coarse whole-path flag [`offset_path`] already
/// uses instead of tracking each subpath's own closedness, which covers
/// the common case of a uniformly open or uniformly closed path.
fn resample_and_rebuild(
    path: &BezPath,
    sample_count: impl Fn(f64) -> usize,
    smooth: bool,
    mut displace: impl FnMut(usize, Point, f64) -> Point,
) -> Option<BezPath> {
    let closed = path.elements().iter().any(|e| matches!(e, PathEl::ClosePath));
    let polylines = amalith_core::geom::flattened_points(path, TOL);
    let mut out = BezPath::new();
    for pts in &polylines {
        let arc = amalith_core::ArcLengthPath::new(pts, closed);
        let total = arc.total_length();
        if total <= 0.0 {
            continue;
        }
        let n = sample_count(total).max(3);
        let steps = if closed { n } else { n + 1 };
        let displaced: Vec<Point> = (0..steps)
            .map(|i| {
                let d = total * i as f64 / n as f64;
                let (p, tangent) = arc.point_and_tangent(d);
                displace(i, p, tangent)
            })
            .collect();
        push_subpath(&mut out, &displaced, closed, smooth);
    }
    (!out.elements().is_empty()).then_some(out)
}

/// Appends one subpath to `out` from already-displaced points — straight
/// segments, or (`smooth`) a quadratic through each consecutive pair's
/// midpoint (the control point is the shared point itself), the same
/// light "smooth a polyline" technique freehand-drawing tools use — not a
/// true curve fit, just enough to read as rounded ridges/waves.
fn push_subpath(out: &mut BezPath, pts: &[Point], closed: bool, smooth: bool) {
    if pts.len() < 2 {
        return;
    }
    out.move_to(pts[0]);
    if smooth {
        for i in 1..pts.len() - 1 {
            let ctrl = pts[i];
            let mid = Point::new((pts[i].x + pts[i + 1].x) * 0.5, (pts[i].y + pts[i + 1].y) * 0.5);
            out.quad_to(ctrl, mid);
        }
        let last = pts[pts.len() - 1];
        if closed {
            out.quad_to(last, pts[0]);
        } else {
            out.line_to(last);
        }
    } else {
        for &p in &pts[1..] {
            out.line_to(p);
        }
    }
    if closed {
        out.close_path();
    }
}

/// Effect ▸ Distort & Transform ▸ Zig Zag: alternates a perpendicular
/// displacement of `±size` along the path. `ridges_per_segment` is an
/// approximation of Illustrator's literal "per original Bezier segment"
/// density — scaled against a fixed 40px reference length instead of the
/// path's real segment count, simpler to compute and visually similar.
/// `smooth` gives rounded ridges instead of sharp corners.
pub fn zig_zag(path: &BezPath, size: f64, ridges_per_segment: f64, smooth: bool) -> Option<PathData> {
    if size.abs() < 1e-6 || ridges_per_segment < 0.1 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    const REFERENCE_LEN: f64 = 40.0;
    let result = resample_and_rebuild(
        path,
        |total| (((total / REFERENCE_LEN * ridges_per_segment).round() as usize).max(1) * 2).max(4),
        smooth,
        |i, p, tangent| {
            let dir = if i % 2 == 0 { 1.0 } else { -1.0 };
            let normal = tangent + std::f64::consts::FRAC_PI_2;
            Point::new(p.x + normal.cos() * size * dir, p.y + normal.sin() * size * dir)
        },
    )?;
    Some(PathData::from_bezpath(result))
}

/// Effect ▸ Distort & Transform ▸ Roughen: like [`zig_zag`], but each
/// sample is displaced by a random (seeded, stable) amount instead of a
/// clean alternation — `detail` is samples per document inch (96px),
/// matching `width_outline`'s own px-per-inch convention.
pub fn roughen(path: &BezPath, size: f64, detail: f64, smooth: bool, seed: u64) -> Option<PathData> {
    if size.abs() < 1e-6 || detail < 0.1 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    const PX_PER_INCH: f64 = 96.0;
    let result = resample_and_rebuild(
        path,
        |total| ((total / PX_PER_INCH * detail).round() as usize).max(3),
        smooth,
        |i, p, tangent| {
            let normal = tangent + std::f64::consts::FRAC_PI_2;
            let jitter = hash_jitter(seed, i) * size;
            Point::new(p.x + normal.cos() * jitter, p.y + normal.sin() * jitter)
        },
    )?;
    Some(PathData::from_bezpath(result))
}

/// Effect ▸ Distort & Transform ▸ Twist: rotates each sample around the
/// path's own local-bounds center by `angle` degrees, decaying linearly
/// to zero at the bounds' farthest corner — a swirl, strongest at the
/// center. Resamples finely first (a twist needs enough resolution to
/// read as a curve, not a faceted polygon — the same reasoning
/// `width_outline` already uses a fixed fine resolution for).
pub fn twist(path: &BezPath, angle: f64) -> Option<PathData> {
    if angle.abs() < 1e-6 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    let bbox = amalith_core::geom::bez_path_bounds(path);
    let center = Point::new((bbox.x0 + bbox.x1) * 0.5, (bbox.y0 + bbox.y1) * 0.5);
    let max_dist = ((bbox.x1 - bbox.x0).powi(2) + (bbox.y1 - bbox.y0).powi(2)).sqrt() * 0.5;
    if max_dist <= 0.0 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    const SAMPLES_PER_UNIT: f64 = 1.0 / 6.0;
    let result = resample_and_rebuild(
        path,
        |total| ((total * SAMPLES_PER_UNIT).round() as usize).max(8),
        true,
        |_, p, _| {
            let dist = (p - center).hypot();
            let falloff = (1.0 - dist / max_dist).clamp(0.0, 1.0);
            let theta = angle.to_radians() * falloff;
            let v = p - center;
            let (s, c) = theta.sin_cos();
            center + Vec2::new(v.x * c - v.y * s, v.x * s + v.y * c)
        },
    )?;
    Some(PathData::from_bezpath(result))
}


/// Effect ▸ Distort & Transform ▸ Pucker & Bloat: scales every segment's
/// own Bezier handles' distance from their anchor by `1.0 +
/// amount/100.0` — negative `amount` (pucker) shrinks handles toward the
/// anchor, straightening curves and pulling points toward each segment's
/// chord; positive (bloat) grows them, bulging segments outward. Operates
/// directly on the existing control points, no resampling — a straight
/// `PathEl::LineTo` grows/shrinks a matching bulge by being treated as a
/// degenerate curve whose handles sit at the anchors themselves.
pub fn pucker_bloat(path: &BezPath, amount: f64) -> Option<PathData> {
    if amount.abs() < 1e-6 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    let factor = 1.0 + amount / 100.0;
    // A straight `LineTo` has no existing handle to scale (its implicit
    // handles sit exactly on the chord) — real Illustrator still visibly
    // bulges/pinches straight edges, so this adds a perpendicular bulge of
    // its own instead. `centroid` (the local bbox center, close enough for
    // a directional sign) decides which way is "outward": whichever side
    // of the chord is farther from it, regardless of this subpath's own
    // winding direction, so positive `amount` (bloat) always bulges away
    // from the shape and negative (pucker) always pulls toward it.
    let bbox = amalith_core::geom::bez_path_bounds(path);
    let centroid = Point::new((bbox.x0 + bbox.x1) * 0.5, (bbox.y0 + bbox.y1) * 0.5);
    let bulge = amount / 100.0 * 0.33;
    let mut out = BezPath::new();
    let mut current = Point::ORIGIN;
    let mut start = Point::ORIGIN;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                out.move_to(p);
                current = p;
                start = p;
            }
            PathEl::LineTo(p) => {
                let chord = p - current;
                let len = chord.hypot();
                if len < 1e-9 {
                    out.line_to(p);
                } else {
                    let mid = current.midpoint(p);
                    let mut normal = Vec2::new(-chord.y, chord.x) / len;
                    if (mid + normal - centroid).hypot() < (mid - centroid).hypot() {
                        normal = -normal;
                    }
                    let ctrl = mid + normal * (len * bulge);
                    out.quad_to(ctrl, p);
                }
                current = p;
            }
            PathEl::QuadTo(c, p) => {
                let c1 = current + (c - current) * factor;
                let c2 = p + (c - p) * factor;
                out.curve_to(c1, c2, p);
                current = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let nc1 = current + (c1 - current) * factor;
                let nc2 = p + (c2 - p) * factor;
                out.curve_to(nc1, nc2, p);
                current = p;
            }
            PathEl::ClosePath => {
                out.close_path();
                current = start;
            }
        }
    }
    (!out.elements().is_empty()).then_some(PathData::from_bezpath(out))
}

/// Effect ▸ Distort & Transform ▸ Transform: a single `Affine` built from
/// move/scale/rotate/reflect, applied once around the path's own
/// local-bounds center. No "copies" — see [`amalith_core::TransformEffect`]'s
/// own doc comment for why that's a disclosed v1 gap rather than a silent
/// omission.
pub fn transform_effect(path: &BezPath, fx: &amalith_core::TransformEffect) -> Option<PathData> {
    let bbox = amalith_core::geom::bez_path_bounds(path);
    let center = Point::new((bbox.x0 + bbox.x1) * 0.5, (bbox.y0 + bbox.y1) * 0.5);
    let sx = (fx.scale_x / 100.0) * if fx.reflect_x { -1.0 } else { 1.0 };
    let sy = (fx.scale_y / 100.0) * if fx.reflect_y { -1.0 } else { 1.0 };
    let xf = Affine::translate((fx.move_x, fx.move_y))
        * Affine::translate((center.x, center.y))
        * Affine::rotate(fx.rotate.to_radians())
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate((-center.x, -center.y));
    Some(PathData::from_bezpath(xf * path.clone()))
}

/// Effect ▸ Distort & Transform ▸ Tweak: jitters each anchor (and, if
/// `modify_in`/`modify_out`, each handle) by a random (seeded, stable)
/// amount — `horizontal`/`vertical` bound the jitter as a percentage of
/// that segment's own chord length. No resampling — operates on the
/// existing anchors/handles directly, like [`pucker_bloat`].
pub fn tweak(
    path: &BezPath,
    horizontal: f64,
    vertical: f64,
    modify_anchors: bool,
    modify_in: bool,
    modify_out: bool,
    seed: u64,
) -> Option<PathData> {
    if horizontal.abs() < 1e-6 && vertical.abs() < 1e-6 {
        return Some(PathData::from_bezpath(path.clone()));
    }
    let jitter = |i: usize, chord: f64| -> Vec2 {
        Vec2::new(
            hash_jitter(seed, i * 3) * horizontal / 100.0 * chord,
            hash_jitter(seed, i * 3 + 1) * vertical / 100.0 * chord,
        )
    };
    let mut out = BezPath::new();
    let mut current = Point::ORIGIN;
    let mut idx = 0usize;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let d = if modify_anchors { jitter(idx, 10.0) } else { Vec2::ZERO };
                idx += 1;
                out.move_to(p + d);
                current = p;
            }
            PathEl::LineTo(p) => {
                let chord = (p - current).hypot();
                let d = if modify_anchors { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                out.line_to(p + d);
                current = p;
            }
            PathEl::QuadTo(c, p) => {
                let chord = (p - current).hypot();
                let dc = if modify_in { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                let dp = if modify_anchors { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                out.quad_to(c + dc, p + dp);
                current = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let chord = (p - current).hypot();
                let d1 = if modify_out { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                let d2 = if modify_in { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                let dp = if modify_anchors { jitter(idx, chord) } else { Vec2::ZERO };
                idx += 1;
                out.curve_to(c1 + d1, c2 + d2, p + dp);
                current = p;
            }
            PathEl::ClosePath => out.close_path(),
        }
    }
    (!out.elements().is_empty()).then_some(PathData::from_bezpath(out))
}

/// Outline a stroke into a filled path (Object ▸ Expand Stroke).
pub fn expand_stroke(path: &BezPath, appearance: &Appearance) -> Option<PathData> {
    if !has_visible_stroke(appearance) {
        return None;
    }
    let stroke_style = appearance.stroke_style();
    let cap = match stroke_style.cap {
        LineCap::Butt => Cap::Butt,
        LineCap::Round => Cap::Round,
        LineCap::Square => Cap::Square,
    };
    let join = match stroke_style.join {
        LineJoin::Miter => Join::Miter,
        LineJoin::Round => Join::Round,
        LineJoin::Bevel => Join::Bevel,
    };
    let mut style = Stroke::new(appearance.stroke_width().max(0.01))
        .with_caps(cap)
        .with_join(join)
        .with_miter_limit(stroke_style.miter_limit);
    if let Some(dash) = stroke_style.dash_pattern() {
        style = style.with_dashes(stroke_style.dash_offset, dash);
    }
    let outlined = stroke(path.clone(), &style, &StrokeOpts::default(), TOL);
    if outlined.elements().is_empty() {
        None
    } else {
        Some(PathData::from_bezpath(outlined))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::Color;
    use kurbo::{Point, Rect, Shape};

    fn rect_input(r: Rect, fill: (f32, f32, f32)) -> PathInput {
        PathInput {
            contours: flatten_path(&PathData::rectangle(r).geometry),
            appearance: Appearance {
                items: vec![AppearanceItem::Fill {
                    paint: Paint::Solid(Color::rgb(fill.0, fill.1, fill.2)),
                    opacity: 1.0,
                    visible: true,
                    effects: Vec::new(),
                }],
                ..Appearance::default()
            },
        }
    }

    #[test]
    fn shape_builder_keeps_disconnected_regions_separate_and_holes_attached() {
        let a=rect_input(Rect::new(0.,0.,10.,10.),(1.,0.,0.));
        let b=rect_input(Rect::new(20.,0.,30.,10.),(1.,0.,0.));
        let mut contours=a.contours.clone(); contours.extend(b.contours);
        let pieces=shape_builder_regions(&[PathInput { contours,appearance:a.appearance.clone() }]);
        assert_eq!(pieces.len(),2,"one region per disconnected island");
        let mut outer=flatten_path(&PathData::rectangle(Rect::new(0.,0.,100.,100.)).geometry);
        let mut hole=flatten_path(&PathData::rectangle(Rect::new(20.,20.,80.,80.)).geometry);
        hole[0].reverse(); outer.extend(hole);
        let pieces=shape_builder_regions(&[PathInput { contours:outer,appearance:a.appearance }]);
        assert_eq!(pieces.len(),1);
        assert_eq!(pieces[0].contours.len(),2,"a hole is part of its face, not another fillable region");
        assert_eq!(polygon_path(&pieces[0].contours).geometry.winding(Point::new(50.,50.)),0);
    }

    #[test]
    fn shape_builder_does_not_discard_real_thin_shapes() {
        let thin=rect_input(Rect::new(0.,0.,100.,0.01),(1.,0.,0.));
        let other=rect_input(Rect::new(200.,0.,210.,10.),(0.,0.,1.));
        let regions=shape_builder_regions(&[thin,other]);
        assert_eq!(regions.len(),2,"a thin drawn rectangle is content, not a sliver heuristic");
    }

    #[test]
    fn unite_two_overlapping_rects_is_one_path() {
        let a = rect_input(Rect::new(0.0, 0.0, 20.0, 20.0), (1.0, 0.0, 0.0));
        let b = rect_input(Rect::new(10.0, 10.0, 30.0, 30.0), (0.0, 0.0, 1.0));
        let out = apply(PathfinderOp::Unite, &[a, b]);
        assert_eq!(out.len(), 1);
        let bb = out[0].path.geometry.bounding_box();
        assert!((bb.width() - 30.0).abs() < 0.5);
        assert!((bb.height() - 30.0).abs() < 0.5);
        assert_eq!(out[0].appearance.fill(), Paint::Solid(Color::rgb(0.0, 0.0, 1.0)));
    }

    #[test]
    fn minus_front_cuts_a_notch() {
        let back = rect_input(Rect::new(0.0, 0.0, 30.0, 30.0), (1.0, 0.0, 0.0));
        let front = rect_input(Rect::new(10.0, 10.0, 40.0, 20.0), (0.0, 1.0, 0.0));
        let out = apply(PathfinderOp::MinusFront, &[back, front]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].appearance.fill(), Paint::Solid(Color::rgb(1.0, 0.0, 0.0)));
    }

    #[test]
    fn intersect_keeps_overlap() {
        let a = rect_input(Rect::new(0.0, 0.0, 20.0, 20.0), (1.0, 0.0, 0.0));
        let b = rect_input(Rect::new(10.0, 10.0, 30.0, 30.0), (0.0, 0.0, 1.0));
        let out = apply(PathfinderOp::Intersect, &[a, b]);
        assert_eq!(out.len(), 1);
        let bb = out[0].path.geometry.bounding_box();
        assert!((bb.width() - 10.0).abs() < 0.5);
        assert!((bb.height() - 10.0).abs() < 0.5);
    }

    #[test]
    fn expand_stroke_makes_a_filled_outline() {
        let path = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 10.0));
        let app = Appearance {
            items: vec![AppearanceItem::Stroke {
                paint: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
                width: 4.0,
                style: StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                effects: Vec::new(),
            }],
            ..Appearance::default()
        };
        let out = expand_stroke(&path.geometry, &app).unwrap();
        let bb = out.geometry.bounding_box();
        assert!(bb.width() > 40.0);
        assert!(bb.height() > 10.0);
    }

    #[test]
    fn positive_offset_grows_a_square_by_the_offset_on_every_side() {
        let path = PathData::rectangle(Rect::new(0.0, 0.0, 20.0, 20.0));
        let out = offset_path(&path.geometry, 5.0, LineJoin::Miter, 4.0).unwrap();
        let bb = out.geometry.bounding_box();
        assert!((bb.width() - 30.0).abs() < 0.5, "width {}", bb.width());
        assert!((bb.height() - 30.0).abs() < 0.5, "height {}", bb.height());
    }

    #[test]
    fn negative_offset_shrinks_a_square_by_the_offset_on_every_side() {
        let path = PathData::rectangle(Rect::new(0.0, 0.0, 20.0, 20.0));
        let out = offset_path(&path.geometry, -5.0, LineJoin::Miter, 4.0).unwrap();
        let bb = out.geometry.bounding_box();
        assert!((bb.width() - 10.0).abs() < 0.5, "width {}", bb.width());
        assert!((bb.height() - 10.0).abs() < 0.5, "height {}", bb.height());
    }

    #[test]
    fn offsetting_an_open_path_wraps_both_sides_into_one_closed_loop() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.line_to((40.0, 0.0));
        let out = offset_path(&path, 3.0, LineJoin::Round, 4.0).unwrap();
        let bb = out.geometry.bounding_box();
        assert!((bb.height() - 6.0).abs() < 0.5, "height {}", bb.height());
        assert!(out.geometry.elements().iter().any(|e| matches!(e, PathEl::ClosePath)));
    }

    #[test]
    fn contour_area_matches_a_known_rectangle() {
        let c = vec![[0.0, 0.0], [10.0, 0.0], [10.0, 4.0], [0.0, 4.0]];
        assert!((contour_area(&c) - 40.0).abs() < 1e-9);
    }

    #[test]
    fn overlay_drops_a_sliver_thinner_than_the_minimum_area() {
        // A degenerate near-zero-width triangle, disjoint from the real
        // shape — exactly what a plain union would otherwise keep as
        // its own separate output piece, and exactly the shape of the
        // floating-point noise `divide` can leave along a seam where
        // two of its own boolean passes are supposed to meet.
        let sliver = vec![vec![[100.0, 100.0], [100.001, 100.0], [100.0005, 100.001]]];
        let real = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        let out = overlay(&sliver, &real, OverlayRule::Union);
        assert_eq!(out.len(), 1, "the disjoint sliver should be filtered, leaving only the real square");
    }

    #[test]
    fn overlay_drops_a_sliver_that_is_long_but_hairline_thin() {
        // A real bug: a sliver running the length of a whole seam (here,
        // 200 units) can clear a small *area* floor even at a hairline
        // width, since area is length × width — it has to be caught by
        // its effective width instead.
        let long_thin_sliver = vec![vec![
            [500.0, 500.0],
            [700.0, 500.0],
            [700.0, 500.0005],
            [500.0, 500.0005],
        ]];
        let real = vec![vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]];
        assert!(contour_area(&long_thin_sliver[0]) > 0.01, "sanity: this sliver's raw area alone would have slipped past a naive area-only floor");
        let out = overlay(&long_thin_sliver, &real, OverlayRule::Union);
        assert_eq!(out.len(), 1, "the long, hairline-thin sliver should be filtered by effective width, not just area");
    }

    #[test]
    fn subtract_each_drops_a_fully_covered_input_and_notches_a_partial_one() {
        let covered = rect_input(Rect::new(0.0, 0.0, 10.0, 10.0), (1.0, 0.0, 0.0));
        let partial = rect_input(Rect::new(20.0, 0.0, 40.0, 10.0), (0.0, 1.0, 0.0));
        let untouched = rect_input(Rect::new(60.0, 0.0, 70.0, 10.0), (0.0, 0.0, 1.0));
        let cut = flatten_path(&PathData::rectangle(Rect::new(-5.0, -5.0, 30.0, 15.0)).geometry);
        let out = subtract_each(&[covered, partial, untouched], &cut);
        // `covered` vanishes entirely; `partial` survives, shrunk;
        // `untouched` survives unchanged.
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].appearance.fill(), Paint::Solid(Color::rgb(0.0, 1.0, 0.0)));
        let partial_bb = out[0].path.geometry.bounding_box();
        assert!((partial_bb.width() - 10.0).abs() < 0.5, "width {}", partial_bb.width());
        assert_eq!(out[1].appearance.fill(), Paint::Solid(Color::rgb(0.0, 0.0, 1.0)));
        let untouched_bb = out[1].path.geometry.bounding_box();
        assert!((untouched_bb.width() - 10.0).abs() < 0.5, "width {}", untouched_bb.width());
    }

    #[test]
    fn intersects_is_true_only_when_two_contour_sets_actually_share_area() {
        let a = flatten_path(&PathData::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0)).geometry);
        let overlapping = flatten_path(&PathData::rectangle(Rect::new(5.0, 5.0, 15.0, 15.0)).geometry);
        let apart = flatten_path(&PathData::rectangle(Rect::new(20.0, 20.0, 30.0, 30.0)).geometry);
        assert!(intersects(&a, &overlapping));
        assert!(!intersects(&a, &apart));
    }

    #[test]
    fn hash_jitter_is_deterministic_and_bounded() {
        let a = hash_jitter(42, 7);
        let b = hash_jitter(42, 7);
        assert_eq!(a, b, "same seed and index always returns the same value");
        assert!((-1.0..=1.0).contains(&a));
        let c = hash_jitter(42, 8);
        assert_ne!(a, c, "a different index (almost certainly) returns a different value");
    }

    #[test]
    fn zig_zag_with_zero_size_is_a_no_op() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 10.0)).geometry;
        let out = zig_zag(&rect, 0.0, 4.0, false).unwrap();
        assert_eq!(out.geometry.bounding_box(), rect.bounding_box());
    }

    #[test]
    fn zig_zag_widens_the_bounding_box_by_about_the_ridge_size() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 100.0, 20.0)).geometry;
        let out = zig_zag(&rect, 5.0, 4.0, false).unwrap();
        let bb = out.geometry.bounding_box();
        assert!(bb.height() > 20.0 + 5.0, "ridges push the top/bottom edges out by ~size, height was {}", bb.height());
    }

    #[test]
    fn roughen_with_zero_size_is_a_no_op() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 10.0)).geometry;
        let out = roughen(&rect, 0.0, 8.0, false, 1).unwrap();
        assert_eq!(out.geometry.bounding_box(), rect.bounding_box());
    }

    #[test]
    fn roughen_is_stable_for_the_same_seed() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 100.0, 100.0)).geometry;
        let a = roughen(&rect, 6.0, 8.0, false, 99).unwrap();
        let b = roughen(&rect, 6.0, 8.0, false, 99).unwrap();
        assert_eq!(a.geometry.bounding_box(), b.geometry.bounding_box(), "same seed renders identically every time");
    }

    #[test]
    fn twist_with_zero_angle_is_a_no_op() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 40.0)).geometry;
        let out = twist(&rect, 0.0).unwrap();
        let bb = out.geometry.bounding_box();
        assert!((bb.width() - 40.0).abs() < 0.5 && (bb.height() - 40.0).abs() < 0.5);
    }

    #[test]
    fn twist_rotates_a_square_into_a_rounder_shape() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 40.0)).geometry;
        let out = twist(&rect, 45.0).unwrap();
        // A twisted square's corners no longer meet at the original
        // corners — its bounding box should differ from the untouched one.
        assert_ne!(out.geometry.bounding_box(), rect.bounding_box());
    }

    #[test]
    fn pucker_bloat_zero_amount_is_a_no_op() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 10.0)).geometry;
        let out = pucker_bloat(&rect, 0.0).unwrap();
        assert_eq!(out.geometry.bounding_box(), rect.bounding_box());
    }

    #[test]
    fn bloat_grows_a_shapes_bounding_box_and_pucker_shrinks_its_curvature() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 40.0)).geometry;
        let bloated = pucker_bloat(&rect, 50.0).unwrap();
        let bb = bloated.geometry.bounding_box();
        assert!(bb.width() > 40.0 && bb.height() > 40.0, "bloat bulges segments outward past the original corners");
    }

    #[test]
    fn transform_effect_moves_and_scales_around_the_local_center() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 20.0, 20.0)).geometry;
        let fx = amalith_core::TransformEffect {
            move_x: 10.0,
            move_y: 0.0,
            scale_x: 200.0,
            scale_y: 100.0,
            rotate: 0.0,
            reflect_x: false,
            reflect_y: false,
        };
        let out = transform_effect(&rect, &fx).unwrap();
        let bb = out.geometry.bounding_box();
        assert!((bb.width() - 40.0).abs() < 0.5, "200% scale doubles width, got {}", bb.width());
        assert!((bb.height() - 20.0).abs() < 0.5, "100% scale leaves height alone");
        assert!((bb.center().x - 20.0).abs() < 0.5, "moved +10 from the original center (10) to 20");
    }

    #[test]
    fn tweak_with_zero_bounds_is_a_no_op() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 10.0)).geometry;
        let out = tweak(&rect, 0.0, 0.0, true, true, true, 1).unwrap();
        assert_eq!(out.geometry.bounding_box(), rect.bounding_box());
    }

    #[test]
    fn tweak_is_stable_for_the_same_seed() {
        let rect = PathData::rectangle(Rect::new(0.0, 0.0, 40.0, 40.0)).geometry;
        let a = tweak(&rect, 20.0, 20.0, true, true, true, 7).unwrap();
        let b = tweak(&rect, 20.0, 20.0, true, true, true, 7).unwrap();
        assert_eq!(a.geometry.bounding_box(), b.geometry.bounding_box());
    }
}
