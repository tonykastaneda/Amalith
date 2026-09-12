//! Blend-tool geometry and color interpolation: producing one in-between
//! step's outline and paint at parameter `t` (0..1) between two source
//! shapes, and the "Smooth Color" step-count heuristic.
//!
//! A step's outline is built by resampling both source outlines to an
//! equal point count via arc length, sidestepping anchor-count /
//! correspondence entirely. The trade-off: a step is always a many-point
//! corner-anchor polygon, not the sources' original bezier curves — at
//! [`BLEND_SAMPLES`] points per subpath this reads as smooth as the
//! originals at ordinary zoom levels, but isn't an exact re-derivation of
//! their curves the way Illustrator's own (unpublished) blend algorithm
//! is. Each shape is detached from its own center before blending, then
//! the blended outline is re-placed at the step's own target center — the
//! straight line between the sources' centers by default, or a point
//! along a spine path (see [`point_on_path`]). This decouples *where* a
//! step sits from *what shape* it is, which is what makes Replace Spine
//! possible: steps still morph start-shape to end-shape while their
//! centers walk the spine instead of the line.

use crate::appearance::{
    Effect, OffsetEffect, Paint, PuckerBloatEffect, RoughenEffect, TransformEffect, TweakEffect,
    TwistEffect, ZigZagEffect,
};
use crate::geom::{Point, Vec2};
use crate::object::{Anchor, PathData, Subpath};
use crate::swatch::Color;

/// Points per interpolated subpath.
const BLEND_SAMPLES: usize = 64;

/// Average of every point across every subpath — "center" for the
/// purposes of detaching a shape from its position before blending.
/// `Point::ORIGIN` for an empty shape.
pub fn shape_center(subpaths: &[Vec<Point>]) -> Point {
    let mut sum = Vec2::ZERO;
    let mut n = 0usize;
    for sp in subpaths {
        for &p in sp {
            sum += p.to_vec2();
            n += 1;
        }
    }
    if n == 0 {
        Point::ORIGIN
    } else {
        Point::ORIGIN + sum / n as f64
    }
}

fn lerp_point(a: Point, b: Point, t: f64) -> Point {
    Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

fn lerp_vec2(a: Vec2, b: Vec2, t: f64) -> Vec2 {
    Vec2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// Cumulative arc length at each point of `points`, plus the closing
/// segment back to `points[0]` when `closed`.
fn arc_lengths(points: &[Point], closed: bool) -> (Vec<Point>, Vec<f64>) {
    let mut pts = points.to_vec();
    if closed {
        if let Some(&first) = points.first() {
            pts.push(first);
        }
    }
    let mut cum = vec![0.0f64; pts.len()];
    for i in 1..pts.len() {
        cum[i] = cum[i - 1] + (pts[i] - pts[i - 1]).hypot();
    }
    (pts, cum)
}

/// Resamples a point loop to exactly `n` points, evenly spaced by arc
/// length. `closed` includes the closing segment in the total (matching
/// [`crate::object::Subpath::closed`]); an open loop runs start to end
/// without wrapping.
fn resample(points: &[Point], closed: bool, n: usize) -> Vec<Point> {
    if n == 0 || points.is_empty() {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0]; n];
    }
    let (pts, cum) = arc_lengths(points, closed);
    let total = *cum.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return vec![pts[0]; n];
    }
    let denom = if closed { n as f64 } else { (n - 1).max(1) as f64 };
    (0..n)
        .map(|i| {
            let target = (total * i as f64 / denom).min(total);
            let seg = cum
                .iter()
                .rposition(|&c| c <= target)
                .unwrap_or(0)
                .min(pts.len() - 2);
            let (c0, c1) = (cum[seg], cum[seg + 1]);
            let frac = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.0 };
            lerp_point(pts[seg], pts[seg + 1], frac)
        })
        .collect()
}

/// A point at arc-length fraction `t` (0 = start, 1 = end, clamped) along
/// an open polyline — a spine is a path, never treated as closed here
/// even if the object itself is a closed shape.
pub fn point_on_path(points: &[Point], t: f64) -> Point {
    let Some(&first) = points.first() else {
        return Point::ORIGIN;
    };
    if points.len() == 1 {
        return first;
    }
    let (pts, cum) = arc_lengths(points, false);
    let total = *cum.last().unwrap_or(&0.0);
    if total <= 0.0 {
        return first;
    }
    let target = total * t.clamp(0.0, 1.0);
    let seg = cum
        .iter()
        .rposition(|&c| c <= target)
        .unwrap_or(0)
        .min(pts.len() - 2);
    let (c0, c1) = (cum[seg], cum[seg + 1]);
    let frac = if c1 > c0 { (target - c0) / (c1 - c0) } else { 0.0 };
    lerp_point(pts[seg], pts[seg + 1], frac)
}

/// One interpolated step's outline, `t` of the way from `a` to `b`. Both
/// are already flattened into the blend group's local space (each
/// subpath's points alongside its `closed` flag), subpaths paired by
/// index up to the shorter side's count. `center_a`/`center_b` are
/// [`shape_center`] of `a`/`b`; the result is re-centered on `center`
/// (the step's own target point).
pub fn interpolate_step(
    a: &[(Vec<Point>, bool)],
    b: &[(Vec<Point>, bool)],
    center_a: Point,
    center_b: Point,
    center: Point,
    t: f64,
) -> PathData {
    let n = a.len().min(b.len());
    let mut subpaths = Vec::with_capacity(n);
    for i in 0..n {
        let (pa, closed) = &a[i];
        let (pb, _) = &b[i];
        let ra = resample(pa, *closed, BLEND_SAMPLES);
        let rb = resample(pb, *closed, BLEND_SAMPLES);
        let anchors: Vec<Anchor> = ra
            .iter()
            .zip(rb.iter())
            .map(|(&pa, &pb)| {
                let rel = lerp_vec2(pa - center_a, pb - center_b, t);
                Anchor::corner(center + rel)
            })
            .collect();
        subpaths.push(Subpath { anchors, closed: *closed });
    }
    PathData::from_subpaths(subpaths)
}

/// `a` and `b`'s paint, `t` of the way from one to the other. Only two
/// solid colors interpolate smoothly (real per-channel lerp); any other
/// combination (a gradient, or no fill) just switches at the midpoint,
/// since blending gradients or `None` into a color has no single obvious
/// answer.
pub fn lerp_paint(a: Paint, b: Paint, t: f64) -> Paint {
    match (a, b) {
        (Paint::Solid(ca), Paint::Solid(cb)) => Paint::Solid(Color {
            r: ca.r + (cb.r - ca.r) * t as f32,
            g: ca.g + (cb.g - ca.g) * t as f32,
            b: ca.b + (cb.b - ca.b) * t as f32,
            a: ca.a + (cb.a - ca.a) * t as f32,
        }),
        _ => {
            if t < 0.5 {
                a
            } else {
                b
            }
        }
    }
}

/// "Smooth Color" step count between two fills: enough steps that
/// adjacent ones differ by roughly one sRGB unit (0..255 per channel,
/// averaged across channels) at most. Illustrator's own rule for this is
/// unpublished — this approximates the visible effect (near-identical
/// colors need few steps, black-to-white needs many) rather than
/// reproducing an exact formula. Anything that isn't two solid colors
/// falls back to a fixed, moderate step count. 256 mirrors Illustrator's
/// own published ceiling on blend steps.
pub fn smooth_color_steps(a: Paint, b: Paint) -> u32 {
    let (Paint::Solid(ca), Paint::Solid(cb)) = (a, b) else {
        return 25;
    };
    let d = ((ca.r - cb.r).abs() + (ca.g - cb.g).abs() + (ca.b - cb.b).abs()) / 3.0;
    ((d * 255.0).round() as u32).clamp(1, 256)
}

/// `a` and `b`, `t` of the way from one to the other — only defined for
/// two effects of the *same* variant (the caller checks discriminants
/// first, see [`lerp_effect_stack`]). Every numeric field lerps; a
/// boolean/style field just switches at the midpoint, the same "no
/// obvious half-and-half" fallback [`lerp_paint`] uses for a paint kind
/// mismatch.
fn lerp_effect_same_kind(a: Effect, b: Effect, t: f64) -> Effect {
    let f = |x: f64, y: f64| x + (y - x) * t;
    let flag = |x: bool, y: bool| if t < 0.5 { x } else { y };
    match (a, b) {
        (Effect::Offset(a), Effect::Offset(b)) => Effect::Offset(OffsetEffect {
            amount: f(a.amount, b.amount),
            join: if t < 0.5 { a.join } else { b.join },
            miter_limit: f(a.miter_limit, b.miter_limit),
        }),
        (Effect::ZigZag(a), Effect::ZigZag(b)) => Effect::ZigZag(ZigZagEffect {
            size: f(a.size, b.size),
            ridges_per_segment: f(a.ridges_per_segment, b.ridges_per_segment),
            smooth: flag(a.smooth, b.smooth),
        }),
        (Effect::PuckerBloat(a), Effect::PuckerBloat(b)) => {
            Effect::PuckerBloat(PuckerBloatEffect { amount: f(a.amount, b.amount) })
        }
        (Effect::Roughen(a), Effect::Roughen(b)) => Effect::Roughen(RoughenEffect {
            size: f(a.size, b.size),
            detail: f(a.detail, b.detail),
            smooth: flag(a.smooth, b.smooth),
            seed: if t < 0.5 { a.seed } else { b.seed },
        }),
        (Effect::Transform(a), Effect::Transform(b)) => Effect::Transform(TransformEffect {
            move_x: f(a.move_x, b.move_x),
            move_y: f(a.move_y, b.move_y),
            scale_x: f(a.scale_x, b.scale_x),
            scale_y: f(a.scale_y, b.scale_y),
            rotate: f(a.rotate, b.rotate),
            reflect_x: flag(a.reflect_x, b.reflect_x),
            reflect_y: flag(a.reflect_y, b.reflect_y),
        }),
        (Effect::Tweak(a), Effect::Tweak(b)) => Effect::Tweak(TweakEffect {
            horizontal: f(a.horizontal, b.horizontal),
            vertical: f(a.vertical, b.vertical),
            modify_anchors: flag(a.modify_anchors, b.modify_anchors),
            modify_in: flag(a.modify_in, b.modify_in),
            modify_out: flag(a.modify_out, b.modify_out),
            seed: if t < 0.5 { a.seed } else { b.seed },
        }),
        (Effect::Twist(a), Effect::Twist(b)) => Effect::Twist(TwistEffect { angle: f(a.angle, b.angle) }),
        // Unreachable via `lerp_effect_stack` (it only calls this once the
        // two discriminants have already been checked equal) — kept total
        // rather than panicking so a future caller mistake degrades to
        // "picks one" instead of crashing.
        (a, _) => a,
    }
}

/// The same effect kind as `e`, with its visible strength dialed to
/// zero (every other field copied from `e` unchanged) — the "nothing"
/// counterpart [`lerp_effect_stack`] blends a real effect toward when
/// the other side of a blend has no matching effect at that slot, so
/// the effect fades out smoothly (amplitude shrinking to 0) instead of
/// vanishing outright partway through the blend.
fn neutral_like(e: Effect) -> Effect {
    match e {
        Effect::Offset(fx) => Effect::Offset(OffsetEffect { amount: 0.0, ..fx }),
        Effect::ZigZag(fx) => Effect::ZigZag(ZigZagEffect { size: 0.0, ..fx }),
        Effect::PuckerBloat(_) => Effect::PuckerBloat(PuckerBloatEffect { amount: 0.0 }),
        Effect::Roughen(fx) => Effect::Roughen(RoughenEffect { size: 0.0, ..fx }),
        Effect::Transform(fx) => Effect::Transform(TransformEffect {
            move_x: 0.0,
            move_y: 0.0,
            scale_x: 100.0,
            scale_y: 100.0,
            rotate: 0.0,
            ..fx
        }),
        Effect::Tweak(fx) => Effect::Tweak(TweakEffect { horizontal: 0.0, vertical: 0.0, ..fx }),
        Effect::Twist(_) => Effect::Twist(TwistEffect { angle: 0.0 }),
    }
}

/// One Fill or Stroke item's whole effect stack, `t` of the way from `a`
/// to `b`. Real Illustrator has no published rule for blending a live
/// Appearance effect stack through a blend's generated steps at all —
/// this is Amalith's own: matched position-by-position (the common case
/// is both stacks the same length with the same kind at each slot, e.g.
/// both blend endpoints have one Zig Zag) rather than requiring the
/// *whole* stack to match before interpolating anything. When one side
/// is simply missing an effect the other side has — blending a plain
/// line into a Zig Zagged one, say — that missing slot fills in with
/// [`neutral_like`] the effect that's actually present, so it fades
/// smoothly toward (or away from) zero strength across the generated
/// steps instead of the effect just vanishing at the halfway step. Only
/// a genuine kind mismatch *at the same slot* (both sides have
/// something there, but a different effect) falls back to switching
/// that one slot at the midpoint, since there's no shared "neutral" two
/// different kinds can fade toward.
pub fn lerp_effect_stack(a: &[Effect], b: &[Effect], t: f64) -> Vec<Effect> {
    let n = a.len().max(b.len());
    (0..n)
        .map(|i| match (a.get(i).copied(), b.get(i).copied()) {
            (Some(x), Some(y)) if std::mem::discriminant(&x) == std::mem::discriminant(&y) => {
                lerp_effect_same_kind(x, y, t)
            }
            (Some(x), Some(y)) => {
                if t < 0.5 {
                    x
                } else {
                    y
                }
            }
            (Some(x), None) => lerp_effect_same_kind(x, neutral_like(x), t),
            (None, Some(y)) => lerp_effect_same_kind(neutral_like(y), y, t),
            (None, None) => unreachable!("i < n == a.len().max(b.len())"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_center_of_a_symmetric_square_is_its_middle() {
        let square = vec![vec![
            Point::new(-10.0, -10.0),
            Point::new(10.0, -10.0),
            Point::new(10.0, 10.0),
            Point::new(-10.0, 10.0),
        ]];
        let c = shape_center(&square);
        assert!(c.x.abs() < 1e-9 && c.y.abs() < 1e-9);
    }

    #[test]
    fn point_on_path_hits_the_endpoints_and_midpoint_of_a_line() {
        let line = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
        assert_eq!(point_on_path(&line, 0.0), Point::new(0.0, 0.0));
        assert_eq!(point_on_path(&line, 1.0), Point::new(10.0, 0.0));
        assert_eq!(point_on_path(&line, 0.5), Point::new(5.0, 0.0));
    }

    #[test]
    fn lerp_paint_of_two_solids_is_the_per_channel_midpoint() {
        let a = Paint::Solid(Color::rgb(0.0, 0.0, 0.0));
        let b = Paint::Solid(Color::rgb(1.0, 1.0, 1.0));
        let Paint::Solid(mid) = lerp_paint(a, b, 0.5) else {
            panic!("expected a solid paint");
        };
        assert!((mid.r - 0.5).abs() < 1e-6);
        assert!((mid.g - 0.5).abs() < 1e-6);
        assert!((mid.b - 0.5).abs() < 1e-6);
    }

    #[test]
    fn smooth_color_steps_wants_few_for_near_identical_colors_and_many_for_black_to_white() {
        let near = smooth_color_steps(
            Paint::Solid(Color::rgb(0.5, 0.5, 0.5)),
            Paint::Solid(Color::rgb(0.51, 0.5, 0.5)),
        );
        let far = smooth_color_steps(
            Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
            Paint::Solid(Color::rgb(1.0, 1.0, 1.0)),
        );
        assert!(near < far);
        assert_eq!(far, 255);
    }

    #[test]
    fn interpolating_two_identical_squares_at_different_positions_reproduces_the_square_at_the_requested_center() {
        // Same shape, shifted 100 units apart — a halfway blend should be
        // that same square, centered wherever we ask it to be (this is
        // exactly the "shape follows a spine" decoupling the whole
        // module exists for).
        let square = |cx: f64| {
            vec![vec![
                Point::new(cx - 10.0, -10.0),
                Point::new(cx + 10.0, -10.0),
                Point::new(cx + 10.0, 10.0),
                Point::new(cx - 10.0, 10.0),
            ]]
        };
        let a = square(0.0);
        let b = square(100.0);
        let ca = shape_center(&a);
        let cb = shape_center(&b);
        let a_pairs: Vec<(Vec<Point>, bool)> = a.into_iter().map(|sp| (sp, true)).collect();
        let b_pairs: Vec<(Vec<Point>, bool)> = b.into_iter().map(|sp| (sp, true)).collect();
        let target_center = Point::new(500.0, 500.0);
        let step = interpolate_step(&a_pairs, &b_pairs, ca, cb, target_center, 0.5);
        let bounds = step.local_bounds();
        let got_center = bounds.center();
        assert!((got_center.x - target_center.x).abs() < 1e-6);
        assert!((got_center.y - target_center.y).abs() < 1e-6);
        assert!((bounds.width() - 20.0).abs() < 0.5);
        assert!((bounds.height() - 20.0).abs() < 0.5);
    }

    #[test]
    fn lerp_effect_stack_interpolates_matching_zig_zag_stacks_field_by_field() {
        let a = [Effect::ZigZag(ZigZagEffect { size: 0.0, ridges_per_segment: 4.0, smooth: false })];
        let b = [Effect::ZigZag(ZigZagEffect { size: 20.0, ridges_per_segment: 4.0, smooth: false })];
        let mid = lerp_effect_stack(&a, &b, 0.5);
        match mid[..] {
            [Effect::ZigZag(fx)] => assert!((fx.size - 10.0).abs() < 1e-9),
            _ => panic!("expected a single interpolated ZigZag"),
        }
    }

    /// The exact bug this replaced: blending a Zig Zag shape into a
    /// plain line (no effects at all) used to just switch the whole
    /// stack at the midpoint — half the generated steps showed the full
    /// zigzag, the other half none at all, a visible snap instead of a
    /// blend. Now the missing side fades the zigzag's own size toward 0
    /// instead of dropping it outright.
    #[test]
    fn lerp_effect_stack_fades_a_zig_zag_toward_zero_when_the_other_side_has_none() {
        let a: [Effect; 1] = [Effect::ZigZag(ZigZagEffect { size: 20.0, ridges_per_segment: 4.0, smooth: false })];
        let b: [Effect; 0] = [];

        let quarter = lerp_effect_stack(&a, &b, 0.25);
        match quarter[..] {
            [Effect::ZigZag(fx)] => assert!((fx.size - 15.0).abs() < 1e-9, "expected size faded 3/4 of the way from 20 toward 0, got {}", fx.size),
            _ => panic!("expected a single ZigZag, faded toward zero, not dropped"),
        }
        let three_quarter = lerp_effect_stack(&a, &b, 0.75);
        match three_quarter[..] {
            [Effect::ZigZag(fx)] => assert!((fx.size - 5.0).abs() < 1e-9, "expected size faded 3/4 of the way toward 0, got {}", fx.size),
            _ => panic!("expected a single ZigZag, faded toward zero, not dropped"),
        }
        // Right at the missing end, it's fully faded out (size 0) — not
        // literally absent, so the generated step's own chain still
        // finishes with a real (no-op) ZigZag rather than skipping it.
        match lerp_effect_stack(&a, &b, 1.0)[..] {
            [Effect::ZigZag(fx)] => assert!(fx.size.abs() < 1e-9),
            _ => panic!("expected a single ZigZag at zero size"),
        }
    }

    #[test]
    fn lerp_effect_stack_switches_at_the_midpoint_only_for_a_genuine_kind_mismatch_at_the_same_slot() {
        let a: [Effect; 1] = [Effect::ZigZag(ZigZagEffect { size: 10.0, ridges_per_segment: 4.0, smooth: false })];
        let b: [Effect; 1] = [Effect::Twist(TwistEffect { angle: 45.0 })];
        assert_eq!(lerp_effect_stack(&a, &b, 0.25), a.to_vec());
        assert_eq!(lerp_effect_stack(&a, &b, 0.75), b.to_vec());
    }

    #[test]
    fn lerp_effect_stack_is_empty_for_two_empty_stacks() {
        assert!(lerp_effect_stack(&[], &[], 0.5).is_empty());
    }
}
