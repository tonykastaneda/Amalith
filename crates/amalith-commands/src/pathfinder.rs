//! Pathfinder boolean ops and stroke expansion.
//!
//! Geometry is flattened to polygons, run through `i_overlay`, then rebuilt
//! as [`PathData`]. Results live in the same coordinate space as the input
//! contours (callers bake world/parent transforms).

use amalith_core::{
    Appearance, LineCap, LineJoin, Paint, PathData,
};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use i_overlay::mesh::outline::offset::OutlineOffset;
use kurbo::{flatten, stroke, BezPath, Cap, Join, PathEl, Point, Stroke, StrokeOpts};

use crate::command::PathfinderOp;

const TOL: f64 = 0.25;

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

fn contours_to_path(contours: &[Vec<[f64; 2]>]) -> Option<PathData> {
    if contours.is_empty() {
        return None;
    }
    let mut path = BezPath::new();
    for c in contours {
        if c.len() < 3 {
            continue;
        }
        path.move_to(Point::new(c[0][0], c[0][1]));
        for p in &c[1..] {
            path.line_to(Point::new(p[0], p[1]));
        }
        path.close_path();
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
    shapes.into_iter().flatten().collect()
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
        .map(|path| PathResult { path, appearance })
        .collect()
}

fn no_stroke(mut a: Appearance) -> Appearance {
    a.stroke = Paint::None;
    a
}

/// Run a Pathfinder op. `inputs` is back → front.
pub fn apply(op: PathfinderOp, inputs: &[PathInput]) -> Vec<PathResult> {
    if inputs.is_empty() {
        return Vec::new();
    }
    match op {
        PathfinderOp::Unite => {
            let all: Vec<_> = inputs.iter().map(|i| i.contours.clone()).collect();
            paths_from_contours(union_all(&all), inputs.last().unwrap().appearance)
        }
        PathfinderOp::MinusFront => {
            let back = &inputs[0];
            let rest: Vec<_> = inputs[1..].iter().map(|i| i.contours.clone()).collect();
            let cut = union_all(&rest);
            paths_from_contours(overlay(&back.contours, &cut, OverlayRule::Difference), back.appearance)
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
                front.appearance,
            )
        }
        PathfinderOp::Intersect => {
            let mut acc = inputs[0].contours.clone();
            for next in &inputs[1..] {
                acc = overlay(&acc, &next.contours, OverlayRule::Intersect);
            }
            paths_from_contours(acc, inputs.last().unwrap().appearance)
        }
        PathfinderOp::Exclude => {
            let mut acc = inputs[0].contours.clone();
            for next in &inputs[1..] {
                acc = overlay(&acc, &next.contours, OverlayRule::Xor);
            }
            paths_from_contours(acc, inputs.last().unwrap().appearance)
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
        return paths_from_contours(inputs[0].contours.clone(), inputs[0].appearance);
    }
    // Pieces are (contours, appearance of the topmost covering original).
    let mut pieces: Vec<(Vec<Vec<[f64; 2]>>, Appearance)> =
        vec![(inputs[0].contours.clone(), inputs[0].appearance)];
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
                out.push((hit.clone(), next.appearance));
                covered = overlay(&covered, &hit, OverlayRule::Union);
            }
        }
        let novel = overlay(&next.contours, &covered, OverlayRule::Difference);
        if !novel.is_empty() {
            out.push((novel, next.appearance));
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
                    appearance: no_stroke(input.appearance),
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
            .find(|(a, _)| a.fill == p.appearance.fill && a.opacity == p.appearance.opacity)
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
                appearance: no_stroke(input.appearance),
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
            r.appearance.fill = Paint::None;
            if r.appearance.stroke == Paint::None {
                r.appearance.stroke = Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0));
                r.appearance.stroke_width = r.appearance.stroke_width.max(1.0);
            }
            r
        })
        .collect()
}

pub fn has_visible_stroke(a: &Appearance) -> bool {
    a.stroke != Paint::None && a.stroke_width > 0.05
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

/// Outline a stroke into a filled path (Object ▸ Expand Stroke).
pub fn expand_stroke(path: &BezPath, appearance: &Appearance) -> Option<PathData> {
    if !has_visible_stroke(appearance) {
        return None;
    }
    let cap = match appearance.stroke_style.cap {
        LineCap::Butt => Cap::Butt,
        LineCap::Round => Cap::Round,
        LineCap::Square => Cap::Square,
    };
    let join = match appearance.stroke_style.join {
        LineJoin::Miter => Join::Miter,
        LineJoin::Round => Join::Round,
        LineJoin::Bevel => Join::Bevel,
    };
    let mut style = Stroke::new(appearance.stroke_width.max(0.01))
        .with_caps(cap)
        .with_join(join)
        .with_miter_limit(appearance.stroke_style.miter_limit);
    if let Some(dash) = appearance.stroke_style.dash_pattern() {
        style = style.with_dashes(appearance.stroke_style.dash_offset, dash);
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
    use kurbo::{Rect, Shape};

    fn rect_input(r: Rect, fill: (f32, f32, f32)) -> PathInput {
        PathInput {
            contours: flatten_path(&PathData::rectangle(r).geometry),
            appearance: Appearance {
                fill: Paint::Solid(Color::rgb(fill.0, fill.1, fill.2)),
                stroke: Paint::None,
                ..Appearance::default()
            },
        }
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
        assert_eq!(out[0].appearance.fill, Paint::Solid(Color::rgb(0.0, 0.0, 1.0)));
    }

    #[test]
    fn minus_front_cuts_a_notch() {
        let back = rect_input(Rect::new(0.0, 0.0, 30.0, 30.0), (1.0, 0.0, 0.0));
        let front = rect_input(Rect::new(10.0, 10.0, 40.0, 20.0), (0.0, 1.0, 0.0));
        let out = apply(PathfinderOp::MinusFront, &[back, front]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].appearance.fill, Paint::Solid(Color::rgb(1.0, 0.0, 0.0)));
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
            fill: Paint::None,
            stroke: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
            stroke_width: 4.0,
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
}
