//! `TextFrame.createOutline()` — ported from `amalith-shell`'s
//! `textedit::outline_text_data`/`pathtext::outline_path_text` (which this
//! crate can't depend on directly: it drags in `winit`/`vello`/`wgpu`).
//! Kept in sync by hand; see the cross-reference in those files.
//!
//! No GPU/paint types: parley's `Brush` generic is instantiated as `()`
//! since this only extracts glyph *geometry*, never draws anything.

use std::borrow::Cow;

use amalith_core::geom::{Affine, BezPath, Point};
use amalith_core::{ArcLengthPath, PathTextAlign, PathTextData, TextAlign, TextData, TextKind, TextPosition, TextStyle};
use parley::layout::PositionedLayoutItem;
use parley::style::{FontFamily, FontFamilyName, FontFeatures, FontStyle, FontWeight, LineHeight, StyleProperty};
use parley::{Alignment, FontContext, LayoutContext};
use skrifa::instance::{LocationRef, NormalizedCoord, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{GlyphId, MetadataProvider};

/// Slim stand-in for `amalith-shell`'s `TextContext`: just enough state to
/// shape text with parley, no draw-time cache (a one-shot batch script
/// never re-shapes the same text object twice, so caching buys nothing
/// here).
pub struct Shaper {
    fonts: FontContext,
    layout: LayoutContext<()>,
}

impl Default for Shaper {
    fn default() -> Self {
        Self::new()
    }
}

impl Shaper {
    pub fn new() -> Self {
        Self { fonts: FontContext::new(), layout: LayoutContext::new() }
    }
}

struct OutlineSink<'a> {
    path: &'a mut BezPath,
    xf: Affine,
    started: bool,
}

impl OutlinePen for OutlineSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.started {
            self.path.close_path();
        }
        self.path.move_to(self.xf * Point::new(x as f64, y as f64));
        self.started = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.xf * Point::new(x as f64, y as f64));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.quad_to(
            self.xf * Point::new(cx as f64, cy as f64),
            self.xf * Point::new(x as f64, y as f64),
        );
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        self.path.curve_to(
            self.xf * Point::new(c0x as f64, c0y as f64),
            self.xf * Point::new(c1x as f64, c1y as f64),
            self.xf * Point::new(x as f64, y as f64),
        );
    }
    fn close(&mut self) {
        if self.started {
            self.path.close_path();
            self.started = false;
        }
    }
}

fn line_height(style: &TextStyle) -> LineHeight {
    match style.leading {
        Some(px) => LineHeight::Absolute(px as f32),
        None => LineHeight::FontSizeRelative(1.2),
    }
}

fn features(style: &TextStyle) -> &'static str {
    if style.small_caps {
        "smcp on"
    } else {
        match style.position {
            TextPosition::Superscript => "sups on",
            TextPosition::Subscript => "subs on",
            TextPosition::Normal => "",
        }
    }
}

fn alignment(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        TextAlign::JustifyLeft | TextAlign::JustifyCenter | TextAlign::JustifyRight | TextAlign::JustifyAll => {
            Alignment::Justify
        }
    }
}

fn align_shift(ascent: f32, descent: f32, align: PathTextAlign) -> f64 {
    match align {
        PathTextAlign::Baseline => 0.0,
        PathTextAlign::Ascender => ascent as f64,
        PathTextAlign::Descender => -(descent as f64),
        PathTextAlign::Center => (ascent as f64 - descent as f64) / 2.0,
    }
}

fn paragraph_offset(align: TextAlign, span: f64, width: f64) -> f64 {
    let spare = (span - width).max(0.0);
    match align {
        TextAlign::Center | TextAlign::JustifyCenter => spare / 2.0,
        TextAlign::End | TextAlign::JustifyRight => spare,
        _ => 0.0,
    }
}

fn build_layout(shaper: &mut Shaper, td: &TextData) -> parley::Layout<()> {
    let width = match td.kind {
        TextKind::Area { width, .. } => Some(width as f32),
        TextKind::Point | TextKind::Path(_) => None,
    };
    let mut b = shaper.layout.ranged_builder(&mut shaper.fonts, &td.content, 1.0, true);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(td.style.family.clone())),
    ]))));
    b.push_default(StyleProperty::FontSize(td.style.size as f32));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(td.style.weight as f32)));
    b.push_default(StyleProperty::FontStyle(if td.style.italic { FontStyle::Italic } else { FontStyle::Normal }));
    b.push_default(StyleProperty::LineHeight(line_height(&td.style)));
    b.push_default(StyleProperty::LetterSpacing((td.style.tracking / 1000.0 * td.style.size) as f32));
    b.push_default(StyleProperty::FontFeatures(FontFeatures::from(features(&td.style))));
    let mut layout = b.build(&td.content);
    layout.break_all_lines(width);
    layout.align(alignment(td.align), parley::layout::AlignmentOptions::default());
    layout
}

fn outline_straight(shaper: &mut Shaper, td: &TextData) -> BezPath {
    let mut out = BezPath::new();
    if td.content.is_empty() {
        return out;
    }
    let layout = build_layout(shaper, td);
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let mut gx = run.offset();
            let gy = run.baseline();
            let r = run.run();
            let font = r.font();
            let font_size = r.font_size();
            let loc: Vec<NormalizedCoord> = r.normalized_coords().iter().map(|&c| NormalizedCoord::from_bits(c)).collect();
            let Ok(font_ref) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else { continue };
            let glyphs = font_ref.outline_glyphs();
            for g in run.glyphs() {
                let x = (gx + g.x) as f64;
                let y = (gy - g.y) as f64;
                gx += g.advance;
                let Some(glyph) = glyphs.get(GlyphId::new(g.id as u32)) else { continue };
                let xf = Affine::new([1.0, 0.0, 0.0, -1.0, x, y]);
                let mut sink = OutlineSink { path: &mut out, xf, started: false };
                let settings = DrawSettings::unhinted(Size::new(font_size), LocationRef::new(&loc));
                let _ = glyph.draw(settings, &mut sink);
                sink.close();
            }
        }
    }
    out
}

fn outline_on_path(shaper: &mut Shaper, td: &TextData, pt: &PathTextData, arc: &ArcLengthPath) -> BezPath {
    let mut out = BezPath::new();
    let layout = build_layout(shaper, td);
    let offset = paragraph_offset(
        td.align,
        pt.end - pt.start,
        layout.lines().next().map(|l| l.metrics().advance as f64).unwrap_or(0.0),
    );
    for line in layout.lines().take(1) {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let r = run.run();
            let font = r.font();
            let Ok(font_ref) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else { continue };
            let glyphs = font_ref.outline_glyphs();
            let loc: Vec<_> = r.normalized_coords().iter().map(|&c| NormalizedCoord::from_bits(c)).collect();
            let metrics = r.metrics();
            let shift = align_shift(metrics.ascent, metrics.descent, pt.align) - td.style.baseline_shift;
            let mut x = run.offset() as f64;
            for g in run.glyphs() {
                let advance = g.advance as f64;
                let center = x + advance / 2.0;
                let distance = if pt.flip { pt.end - offset - center } else { pt.start + offset + center };
                let fits = offset + x >= 0.0 && offset + x + advance <= pt.end - pt.start + 1e-6;
                x += advance;
                if !fits {
                    continue;
                }
                let Some(glyph) = glyphs.get(GlyphId::new(g.id as u32)) else { continue };
                let (p, angle) = arc.point_and_tangent(distance);
                let pose = Affine::translate((p.x, p.y))
                    * Affine::rotate(angle + if pt.flip { std::f64::consts::PI } else { 0.0 })
                    * Affine::translate((g.x as f64 - advance / 2.0, shift - g.y as f64))
                    * Affine::scale_non_uniform(1.0, -1.0);
                let mut sink = OutlineSink { path: &mut out, xf: pose, started: false };
                let _ = glyph.draw(
                    DrawSettings::unhinted(Size::new(r.font_size()), LocationRef::new(&loc)),
                    &mut sink,
                );
                sink.close();
            }
        }
    }
    out
}

/// Convert a committed `TextData`'s glyphs into one filled path in the
/// text object's local space, matching `amalith-shell::textedit::outline_text_data`.
pub fn outline_text_data(shaper: &mut Shaper, td: &TextData) -> BezPath {
    if let (TextKind::Path(pt), Some(pd)) = (td.kind.clone(), td.path_geometry.as_ref()) {
        if let Some(points) = pd.flattened_points(0.05).into_iter().next() {
            let closed = pd.subpaths().first().is_some_and(|s| s.closed);
            let arc = ArcLengthPath::new(&points, closed);
            return outline_on_path(shaper, td, &pt, &arc);
        }
    }
    outline_straight(shaper, td)
}
