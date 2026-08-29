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
const ROUND_RECT_SVG: &str = include_str!("../../../branding/SVG/round-square.svg");
const ELLIPSE_SVG: &str = include_str!("../../../branding/SVG/Circle.svg");
const POLYGON_SVG: &str = include_str!("../../../branding/SVG/Polygon.svg");
const STAR_SVG: &str = include_str!("../../../branding/SVG/Start.svg");
const ARTBOARD_SVG: &str = include_str!("../../../branding/SVG/Artboard Tool.svg");

// "-onDocument" variants: how a tool glyph is drawn as the canvas cursor
// (a light body with a dark keyline, readable on any background).
pub const CURSOR_SELECT_SVG: &str =
    include_str!("../../../branding/SVG/V-selectio-onDocument.svg");
pub const CURSOR_DIRECT_SELECT_SVG: &str =
    include_str!("../../../branding/SVG/A-selection-onDocument.svg");
pub const CURSOR_PEN_DRAWING_SVG: &str =
    include_str!("../../../branding/SVG/Pen-drawingShape-onDocument.svg");
pub const CURSOR_PEN_CLOSING_SVG: &str =
    include_str!("../../../branding/SVG/Pen-closingShape-onDocument.svg");

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

fn brand_svg(icon: Icon) -> &'static str {
    match icon {
        Icon::Select => SELECT_SVG,
        Icon::DirectSelect => DIRECT_SELECT_SVG,
        Icon::Pen => PEN_SVG,
        Icon::Rectangle => RECT_SVG,
        Icon::RoundedRect => ROUND_RECT_SVG,
        Icon::Ellipse => ELLIPSE_SVG,
        Icon::Polygon => POLYGON_SVG,
        Icon::Star => STAR_SVG,
        Icon::Artboard => ARTBOARD_SVG,
    }
}

/// Draw `icon` filling `box_` (screen px), tinted `color` — the panel look.
pub fn draw(scene: &mut Scene, icon: Icon, box_: Rect, color: Color) {
    paint_brand(scene, brand_svg(icon), box_, color, icon == Icon::DirectSelect);
}

/// Draw a cursor SVG (`CURSOR_*`) at `box_`, honouring the fill / stroke
/// / stroke-width the artwork declares per CSS class — those colours are
/// chosen deliberately (a light body, a white halo, a black keyline) so
/// the cursor reads over any object it's on.
pub fn draw_cursor(scene: &mut Scene, src: &str, box_: Rect) {
    let styles = parse_styles(src);
    let scale = box_.width() / 100.0;
    let map = |x: f64, y: f64| Point::new(box_.x0 + x * scale, box_.y0 + box_.height() * y / 100.0);
    let resolve = |tag: &str| -> Style {
        svg_attr(tag, "class")
            .and_then(|c| c.split_whitespace().find_map(|cls| styles.get(cls).copied()))
            .unwrap_or_default()
    };
    fn paint<S: vello::kurbo::Shape>(scene: &mut Scene, st: Style, scale: f64, shape: &S) {
        // SVG's default fill is black; only an explicit `fill:none` skips it.
        match st.fill {
            FillSpec::Solid(c) => scene.fill(Fill::NonZero, ID, c, None, shape),
            FillSpec::Unset => {
                scene.fill(Fill::NonZero, ID, Color::from_rgb8(0, 0, 0), None, shape)
            }
            FillSpec::None => {}
        }
        if let Some(c) = st.stroke {
            let w = (st.stroke_width.unwrap_or(3.0) * scale).max(1.1);
            scene.stroke(&Stroke::new(w), ID, c, None, shape);
        }
    }

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
        paint(scene, resolve(tag), scale, &path);
    }
    for tag in svg_tags(src, "circle") {
        if let (Some(cx), Some(cy), Some(r)) =
            (svg_num(tag, "cx"), svg_num(tag, "cy"), svg_num(tag, "r"))
        {
            paint(scene, resolve(tag), scale, &Circle::new(map(cx, cy), r * scale));
        }
    }
    for tag in svg_tags(src, "line") {
        if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
            svg_num(tag, "x1"),
            svg_num(tag, "y1"),
            svg_num(tag, "x2"),
            svg_num(tag, "y2"),
        ) {
            let st = resolve(tag);
            if let Some(c) = st.stroke {
                let w = (st.stroke_width.unwrap_or(3.0) * scale).max(1.1);
                scene.stroke(
                    &Stroke::new(w),
                    ID,
                    c,
                    None,
                    &Line::new(map(x1, y1), map(x2, y2)),
                );
            }
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Style {
    fill: FillSpec,
    stroke: Option<Color>,
    stroke_width: Option<f64>,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum FillSpec {
    #[default]
    Unset,
    None,
    Solid(Color),
}

/// CSS-class → resolved style, from the `<style>` block. Comma selectors
/// and repeated rules for one class are merged (later wins).
fn parse_styles(src: &str) -> std::collections::HashMap<&str, Style> {
    let mut out: std::collections::HashMap<&str, Style> = std::collections::HashMap::new();
    let Some(a) = src.find("<style>") else {
        return out;
    };
    let block = &src[a + 7..src[a..].find("</style>").map_or(src.len(), |e| a + e)];
    for rule in block.split('}') {
        let Some((sels, decls)) = rule.split_once('{') else {
            continue;
        };
        for sel in sels.split(',') {
            let name = sel.trim().trim_start_matches('.');
            if name.is_empty() {
                continue;
            }
            let st = out.entry(name).or_default();
            for decl in decls.split(';') {
                let Some((k, v)) = decl.split_once(':') else {
                    continue;
                };
                let (k, v) = (k.trim(), v.trim());
                match k {
                    "fill" if v == "none" => st.fill = FillSpec::None,
                    "fill" => {
                        if let Some(c) = parse_color(v) {
                            st.fill = FillSpec::Solid(c);
                        }
                    }
                    "stroke" if v == "none" => st.stroke = None,
                    "stroke" => st.stroke = parse_color(v),
                    "stroke-width" => {
                        st.stroke_width = v.trim_end_matches("px").parse().ok();
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

fn parse_color(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    let (r, g, b) = match h.len() {
        3 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|n| n * 17);
            (d(0)?, d(1)?, d(2)?)
        }
        6 => {
            let d = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
            (d(0)?, d(2)?, d(4)?)
        }
        _ => return None,
    };
    Some(Color::from_rgb8(r, g, b))
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
