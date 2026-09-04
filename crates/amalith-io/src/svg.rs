//! SVG import/export: the interchange format for copy/paste with the rest
//! of the OS (Illustrator, Figma, a browser, ...), as opposed to the
//! `.amalith` container (`container.rs`), which is Amalith's own lossless
//! save format. See `DESIGN.md`'s "Why not an XML repr tree" note: SVG is
//! deliberately *not* an internal representation here, only an
//! export/import translation at the boundary — `Document`'s object graph
//! stays the one source of truth on the Amalith side of that boundary.
//!
//! Export is exact for what Amalith can currently create (`Path`,
//! `Group`, `CompoundPath`); the stub kinds (`Text`, `Image`, `Symbol`)
//! have no real content to export yet and are skipped. Import is
//! deliberately best-effort over a useful common subset (`<path>`,
//! `<rect>`, `<circle>`, `<ellipse>`, `<g>`, with `matrix`/`translate`
//! transforms) rather than a full SVG implementation — an unsupported
//! element or transform function is silently skipped, not a hard error,
//! so pasting a complex real-world SVG still recovers whatever Amalith
//! *can* represent instead of failing outright.
use amalith_core::{
    Affine, Appearance, Color, Document, GradientKind, GroupData, LayerId, LineCap, LineJoin, Object,
    ObjectId, ObjectKind, ObjectParent, Paint, PathData, Rect, StrokeStyle,
};
use kurbo::BezPath;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SvgError {
    #[error("malformed SVG/XML: {0}")]
    Xml(#[from] roxmltree::Error),
    #[error("document root is not an <svg> element")]
    NoSvgRoot,
    #[error(
        "no supported shapes found in SVG (only <path>, <rect>, <circle>, <ellipse>, <g> are understood)"
    )]
    NoSupportedContent,
}

/// A set of freshly-id'd objects parsed from an SVG document, with no
/// back-reference to any `Document` (there wasn't one — the geometry came
/// from outside). `roots` lists the top-level object ids, in document
/// order; `objects` holds every object (roots and group descendants
/// alike), keyed by id — the same shape `amalith-commands`' clipboard
/// uses internally, so a caller there can drop this straight in as one.
/// Each root's `parent` field is a placeholder (an unused, freshly
/// generated `LayerId`) since it never had a real one; a paste always
/// overwrites it before insertion.
#[derive(Debug, Clone)]
pub struct ImportedSvg {
    pub roots: Vec<ObjectId>,
    pub objects: HashMap<ObjectId, Object>,
}

/// The deliberately-small CSS subset used by Illustrator's SVG exports:
/// class selectors whose declarations are relevant to paint.
type ClassStyles = HashMap<String, Declarations>;
type Declarations = HashMap<String, String>;

/// Serializes `ids` (and, for a group, its full descendant tree) from
/// `document` to a standalone `<svg>` document in document-space
/// coordinates. `None` if none of `ids` resolve to an object with
/// contributing geometry (e.g. all missing, or only empty groups).
pub fn export_svg(document: &Document, ids: &[ObjectId]) -> Option<String> {
    let bounds = ids
        .iter()
        .filter_map(|&id| document.bounds_of(id))
        .reduce(|a, b| a.union(b))?;
    let mut body = String::new();
    let mut defs = Defs::default();
    for &id in ids {
        export_node(document, id, &mut body, &mut defs);
    }
    let defs_block = if defs.xml.is_empty() {
        String::new()
    } else {
        format!("<defs>{}</defs>", defs.xml)
    };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\">{defs_block}{body}</svg>",
        bounds.x0,
        bounds.y0,
        bounds.width(),
        bounds.height(),
    ))
}

/// Accumulates `<defs>` content (currently just gradients) while walking
/// the object tree, so each referenced gradient is emitted exactly once.
#[derive(Default)]
struct Defs {
    xml: String,
    seen: std::collections::HashSet<String>,
}

fn export_node(document: &Document, id: ObjectId, out: &mut String, defs: &mut Defs) {
    let Some(object) = document.object(id) else {
        return;
    };
    let [a, b, c, d, e, f] = object.transform.as_coeffs();
    let transform_attr = format!(" transform=\"matrix({a} {b} {c} {d} {e} {f})\"");
    match &object.kind {
        ObjectKind::Group(group) => {
            out.push_str(&format!("<g{transform_attr}>"));
            for &child in &group.children {
                export_node(document, child, out, defs);
            }
            out.push_str("</g>");
        }
        ObjectKind::Path(path) => {
            let paint_attrs = paint_attrs(&object.appearance, document, defs);
            out.push_str(&format!(
                "<path d=\"{}\"{transform_attr}{paint_attrs} />",
                path.geometry.to_svg()
            ));
        }
        ObjectKind::CompoundPath(compound) => {
            let paint_attrs = paint_attrs(&object.appearance, document, defs);
            out.push_str(&format!("<g{transform_attr}>"));
            for subpath in &compound.subpaths {
                out.push_str(&format!("<path d=\"{}\"{paint_attrs} />", subpath.to_svg()));
            }
            out.push_str("</g>");
        }
        // Stub kinds with no real content to export yet (see module docs).
        ObjectKind::Text(_) | ObjectKind::Image(_) | ObjectKind::Symbol(_) => {}
    }
}

/// SVG element id for a pooled gradient (`grad-<uuid>`).
fn gradient_svg_id(id: amalith_core::GradientId) -> String {
    format!("grad-{}", id.as_uuid())
}

/// Emits `gradient` into `defs` once, returning its `url(#...)` reference.
/// Coordinates are `objectBoundingBox` fractions, matching how Amalith
/// stores gradient geometry (see `amalith-core`'s `gradient` module).
fn emit_gradient(gradient: &amalith_core::Gradient, defs: &mut Defs) -> String {
    let svg_id = gradient_svg_id(gradient.id);
    if defs.seen.insert(svg_id.clone()) {
        // SVG stops, like our GPU gradient, only interpolate linearly, so
        // export the midpoint-baked stops (see `render_stops`) rather than
        // the raw ones — otherwise a skewed midpoint would silently vanish
        // on export.
        let stops: String = gradient
            .render_stops()
            .iter()
            .map(|s| {
                let c = s.color;
                format!(
                    "<stop offset=\"{}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
                    s.offset.clamp(0.0, 1.0),
                    hex_color(c),
                    c.a.clamp(0.0, 1.0),
                )
            })
            .collect();
        let [sx, sy] = gradient.start;
        let [ex, ey] = gradient.end;
        match gradient.kind {
            GradientKind::Linear => defs.xml.push_str(&format!(
                "<linearGradient id=\"{svg_id}\" gradientUnits=\"objectBoundingBox\" \
                 x1=\"{sx}\" y1=\"{sy}\" x2=\"{ex}\" y2=\"{ey}\">{stops}</linearGradient>"
            )),
            GradientKind::Radial => {
                let r = gradient.radius();
                defs.xml.push_str(&format!(
                    "<radialGradient id=\"{svg_id}\" gradientUnits=\"objectBoundingBox\" \
                     cx=\"{sx}\" cy=\"{sy}\" r=\"{r}\" fx=\"{sx}\" fy=\"{sy}\">{stops}</radialGradient>"
                ));
            }
        }
    }
    format!("url(#{svg_id})")
}

/// The `fill`/`stroke`/`stroke-width`/`opacity` attribute string for `appearance`,
/// e.g. ` fill="#e0e0e0" stroke="#2e2e2e" stroke-width="10"`. Without this,
/// an exported `<path>` carries no paint attributes at all, and SVG's own
/// spec default (fill: black, stroke: none) silently wins over whatever
/// color the object actually had in Amalith — pasting *any* Amalith shape
/// into another app renders it as a plain black square regardless of its
/// real appearance. This is the fix for exactly that.
fn paint_attrs(appearance: &Appearance, document: &Document, defs: &mut Defs) -> String {
    let mut attrs = paint_attr("fill", appearance.fill, document, defs);
    attrs.push_str(&paint_attr("stroke", appearance.stroke, document, defs));
    if appearance.stroke.is_visible() {
        attrs.push_str(&format!(" stroke-width=\"{}\"", appearance.stroke_width));
        let style = &appearance.stroke_style;
        attrs.push_str(match style.cap {
            LineCap::Butt => " stroke-linecap=\"butt\"",
            LineCap::Round => " stroke-linecap=\"round\"",
            LineCap::Square => " stroke-linecap=\"square\"",
        });
        attrs.push_str(match style.join {
            LineJoin::Miter => " stroke-linejoin=\"miter\"",
            LineJoin::Round => " stroke-linejoin=\"round\"",
            LineJoin::Bevel => " stroke-linejoin=\"bevel\"",
        });
        if style.join == LineJoin::Miter {
            attrs.push_str(&format!(" stroke-miterlimit=\"{}\"", style.miter_limit));
        }
        if let Some(pattern) = style.dash_pattern() {
            let dash = pattern
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",");
            attrs.push_str(&format!(" stroke-dasharray=\"{dash}\""));
            if style.dash_offset != 0.0 {
                attrs.push_str(&format!(" stroke-dashoffset=\"{}\"", style.dash_offset));
            }
        }
    }
    if appearance.opacity != 1.0 {
        attrs.push_str(&format!(
            " opacity=\"{}\"",
            appearance.opacity.clamp(0.0, 1.0)
        ));
    }
    attrs
}

fn paint_attr(name: &str, paint: Paint, document: &Document, defs: &mut Defs) -> String {
    match paint {
        Paint::None => format!(" {name}=\"none\""),
        Paint::Solid(color) => {
            let mut attr = format!(" {name}=\"{}\"", hex_color(color));
            if color.a < 1.0 {
                attr.push_str(&format!(" {name}-opacity=\"{}\"", color.a.clamp(0.0, 1.0)));
            }
            attr
        }
        Paint::Gradient(id) => match document.gradient(id) {
            Some(gradient) => format!(" {name}=\"{}\"", emit_gradient(gradient, defs)),
            // Stale reference (gradient deleted from the pool): render as
            // nothing rather than SVG's black default.
            None => format!(" {name}=\"none\""),
        },
    }
}

fn hex_color(color: Color) -> String {
    format!(
        "#{:02x}{:02x}{:02x}",
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

/// Parses an SVG document into a flat, freshly-id'd object set. Errors
/// only when the text isn't XML, isn't rooted at `<svg>`, or contains no
/// element this importer understands at all — see module docs for the
/// best-effort policy on individual unsupported elements/transforms.
pub fn import_svg(svg: &str) -> Result<ImportedSvg, SvgError> {
    let document = roxmltree::Document::parse(svg)?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return Err(SvgError::NoSvgRoot);
    }
    let class_styles = parse_class_styles(&root);

    let mut objects = HashMap::new();
    let mut roots = Vec::new();
    let placeholder_parent = ObjectParent::Layer(LayerId::new());
    for child in root.children().filter(|node| node.is_element()) {
        if let Some(id) = import_node(&child, placeholder_parent, &class_styles, &mut objects) {
            roots.push(id);
        }
    }
    if roots.is_empty() {
        return Err(SvgError::NoSupportedContent);
    }
    Ok(ImportedSvg { roots, objects })
}

fn import_node(
    node: &roxmltree::Node,
    parent: ObjectParent,
    class_styles: &ClassStyles,
    objects: &mut HashMap<ObjectId, Object>,
) -> Option<ObjectId> {
    let transform = node
        .attribute("transform")
        .map(parse_transform_list)
        .unwrap_or(Affine::IDENTITY);

    let kind = match node.tag_name().name() {
        "path" => ObjectKind::Path(PathData::from_bezpath(
            BezPath::from_svg(node.attribute("d")?).ok()?,
        )),
        "rect" => {
            let x = attr_f64(node, "x").unwrap_or(0.0);
            let y = attr_f64(node, "y").unwrap_or(0.0);
            let width = attr_f64(node, "width")?;
            let height = attr_f64(node, "height")?;
            ObjectKind::Path(PathData::rectangle(Rect::new(x, y, x + width, y + height)))
        }
        "circle" => {
            let cx = attr_f64(node, "cx").unwrap_or(0.0);
            let cy = attr_f64(node, "cy").unwrap_or(0.0);
            let r = attr_f64(node, "r")?;
            ObjectKind::Path(PathData::ellipse(Rect::new(cx - r, cy - r, cx + r, cy + r)))
        }
        "ellipse" => {
            let cx = attr_f64(node, "cx").unwrap_or(0.0);
            let cy = attr_f64(node, "cy").unwrap_or(0.0);
            let rx = attr_f64(node, "rx")?;
            let ry = attr_f64(node, "ry")?;
            ObjectKind::Path(PathData::ellipse(Rect::new(
                cx - rx,
                cy - ry,
                cx + rx,
                cy + ry,
            )))
        }
        "g" => {
            let id = ObjectId::new();
            let children: Vec<ObjectId> = node
                .children()
                .filter(|child| child.is_element())
                .filter_map(|child| {
                    import_node(&child, ObjectParent::Group(id), class_styles, objects)
                })
                .collect();
            if children.is_empty() {
                // An empty group has no contributing geometry and nothing
                // downstream (bounds, paste) can do with it.
                return None;
            }
            let mut object = Object::new(id, parent, ObjectKind::Group(GroupData { children, clip: None }));
            object.transform = transform;
            objects.insert(id, object);
            return Some(id);
        }
        // Best-effort: anything else (defs, filters, text, image, ...) is
        // silently skipped rather than failing the whole import.
        _ => return None,
    };

    let mut object = Object::new(ObjectId::new(), parent, kind);
    object.transform = transform;
    object.appearance = parse_appearance(node, class_styles);
    let id = object.id;
    objects.insert(id, object);
    Some(id)
}

fn attr_f64(node: &roxmltree::Node, name: &str) -> Option<f64> {
    node.attribute(name)?.trim().parse().ok()
}

/// Reads `fill`/`stroke`/`stroke-width` off a leaf node, starting from
/// SVG's own real defaults (fill: black, stroke: none) — not
/// [`Appearance::default`], which is Amalith's own "what a freshly-drawn
/// primitive looks like" default, not what plain SVG with no paint
/// attributes actually means. An external SVG that omits `fill` is
/// specifying opaque black, same as any standards-compliant renderer
/// would show it. Presentation attributes are lowest priority; an
/// Illustrator-style class rule wins over them, and an inline `style`
/// declaration wins over either. When a node has several classes, the first
/// listed class that declares a given property wins (a sufficient,
/// deterministic subset for Illustrator's normally single-class output).
fn parse_appearance(node: &roxmltree::Node, class_styles: &ClassStyles) -> Appearance {
    let mut appearance = Appearance {
        fill: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
        stroke: Paint::None,
        stroke_width: Appearance::DEFAULT_STROKE_WIDTH,
        ..Appearance::default()
    };
    let inline_style = node
        .attribute("style")
        .map(parse_declarations)
        .unwrap_or_default();
    let fill = resolve_property(node, &inline_style, class_styles, "fill");
    let fill_opacity = resolve_property(node, &inline_style, class_styles, "fill-opacity");
    let stroke = resolve_property(node, &inline_style, class_styles, "stroke");
    let stroke_opacity = resolve_property(node, &inline_style, class_styles, "stroke-opacity");
    let stroke_width = resolve_property(node, &inline_style, class_styles, "stroke-width");
    let opacity = resolve_property(node, &inline_style, class_styles, "opacity");
    let linecap = resolve_property(node, &inline_style, class_styles, "stroke-linecap");
    let linejoin = resolve_property(node, &inline_style, class_styles, "stroke-linejoin");
    let miterlimit = resolve_property(node, &inline_style, class_styles, "stroke-miterlimit");
    let dasharray = resolve_property(node, &inline_style, class_styles, "stroke-dasharray");
    let dashoffset = resolve_property(node, &inline_style, class_styles, "stroke-dashoffset");

    if let Some(fill) = fill.as_deref() {
        appearance.fill = parse_paint(fill, fill_opacity.as_deref());
    }
    if let Some(stroke) = stroke.as_deref() {
        appearance.stroke = parse_paint(stroke, stroke_opacity.as_deref());
    }
    if let Some(width) = stroke_width.and_then(|width| width.trim().parse().ok()) {
        appearance.stroke_width = width;
    }
    if let Some(opacity) = opacity.and_then(|opacity| opacity.trim().parse().ok()) {
        appearance.opacity = opacity;
    }
    appearance.stroke_style = parse_stroke_style(
        linecap.as_deref(),
        linejoin.as_deref(),
        miterlimit.as_deref(),
        dasharray.as_deref(),
        dashoffset.as_deref(),
    );
    appearance
}

/// Maps SVG's `stroke-linecap` / `-linejoin` / `-miterlimit` /
/// `-dasharray` / `-dashoffset` onto a [`StrokeStyle`]. Anything absent
/// or unrecognised keeps its default.
fn parse_stroke_style(
    linecap: Option<&str>,
    linejoin: Option<&str>,
    miterlimit: Option<&str>,
    dasharray: Option<&str>,
    dashoffset: Option<&str>,
) -> StrokeStyle {
    let mut style = StrokeStyle::default();
    match linecap.map(str::trim) {
        Some("round") => style.cap = LineCap::Round,
        Some("square") => style.cap = LineCap::Square,
        Some("butt") => style.cap = LineCap::Butt,
        _ => {}
    }
    match linejoin.map(str::trim) {
        Some("round") => style.join = LineJoin::Round,
        Some("bevel") => style.join = LineJoin::Bevel,
        Some("miter") | Some("miter-clip") | Some("arcs") => style.join = LineJoin::Miter,
        _ => {}
    }
    if let Some(limit) = miterlimit.and_then(|v| v.trim().parse().ok()) {
        style.miter_limit = limit;
    }
    // `none` / empty / all-zero → solid. Otherwise take the first three
    // dash/gap lengths (comma or whitespace separated, per SVG).
    if let Some(list) = dasharray.map(str::trim).filter(|v| !v.is_empty() && *v != "none") {
        let nums: Vec<f64> = list
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        if nums.iter().any(|n| *n > 0.0) {
            style.dashed = true;
            for (slot, value) in style.dash.iter_mut().zip(nums.iter()) {
                *slot = value.max(0.0);
            }
        }
    }
    if let Some(offset) = dashoffset.and_then(|v| v.trim().parse().ok()) {
        style.dash_offset = offset;
    }
    style
}

/// Collects the small CSS subset Illustrator places in `<defs><style>`.
/// Invalid rules/declarations are ignored so an unrelated unsupported CSS
/// feature never prevents the rest of an SVG from importing.
fn parse_class_styles(root: &roxmltree::Node) -> ClassStyles {
    let mut styles = ClassStyles::new();
    for defs in root
        .descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "defs")
    {
        for style in defs
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "style")
        {
            let css = strip_css_comments(style.text().unwrap_or(""));
            for block in css.split('}') {
                let Some((selectors, declarations)) = block.split_once('{') else {
                    continue;
                };
                let declarations = parse_declarations(declarations);
                if declarations.is_empty() {
                    continue;
                }
                for selector in selectors.split(',') {
                    let Some(class) = selector.trim().strip_prefix('.') else {
                        continue;
                    };
                    let class = class
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_end_matches(|c: char| c == ':' || c == '.');
                    if !class.is_empty() {
                        styles.insert(class.to_string(), declarations.clone());
                    }
                }
            }
        }
    }
    styles
}

/// Parses semicolon-delimited declarations from either a class rule or an
/// element's inline `style` attribute. Only paint properties in scope for
/// this importer are retained.
fn parse_declarations(value: &str) -> Declarations {
    value
        .split(';')
        .filter_map(|declaration| {
            let (property, value) = declaration.split_once(':')?;
            let property = property.trim();
            let value = value.trim();
            matches!(
                property,
                "fill"
                    | "stroke"
                    | "stroke-width"
                    | "fill-opacity"
                    | "stroke-opacity"
                    | "opacity"
                    | "stroke-linecap"
                    | "stroke-linejoin"
                    | "stroke-miterlimit"
                    | "stroke-dasharray"
                    | "stroke-dashoffset"
            )
            .then(|| (property.to_string(), value.to_string()))
        })
        .collect()
}

/// Removes `/* ... */` blocks before the deliberately-simple CSS parser
/// sees selectors/declarations. An unterminated comment simply consumes the
/// rest of the style text.
fn strip_css_comments(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("/*") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("*/") else {
            return result;
        };
        rest = &after_start[end + 2..];
    }
    result.push_str(rest);
    result
}

fn resolve_property(
    node: &roxmltree::Node,
    inline_style: &Declarations,
    class_styles: &ClassStyles,
    property: &str,
) -> Option<String> {
    inline_style
        .get(property)
        .cloned()
        .or_else(|| {
            node.attribute("class")
                .into_iter()
                .flat_map(str::split_whitespace)
                .find_map(|class| {
                    class_styles
                        .get(class)
                        .and_then(|rules| rules.get(property))
                })
                .cloned()
        })
        .or_else(|| node.attribute(property).map(str::to_string))
}

/// `value` is a `fill`/`stroke` attribute's value; `opacity` is the
/// matching `fill-opacity`/`stroke-opacity` attribute, if present.
/// Best-effort: `#rgb`/`#rrggbb` hex and `rgb(r, g, b)` cover what
/// Amalith's own exporter and most real-world tools emit; anything else
/// (a named color, a `url(#gradient)` reference, ...) falls back to
/// `Paint::None` rather than erroring the whole import over one
/// unsupported paint.
fn parse_paint(value: &str, opacity: Option<&str>) -> Paint {
    let value = value.trim();
    if value.eq_ignore_ascii_case("none") {
        return Paint::None;
    }
    let Some((r, g, b)) = parse_css_color(value) else {
        return Paint::None;
    };
    let a = opacity
        .and_then(|o| o.trim().parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 1.0);
    Paint::Solid(Color::rgba(r, g, b, a))
}

fn parse_css_color(value: &str) -> Option<(f32, f32, f32)> {
    if let Some(hex) = value.strip_prefix('#') {
        let expand = |c: char| u8::from_str_radix(&c.to_string().repeat(2), 16).ok();
        let channel = |s: &str| u8::from_str_radix(s, 16).ok();
        let (r, g, b) = match hex.len() {
            6 => (
                channel(&hex[0..2])?,
                channel(&hex[2..4])?,
                channel(&hex[4..6])?,
            ),
            3 => {
                let mut chars = hex.chars();
                (
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                    expand(chars.next()?)?,
                )
            }
            _ => return None,
        };
        Some((r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0))
    } else if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<f32> = inner
            .split(',')
            .filter_map(|p| p.trim().trim_end_matches('%').parse().ok())
            .collect();
        match parts.as_slice() {
            [r, g, b] => Some((r / 255.0, g / 255.0, b / 255.0)),
            _ => None,
        }
    } else {
        None
    }
}

/// Parses an SVG `transform` attribute's function list (e.g. `"translate(4
/// 5) matrix(1 0 0 1 2 3)"`), composing understood functions left to right
/// to match SVG's semantics (the first-listed function is applied closest
/// to the point). `matrix` and `translate` cover the common case (and
/// everything this module's own `export_svg` ever emits); any other
/// function name is skipped rather than erroring.
fn parse_transform_list(value: &str) -> Affine {
    let mut result = Affine::IDENTITY;
    let mut rest = value;
    while let Some(open) = rest.find('(') {
        let name = rest[..open].trim();
        let Some(close) = rest[open..].find(')') else {
            break;
        };
        let args: Vec<f64> = rest[open + 1..open + close]
            .split([',', ' '])
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        if let Some(affine) = parse_transform_function(name, &args) {
            result = result * affine;
        }
        rest = &rest[open + close + 1..];
    }
    result
}

fn parse_transform_function(name: &str, args: &[f64]) -> Option<Affine> {
    match (name, args) {
        ("matrix", [a, b, c, d, e, f]) => Some(Affine::new([*a, *b, *c, *d, *e, *f])),
        ("translate", [tx]) => Some(Affine::translate((*tx, 0.0))),
        ("translate", [tx, ty]) => Some(Affine::translate((*tx, *ty))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{Layer, LayerId};

    fn doc_with_rect(rect: Rect) -> (Document, ObjectId) {
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let object = Object::rectangle(ObjectId::new(), ObjectParent::Layer(layer_id), rect);
        let id = object.id;
        document.insert_object(object, 0).unwrap();
        (document, id)
    }

    #[test]
    fn export_then_import_preserves_rectangle_bounds() {
        let rect = Rect::new(10.0, 20.0, 110.0, 70.0);
        let (document, id) = doc_with_rect(rect);
        let expected_bounds = document.bounds_of(id).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();
        let imported = import_svg(&svg).unwrap();

        assert_eq!(imported.roots.len(), 1);
        let imported_object = &imported.objects[&imported.roots[0]];
        let imported_bounds = imported_object
            .kind
            .own_local_bounds()
            .map(|local| imported_object.transform.transform_rect_bbox(local))
            .unwrap();
        assert_eq!(imported_bounds, expected_bounds);
    }

    #[test]
    fn import_preserves_group_children_and_order() {
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let group = Object::new(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            ObjectKind::Group(Default::default()),
        );
        let group_id = group.id;
        document.insert_object(group, 0).unwrap();
        let a = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        document.insert_object(a, 0).unwrap();
        let b = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Group(group_id),
            Rect::new(20.0, 0.0, 30.0, 10.0),
        );
        document.insert_object(b, 1).unwrap();

        let svg = export_svg(&document, &[group_id]).unwrap();
        let imported = import_svg(&svg).unwrap();

        assert_eq!(imported.roots.len(), 1);
        let ObjectKind::Group(imported_group) = &imported.objects[&imported.roots[0]].kind else {
            panic!("expected a group");
        };
        assert_eq!(imported_group.children.len(), 2);
        // Every imported id is fresh — none reuse the source document's ids.
        for child_id in &imported_group.children {
            assert!(document.object(*child_id).is_none());
        }
    }

    #[test]
    fn import_applies_translate_transform() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="0" y="0" width="10" height="10" transform="translate(5 7)" /></svg>"#;
        let imported = import_svg(svg).unwrap();
        let object = &imported.objects[&imported.roots[0]];
        assert_eq!(
            object.transform * kurbo::Point::ORIGIN,
            kurbo::Point::new(5.0, 7.0)
        );
    }

    #[test]
    fn import_rejects_non_svg_text() {
        let err = import_svg("hello, this is not svg at all").unwrap_err();
        assert!(matches!(err, SvgError::Xml(_)));
    }

    #[test]
    fn import_rejects_svg_with_no_understood_content() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><defs></defs><text>hi</text></svg>"#;
        let err = import_svg(svg).unwrap_err();
        assert!(matches!(err, SvgError::NoSupportedContent));
    }

    #[test]
    fn export_skips_unknown_ids_but_keeps_the_rest() {
        let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
        let (document, id) = doc_with_rect(rect);
        let bogus = ObjectId::new();
        let svg = export_svg(&document, &[bogus, id]).unwrap();
        let imported = import_svg(&svg).unwrap();
        assert_eq!(imported.roots.len(), 1);
    }

    #[test]
    fn export_writes_fill_and_stroke_attributes() {
        // Regression test for the "always pastes into Illustrator as a
        // black square" bug: exported paths used to carry no paint
        // attributes at all, so SVG's own default (fill: black, stroke:
        // none) silently overrode whatever color the object really had.
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let mut object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        object.appearance = Appearance {
            fill: Paint::Solid(Color::rgb(0.0, 1.0, 0.0)),
            stroke: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)),
            stroke_width: 10.0,
            ..Appearance::default()
        };
        let id = object.id;
        document.insert_object(object, 0).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();

        assert!(svg.contains("fill=\"#00ff00\""), "svg was: {svg}");
        assert!(svg.contains("stroke=\"#ff0000\""), "svg was: {svg}");
        assert!(svg.contains("stroke-width=\"10\""), "svg was: {svg}");
    }

    #[test]
    fn stroke_style_round_trips_through_svg() {
        use amalith_core::{StrokeAlign, StrokeStyle};
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let mut object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            Rect::new(0.0, 0.0, 40.0, 40.0),
        );
        object.appearance.stroke = Paint::Solid(Color::rgb(0.0, 0.0, 0.0));
        object.appearance.stroke_style = StrokeStyle {
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 8.0,
            // Align has no SVG representation — it should reset on import.
            align: StrokeAlign::Outside,
            dashed: true,
            dash: [6.0, 3.0, 0.0, 0.0, 0.0, 0.0],
            dash_offset: 2.0,
        };
        let id = object.id;
        document.insert_object(object, 0).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();
        assert!(svg.contains("stroke-linecap=\"round\""), "svg was: {svg}");
        assert!(svg.contains("stroke-linejoin=\"bevel\""), "svg was: {svg}");
        assert!(svg.contains("stroke-dasharray=\"6,3\""), "svg was: {svg}");
        assert!(svg.contains("stroke-dashoffset=\"2\""), "svg was: {svg}");

        let imported = import_svg(&svg).unwrap();
        let style = imported.objects[&imported.roots[0]].appearance.stroke_style;
        assert_eq!(style.cap, LineCap::Round);
        assert_eq!(style.join, LineJoin::Bevel);
        assert!(style.dashed);
        assert_eq!(style.dash_pattern(), Some(vec![6.0, 3.0]));
        assert_eq!(style.dash_offset, 2.0);
        assert_eq!(style.align, StrokeAlign::Center);
    }

    #[test]
    fn export_writes_none_for_no_paint() {
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let mut object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        object.appearance = Appearance {
            fill: Paint::None,
            stroke: Paint::None,
            stroke_width: 10.0,
            ..Appearance::default()
        };
        let id = object.id;
        document.insert_object(object, 0).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();

        assert!(svg.contains("fill=\"none\""), "svg was: {svg}");
        assert!(svg.contains("stroke=\"none\""), "svg was: {svg}");
        // No stroke color means no stroke-width attribute either.
        assert!(!svg.contains("stroke-width"), "svg was: {svg}");
    }

    #[test]
    fn export_then_import_preserves_appearance() {
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let mut object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        object.appearance = Appearance {
            fill: Paint::Solid(Color::rgb(0.2, 0.4, 0.6)),
            stroke: Paint::None,
            stroke_width: 10.0,
            ..Appearance::default()
        };
        let id = object.id;
        document.insert_object(object, 0).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();
        let imported = import_svg(&svg).unwrap();
        let imported_object = &imported.objects[&imported.roots[0]];

        assert_eq!(imported_object.appearance.stroke, Paint::None);
        let Paint::Solid(fill) = imported_object.appearance.fill else {
            panic!("expected a solid fill");
        };
        assert!((fill.r - 0.2).abs() < 0.01, "fill was {fill:?}");
        assert!((fill.g - 0.4).abs() < 0.01, "fill was {fill:?}");
        assert!((fill.b - 0.6).abs() < 0.01, "fill was {fill:?}");
    }

    #[test]
    fn import_defaults_to_svg_spec_black_fill_when_no_paint_attrs() {
        // Content from *outside* Amalith (hand-authored, or another tool's
        // export) that omits fill/stroke means SVG's real default (opaque
        // black fill, no stroke) -- not Amalith's own "freshly drawn
        // primitive" default of a light fill and a dark stroke.
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg"><rect x="0" y="0" width="10" height="10" /></svg>"#;
        let imported = import_svg(svg).unwrap();
        let object = &imported.objects[&imported.roots[0]];
        assert_eq!(
            object.appearance.fill,
            Paint::Solid(Color::rgb(0.0, 0.0, 0.0))
        );
        assert_eq!(object.appearance.stroke, Paint::None);
    }

    #[test]
    fn import_parses_short_and_long_hex_colors_and_none() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <rect x="0" y="0" width="10" height="10" fill="#f00" stroke="none" />
        </svg>"##;
        let imported = import_svg(svg).unwrap();
        let object = &imported.objects[&imported.roots[0]];
        assert_eq!(
            object.appearance.fill,
            Paint::Solid(Color::rgb(1.0, 0.0, 0.0))
        );
        assert_eq!(object.appearance.stroke, Paint::None);
    }

    #[test]
    fn import_resolves_illustrator_class_fill_from_defs_style() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 4935 4788.32">
            <defs><style>
                /* Illustrator commonly emits this class-based paint. */
                .cls-1 { fill: #9eabeb; }
            </style></defs>
            <path class="cls-1" d="M2583.06,4004.64 L2600,4004.64 L2600,4020 Z"/>
            <path class="cls-1" d="M2700,4000 L2720,4000 L2720,4020 Z"/>
        </svg>"##;

        let imported = import_svg(svg).unwrap();
        assert_eq!(imported.roots.len(), 2);
        for id in imported.roots {
            assert_eq!(
                imported.objects[&id].appearance.fill,
                Paint::Solid(Color::rgb(158.0 / 255.0, 171.0 / 255.0, 235.0 / 255.0))
            );
        }
    }

    #[test]
    fn import_parses_inline_style_paint_declarations() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <path d="M0,0 L10,0 L10,10 Z"
                style="fill: #123456; stroke: #abcdef; stroke-width: 3;"/>
        </svg>"##;

        let imported = import_svg(svg).unwrap();
        let appearance = &imported.objects[&imported.roots[0]].appearance;
        assert_eq!(
            appearance.fill,
            Paint::Solid(Color::rgb(18.0 / 255.0, 52.0 / 255.0, 86.0 / 255.0))
        );
        assert_eq!(
            appearance.stroke,
            Paint::Solid(Color::rgb(171.0 / 255.0, 205.0 / 255.0, 239.0 / 255.0))
        );
        assert_eq!(appearance.stroke_width, 3.0);
    }

    #[test]
    fn import_style_and_class_paint_override_presentation_attributes() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <defs><style>.cls-1 { fill: #00ff00; }</style></defs>
            <path class="cls-1" fill="#ff0000" d="M0,0 L10,0 L10,10 Z"/>
            <path class="cls-1" fill="#ff0000" style="fill: #0000ff;"
                d="M20,0 L30,0 L30,10 Z"/>
        </svg>"##;

        let imported = import_svg(svg).unwrap();
        assert_eq!(
            imported.objects[&imported.roots[0]].appearance.fill,
            Paint::Solid(Color::rgb(0.0, 1.0, 0.0)),
            "class declarations beat presentation attributes"
        );
        assert_eq!(
            imported.objects[&imported.roots[1]].appearance.fill,
            Paint::Solid(Color::rgb(0.0, 0.0, 1.0)),
            "inline style declarations beat class declarations"
        );
    }

    /// Regression test for the "massive corruption on pasting a heavy
    /// real-world Illustrator file" report. The fixture is one representative
    /// path lifted verbatim from an Illustrator SVG-clipboard export: a
    /// multi-thousand-element, multi-subpath path mixing straight segments
    /// and relative cubics, using Illustrator's compressed number syntax
    /// (`.66.33`, `-2.44-45.28`, `528.32c…`), viewBox `0 0 4935 4788.32`.
    ///
    /// The visible corruption was traced to the app renderer filling each
    /// subpath with `egui::Shape::convex_polygon` (a triangle fan), which
    /// only draws convex polygons correctly — every subpath in artwork like
    /// this is strongly non-convex. The imported *geometry* is correct, and
    /// this test locks that down so a future parser or `geom.rs` change
    /// can't quietly turn a rendering-only artifact into a real data bug:
    /// the element composition, subpath count, and all coordinates/bounds
    /// must survive import, and the recently-added per-subpath anchor walk
    /// (`anchor_indices` / `anchor_position` / `translate_anchor`) must
    /// handle every anchor of a 1400+ element path without panicking or
    /// producing non-finite coordinates.
    #[test]
    fn import_heavy_illustrator_path_keeps_geometry_intact() {
        use amalith_core::Vec2;
        use kurbo::PathEl;

        let svg = include_str!("../tests/fixtures/illustrator-heavy-paste.svg");
        let imported = import_svg(svg).expect("heavy Illustrator path must import");

        assert_eq!(imported.roots.len(), 1, "one <path> in, one object out");
        let ObjectKind::Path(path) = &imported.objects[&imported.roots[0]].kind else {
            panic!("expected a Path");
        };
        let elements = path.geometry.elements();

        // Exact command composition, matching an independent count of the
        // `d` string (M=4, C=1370 from `c`/`s`, L=41 from `l`/`h`/`v`,
        // Q=2, Z=4). A parser that drops or duplicates segments trips here.
        let count = |pred: fn(&PathEl) -> bool| elements.iter().filter(|e| pred(e)).count();
        assert_eq!(elements.len(), 1421, "total element count");
        assert_eq!(
            count(|e| matches!(e, PathEl::MoveTo(_))),
            4,
            "four subpaths (four MoveTo)"
        );
        assert_eq!(
            count(|e| matches!(e, PathEl::ClosePath)),
            4,
            "every subpath is closed"
        );
        assert_eq!(count(|e| matches!(e, PathEl::LineTo(_))), 41);
        assert_eq!(count(|e| matches!(e, PathEl::CurveTo(_, _, _))), 1370);
        assert_eq!(count(|e| matches!(e, PathEl::QuadTo(_, _))), 2);

        // No NaN / Infinity anywhere in the parsed geometry.
        let coords = |e: &PathEl| -> Vec<f64> {
            match *e {
                PathEl::MoveTo(p) | PathEl::LineTo(p) => vec![p.x, p.y],
                PathEl::QuadTo(a, b) => vec![a.x, a.y, b.x, b.y],
                PathEl::CurveTo(a, b, c) => vec![a.x, a.y, b.x, b.y, c.x, c.y],
                PathEl::ClosePath => vec![],
            }
        };
        assert!(
            elements.iter().flat_map(coords).all(f64::is_finite),
            "all coordinates must be finite"
        );

        // Bounds are finite and sit inside the document's viewBox — a
        // mis-parsed relative command would fling a point far outside it.
        let bounds = amalith_core::geom::bez_path_bounds(&path.geometry);
        assert!(
            [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
                .iter()
                .all(|v| v.is_finite()),
            "bounds must be finite: {bounds:?}"
        );
        assert!(
            bounds.x0 >= 0.0 && bounds.x1 <= 4935.0 && bounds.y0 >= 0.0 && bounds.y1 <= 4788.32,
            "bounds must stay within the viewBox: {bounds:?}"
        );

        // Flattening recovers exactly the four subpaths (the renderer walks
        // this list; a dropped subpath here would drop part of the shape).
        assert_eq!(
            path.flattened_points(0.5).len(),
            4,
            "four flattened subpaths"
        );

        // The anchor model must cope with a 1400+ element, 4-subpath
        // path: every anchor resolves to a finite position, and
        // translating each in turn never panics or introduces a
        // non-finite coordinate.
        let n = amalith_core::anchor_count(path.subpaths());
        assert!(
            (1000..elements.len()).contains(&n),
            "expected most elements to be editable anchors, got {n}"
        );
        for i in 0..n {
            let a = amalith_core::anchor_at(path.subpaths(), i)
                .unwrap_or_else(|| panic!("anchor {i} missing"));
            assert!(a.point.x.is_finite() && a.point.y.is_finite());
        }
        let mut sp = path.subpaths().to_vec();
        for i in 0..n {
            amalith_core::translate_anchor_n(&mut sp, i, Vec2::new(1.5, -2.0));
        }
        let moved = amalith_core::subpaths_to_bezpath(&sp);
        assert!(
            moved.elements().iter().flat_map(coords).all(f64::is_finite),
            "translating every anchor must keep coordinates finite"
        );
        assert_eq!(
            moved.elements().iter().filter(|e| matches!(e, PathEl::MoveTo(_))).count(),
            4,
            "the four subpaths survive the anchor round-trip"
        );
    }

    #[test]
    fn import_resolves_object_opacity_from_class_and_inline_style() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg">
            <defs><style>.cls-1 { opacity: 0.25; }</style></defs>
            <path class="cls-1" d="M0,0 L10,0 L10,10 Z"/>
            <path class="cls-1" style="opacity: 0.6" d="M20,0 L30,0 L30,10 Z"/>
        </svg>"##;
        let imported = import_svg(svg).unwrap();
        assert_eq!(
            imported.objects[&imported.roots[0]].appearance.opacity,
            0.25
        );
        assert_eq!(imported.objects[&imported.roots[1]].appearance.opacity, 0.6);
    }

    #[test]
    fn export_writes_and_round_trips_object_opacity() {
        let mut document = Document::new("Test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        document.insert_layer(layer, 0);
        let mut object = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            Rect::new(0.0, 0.0, 10.0, 10.0),
        );
        object.appearance.opacity = 0.4;
        let id = object.id;
        document.insert_object(object, 0).unwrap();

        let svg = export_svg(&document, &[id]).unwrap();
        assert!(svg.contains("opacity=\"0.4\""), "svg was: {svg}");
        let imported = import_svg(&svg).unwrap();
        assert_eq!(imported.objects[&imported.roots[0]].appearance.opacity, 0.4);
    }
}
