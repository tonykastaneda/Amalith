//! Tool icons. The six tools with brand artwork are painted from the
//! `assets/tool-icons` files via a tiny SVG-primitive reader (ported from
//! `amalith-app`'s `paint_brand_tool_icon`); the rest stay hand-drawn.
//! `vello_svg` still targets vello 0.9, so a full SVG stack isn't an
//! option — but the glyphs only use `<polygon>` / `<rect>` / `<ellipse>`
//! / `<circle>` / `<line>` in a 0..100 view box, which is easy to walk.

use vello::kurbo::{Affine, Arc, BezPath, Circle, Ellipse, Line, Point, Rect, Stroke, Vec2};
use vello::peniko::{Color, Fill};
use vello::Scene;

/// Transform-cursor ink: a wide white halo under a near-black body, so it
/// reads on the pasteboard and on a white artboard alike.
const CURSOR_HALO: Color = Color::from_rgb8(0xff, 0xff, 0xff);
const CURSOR_BODY: Color = Color::from_rgb8(0x1a, 0x1a, 0x1a);

const ID: Affine = Affine::IDENTITY;

const SELECT_SVG: &str = include_str!("../assets/tool-icons/V-selectio.svg");
const DIRECT_SELECT_SVG: &str = include_str!("../assets/tool-icons/A-selection.svg");
const PEN_SVG: &str = include_str!("../assets/tool-icons/Pen.svg");
const RECT_SVG: &str = include_str!("../assets/tool-icons/Square.svg");
const ROUND_RECT_SVG: &str = include_str!("../assets/tool-icons/round-square.svg");
const ELLIPSE_SVG: &str = include_str!("../assets/tool-icons/Circle.svg");
const POLYGON_SVG: &str = include_str!("../assets/tool-icons/Polygon.svg");
const STAR_SVG: &str = include_str!("../assets/tool-icons/Start.svg");
const ARTBOARD_SVG: &str = include_str!("../assets/tool-icons/Artboard Tool.svg");

// "-onDocument" variants: how a tool glyph is drawn as the canvas cursor
// (a light body with a dark keyline, readable on any background).
pub const CURSOR_SELECT_SVG: &str =
    include_str!("../assets/tool-icons/V-selectio-onDocument.svg");
pub const CURSOR_DIRECT_SELECT_SVG: &str =
    include_str!("../assets/tool-icons/A-selection-onDocument.svg");
pub const CURSOR_PEN_DRAWING_SVG: &str =
    include_str!("../assets/tool-icons/Pen-drawingShape-onDocument.svg");
pub const CURSOR_PEN_CLOSING_SVG: &str =
    include_str!("../assets/tool-icons/Pen-closingShape-onDocument.svg");

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Select,
    DirectSelect,
    Pen,
    Line,
    Text,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Artboard,
    Hand,
    Zoom,
    Eyedropper,
    Gradient,
    Rotate,
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
        // Hand-drawn in `draw`; never reach the brand-SVG path.
        Icon::Text | Icon::Line | Icon::Hand | Icon::Zoom | Icon::Eyedropper | Icon::Gradient
        | Icon::Rotate => "",
    }
}

/// Draw `icon` filling `box_` (screen px), tinted `color` — the panel look.
pub fn draw(scene: &mut Scene, icon: Icon, box_: Rect, color: Color) {
    if icon == Icon::Text {
        draw_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Line {
        draw_line_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Hand {
        draw_hand_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Zoom {
        draw_zoom_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Eyedropper {
        draw_eyedropper_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Gradient {
        draw_gradient_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Rotate {
        draw_rotate_glyph(scene, box_, color);
        return;
    }
    paint_brand(scene, brand_svg(icon), box_, color, icon == Icon::DirectSelect);
}

/// A circular arrow — the Rotate tool.
fn draw_rotate_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let c = box_.center();
    let r = box_.width().min(box_.height()) * 0.30;
    let sw = (box_.width() * 0.10).max(1.6);
    // ~270° sweep, gap toward the top-right.
    let a0 = -2.1;
    let sweep = 4.7;
    scene.stroke(
        &Stroke::new(sw),
        ID,
        color,
        None,
        &Arc::new(c, (r, r), a0, sweep, 0.0),
    );
    // A tangent arrowhead at the arc's leading end.
    let a1 = a0 + sweep;
    let (s, cs) = a1.sin_cos();
    let p = c + Vec2::new(cs, s) * r;
    let tan = Vec2::new(-s, cs); // CCW travel direction
    let perp = Vec2::new(-tan.y, tan.x);
    let ah = r * 0.95;
    let aw = r * 0.6;
    let mut head = BezPath::new();
    head.move_to(p + tan * ah);
    head.line_to(p + perp * aw);
    head.line_to(p - perp * aw);
    head.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &head);
}

/// A rounded square with a left→right light-to-dark ramp — the Gradient tool.
fn draw_gradient_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let r = Rect::new(
        box_.x0 + w * 0.16,
        box_.y0 + h * 0.20,
        box_.x1 - w * 0.16,
        box_.y1 - h * 0.20,
    );
    let n = (r.width().ceil() as i64).max(1);
    for i in 0..n {
        let t = i as f64 / n as f64;
        // Fade the tint from ~15% to full so it reads as a ramp even
        // when `color` is a flat panel ink.
        let a = 0.18 + t * 0.82;
        let x = r.x0 + i as f64;
        scene.fill(
            Fill::NonZero,
            ID,
            color.with_alpha(a as f32),
            None,
            &Rect::new(x, r.y0, x + 1.0, r.y1),
        );
    }
    scene.stroke(&Stroke::new((w * 0.08).max(1.2)), ID, color, None, &r);
}

/// A pipette — bulb top-right, tip bottom-left — the Eyedropper tool.
fn draw_eyedropper_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let tip = Point::new(box_.x0 + w * 0.16, box_.y1 - h * 0.16);
    let neck = Point::new(box_.x0 + w * 0.60, box_.y0 + h * 0.40);
    // Barrel.
    scene.stroke(
        &Stroke::new((w * 0.13).max(2.0)),
        ID,
        color,
        None,
        &Line::new(tip, neck),
    );
    // Bulb.
    scene.stroke(
        &Stroke::new((w * 0.10).max(1.6)),
        ID,
        color,
        None,
        &Line::new(neck, Point::new(box_.x1 - w * 0.16, box_.y0 + h * 0.16)),
    );
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Circle::new(Point::new(box_.x1 - w * 0.20, box_.y0 + h * 0.20), w * 0.13),
    );
    // A drop at the tip.
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(tip, w * 0.055));
}

/// A four-finger mitt + thumb — the Hand tool.
fn draw_hand_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let (x, y) = (box_.x0, box_.y0);
    // Palm.
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(x + w * 0.24, y + h * 0.42, x + w * 0.78, y + h * 0.84),
    );
    // Four fingers.
    let fw = w * 0.115;
    for (i, top) in [0.20, 0.14, 0.16, 0.24].into_iter().enumerate() {
        let fx = x + w * 0.27 + i as f64 * (fw + w * 0.04);
        scene.fill(
            Fill::NonZero,
            ID,
            color,
            None,
            &Rect::new(fx, y + h * top, fx + fw, y + h * 0.58),
        );
    }
    // Thumb.
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(x + w * 0.13, y + h * 0.50, x + w * 0.29, y + h * 0.74),
    );
}

/// A magnifying glass — the Zoom tool.
fn draw_zoom_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let c = Point::new(box_.x0 + w * 0.42, box_.y0 + box_.height() * 0.42);
    let r = w * 0.26;
    scene.stroke(
        &Stroke::new((w * 0.09).max(1.6)),
        ID,
        color,
        None,
        &Circle::new(c, r),
    );
    let handle_a = Point::new(c.x + r * 0.72, c.y + r * 0.72);
    let handle_b = Point::new(box_.x1 - w * 0.12, box_.y1 - box_.height() * 0.12);
    scene.stroke(
        &Stroke::new((w * 0.13).max(2.0)),
        ID,
        color,
        None,
        &Line::new(handle_a, handle_b),
    );
}

/// A bottom-left → top-right diagonal with a small end node at each tip —
/// the Line Segment tool.
fn draw_line_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let a = Point::new(box_.x0 + box_.width() * 0.14, box_.y1 - box_.height() * 0.14);
    let b = Point::new(box_.x1 - box_.width() * 0.14, box_.y0 + box_.height() * 0.14);
    scene.stroke(&Stroke::new((box_.width() * 0.09).max(1.6)), ID, color, None, &Line::new(a, b));
    let r = (box_.width() * 0.08).max(1.5);
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(a, r));
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(b, r));
}

/// A serif "T" — the Type tool.
fn draw_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x = box_.x0;
    let y = box_.y0;
    let bar = (h * 0.14).max(1.5);
    let stem = (w * 0.14).max(1.5);
    let inset = w * 0.16;
    let serif = h * 0.12;
    // Top bar.
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(x + inset, y + inset, x + w - inset, y + inset + bar),
    );
    // Stem.
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(
            box_.center().x - stem / 2.0,
            y + inset,
            box_.center().x + stem / 2.0,
            y + h - inset,
        ),
    );
    // Foot serif.
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(
            box_.center().x - stem * 1.6,
            y + h - inset - serif,
            box_.center().x + stem * 1.6,
            y + h - inset,
        ),
    );
}

/// A magnifying-glass cursor centred at `center`, with a `+` (`plus`) or
/// `−` inside. Light body + dark keyline so it reads on any background.
pub fn draw_magnifier(scene: &mut Scene, center: Point, plus: bool) {
    let body = Color::from_rgb8(0xe4, 0xe3, 0xe3);
    let key = Color::from_rgb8(0x12, 0x12, 0x12);
    let (cx, cy, r) = (center.x, center.y, 6.0);

    // Handle (behind the lens).
    let h0 = Point::new(cx + r * 0.72, cy + r * 0.72);
    let h1 = Point::new(cx + r * 1.7, cy + r * 1.7);
    scene.stroke(&Stroke::new(3.5), ID, key, None, &Line::new(h0, h1));
    scene.stroke(&Stroke::new(2.0), ID, body, None, &Line::new(h0, h1));

    // Lens.
    let lens = Circle::new((cx, cy), r);
    scene.fill(Fill::NonZero, ID, body, None, &lens);
    scene.stroke(&Stroke::new(1.6), ID, key, None, &lens);

    // Sign.
    let s = r * 0.55;
    scene.stroke(
        &Stroke::new(1.5),
        ID,
        key,
        None,
        &Line::new(Point::new(cx - s, cy), Point::new(cx + s, cy)),
    );
    if plus {
        scene.stroke(
            &Stroke::new(1.5),
            ID,
            key,
            None,
            &Line::new(Point::new(cx, cy - s), Point::new(cx, cy + s)),
        );
    }
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
            let st = resolve(tag);
            let circle = Circle::new(map(cx, cy), r * scale);
            // A black, fill-less ring (the pen close-shape indicator) gets
            // a white halo on both edges so it reads on the pasteboard
            // *and* on a white artboard.
            if st.fill == FillSpec::None && st.stroke == Some(Color::from_rgb8(0, 0, 0)) {
                let w = (st.stroke_width.unwrap_or(3.0) * scale).max(1.4);
                scene.stroke(
                    &Stroke::new(w * 2.4),
                    ID,
                    Color::from_rgb8(0xff, 0xff, 0xff),
                    None,
                    &circle,
                );
                scene.stroke(&Stroke::new(w * 1.2), ID, Color::from_rgb8(0, 0, 0), None, &circle);
            } else {
                paint(scene, st, scale, &circle);
            }
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

/// Illustrator-style scale cursor: a double-headed arrow along `angle`
/// (radians; 0 = horizontal), centred on `center`. Painted white-halo
/// then dark body.
/// "Fit to text" cursor: an up-arrow standing on a short bar (⤒). Shown
/// when hovering an area-text box's auto-fit tab. Sized to match the
/// scale / rotate cursors.
pub fn draw_fit_up_cursor(scene: &mut Scene, center: Point) {
    let p = |x: f64, y: f64| Point::new(center.x + x, center.y + y);
    let mut arrow = BezPath::new();
    arrow.move_to(p(0.0, -8.0));
    arrow.line_to(p(6.0, -1.0));
    arrow.line_to(p(2.5, -1.0));
    arrow.line_to(p(2.5, 5.0));
    arrow.line_to(p(-2.5, 5.0));
    arrow.line_to(p(-2.5, -1.0));
    arrow.line_to(p(-6.0, -1.0));
    arrow.close_path();
    let bar = Rect::new(center.x - 6.0, center.y + 7.0, center.x + 6.0, center.y + 10.0);
    let mut pass = |col: Color, sw: f64| {
        scene.stroke(&Stroke::new(sw), ID, col, None, &arrow);
        scene.fill(Fill::NonZero, ID, col, None, &arrow);
        scene.stroke(&Stroke::new(sw), ID, col, None, &bar);
        scene.fill(Fill::NonZero, ID, col, None, &bar);
    };
    pass(CURSOR_HALO, 3.0);
    pass(CURSOR_BODY, 1.5);
}

pub fn draw_scale_cursor(scene: &mut Scene, center: Point, angle: f64) {
    let (s, c) = angle.sin_cos();
    let dir = Vec2::new(c, s);
    let perp = Vec2::new(-s, c);
    let hl = 8.5; // half of the total arrow length
    let ah = 4.5; // arrowhead length
    let aw = 3.0; // arrowhead half-width
    let base_p = center + dir * (hl - ah);
    let base_m = center - dir * (hl - ah);
    let head = |tip: Point, base: Point| {
        let mut p = BezPath::new();
        p.move_to(tip);
        p.line_to(base + perp * aw);
        p.line_to(base - perp * aw);
        p.close_path();
        p
    };
    let hp = head(center + dir * hl, base_p);
    let hm = head(center - dir * hl, base_m);
    let shaft = Line::new(base_m, base_p);
    let mut pass = |col: Color, sw: f64| {
        scene.stroke(&Stroke::new(sw), ID, col, None, &shaft);
        for h in [&hp, &hm] {
            scene.fill(Fill::NonZero, ID, col, None, h);
            scene.stroke(&Stroke::new(sw), ID, col, None, h);
        }
    };
    pass(CURSOR_HALO, 3.0);
    pass(CURSOR_BODY, 1.5);
}

/// Illustrator-style rotate cursor: a ~115° arc with a tangent arrowhead
/// at each end, centred on `center`. The arc is centred on `angle`
/// (radians) so it can be rotated to face the corner being hovered.
pub fn draw_rotate_cursor(scene: &mut Scene, center: Point, angle: f64) {
    let r = 7.0;
    let sweep = 2.0;
    let a0 = angle - sweep * 0.5;
    let arc = Arc::new(center, (r, r), a0, sweep, 0.0);
    let ah = 4.0;
    let aw = 2.8;
    let head = |ang: f64, along: f64| {
        let (s, c) = ang.sin_cos();
        let p = center + Vec2::new(c, s) * r;
        let tan = Vec2::new(-s, c) * along; // travel direction, signed
        let perp = Vec2::new(-tan.y, tan.x);
        let mut path = BezPath::new();
        path.move_to(p + tan * ah);
        path.line_to(p + perp * aw);
        path.line_to(p - perp * aw);
        path.close_path();
        path
    };
    let h0 = head(a0, -1.0);
    let h1 = head(a0 + sweep, 1.0);
    let mut pass = |col: Color, sw: f64| {
        scene.stroke(&Stroke::new(sw), ID, col, None, &arc);
        for h in [&h0, &h1] {
            scene.fill(Fill::NonZero, ID, col, None, h);
            scene.stroke(&Stroke::new(sw), ID, col, None, h);
        }
    };
    pass(CURSOR_HALO, 3.0);
    pass(CURSOR_BODY, 1.5);
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
