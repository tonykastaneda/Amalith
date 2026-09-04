//! `amalith-core` geometry → vello geometry.
//!
//! The core crates pin kurbo 0.11; vello 0.10 carries kurbo 0.13. The
//! types are structurally identical but distinct to the compiler, so
//! anything rendered from a `Document` crosses this boundary once.

use amalith_core::geom as core;
use vello::kurbo as vk;

pub fn point(p: core::Point) -> vk::Point {
    vk::Point::new(p.x, p.y)
}

pub fn rect(r: core::Rect) -> vk::Rect {
    vk::Rect::new(r.x0, r.y0, r.x1, r.y1)
}

pub fn affine(a: core::Affine) -> vk::Affine {
    vk::Affine::new(a.as_coeffs())
}

/// vello → core (for feeding deltas back into commands).
pub fn vec2_to_core(v: vk::Vec2) -> core::Vec2 {
    core::Vec2::new(v.x, v.y)
}

pub fn affine_to_core(a: vk::Affine) -> core::Affine {
    core::Affine::new(a.as_coeffs())
}

pub fn bez_path(src: &core::BezPath) -> vk::BezPath {
    let mut out = vk::BezPath::new();
    for el in src.elements() {
        out.push(match *el {
            core::PathEl::MoveTo(p) => vk::PathEl::MoveTo(point(p)),
            core::PathEl::LineTo(p) => vk::PathEl::LineTo(point(p)),
            core::PathEl::QuadTo(a, b) => vk::PathEl::QuadTo(point(a), point(b)),
            core::PathEl::CurveTo(a, b, c) => vk::PathEl::CurveTo(point(a), point(b), point(c)),
            core::PathEl::ClosePath => vk::PathEl::ClosePath,
        });
    }
    out
}

pub fn color(c: amalith_core::Color) -> vello::peniko::Color {
    vello::peniko::Color::new([c.r, c.g, c.b, c.a])
}

/// Build a vello gradient from a pooled [`amalith_core::Gradient`], in
/// **bounding-box unit space** (`0..1`). The caller pairs it with a
/// `brush_transform` that maps the unit square onto the object's local
/// bounds (`translate(x0,y0) * scale(w,h)`), matching SVG's
/// `objectBoundingBox` gradient units. Per-stop opacity is folded into
/// each stop's alpha.
pub fn peniko_gradient(g: &amalith_core::Gradient) -> vello::peniko::Gradient {
    use vello::peniko::{Color, ColorStop, Extend, Gradient};

    let stops: Vec<ColorStop> = g
        .stops
        .iter()
        .map(|s| {
            let c = s.color;
            let a = (c.a * s.opacity).clamp(0.0, 1.0);
            ColorStop::from((s.offset.clamp(0.0, 1.0), Color::new([c.r, c.g, c.b, a])))
        })
        .collect();

    let base = match g.kind {
        amalith_core::GradientKind::Linear => {
            Gradient::new_linear((g.start[0], g.start[1]), (g.end[0], g.end[1]))
        }
        amalith_core::GradientKind::Radial => {
            Gradient::new_radial((g.start[0], g.start[1]), g.radius() as f32)
        }
    };
    base.with_extend(Extend::Pad).with_stops(stops.as_slice())
}
