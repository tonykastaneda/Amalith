//! Vector PDF export: walks the document tree and writes real PDF paths,
//! fills, strokes, and gradients instead of a rasterized snapshot of the
//! whole canvas. `Path`, `CompoundPath`, and `Group` are genuine vector
//! content; `Text` is too, via its glyph outlines (the same conversion
//! Type ▸ Create Outlines already does — no font embedding needed).
//! `Image` is the one kind that's raster by nature: it's embedded as a
//! JPEG XObject straight from the already-decoded CPU pixels in the
//! app's image cache (see `export_vector_pdf`'s `images` argument) —
//! there's no rendering here, just a JPEG encode of bytes that already
//! exist.
//!
//! # Coordinate strategy
//!
//! A PDF page has its own coordinate system (points, y-up, origin at the
//! bottom-left) distinct from this app's document space (px @ 96dpi,
//! y-down). The obvious way to bridge that — emit a `cm` operator per
//! object composing its transform into the content stream's CTM — breaks
//! for gradients: a PDF Pattern's `/Matrix` maps pattern space to the
//! *default* (initial) coordinate system of its parent content stream,
//! ignoring whatever `cm`s are in effect when the pattern is actually
//! painted. Mixing `cm`-placed path geometry with pattern-filled geometry
//! would then silently misalign the two.
//!
//! So this module never emits `cm` at all. Every point — path geometry
//! and pattern matrices alike — is pre-multiplied in Rust through the
//! object's full cumulative transform *and* the doc-space→page-space map
//! ([`PageMap::affine`]) before it's written, landing everything in one
//! absolute coordinate system a Pattern's `/Matrix` and a path's raw
//! coordinates always agree on.
//!
//! # Alpha
//!
//! A PDF shading dictionary only ever describes colour — there's no
//! per-stop alpha in the format at all — but this app's gradients
//! routinely fade a stop's *alpha* (`GradientStop::opacity`, or a color
//! with `a < 1`), and every freeform point fades all the way to
//! transparent by design. This is reproduced with PDF's standard
//! technique for spatially-varying alpha: paint a second, grayscale
//! shading with the *same* geometry (white where the color shading is
//! opaque, black where it's transparent) into an isolated form, and apply
//! that as a luminosity soft mask (`ExtGState /SMask`) while the real
//! color shading paints. See `paint_shaded_layer`.
//!
//! # Known simplifications
//!
//! - **Group opacity** is applied per leaf object (its own fill/stroke),
//!   not as a true isolated transparency group — matters only where an
//!   object's own semi-transparent fill and stroke overlap at an
//!   anti-aliased edge, an imperceptible difference in practice.
//! - **Non-uniform-scale stroke width**: PDF strokes a path with a scalar
//!   line width in the stream's current space; since geometry is baked to
//!   absolute page space rather than placed via `cm`, a heavily
//!   skewed/non-uniformly-scaled object's stroke is approximated with the
//!   geometric mean of its transform's two axis scales rather than truly
//!   varying around the contour.
//! - **Clip groups** aren't clipped (painted as if the clip mask child
//!   didn't restrict anything) — matching `amalith_io::svg::export_node`,
//!   which has the same gap.
//! - **Spot colors / overprint** aren't modeled at all yet.
use std::collections::HashMap;

use amalith_core::{
    Affine as CoreAffine, Appearance, AssetId, Color, Document, FreeformPoint, Gradient,
    GradientKind, GradientStop, LineCap, LineJoin, ObjectId, ObjectKind, Paint, Point as CorePoint,
    Rect as CoreRect,
};
use kurbo::{BezPath, PathEl};
use pdf_writer::types::{ColorSpaceOperand, FunctionShadingType, LineCapStyle, LineJoinStyle, MaskType};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect as PdfRect, Ref};

use crate::lod::ImageLods;
use crate::text::TextContext;

/// Maps document space (px @ 96dpi, y-down, origin at `src`'s own origin)
/// to PDF page space (pt @ 72dpi, y-up, origin at the page's bottom-left)
/// for the artboard rect `src` being exported as one page.
#[derive(Clone, Copy)]
struct PageMap {
    to_pt: f64,
    ox: f64,
    oy: f64,
    ph: f64,
}

impl PageMap {
    fn new(src: CoreRect) -> Self {
        let to_pt = 72.0 / 96.0;
        Self {
            to_pt,
            ox: src.x0,
            oy: src.y0,
            ph: (src.y1 - src.y0) * to_pt,
        }
    }

    fn page_size(&self, src: CoreRect) -> (f32, f32) {
        (((src.x1 - src.x0) * self.to_pt) as f32, self.ph as f32)
    }

    /// See the module docs on why this is baked into every point/matrix
    /// in Rust instead of ever being emitted as a PDF `cm`.
    fn affine(&self) -> CoreAffine {
        CoreAffine::new([
            self.to_pt,
            0.0,
            0.0,
            -self.to_pt,
            -self.to_pt * self.ox,
            self.ph + self.to_pt * self.oy,
        ])
    }
}

fn pt(xf: CoreAffine, p: CorePoint) -> (f32, f32) {
    let p = xf * p;
    (p.x as f32, p.y as f32)
}

/// The six coefficients of `xf`, in the `[a b c d e f]` order every
/// `/Matrix` attribute in this module expects.
fn coeffs(xf: CoreAffine) -> [f32; 6] {
    let c = xf.as_coeffs();
    [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32, c[5] as f32]
}

/// Radial-only: the extra unit-space transform that squishes a plain
/// circle into the ellipse `aspect` describes, pivoting on `start` — the
/// same construction as `convert::radial_squish`, reimplemented here on
/// `amalith_core::Affine` (kurbo 0.11) rather than `vello::kurbo`'s
/// vendored 0.13, since this module never touches vello at all.
fn radial_squish(g: &Gradient) -> CoreAffine {
    if g.kind != GradientKind::Radial || (g.aspect - 1.0).abs() < 1e-9 {
        return CoreAffine::IDENTITY;
    }
    let center = CorePoint::new(g.start[0], g.start[1]);
    let angle = g.radial_axis_rad();
    CoreAffine::translate(center.to_vec2())
        * CoreAffine::rotate(angle)
        * CoreAffine::scale_non_uniform(1.0, g.aspect)
        * CoreAffine::rotate(-angle)
        * CoreAffine::translate(-center.to_vec2())
}

/// Accumulated PDF-writer state for the whole export: the object stream
/// under construction, the id allocator, and every resource (image,
/// pattern, ext-gstate) collected so far, written into the page's
/// `/Resources` once the walk finishes.
struct PdfCtx {
    pdf: Pdf,
    alloc: Ref,
    content: Content,
    base: CoreAffine,
    cmyk: bool,
    x_objects: Vec<(String, Ref)>,
    patterns: Vec<(String, Ref)>,
    ext_gs: Vec<(String, Ref)>,
    counter: u32,
}

impl PdfCtx {
    fn bump(&mut self) -> Ref {
        self.alloc.bump()
    }

    fn fresh_name(&mut self, prefix: &str) -> String {
        self.counter += 1;
        format!("{prefix}{}", self.counter)
    }

    fn set_fill_solid(&mut self, c: Color) {
        if self.cmyk {
            let [cy, m, y, k] = c.to_cmyk();
            self.content.set_fill_cmyk(cy, m, y, k);
        } else {
            self.content.set_fill_rgb(c.r, c.g, c.b);
        }
    }

    fn set_stroke_solid(&mut self, c: Color) {
        if self.cmyk {
            let [cy, m, y, k] = c.to_cmyk();
            self.content.set_stroke_cmyk(cy, m, y, k);
        } else {
            self.content.set_stroke_rgb(c.r, c.g, c.b);
        }
    }

    /// Emits `bez` (local object space) as path-construction operators in
    /// absolute page space via `xf` (this object's cumulative transform
    /// composed with the doc→page map). Quadratic segments are elevated
    /// to cubic — PDF's path operators have no quadratic curve.
    fn emit_geometry(&mut self, bez: &BezPath, xf: CoreAffine) {
        emit_geometry_into(&mut self.content, bez, xf);
    }
}

fn emit_geometry_into(content: &mut Content, bez: &BezPath, xf: CoreAffine) {
    let mut cur = CorePoint::ZERO;
    for el in bez.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = pt(xf, p);
                content.move_to(x, y);
                cur = p;
            }
            PathEl::LineTo(p) => {
                let (x, y) = pt(xf, p);
                content.line_to(x, y);
                cur = p;
            }
            PathEl::QuadTo(c, p) => {
                // Exact degree elevation 2→3: c1 = cur + 2/3*(c-cur), c2 = p + 2/3*(c-p).
                let c1 = CorePoint::new(cur.x + (c.x - cur.x) * 2.0 / 3.0, cur.y + (c.y - cur.y) * 2.0 / 3.0);
                let c2 = CorePoint::new(p.x + (c.x - p.x) * 2.0 / 3.0, p.y + (c.y - p.y) * 2.0 / 3.0);
                let (x1, y1) = pt(xf, c1);
                let (x2, y2) = pt(xf, c2);
                let (x3, y3) = pt(xf, p);
                content.cubic_to(x1, y1, x2, y2, x3, y3);
                cur = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let (x1, y1) = pt(xf, c1);
                let (x2, y2) = pt(xf, c2);
                let (x3, y3) = pt(xf, p);
                content.cubic_to(x1, y1, x2, y2, x3, y3);
                cur = p;
            }
            PathEl::ClosePath => {
                content.close_path();
            }
        }
    }
}

/// A function object mapping `t` in `0..=1` to `sample`'s output
/// dimension, built by stitching one linear (`N=1`) exponential segment
/// per gap in `stops` — mirrors exactly what `Gradient::render_stops`
/// already flattens a midpoint-skewed gradient into, so a skewed midpoint
/// reproduces here the same way it does in the GPU renderer and SVG
/// export. `stops` must have at least one entry.
fn stitching_function(ctx: &mut PdfCtx, stops: &[GradientStop], sample: impl Fn(Color) -> Vec<f32>) -> Ref {
    if stops.len() < 2 {
        // A single-stop "gradient" — reduce to one constant function.
        let dims = sample(stops[0].color).len();
        let id = ctx.bump();
        let mut f = ctx.pdf.exponential_function(id);
        f.domain([0.0, 1.0]);
        f.range((0..dims).flat_map(|_| [0.0, 1.0]));
        f.c0(sample(stops[0].color));
        f.c1(sample(stops[0].color));
        f.n(1.0);
        f.finish();
        return id;
    }
    let dims = sample(stops[0].color).len();
    let mut subs = Vec::with_capacity(stops.len() - 1);
    for pair in stops.windows(2) {
        let id = ctx.bump();
        let mut f = ctx.pdf.exponential_function(id);
        f.domain([0.0, 1.0]);
        f.range((0..dims).flat_map(|_| [0.0, 1.0]));
        f.c0(sample(pair[0].color));
        f.c1(sample(pair[1].color));
        f.n(1.0);
        f.finish();
        subs.push(id);
    }
    if subs.len() == 1 {
        return subs[0];
    }
    let bounds: Vec<f32> = stops[1..stops.len() - 1].iter().map(|s| s.offset).collect();
    let encode: Vec<f32> = subs.iter().flat_map(|_| [0.0f32, 1.0]).collect();
    let id = ctx.bump();
    let mut sf = ctx.pdf.stitching_function(id);
    sf.domain([0.0, 1.0]);
    sf.range((0..dims).flat_map(|_| [0.0, 1.0]));
    sf.functions(subs);
    sf.bounds(bounds);
    sf.encode(encode);
    sf.finish();
    id
}

/// Builds one axial/radial shading pattern and registers it as a page
/// resource, returning its resource name *and* its object reference (the
/// caller needs the latter to also point a soft-mask form's own resource
/// dictionary at the same pattern object — returning both here means
/// there's no need to fish the just-pushed entry back out of
/// `ctx.patterns` by assuming it's still last). `pattern_xf` maps the
/// gradient's own unit space directly to page space (already includes
/// the object's bbox→local, world, and doc→page transforms — see the
/// module docs on why this must be absolute rather than relying on `cm`).
/// `gray` swaps the color function for a luminosity (alpha-as-gray) one,
/// for the soft-mask pass in `paint_shaded_layer`.
fn shading_pattern(ctx: &mut PdfCtx, g: &Gradient, coords: &[f32], pattern_xf: CoreAffine, gray: bool, cmyk: bool) -> (String, Ref) {
    let stops = g.render_stops();
    let func = if gray {
        stitching_function(ctx, &stops, |c| vec![c.a])
    } else if cmyk {
        stitching_function(ctx, &stops, |c| c.to_cmyk().to_vec())
    } else {
        stitching_function(ctx, &stops, |c| vec![c.r, c.g, c.b])
    };
    let id = ctx.bump();
    let matrix = coeffs(pattern_xf);
    let mut pat = ctx.pdf.shading_pattern(id);
    pat.matrix(matrix);
    {
        let mut sh = pat.function_shading();
        if gray {
            sh.color_space().device_gray();
        } else if cmyk {
            sh.color_space().device_cmyk();
        } else {
            sh.color_space().device_rgb();
        }
        sh.shading_type(if coords.len() == 6 { FunctionShadingType::Radial } else { FunctionShadingType::Axial });
        sh.function(func);
        sh.coords(coords.iter().copied());
        sh.extend([true, true]);
    }
    pat.finish();
    let name = ctx.fresh_name("Sh");
    ctx.patterns.push((name.clone(), id));
    (name, id)
}

/// This gradient's coordinates + pattern matrix in its *own* kind
/// (Linear/Radial) unit-space convention, combined with `unit_to_page`
/// (bbox→local→world→page): axial `[x0,y0,x1,y1]` along `start`→`end`, or
/// radial `[cx,cy,0,cx,cy,r]` (a point growing to `radius()`), with the
/// aspect squish folded into the matrix for radial.
fn gradient_coords_and_matrix(g: &Gradient, unit_to_page: CoreAffine) -> (Vec<f32>, CoreAffine) {
    match g.kind {
        GradientKind::Radial => (
            vec![g.start[0] as f32, g.start[1] as f32, 0.0, g.start[0] as f32, g.start[1] as f32, g.radius() as f32],
            unit_to_page * radial_squish(g),
        ),
        // Linear and Freeform-on-stroke (the placeholder) both use the
        // plain axial start/end axis.
        _ => (
            vec![g.start[0] as f32, g.start[1] as f32, g.end[0] as f32, g.end[1] as f32],
            unit_to_page,
        ),
    }
}

/// Paints one shading-fill layer of `bez` (re-emitted here since a PDF
/// path is consumed by whichever operator paints it): the color pattern,
/// plus — when `stops` carry any non-opaque color — a matching luminosity
/// soft mask so the fade to transparency actually renders instead of
/// silently becoming solid (PDF shadings have no alpha channel of their
/// own; see the module docs).
#[allow(clippy::too_many_arguments)]
fn paint_shaded_layer(ctx: &mut PdfCtx, bez: &BezPath, xf: CoreAffine, g: &Gradient, coords: &[f32], pattern_xf: CoreAffine, page_w: f32, page_h: f32) {
    let needs_alpha = g.render_stops().iter().any(|s| s.color.a < 0.999);
    ctx.content.save_state();
    if needs_alpha {
        let (mask_pat, mask_pat_id) = shading_pattern(ctx, g, coords, pattern_xf, true, false);
        let mut mc = Content::new();
        mc.set_fill_color_space(ColorSpaceOperand::Pattern);
        mc.set_fill_pattern([], Name(mask_pat.as_bytes()));
        emit_geometry_into(&mut mc, bez, xf);
        mc.fill_nonzero();
        let mask_bytes = mc.finish();
        let form_id = ctx.bump();
        let mut form = ctx.pdf.form_xobject(form_id, &mask_bytes);
        form.bbox(PdfRect::new(0.0, 0.0, page_w, page_h));
        form.resources().patterns().pair(Name(mask_pat.as_bytes()), mask_pat_id);
        {
            let mut grp = form.group();
            grp.transparency();
            grp.color_space().device_gray();
        }
        form.finish();
        let gs_id = ctx.bump();
        let mut gs = ctx.pdf.ext_graphics(gs_id);
        {
            let mut sm = gs.soft_mask();
            sm.group(form_id);
            sm.subtype(MaskType::Luminosity);
        }
        gs.finish();
        let gs_name = ctx.fresh_name("Gs");
        ctx.ext_gs.push((gs_name.clone(), gs_id));
        ctx.content.set_parameters(Name(gs_name.as_bytes()));
    }
    let (pat, _) = shading_pattern(ctx, g, coords, pattern_xf, false, ctx.cmyk);
    ctx.content.set_fill_color_space(ColorSpaceOperand::Pattern);
    ctx.content.set_fill_pattern([], Name(pat.as_bytes()));
    ctx.emit_geometry(bez, xf);
    ctx.content.fill_nonzero();
    ctx.content.restore_state();
}

/// This point's color-average backstop for a freeform fill — the flat
/// fill shown where no point's spread reaches, matching
/// `canvas.rs::paint_freeform_fill`'s backstop exactly.
fn average_point_color(points: &[FreeformPoint]) -> Color {
    let n = points.len().max(1) as f32;
    let mut sum = [0.0f32; 4];
    for p in points {
        let c = p.effective_color();
        sum[0] += c.r;
        sum[1] += c.g;
        sum[2] += c.b;
        sum[3] += c.a;
    }
    Color::rgba(sum[0] / n, sum[1] / n, sum[2] / n, sum[3] / n)
}

/// Paints `bez`'s fill (solid, gradient, or the freeform layered
/// composite) and, if visible, its stroke. `xf` is the object's local
/// space → page space; `unit_to_page` is bbox-unit space → page space
/// (only meaningful when the fill/stroke is a gradient).
#[allow(clippy::too_many_arguments)]
fn paint_shape(ctx: &mut PdfCtx, doc: &Document, bez: &BezPath, xf: CoreAffine, unit_to_page: CoreAffine, appearance: &Appearance, page_w: f32, page_h: f32) {
    if appearance.opacity <= 0.0 {
        return;
    }
    let opaque = (appearance.opacity - 1.0).abs() < 1e-4;

    // --- Fill -------------------------------------------------------
    match appearance.fill {
        Paint::None => {}
        Paint::Solid(c) => {
            // The color's own alpha (e.g. a fill picked at 50% in the
            // color picker) is independent of `appearance.opacity` (the
            // object-level Transparency-panel slider) — SVG export keeps
            // these separate too (`fill-opacity` vs `opacity`); PDF's
            // graphics state only has one constant alpha to paint with,
            // so the two multiply together into it here.
            let a = c.a * appearance.opacity;
            ctx.content.save_state();
            if a < 0.999 {
                let gs_id = ctx.bump();
                ctx.pdf.ext_graphics(gs_id).non_stroking_alpha(a).finish();
                let name = ctx.fresh_name("Gs");
                ctx.ext_gs.push((name.clone(), gs_id));
                ctx.content.set_parameters(Name(name.as_bytes()));
            }
            ctx.set_fill_solid(c);
            ctx.emit_geometry(bez, xf);
            ctx.content.fill_nonzero();
            ctx.content.restore_state();
        }
        Paint::Gradient(gid) => {
            if let Some(g) = doc.gradient(gid) {
                ctx.content.save_state();
                if !opaque {
                    let gs_id = ctx.bump();
                    ctx.pdf.ext_graphics(gs_id).non_stroking_alpha(appearance.opacity).finish();
                    let name = ctx.fresh_name("Gs");
                    ctx.ext_gs.push((name.clone(), gs_id));
                    ctx.content.set_parameters(Name(name.as_bytes()));
                }
                if g.kind == GradientKind::Freeform {
                    if !g.points.is_empty() {
                        let backstop = average_point_color(&g.points);
                        // A PDF `gs` call *replaces* the graphics state's
                        // alpha rather than multiplying into whatever's
                        // already active, so the outer `ca` set just above
                        // for `appearance.opacity` would otherwise be
                        // silently lost here rather than combined with it —
                        // fold both into the one value this `gs` actually
                        // sets instead of relying on nesting to compose them.
                        let backstop_alpha = backstop.a * appearance.opacity;
                        ctx.content.save_state();
                        if backstop_alpha < 0.999 {
                            let gs_id = ctx.bump();
                            ctx.pdf.ext_graphics(gs_id).non_stroking_alpha(backstop_alpha).finish();
                            let name = ctx.fresh_name("Gs");
                            ctx.ext_gs.push((name.clone(), gs_id));
                            ctx.content.set_parameters(Name(name.as_bytes()));
                        }
                        ctx.set_fill_solid(Color::rgb(backstop.r, backstop.g, backstop.b));
                        ctx.emit_geometry(bez, xf);
                        ctx.content.fill_nonzero();
                        ctx.content.restore_state();
                        for p in &g.points {
                            let outer = (p.spread * 2.2).max(0.02) as f32;
                            let coords = vec![p.pos[0] as f32, p.pos[1] as f32, 0.0, p.pos[0] as f32, p.pos[1] as f32, outer];
                            // A one-point "gradient" from opaque to itself
                            // at alpha 0 — reuse the same stitching-based
                            // shading machinery with a synthetic 2-stop list.
                            let point_grad = point_as_gradient(g, p);
                            paint_shaded_layer(ctx, bez, xf, &point_grad, &coords, unit_to_page, page_w, page_h);
                        }
                    }
                } else {
                    let (coords, pattern_xf) = gradient_coords_and_matrix(g, unit_to_page);
                    paint_shaded_layer(ctx, bez, xf, g, &coords, pattern_xf, page_w, page_h);
                }
                ctx.content.restore_state();
            }
        }
    }

    // --- Stroke -------------------------------------------------------
    if appearance.stroke.is_visible() {
        // As with the fill above, a solid stroke color's own alpha folds
        // in alongside the object's opacity. A gradient stroke's own stop
        // alpha isn't folded in here — deliberately unmasked, see the
        // comment where it's painted below.
        let stroke_alpha = match appearance.stroke {
            Paint::Solid(c) => c.a * appearance.opacity,
            _ => appearance.opacity,
        };
        ctx.content.save_state();
        if stroke_alpha < 0.999 {
            let gs_id = ctx.bump();
            ctx.pdf.ext_graphics(gs_id).stroking_alpha(stroke_alpha).finish();
            let name = ctx.fresh_name("Gs");
            ctx.ext_gs.push((name.clone(), gs_id));
            ctx.content.set_parameters(Name(name.as_bytes()));
        }
        let scale = {
            let c = xf.as_coeffs();
            let sx = (c[0] * c[0] + c[1] * c[1]).sqrt();
            let sy = (c[2] * c[2] + c[3] * c[3]).sqrt();
            (sx * sy).sqrt()
        };
        ctx.content.set_line_width((appearance.stroke_width * scale) as f32);
        ctx.content.set_line_cap(match appearance.stroke_style.cap {
            LineCap::Butt => LineCapStyle::ButtCap,
            LineCap::Round => LineCapStyle::RoundCap,
            LineCap::Square => LineCapStyle::ProjectingSquareCap,
        });
        ctx.content.set_line_join(match appearance.stroke_style.join {
            LineJoin::Miter => LineJoinStyle::MiterJoin,
            LineJoin::Round => LineJoinStyle::RoundJoin,
            LineJoin::Bevel => LineJoinStyle::BevelJoin,
        });
        ctx.content.set_miter_limit(appearance.stroke_style.miter_limit as f32);
        if let Some(pattern) = appearance.stroke_style.dash_pattern() {
            let dashes: Vec<f32> = pattern.iter().map(|v| (*v * scale) as f32).collect();
            ctx.content.set_dash_pattern(dashes, (appearance.stroke_style.dash_offset * scale) as f32);
        }
        match appearance.stroke {
            Paint::Solid(c) => ctx.set_stroke_solid(c),
            Paint::Gradient(gid) => {
                if let Some(g) = doc.gradient(gid) {
                    // A stroke's own alpha-fade (a semi-transparent stop) is
                    // deliberately not soft-masked here the way a fill's is:
                    // stroking traces a thin, self-contained ribbon rather
                    // than filling an arbitrarily large region, so a flat
                    // (unmasked) colour-only pattern reproduces it closely
                    // enough not to be worth a second geometry pass.
                    let (coords, pattern_xf) = gradient_coords_and_matrix(g, unit_to_page);
                    let (name, _) = shading_pattern(ctx, g, &coords, pattern_xf, false, ctx.cmyk);
                    ctx.content.set_stroke_color_space(ColorSpaceOperand::Pattern);
                    ctx.content.set_stroke_pattern([], Name(name.as_bytes()));
                }
            }
            Paint::None => {}
        }
        ctx.emit_geometry(bez, xf);
        ctx.content.stroke();
        ctx.content.restore_state();
    }
}

/// A synthetic 2-stop gradient standing in for one freeform point's own
/// blob (opaque `p`'s color fading to the same color at alpha 0), so it
/// can flow through the same `render_stops`/stitching-function machinery
/// as a real gradient.
fn point_as_gradient(g: &Gradient, p: &FreeformPoint) -> Gradient {
    // `Gradient::radial(g.id)`'s own start/end/aspect are all irrelevant
    // here — `shading_pattern` only ever reads `.render_stops()` off
    // whatever's passed to it, since the caller (the freeform loop above)
    // computes this blob's own coords/matrix directly rather than going
    // through `gradient_coords_and_matrix`. Only `.stops` matters.
    let mut out = Gradient::radial(g.id);
    let c = p.effective_color();
    out.stops = vec![
        GradientStop::new(0.0, c),
        GradientStop {
            offset: 1.0,
            color: Color::rgba(c.r, c.g, c.b, 0.0),
            opacity: 1.0,
            midpoint: 0.5,
        },
    ];
    out
}

/// Walks `ids` (and, for a group, its descendants) from `document`,
/// producing a single-page PDF for the `src` rect (document space, px @
/// 96dpi) — see the module docs for the coordinate/alpha strategy and
/// known gaps. `cmyk` selects `DeviceCMYK` over `DeviceRGB` for every
/// solid and gradient color (matching the document's declared color
/// mode). `images` is the same asset→decoded-pixels cache the live
/// canvas paints from (`App::image_cache`): reading the already-decoded
/// CPU pixels straight out of it, rather than re-rendering the GPU
/// scene, is what keeps this a plain, synchronous, `&self`-only function.
pub fn export_vector_pdf(
    document: &Document,
    ids: &[ObjectId],
    src: CoreRect,
    bg: Option<Color>,
    cmyk: bool,
    text: &mut TextContext,
    images: &HashMap<AssetId, ImageLods>,
) -> Vec<u8> {
    let map = PageMap::new(src);
    let base = map.affine();
    let (pw, ph) = map.page_size(src);

    let mut ctx = PdfCtx {
        pdf: Pdf::new(),
        alloc: Ref::new(1),
        content: Content::new(),
        base,
        cmyk,
        x_objects: Vec::new(),
        patterns: Vec::new(),
        ext_gs: Vec::new(),
        counter: 0,
    };

    if let Some(c) = bg {
        ctx.set_fill_solid(c);
        ctx.content.rect(0.0, 0.0, pw, ph);
        ctx.content.fill_nonzero();
    }

    for &id in ids {
        walk(&mut ctx, document, id, text, images, pw, ph);
    }

    let catalog_id = ctx.bump();
    let page_tree_id = ctx.bump();
    let page_id = ctx.bump();
    let content_id = ctx.bump();

    ctx.pdf.catalog(catalog_id).pages(page_tree_id);
    ctx.pdf.pages(page_tree_id).kids([page_id]).count(1);

    let content_bytes = ctx.content.finish();
    let mut page = ctx.pdf.page(page_id);
    page.media_box(PdfRect::new(0.0, 0.0, pw, ph));
    page.parent(page_tree_id);
    page.contents(content_id);
    {
        let mut res = page.resources();
        {
            let mut xo = res.x_objects();
            for (name, id) in &ctx.x_objects {
                xo.pair(Name(name.as_bytes()), *id);
            }
        }
        {
            let mut pats = res.patterns();
            for (name, id) in &ctx.patterns {
                pats.pair(Name(name.as_bytes()), *id);
            }
        }
        {
            let mut gs = res.ext_g_states();
            for (name, id) in &ctx.ext_gs {
                gs.pair(Name(name.as_bytes()), *id);
            }
        }
    }
    page.finish();
    ctx.pdf.stream(content_id, &content_bytes);

    ctx.pdf.finish()
}

fn walk(ctx: &mut PdfCtx, doc: &Document, id: ObjectId, text: &mut TextContext, images: &HashMap<AssetId, ImageLods>, pw: f32, ph: f32) {
    let Some(obj) = doc.object(id) else {
        return;
    };
    match &obj.kind {
        ObjectKind::Group(group) => {
            for &child in &group.children {
                walk(ctx, doc, child, text, images, pw, ph);
            }
        }
        ObjectKind::Path(path) => {
            paint_one(ctx, doc, id, &path.geometry, &obj.appearance, pw, ph);
        }
        ObjectKind::CompoundPath(compound) => {
            let mut bez = BezPath::new();
            for sub in &compound.subpaths {
                bez.extend(sub.elements().iter().copied());
            }
            paint_one(ctx, doc, id, &bez, &obj.appearance, pw, ph);
        }
        ObjectKind::Text(td) => {
            let bez = crate::textedit::outline_text_data(td, text);
            if !bez.elements().is_empty() {
                paint_one(ctx, doc, id, &bez, &obj.appearance, pw, ph);
            }
        }
        ObjectKind::Image(img) => {
            let Some(bounds) = obj.kind.own_local_bounds() else {
                return;
            };
            let wt = doc.world_transform(id);
            let doc_bounds = transformed_bounds(wt, bounds);
            embed_image_object(ctx, images, img.asset, doc_bounds);
        }
        ObjectKind::Symbol(_) => {}
    }
}

/// Encodes the asset's highest-resolution decoded LOD as a JPEG and
/// embeds it positioned at `doc_bounds` (document space) — a no-op if
/// the asset isn't in `images` (not yet decoded) or is in a format this
/// can't re-encode.
fn embed_image_object(ctx: &mut PdfCtx, images: &HashMap<AssetId, ImageLods>, asset: AssetId, doc_bounds: CoreRect) {
    let Some(lods) = images.get(&asset) else {
        return;
    };
    let Some(img) = lods.levels.iter().rev().flatten().next() else {
        return;
    };
    let (w, h) = (img.width, img.height);
    let Some(mut rgba) = rgba_bytes(img) else {
        return;
    };
    if img.alpha_type == vello::peniko::ImageAlphaType::AlphaPremultiplied {
        for px in rgba.chunks_exact_mut(4) {
            let a = px[3] as f32 / 255.0;
            if a > 0.0 {
                for c in &mut px[..3] {
                    *c = (*c as f32 / a).min(255.0) as u8;
                }
            }
        }
    }
    let Some(rgba_img) = image::RgbaImage::from_raw(w, h, rgba) else {
        return;
    };
    let rgb = image::DynamicImage::ImageRgba8(rgba_img).to_rgb8();
    let mut jpeg = Vec::new();
    if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&rgb)
        .is_err()
    {
        return;
    }
    embed_image(ctx, &jpeg, w, h, doc_bounds);
}

/// This image's pixels as plain RGBA8, converting from BGRA8 if needed.
/// `None` for any other (future) format this doesn't understand yet.
fn rgba_bytes(img: &vello::peniko::ImageData) -> Option<Vec<u8>> {
    let bytes = img.data.data();
    match img.format {
        vello::peniko::ImageFormat::Rgba8 => Some(bytes.to_vec()),
        vello::peniko::ImageFormat::Bgra8 => {
            let mut out = bytes.to_vec();
            for px in out.chunks_exact_mut(4) {
                px.swap(0, 2);
            }
            Some(out)
        }
        _ => None,
    }
}

fn transformed_bounds(xf: CoreAffine, b: CoreRect) -> CoreRect {
    let pts = [
        xf * CorePoint::new(b.x0, b.y0),
        xf * CorePoint::new(b.x1, b.y0),
        xf * CorePoint::new(b.x1, b.y1),
        xf * CorePoint::new(b.x0, b.y1),
    ];
    let x0 = pts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let x1 = pts.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
    let y0 = pts.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let y1 = pts.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    CoreRect::new(x0, y0, x1, y1)
}

fn embed_image(ctx: &mut PdfCtx, jpeg: &[u8], iw: u32, ih: u32, doc_bounds: CoreRect) {
    let id = ctx.bump();
    {
        let mut img = ctx.pdf.image_xobject(id, jpeg);
        img.filter(Filter::DctDecode);
        img.width(iw as i32);
        img.height(ih as i32);
        img.color_space().device_rgb();
        img.bits_per_component(8);
    }
    let name = ctx.fresh_name("Im");
    ctx.x_objects.push((name.clone(), id));

    // Position the unit image square (Do always paints into [0,1]x[0,1])
    // onto the image's document-space bounds, then into page space —
    // baked directly like everything else in this module, via a `cm`
    // *inside a save/restore* (safe here: images are never patterns, so
    // the CTM-vs-pattern-matrix mismatch this module otherwise avoids
    // doesn't apply).
    let unit_to_doc = CoreAffine::new([
        doc_bounds.width(),
        0.0,
        0.0,
        -doc_bounds.height(),
        doc_bounds.x0,
        doc_bounds.y1,
    ]);
    let m = coeffs(ctx.base * unit_to_doc);
    ctx.content.save_state();
    ctx.content.transform(m);
    ctx.content.x_object(Name(name.as_bytes()));
    ctx.content.restore_state();
}

fn paint_one(ctx: &mut PdfCtx, doc: &Document, id: ObjectId, bez: &BezPath, appearance: &Appearance, pw: f32, ph: f32) {
    let world = doc.world_transform(id);
    let xf = ctx.base * world;
    let unit_to_page = match doc.object(id).and_then(|o| o.kind.own_local_bounds()) {
        Some(b) => {
            let bbox_xf = CoreAffine::new([b.width(), 0.0, 0.0, b.height(), b.x0, b.y0]);
            xf * bbox_xf
        }
        None => xf,
    };
    paint_shape(ctx, doc, bez, xf, unit_to_page, appearance, pw, ph);
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{Document, GradientId, Layer, LayerId, Object, ObjectParent, TextData};

    fn sample_document() -> (Document, Vec<ObjectId>) {
        let mut doc = Document::new("PDF smoke test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        doc.insert_layer(layer, 0);

        let mut ids = Vec::new();

        // A plain solid-filled, solid-stroked rectangle.
        let mut solid = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            CoreRect::new(0.0, 0.0, 100.0, 60.0),
        );
        solid.appearance.fill = Paint::Solid(Color::rgb(0.8, 0.2, 0.1));
        solid.appearance.stroke = Paint::Solid(Color::rgb(0.0, 0.0, 0.0));
        solid.appearance.stroke_width = 2.0;
        ids.push(solid.id);
        doc.insert_object(solid, 0).unwrap();

        // A linear gradient with a semi-transparent stop (exercises the
        // soft-mask path).
        let lin_id = GradientId::new();
        let mut lin = Gradient::linear(lin_id);
        lin.stops[1].opacity = 0.3;
        doc.add_gradient(lin);
        let mut lin_obj = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            CoreRect::new(120.0, 0.0, 220.0, 60.0),
        );
        lin_obj.appearance.fill = Paint::Gradient(lin_id);
        ids.push(lin_obj.id);
        doc.insert_object(lin_obj, 0).unwrap();

        // A radial gradient, fully opaque (no soft mask needed).
        let rad_id = GradientId::new();
        doc.add_gradient(Gradient::radial(rad_id));
        let mut rad_obj = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            CoreRect::new(240.0, 0.0, 340.0, 60.0),
        );
        rad_obj.appearance.fill = Paint::Gradient(rad_id);
        ids.push(rad_obj.id);
        doc.insert_object(rad_obj, 0).unwrap();

        // A freeform gradient (the layered composite + per-point soft masks).
        let free_id = GradientId::new();
        doc.add_gradient(Gradient::freeform(free_id));
        let mut free_obj = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            CoreRect::new(360.0, 0.0, 460.0, 60.0),
        );
        free_obj.appearance.fill = Paint::Gradient(free_id);
        ids.push(free_obj.id);
        doc.insert_object(free_obj, 0).unwrap();

        // A text object — exercises the glyph-outline path.
        let mut text_obj = Object::new(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            ObjectKind::Text(TextData {
                content: "PDF".into(),
                local_bounds: CoreRect::new(0.0, 0.0, 80.0, 30.0),
                ..TextData::default()
            }),
        );
        text_obj.transform = CoreAffine::translate(kurbo::Vec2::new(480.0, 0.0));
        text_obj.appearance.fill = Paint::Solid(Color::rgb(0.1, 0.1, 0.8));
        ids.push(text_obj.id);
        doc.insert_object(text_obj, 0).unwrap();

        (doc, ids)
    }

    #[test]
    fn export_vector_pdf_produces_a_well_formed_pdf() {
        let (doc, ids) = sample_document();
        let mut text = TextContext::new();
        let images = HashMap::new();
        let bytes = export_vector_pdf(
            &doc,
            &ids,
            CoreRect::new(-10.0, -10.0, 580.0, 70.0),
            Some(Color::rgb(1.0, 1.0, 1.0)),
            false,
            &mut text,
            &images,
        );
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.starts_with("%PDF-1."), "missing PDF header: {}", &s[..20.min(s.len())]);
        assert!(s.trim_end().ends_with("%%EOF"), "missing PDF trailer");
        assert!(s.contains("/ShadingType 2"), "no axial shading emitted");
        assert!(s.contains("/ShadingType 3"), "no radial shading emitted");
        assert!(s.contains("/SMask"), "no soft mask emitted for the alpha-fading gradient/freeform");
        assert!(s.contains("/PatternType"), "no shading pattern emitted");
        assert!(s.contains("xref") && s.contains("trailer"), "missing xref table / trailer");
    }

    /// Regression test: a PDF `gs` call *replaces* the graphics state's
    /// alpha rather than multiplying into whatever's already active, so
    /// an object's own opacity and a freeform gradient's backstop alpha
    /// (two independently-computed alphas painted via nested `q`/`gs`
    /// blocks) must be folded into one value *before* either `gs` is
    /// written, not left to compose implicitly across the nesting — see
    /// `paint_shape`'s Freeform branch. With opacity 0.5 and two points
    /// both at alpha 0.5, the correctly-combined backstop `ca` is 0.25;
    /// the bug this guards against would instead emit a bare 0.5.
    #[test]
    fn freeform_backstop_alpha_multiplies_with_object_opacity() {
        let mut doc = Document::new("alpha compose test");
        let layer = Layer::new(LayerId::new(), "Layer 1");
        let layer_id = layer.id;
        doc.insert_layer(layer, 0);

        let gid = GradientId::new();
        let mut g = Gradient::freeform(gid);
        for p in &mut g.points {
            p.color = Color::rgba(p.color.r, p.color.g, p.color.b, 0.5);
            p.opacity = 1.0;
        }
        doc.add_gradient(g);

        let mut obj = Object::rectangle(
            ObjectId::new(),
            ObjectParent::Layer(layer_id),
            CoreRect::new(0.0, 0.0, 100.0, 60.0),
        );
        obj.appearance.fill = Paint::Gradient(gid);
        obj.appearance.stroke = Paint::None;
        obj.appearance.opacity = 0.5;
        let id = obj.id;
        doc.insert_object(obj, 0).unwrap();

        let mut text = TextContext::new();
        let images = HashMap::new();
        let bytes = export_vector_pdf(
            &doc,
            &[id],
            CoreRect::new(0.0, 0.0, 100.0, 60.0),
            None,
            false,
            &mut text,
            &images,
        );
        let s = String::from_utf8_lossy(&bytes);
        // The outer `ca 0.5` for the object's own opacity legitimately
        // exists too (it's what the per-point blobs paint with) — the
        // bug this guards against is the *backstop* using a bare 0.5
        // instead of the combined 0.25, not the presence of 0.5 at all.
        assert!(
            s.contains("/ca 0.25"),
            "expected the backstop's combined alpha (0.5 opacity * 0.5 average point alpha = 0.25); got: {s}"
        );
    }

    #[test]
    fn export_vector_pdf_uses_devicecmyk_when_requested() {
        let (doc, ids) = sample_document();
        let mut text = TextContext::new();
        let images = HashMap::new();
        let bytes = export_vector_pdf(
            &doc,
            &ids,
            CoreRect::new(-10.0, -10.0, 580.0, 70.0),
            None,
            true,
            &mut text,
            &images,
        );
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/DeviceCMYK"), "CMYK mode should emit at least one DeviceCMYK color space");
        assert!(!s.contains(" rg\n") && !s.contains(" rg "), "CMYK mode shouldn't fall back to any RGB fill operator");
    }
}
