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
/// reads on the pasteboard and on a white artboard alike. Deliberately not
/// a `Theme` field — on-document cursor glyphs read against arbitrary
/// document content, not the app chrome, so they stay fixed regardless of
/// the user's accent color. Shared with every other on-document cursor
/// badge in `app/render/main_view.rs`, which draws the same near-black
/// ink over the same white halo.
const CURSOR_HALO: Color = Color::from_rgb8(0xff, 0xff, 0xff);
pub(crate) const CURSOR_INK: Color = Color::from_rgb8(0x1a, 0x1a, 0x1a);

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
    Reflect,
    Shear,
    Scale,
    Blend,
    Width,
    Arc,
    Spiral,
    FreeTransform,
    Join,
    ShapeBuilder,
    Eraser,
    VerticalText,
    AreaType,
    PathType,
    VerticalAreaType,
    VerticalPathType,
    // Illustrator tools Amalith doesn't implement yet — used only by the
    // Tools panel's greyed-out "(WIP)" placeholder slots.
    MagicWand,
    Lasso,
    CurvaturePen,
    Paintbrush,
    Pencil,
    Mesh,
    Measure,
    SymbolSprayer,
    Slice,
    Shaper,
    PerspectiveGrid,
    ColumnGraph,
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
        | Icon::Rotate | Icon::Reflect | Icon::Shear | Icon::Scale | Icon::Blend | Icon::Width
        | Icon::Arc | Icon::Spiral | Icon::FreeTransform | Icon::Join | Icon::ShapeBuilder
        | Icon::Eraser | Icon::VerticalText | Icon::AreaType | Icon::PathType
        | Icon::VerticalAreaType | Icon::VerticalPathType
        | Icon::MagicWand | Icon::Lasso | Icon::CurvaturePen | Icon::Paintbrush
        | Icon::Pencil | Icon::Mesh | Icon::Measure | Icon::SymbolSprayer
        | Icon::Slice | Icon::Shaper | Icon::PerspectiveGrid | Icon::ColumnGraph => "",
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
    if icon == Icon::Reflect {
        draw_reflect_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Shear {
        draw_shear_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Scale {
        draw_scale_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Blend {
        draw_blend_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Width {
        draw_width_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Arc {
        draw_arc_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Spiral {
        draw_spiral_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::FreeTransform {
        draw_free_transform_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Join {
        draw_join_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::ShapeBuilder {
        draw_shape_builder_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Eraser {
        draw_eraser_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::VerticalText {
        draw_vertical_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::AreaType {
        draw_area_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::PathType {
        draw_path_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::VerticalAreaType {
        draw_vertical_area_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::VerticalPathType {
        draw_vertical_path_type_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::MagicWand {
        draw_magic_wand_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Lasso {
        draw_lasso_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::CurvaturePen {
        draw_curvature_pen_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Paintbrush {
        draw_paintbrush_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Pencil {
        draw_pencil_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Mesh {
        draw_mesh_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Measure {
        draw_measure_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::SymbolSprayer {
        draw_symbol_sprayer_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Slice {
        draw_slice_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::Shaper {
        draw_shaper_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::PerspectiveGrid {
        draw_perspective_grid_glyph(scene, box_, color);
        return;
    }
    if icon == Icon::ColumnGraph {
        draw_column_graph_glyph(scene, box_, color);
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

/// A triangle mirrored across a dashed vertical axis — the Reflect tool.
fn draw_reflect_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let cx = box_.center().x;
    let top = box_.y0 + h * 0.22;
    let bot = box_.y1 - h * 0.22;
    let gap = w * 0.08;
    let tri = |mirror: f64| {
        let x0 = cx + mirror * gap;
        let x1 = cx + mirror * (gap + w * 0.28);
        let mut p = BezPath::new();
        p.move_to((x1, top));
        p.line_to((x1, bot));
        p.line_to((x0, bot));
        p.close_path();
        p
    };
    scene.fill(Fill::NonZero, ID, color, None, &tri(1.0));
    scene.fill(Fill::NonZero, ID, color, None, &tri(-1.0));
    // Dashed mirror axis.
    let sw = (w * 0.07).max(1.2);
    let mut y = top - h * 0.06;
    let (dash, hole) = (h * 0.09, h * 0.06);
    while y < bot + h * 0.06 {
        let y1 = (y + dash).min(bot + h * 0.06);
        scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new((cx, y), (cx, y1)));
        y = y1 + hole;
    }
}

/// A slanted parallelogram against an upright square — the Shear tool.
fn draw_shear_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x0 = box_.x0 + w * 0.20;
    let x1 = box_.x1 - w * 0.20;
    let y0 = box_.y0 + h * 0.26;
    let y1 = box_.y1 - h * 0.26;
    let shear = w * 0.22;
    let mut p = BezPath::new();
    p.move_to((x0 + shear, y0));
    p.line_to((x1 + shear, y0));
    p.line_to((x1 - shear, y1));
    p.line_to((x0 - shear, y1));
    p.close_path();
    scene.stroke(&Stroke::new((w * 0.09).max(1.6)), ID, color, None, &p);
    // Baseline the slant runs from.
    scene.stroke(
        &Stroke::new((w * 0.06).max(1.0)),
        ID,
        color,
        None,
        &Line::new((box_.x0 + w * 0.1, box_.center().y), (box_.x1 - w * 0.1, box_.center().y)),
    );
}

/// A small square nested in a larger one, joined by a diagonal arrow —
/// the Scale tool.
fn draw_scale_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let big = Rect::new(box_.x0 + w * 0.14, box_.y0 + h * 0.14, box_.x1 - w * 0.40, box_.y1 - h * 0.40);
    let small = Rect::new(box_.x0 + w * 0.46, box_.y0 + h * 0.46, box_.x1 - w * 0.14, box_.y1 - h * 0.14);
    let sw = (w * 0.08).max(1.4);
    scene.stroke(&Stroke::new(sw), ID, color, None, &big);
    scene.stroke(&Stroke::new(sw), ID, color, None, &small);
    // Arrowhead pointing out past the small square's corner, along the
    // same NW-SE diagonal the two squares already sit on.
    let tip = Point::new(small.x1 + w * 0.08, small.y1 + h * 0.08);
    let dir = Vec2::new(1.0, 1.0) / std::f64::consts::SQRT_2;
    let perp = Vec2::new(-dir.y, dir.x);
    let ah = w * 0.14;
    let aw = w * 0.09;
    let base = tip - dir * ah;
    let mut head = BezPath::new();
    head.move_to(tip);
    head.line_to(base + perp * aw);
    head.line_to(base - perp * aw);
    head.close_path();
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(small.center(), base));
    scene.fill(Fill::NonZero, ID, color, None, &head);
}

/// A dashed bounding box with a filled square handle at each corner —
/// the Free Transform tool.
fn draw_free_transform_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let r = Rect::new(
        box_.x0 + w * 0.20,
        box_.y0 + h * 0.20,
        box_.x1 - w * 0.20,
        box_.y1 - h * 0.20,
    );
    let sw = (w * 0.07).max(1.2);
    let corners = [
        Point::new(r.x0, r.y0),
        Point::new(r.x1, r.y0),
        Point::new(r.x1, r.y1),
        Point::new(r.x0, r.y1),
    ];
    for i in 0..4 {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let len = (b - a).hypot();
        let dir = (b - a) / len;
        let (dash, hole) = (len * 0.22, len * 0.14);
        let mut t = 0.0;
        while t < len {
            let t1 = (t + dash).min(len);
            scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(a + dir * t, a + dir * t1));
            t = t1 + hole;
        }
    }
    let hs = w * 0.11;
    for c in corners {
        scene.fill(Fill::NonZero, ID, color, None, &Rect::from_center_size(c, (hs, hs)));
    }
}

/// A circle with a square tucked behind it, three small dots ramping
/// between them — the Blend tool.
fn draw_blend_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let sq = Rect::new(box_.x0 + w * 0.14, box_.y0 + h * 0.14, box_.x0 + w * 0.56, box_.y0 + h * 0.56);
    let sw = (w * 0.08).max(1.4);
    scene.stroke(&Stroke::new(sw), ID, color, None, &sq);
    let c = Point::new(box_.x1 - w * 0.32, box_.y1 - h * 0.32);
    let r = w * 0.21;
    scene.stroke(&Stroke::new(sw), ID, color, None, &Circle::new(c, r));
    // Three small in-between dots along the diagonal from the square's
    // center to the circle's, shrinking as they approach the circle.
    let from = sq.center();
    for i in 1..=3 {
        let t = i as f64 / 4.0;
        let p = Point::new(from.x + (c.x - from.x) * t, from.y + (c.y - from.y) * t);
        scene.fill(Fill::NonZero, ID, color, None, &Circle::new(p, w * 0.035));
    }
}

/// A horizontal ribbon that tapers thin → wide → thin, with a small
/// diamond marking a width point mid-ribbon — the Width tool.
fn draw_width_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x0 = box_.x0 + w * 0.16;
    let x1 = box_.x1 - w * 0.16;
    let cx = box_.center().x;
    let cy = box_.center().y;
    let thin = h * 0.04;
    let thick = h * 0.20;
    let mut ribbon = BezPath::new();
    ribbon.move_to((x0, cy - thin));
    ribbon.quad_to((cx, cy - thick), (x1, cy - thin));
    ribbon.line_to((x1, cy + thin));
    ribbon.quad_to((cx, cy + thick), (x0, cy + thin));
    ribbon.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &ribbon);
    let d = h * 0.11;
    let mut diamond = BezPath::new();
    diamond.move_to((cx, cy - thick - d));
    diamond.line_to((cx + d, cy - thick));
    diamond.line_to((cx, cy - thick + d));
    diamond.line_to((cx - d, cy - thick));
    diamond.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &diamond);
}

/// Two short strokes approaching from opposite corners, meeting at a dot —
/// the Join tool.
fn draw_join_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let sw = (box_.width() * 0.10).max(1.6);
    let c = box_.center();
    let r = box_.width().min(box_.height()) * 0.30;
    let a = c + Vec2::new(-r, -r * 0.4);
    let b = c + Vec2::new(r, r * 0.4);
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(a, c));
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(c, b));
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(c, sw * 0.9));
}

/// Overlapping round regions with a solid shared face — Shape Builder.
fn draw_shape_builder_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let scale = box_.width().min(box_.height()) / 24.0;
    let c = box_.center();
    let transform = Affine::translate((c.x - 12.0 * scale, c.y - 12.0 * scale))
        * Affine::scale(scale);
    let stroke = Stroke::new(1.8);
    // Keep the shared region readable at toolbar size without extending
    // either circle beyond the icon's padding.
    let mut shared = BezPath::new();
    shared.move_to((12.0, 6.347));
    shared.curve_to((9.904, 7.459), (8.6, 9.627), (8.6, 12.0));
    shared.curve_to((8.6, 14.373), (9.904, 16.541), (12.0, 17.653));
    shared.curve_to((14.096, 16.541), (15.4, 14.373), (15.4, 12.0));
    shared.curve_to((15.4, 9.627), (14.096, 7.459), (12.0, 6.347));
    shared.close_path();
    scene.fill(Fill::NonZero, transform, color, None, &shared);
    for x in [9.0, 15.0] {
        scene.stroke(&stroke, transform, color, None, &Circle::new((x, 12.0), 6.4));
    }
}

/// A tilted eraser block, its lower-left tip shaded to read as the used
/// corner — the Eraser tool.
fn draw_eraser_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let scale = box_.width().min(box_.height()) / 24.0;
    let c = box_.center();
    let transform = Affine::translate((c.x - 12.0 * scale, c.y - 12.0 * scale)) * Affine::scale(scale);
    let stroke = Stroke::new(1.6);

    let mut body = BezPath::new();
    body.move_to((6.5, 17.5));
    body.line_to((3.8, 14.8));
    body.curve_to((3.1, 14.1), (3.1, 13.0), (3.8, 12.3));
    body.line_to((13.7, 2.4));
    body.curve_to((14.4, 1.7), (15.5, 1.7), (16.2, 2.4));
    body.line_to((21.6, 7.8));
    body.curve_to((22.3, 8.5), (22.3, 9.6), (21.6, 10.3));
    body.line_to((11.7, 20.2));
    body.curve_to((11.0, 20.9), (9.9, 20.9), (9.2, 20.2));
    body.close_path();
    scene.stroke(&stroke, transform, color, None, &body);

    let mut tip = BezPath::new();
    tip.move_to((6.5, 17.5));
    tip.line_to((3.8, 14.8));
    tip.curve_to((3.1, 14.1), (3.1, 13.0), (3.8, 12.3));
    tip.line_to((9.2, 6.9));
    tip.line_to((15.6, 13.3));
    tip.line_to((11.7, 20.2));
    tip.curve_to((11.0, 20.9), (9.9, 20.9), (9.2, 20.2));
    tip.close_path();
    scene.fill(Fill::NonZero, transform, color, None, &tip);
}

/// A quarter-ellipse sweeping from the bottom-left up to the top-right —
/// the Arc tool.
fn draw_arc_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let x0 = box_.x0 + box_.width() * 0.22;
    let x1 = box_.x1 - box_.width() * 0.22;
    let y0 = box_.y0 + box_.height() * 0.22;
    let y1 = box_.y1 - box_.height() * 0.22;
    let k = 0.552_284_749_830_793_6;
    let mut p = BezPath::new();
    p.move_to((x0, y1));
    p.curve_to((x0, y1 - (y1 - y0) * k), (x1 - (x1 - x0) * k, y0), (x1, y0));
    scene.stroke(&Stroke::new((box_.width() * 0.09).max(1.6)), ID, color, None, &p);
}

/// A few decreasing concentric loops — the Spiral tool.
fn draw_spiral_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let c = box_.center();
    let (rx, ry) = (w * 0.34, h * 0.34);
    let (turns, decay, steps_per_turn) = (2.4_f64, 0.72_f64, 48i64);
    let total_steps = (turns * steps_per_turn as f64) as i64;
    let mut p = BezPath::new();
    for i in 0..=total_steps {
        let t = i as f64 / steps_per_turn as f64 * std::f64::consts::TAU;
        let k = decay.powf(t / std::f64::consts::FRAC_PI_2);
        let pt = Point::new(c.x + rx * k * t.cos(), c.y + ry * k * t.sin());
        if i == 0 {
            p.move_to(pt);
        } else {
            p.line_to(pt);
        }
    }
    scene.stroke(&Stroke::new((w * 0.06).max(1.2)), ID, color, None, &p);
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

/// A diagonal pipette with an open glass tube, collar, and rubber bulb.
fn draw_eyedropper_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let scale = box_.width().min(box_.height()) / 24.0;
    let c = box_.center();
    let transform = Affine::translate((c.x - 12.0 * scale, c.y - 12.0 * scale))
        * Affine::scale(scale);
    let mut tube = BezPath::new();
    tube.move_to((12.0, 8.5));
    tube.line_to((4.6, 15.9));
    tube.line_to((4.1, 18.0));
    tube.line_to((2.8, 19.3));
    tube.line_to((4.7, 21.2));
    tube.line_to((6.0, 19.9));
    tube.line_to((8.1, 19.4));
    tube.line_to((15.5, 12.0));
    tube.close_path();
    scene.stroke(&Stroke::new(1.7), transform, color, None, &tube);

    let mut bulb = BezPath::new();
    bulb.move_to((13.0, 7.5));
    bulb.line_to((16.8, 3.7));
    bulb.curve_to((19.6, 0.9), (23.1, 4.4), (20.3, 7.2));
    bulb.line_to((16.5, 11.0));
    bulb.line_to((17.7, 12.2));
    bulb.line_to((16.2, 13.7));
    bulb.line_to((10.3, 7.8));
    bulb.line_to((11.8, 6.3));
    bulb.close_path();
    scene.fill(Fill::NonZero, transform, color, None, &bulb);
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

/// A sparkling wand — the Magic Wand tool.
fn draw_magic_wand_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let sw = (w * 0.09).max(1.6);
    let a = Point::new(box_.x0 + w * 0.24, box_.y1 - h * 0.24);
    let b = Point::new(box_.x1 - w * 0.20, box_.y0 + h * 0.20);
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(a, b));
    // A four-point sparkle at the tip, plus two smaller ones trailing off.
    let star = |scene: &mut Scene, c: Point, r: f64| {
        let mut p = BezPath::new();
        p.move_to((c.x, c.y - r));
        p.line_to((c.x + r * 0.28, c.y - r * 0.28));
        p.line_to((c.x + r, c.y));
        p.line_to((c.x + r * 0.28, c.y + r * 0.28));
        p.line_to((c.x, c.y + r));
        p.line_to((c.x - r * 0.28, c.y + r * 0.28));
        p.line_to((c.x - r, c.y));
        p.line_to((c.x - r * 0.28, c.y - r * 0.28));
        p.close_path();
        scene.fill(Fill::NonZero, ID, color, None, &p);
    };
    star(scene, b, w * 0.16);
    star(scene, Point::new(b.x - w * 0.02, b.y + h * 0.30), w * 0.07);
    star(scene, Point::new(b.x + w * 0.24, b.y + h * 0.10), w * 0.055);
}

/// A closed rope loop with a trailing tail — the Lasso tool.
fn draw_lasso_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let c = Point::new(box_.center().x - w * 0.06, box_.y0 + h * 0.42);
    let (rx, ry) = (w * 0.30, h * 0.26);
    let sw = (w * 0.08).max(1.4);
    scene.stroke(&Stroke::new(sw), ID, color, None, &Ellipse::new(c, (rx, ry), 0.0));
    let mut tail = BezPath::new();
    tail.move_to((c.x + rx * 0.55, c.y + ry * 0.75));
    tail.curve_to(
        (c.x + rx * 0.9, box_.y1 - h * 0.28),
        (box_.x1 - w * 0.22, box_.y1 - h * 0.30),
        (box_.x1 - w * 0.18, box_.y1 - h * 0.16),
    );
    scene.stroke(&Stroke::new(sw), ID, color, None, &tail);
}

/// A curved path between two anchor points, like Pen but bowed — the
/// Curvature Pen tool.
fn draw_curvature_pen_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let a = Point::new(box_.x0 + w * 0.20, box_.y1 - h * 0.22);
    let b = Point::new(box_.x1 - w * 0.20, box_.y0 + h * 0.22);
    let mid = Point::new(box_.x0 + w * 0.30, box_.y0 + h * 0.30);
    let mut p = BezPath::new();
    p.move_to(a);
    p.quad_to(mid, b);
    scene.stroke(&Stroke::new((w * 0.08).max(1.4)), ID, color, None, &p);
    let r = w * 0.07;
    scene.stroke(&Stroke::new((w * 0.05).max(1.0)), ID, color, None, &Circle::new(a, r));
    scene.stroke(&Stroke::new((w * 0.05).max(1.0)), ID, color, None, &Circle::new(b, r));
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(a, r * 0.45));
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(b, r * 0.45));
}

/// A bristled brush tip on an angled handle — the Paintbrush tool.
fn draw_paintbrush_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let handle_top = Point::new(box_.x1 - w * 0.20, box_.y0 + h * 0.16);
    let ferrule = Point::new(box_.center().x + w * 0.06, box_.center().y - h * 0.02);
    scene.stroke(&Stroke::new((w * 0.11).max(1.8)), ID, color, None, &Line::new(handle_top, ferrule));
    // Splayed bristle tip fanning down to the left.
    let tip = Point::new(box_.x0 + w * 0.22, box_.y1 - h * 0.20);
    let mut bristles = BezPath::new();
    bristles.move_to(ferrule + Point::new(-w * 0.07, h * 0.02).to_vec2());
    bristles.line_to(Point::new(tip.x - w * 0.05, tip.y - h * 0.03));
    bristles.line_to(tip);
    bristles.line_to(Point::new(tip.x + w * 0.05, tip.y + h * 0.02));
    bristles.line_to(ferrule + Point::new(w * 0.05, -h * 0.03).to_vec2());
    bristles.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &bristles);
}

/// A tilted pencil, point down-left — the Pencil tool.
fn draw_pencil_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let top = Point::new(box_.x1 - w * 0.22, box_.y0 + h * 0.18);
    let shoulder = Point::new(box_.x0 + w * 0.34, box_.y1 - h * 0.32);
    let tip = Point::new(box_.x0 + w * 0.18, box_.y1 - h * 0.18);
    let perp = {
        let d = shoulder - top;
        let len = d.hypot().max(0.001);
        Vec2::new(-d.y, d.x) / len
    };
    let hw = w * 0.075;
    let mut body = BezPath::new();
    body.move_to(top + perp * hw);
    body.line_to(top - perp * hw);
    body.line_to(shoulder - perp * hw);
    body.line_to(tip);
    body.line_to(shoulder + perp * hw);
    body.close_path();
    scene.stroke(&Stroke::new((w * 0.055).max(1.0)), ID, color, None, &body);
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(tip, w * 0.03));
}

/// A warped 3×3 grid of curved lines with node dots — the Mesh tool.
fn draw_mesh_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let r = Rect::new(box_.x0 + w * 0.18, box_.y0 + h * 0.18, box_.x1 - w * 0.18, box_.y1 - h * 0.18);
    let sw = (w * 0.045).max(0.9);
    let bow = w * 0.06;
    for i in 0..=2 {
        let t = i as f64 / 2.0;
        let y = r.y0 + t * r.height();
        let mid_y = y + (0.5 - t).abs() * -bow + bow * 0.5;
        let mut p = BezPath::new();
        p.move_to((r.x0, y));
        p.quad_to((r.center().x, mid_y), (r.x1, y));
        scene.stroke(&Stroke::new(sw), ID, color, None, &p);
    }
    for i in 0..=2 {
        let t = i as f64 / 2.0;
        let x = r.x0 + t * r.width();
        let mid_x = x + (0.5 - t).abs() * -bow + bow * 0.5;
        let mut p = BezPath::new();
        p.move_to((x, r.y0));
        p.quad_to((mid_x, r.center().y), (x, r.y1));
        scene.stroke(&Stroke::new(sw), ID, color, None, &p);
    }
    for xi in 0..=2 {
        for yi in 0..=2 {
            let x = r.x0 + xi as f64 / 2.0 * r.width();
            let y = r.y0 + yi as f64 / 2.0 * r.height();
            scene.fill(Fill::NonZero, ID, color, None, &Circle::new((x, y), w * 0.025));
        }
    }
}

/// A ruler segment with tick marks and end nodes — the Measure tool.
fn draw_measure_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let a = Point::new(box_.x0 + w * 0.20, box_.y1 - h * 0.22);
    let b = Point::new(box_.x1 - w * 0.20, box_.y0 + h * 0.22);
    let sw = (w * 0.06).max(1.2);
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(a, b));
    let dir = (b - a) / (b - a).hypot();
    let perp = Vec2::new(-dir.y, dir.x);
    for i in 1..4 {
        let t = i as f64 / 4.0;
        let p = a + (b - a) * t;
        let tick = w * 0.06;
        scene.stroke(&Stroke::new(sw * 0.8), ID, color, None, &Line::new(p - perp * tick, p + perp * tick));
    }
    let r = w * 0.045;
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(a, r));
    scene.fill(Fill::NonZero, ID, color, None, &Circle::new(b, r));
}

/// A spray can with drifting particles — the Symbol Sprayer tool.
fn draw_symbol_sprayer_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let can = Rect::new(box_.x0 + w * 0.22, box_.center().y - h * 0.04, box_.x0 + w * 0.50, box_.y1 - h * 0.18);
    scene.stroke(&Stroke::new((w * 0.07).max(1.2)), ID, color, None, &can);
    let cap = Rect::new(can.x0 + w * 0.03, can.y0 - h * 0.10, can.x1 - w * 0.03, can.y0);
    scene.fill(Fill::NonZero, ID, color, None, &cap);
    let nozzle = Rect::new(cap.center().x - w * 0.015, cap.y0 - h * 0.06, cap.center().x + w * 0.015, cap.y0);
    scene.fill(Fill::NonZero, ID, color, None, &nozzle);
    // Spray particles fanning up and to the right of the nozzle.
    let origin = Point::new(nozzle.x1, nozzle.y0);
    for (dx, dy, r) in [
        (0.14, -0.10, 0.028),
        (0.22, -0.02, 0.020),
        (0.20, -0.20, 0.020),
        (0.30, -0.14, 0.016),
    ] {
        scene.fill(
            Fill::NonZero,
            ID,
            color,
            None,
            &Circle::new((origin.x + w * dx, origin.y + h * dy), w * r),
        );
    }
}

/// A dashed cut line with a triangular blade — the Slice tool.
fn draw_slice_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let a = Point::new(box_.x0 + w * 0.18, box_.y1 - h * 0.22);
    let b = Point::new(box_.x1 - w * 0.30, box_.y0 + h * 0.24);
    let sw = (w * 0.05).max(1.0);
    let dir = (b - a) / (b - a).hypot();
    let (dash, hole) = (w * 0.09, w * 0.06);
    let total = (b - a).hypot();
    let mut t = 0.0;
    while t < total {
        let t1 = (t + dash).min(total);
        scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(a + dir * t, a + dir * t1));
        t = t1 + hole;
    }
    // Blade at the leading end.
    let perp = Vec2::new(-dir.y, dir.x);
    let bl = w * 0.14;
    let mut blade = BezPath::new();
    blade.move_to(b + dir * bl);
    blade.line_to(b - perp * bl * 0.5);
    blade.line_to(b + perp * bl * 0.5);
    blade.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &blade);
}

/// A rounded blob traced by a short pencil stroke — the Shaper tool.
fn draw_shaper_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let r = Rect::new(box_.x0 + w * 0.20, box_.y0 + h * 0.26, box_.x1 - w * 0.34, box_.y1 - h * 0.20);
    let radius = w.min(h) * 0.10;
    scene.stroke(
        &Stroke::new((w * 0.08).max(1.4)),
        ID,
        color,
        None,
        &vello::kurbo::RoundedRect::from_rect(r, radius),
    );
    // A short diagonal "drawing" stroke poking past the top-right corner.
    let a = Point::new(r.x1 - w * 0.02, r.y0 + h * 0.06);
    let b = Point::new(box_.x1 - w * 0.10, box_.y0 + h * 0.12);
    scene.stroke(&Stroke::new((w * 0.06).max(1.1)), ID, color, None, &Line::new(a, b));
}

/// A grid converging toward a vanishing point — the Perspective Grid tool.
fn draw_perspective_grid_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let sw = (w * 0.045).max(0.9);
    let horizon_y = box_.y0 + h * 0.30;
    let base_y = box_.y1 - h * 0.16;
    let vp = Point::new(box_.center().x, horizon_y);
    scene.stroke(
        &Stroke::new(sw),
        ID,
        color,
        None,
        &Line::new((box_.x0 + w * 0.10, horizon_y), (box_.x1 - w * 0.10, horizon_y)),
    );
    // Converging rays from the base up to the vanishing point.
    for t in [-0.36, -0.14, 0.14, 0.36] {
        let base = Point::new(box_.center().x + w * t, base_y);
        scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new(base, vp));
    }
    // Horizontal cross-lines, closer together near the horizon.
    for t in [0.35, 0.62, 1.0] {
        let y = horizon_y + (base_y - horizon_y) * t;
        let half_w = w * 0.06 + (w * 0.30) * t;
        scene.stroke(
            &Stroke::new(sw),
            ID,
            color,
            None,
            &Line::new((box_.center().x - half_w, y), (box_.center().x + half_w, y)),
        );
    }
}

/// Ascending vertical bars on a baseline — the Column Graph tool.
fn draw_column_graph_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let base_y = box_.y1 - h * 0.18;
    let bar_w = w * 0.14;
    let gap = w * 0.06;
    let heights = [0.28, 0.46, 0.64];
    let x0 = box_.x0 + w * 0.20;
    for (i, hh) in heights.into_iter().enumerate() {
        let x = x0 + i as f64 * (bar_w + gap);
        let r = Rect::new(x, base_y - h * hh, x + bar_w, base_y);
        scene.fill(Fill::NonZero, ID, color, None, &r);
    }
    scene.stroke(
        &Stroke::new((w * 0.045).max(1.0)),
        ID,
        color,
        None,
        &Line::new((box_.x0 + w * 0.14, base_y), (box_.x1 - w * 0.14, base_y)),
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

/// The top-to-bottom arrow every Vertical-* type tool icon shares,
/// occupying the box's left third — Illustrator's own convention for
/// marking a tool as the vertical sibling of a horizontal one.
fn draw_down_arrow(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x = box_.x0;
    let y = box_.y0;
    let inset = w * 0.16;
    let shaft_x = x + w * 0.16;
    let shaft_top = y + inset;
    let shaft_bottom = y + h - inset - w * 0.08;
    scene.stroke(
        &Stroke::new((w * 0.09).max(1.2)),
        ID,
        color,
        None,
        &Line::new((shaft_x, shaft_top), (shaft_x, shaft_bottom)),
    );
    let ah = w * 0.11;
    let mut arrow = BezPath::new();
    arrow.move_to((shaft_x - ah, shaft_bottom - ah * 0.3));
    arrow.line_to((shaft_x + ah, shaft_bottom - ah * 0.3));
    arrow.line_to((shaft_x, shaft_bottom + ah * 0.9));
    arrow.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &arrow);
}

/// The box's right two-thirds — where a Vertical-* icon draws its base
/// (horizontal) glyph, beside [`draw_down_arrow`] in the left third.
fn right_two_thirds(box_: Rect) -> Rect {
    Rect::new(box_.x0 + box_.width() * 0.4, box_.y0, box_.x1, box_.y1)
}

/// A downward arrow beside a `T` — Illustrator's own Vertical Type Tool
/// mark: the type glyph, shifted into the box's right two-thirds, plus a
/// top-to-bottom arrow occupying the left third.
fn draw_vertical_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    draw_down_arrow(scene, box_, color);
    let r = right_two_thirds(box_);
    let w = box_.width();
    let h = r.height();
    let x = r.x0;
    let y = r.y0;
    let inset = w * 0.16;
    let bar = (h * 0.14).max(1.5);
    let stem = (w * 0.13).max(1.5);
    let cx = r.center().x;
    scene.fill(Fill::NonZero, ID, color, None, &Rect::new(x, y + inset, r.x1 - inset, y + inset + bar));
    scene.fill(Fill::NonZero, ID, color, None, &Rect::new(cx - stem / 2.0, y + inset, cx + stem / 2.0, y + h - inset));
    scene.fill(
        Fill::NonZero,
        ID,
        color,
        None,
        &Rect::new(cx - stem * 1.4, y + h - inset - bar * 0.85, cx + stem * 1.4, y + h - inset),
    );
}

/// A `T` inside a trapezoid — Illustrator's Area Type Tool mark (text
/// bound to a shape's area).
fn draw_area_type_glyph_in(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x = box_.x0;
    let y = box_.y0;
    let inset = w * 0.14;
    let bot_inset = w * 0.26;
    let mut trapezoid = BezPath::new();
    trapezoid.move_to((x + inset, y + inset));
    trapezoid.line_to((x + w - inset, y + inset));
    trapezoid.line_to((x + w - bot_inset, y + h - inset));
    trapezoid.line_to((x + bot_inset, y + h - inset));
    trapezoid.close_path();
    scene.stroke(&Stroke::new((w * 0.07).max(1.1)), ID, color, None, &trapezoid);
    let cx = box_.center().x;
    let bar_y = y + h * 0.42;
    let bar_half = w * 0.14;
    let sw = (w * 0.08).max(1.1);
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new((cx - bar_half, bar_y), (cx + bar_half, bar_y)));
    scene.stroke(&Stroke::new(sw), ID, color, None, &Line::new((cx, bar_y), (cx, y + h * 0.66)));
}

fn draw_area_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    draw_area_type_glyph_in(scene, box_, color);
}

fn draw_vertical_area_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    draw_down_arrow(scene, box_, color);
    draw_area_type_glyph_in(scene, right_two_thirds(box_), color);
}

/// A shallow curve with a few dots riding it — Illustrator's Type on a
/// Path Tool mark (characters following a curve rather than a straight
/// baseline).
fn draw_path_type_glyph_in(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let x = box_.x0;
    let y = box_.y0;
    let mut curve = BezPath::new();
    curve.move_to((x + w * 0.14, y + h * 0.72));
    curve.curve_to(
        (x + w * 0.30, y + h * 0.16),
        (x + w * 0.70, y + h * 0.16),
        (x + w * 0.86, y + h * 0.72),
    );
    scene.stroke(&Stroke::new((w * 0.08).max(1.1)), ID, color, None, &curve);
    let dot_r = (w * 0.045).max(1.0);
    for &(fx, fy) in &[(0.30, 0.46), (0.5, 0.30), (0.70, 0.46)] {
        scene.fill(Fill::NonZero, ID, color, None, &Circle::new((x + w * fx, y + h * fy), dot_r));
    }
}

fn draw_path_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    draw_path_type_glyph_in(scene, box_, color);
}

fn draw_vertical_path_type_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    draw_down_arrow(scene, box_, color);
    draw_path_type_glyph_in(scene, right_two_thirds(box_), color);
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
    pass(CURSOR_INK, 1.5);
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
    pass(CURSOR_INK, 1.5);
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
    pass(CURSOR_INK, 1.5);
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
