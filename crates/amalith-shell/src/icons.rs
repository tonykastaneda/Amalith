//! Tool icons. The six tools with brand artwork are painted from the
//! `branding/SVG` files via a tiny SVG-primitive reader (ported from
//! `amalith-app`'s `paint_brand_tool_icon`); the rest stay hand-drawn.
//! `vello_svg` still targets vello 0.9, so a full SVG stack isn't an
//! option — but the glyphs only use `<polygon>` / `<rect>` / `<ellipse>`
//! / `<circle>` / `<line>` in a 0..100 view box, which is easy to walk.

use vello::kurbo::{Affine, BezPath, Circle, Ellipse, Line, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

const ID: Affine = Affine::IDENTITY;

const SELECT_SVG: &str = include_str!("../../../branding/SVG/V-selectio.svg");
const DIRECT_SELECT_SVG: &str = include_str!("../../../branding/SVG/A-selection.svg");
const PEN_SVG: &str = include_str!("../../../branding/SVG/Pen.svg");
const RECT_SVG: &str = include_str!("../../../branding/SVG/Square.svg");
const ELLIPSE_SVG: &str = include_str!("../../../branding/SVG/Circle.svg");
const POLYGON_SVG: &str = include_str!("../../../branding/SVG/Polygon.svg");
const STAR_SVG: &str = include_str!("../../../branding/SVG/Start.svg");
const ARTBOARD_SVG: &str = include_str!("../../../branding/SVG/Artboard Tool.svg");

/// Pointer-cursor glyphs for the Pen tool while it's drawing.
pub const PEN_DRAWING_SVG: &str = include_str!("../../../branding/SVG/Pen-drawingShape.svg");
pub const PEN_CLOSING_SVG: &str = include_str!("../../../branding/SVG/Pen-closingShape.svg");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    DirectSelect,
    Pen,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Artboard,
}

fn brand_svg(icon: Icon) -> Option<&'static str> {
    Some(match icon {
        Icon::Select => SELECT_SVG,
        Icon::DirectSelect => DIRECT_SELECT_SVG,
        Icon::Pen => PEN_SVG,
        Icon::Rectangle | Icon::RoundedRect => RECT_SVG,
        Icon::Ellipse => ELLIPSE_SVG,
        Icon::Polygon => POLYGON_SVG,
        Icon::Star => STAR_SVG,
        Icon::Artboard => ARTBOARD_SVG,
    })
}

/// Paint an arbitrary brand SVG (e.g. a pen cursor) into `box_`, tinted.
pub fn draw_svg(scene: &mut Scene, src: &str, box_: Rect, color: Color) {
    paint_brand(scene, src, box_, color, false);
}

/// Draw `icon` filling `box_` (screen px), tinted `color`.
pub fn draw(scene: &mut Scene, icon: Icon, box_: Rect, color: Color) {
    if let Some(src) = brand_svg(icon) {
        paint_brand(scene, src, box_, color, icon == Icon::DirectSelect);
    }
}

/// Paint the primitive shapes of a brand-icon SVG into `box_`.
fn paint_brand(scene: &mut Scene, src: &str, box_: Rect, color: Color, direct: bool) {
    let map = |x: f64, y: f64| {
        Point::new(
            box_.x0 + box_.width() * x / 100.0,
            box_.y0 + box_.height() * y / 100.0,
        )
    };
    let sw = (4.0 * box_.width() / 100.0).max(0.75);
    let stroke_col = color;
    let dark = color.with_alpha(0.24);
    // `.cls-3` and the whole Direct-Selection arrow are the light fill.
    let poly_fill = |tag: &str| {
        if direct || tag.contains("cls-3") {
            color
        } else {
            dark
        }
    };

    for tag in svg_tags(src, "polygon") {
        let pts: Vec<Point> = svg_attr(tag, "points")
            .map(svg_nums)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|p| map(p[0], p[1]))
            .collect();
        if pts.len() < 3 {
            continue;
        }
        let mut path = BezPath::new();
        path.move_to(pts[0]);
        for p in &pts[1..] {
            path.line_to(*p);
        }
        path.close_path();
        scene.fill(Fill::NonZero, ID, poly_fill(tag), None, &path);
        scene.stroke(&Stroke::new(sw), ID, stroke_col, None, &path);
    }
    for tag in svg_tags(src, "rect") {
        if let (Some(x), Some(y), Some(w), Some(h)) = (
            svg_num(tag, "x"),
            svg_num(tag, "y"),
            svg_num(tag, "width"),
            svg_num(tag, "height"),
        ) {
            let r = Rect::from_points(map(x, y), map(x + w, y + h));
            scene.fill(Fill::NonZero, ID, dark, None, &r);
            scene.stroke(&Stroke::new(sw), ID, stroke_col, None, &r);
        }
    }
    for tag in svg_tags(src, "ellipse") {
        if let (Some(cx), Some(cy), Some(rx), Some(ry)) = (
            svg_num(tag, "cx"),
            svg_num(tag, "cy"),
            svg_num(tag, "rx"),
            svg_num(tag, "ry"),
        ) {
            let e = Ellipse::new(
                map(cx, cy),
                (rx * box_.width() / 100.0, ry * box_.height() / 100.0),
                0.0,
            );
            scene.fill(Fill::NonZero, ID, dark, None, &e);
            scene.stroke(&Stroke::new(sw), ID, stroke_col, None, &e);
        }
    }
    for tag in svg_tags(src, "circle") {
        if let (Some(cx), Some(cy), Some(rr)) =
            (svg_num(tag, "cx"), svg_num(tag, "cy"), svg_num(tag, "r"))
        {
            scene.fill(
                Fill::NonZero,
                ID,
                stroke_col,
                None,
                &Circle::new(map(cx, cy), rr * box_.width() / 100.0),
            );
        }
    }
    for tag in svg_tags(src, "line") {
        if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
            svg_num(tag, "x1"),
            svg_num(tag, "y1"),
            svg_num(tag, "x2"),
            svg_num(tag, "y2"),
        ) {
            scene.stroke(
                &Stroke::new(sw),
                ID,
                stroke_col,
                None,
                &Line::new(map(x1, y1), map(x2, y2)),
            );
        }
    }
}

// ---- minimal SVG-primitive reader ----------------------------------

/// The text of every `<name …>` opening tag in `src`.
fn svg_tags<'a>(src: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = src;
    while let Some(i) = rest.find(&open) {
        rest = &rest[i + open.len()..];
        let Some(j) = rest.find('>') else { break };
        out.push(&rest[..j]);
        rest = &rest[j + 1..];
    }
    out
}

/// The value of `key="…"` in `tag`.
fn svg_attr<'a>(tag: &'a str, key: &str) -> Option<&'a str> {
    let pat = format!("{key}=\"");
    let start = tag.find(&pat)? + pat.len();
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

fn svg_num(tag: &str, key: &str) -> Option<f64> {
    svg_attr(tag, key)?.trim().trim_end_matches("px").parse().ok()
}

fn svg_nums(s: &str) -> Vec<f64> {
    s.split(|c: char| c.is_whitespace() || c == ',')
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.trim_end_matches("px").parse().ok())
        .collect()
}
