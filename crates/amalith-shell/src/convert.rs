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
