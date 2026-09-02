//! Text: parley for layout, vello for glyph rasterization.
//!
//! One [`TextContext`] per thread, kept alive for the whole app — it caches
//! the system font set and parley's scratch buffers. `measure` gives an
//! advance width (used to size tabs); `draw` pushes a positioned glyph run
//! into a vello scene.

use std::collections::HashMap;

use parley::{
    FontContext, FontWeight, GenericFamily, Layout, LayoutContext, LineHeight,
    PositionedLayoutItem, StyleProperty,
};
use vello::kurbo::Affine;
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

/// Owns the font database and parley's reusable layout buffers.
pub struct TextContext {
    fonts: FontContext,
    layout: LayoutContext<Brush>,
    /// Memoized single-line layouts, keyed by `(text, size.to_bits(),
    /// bold)`. The glyph colour is applied at draw time (see
    /// [`Self::emit`]), so it isn't part of the key. Cleared wholesale
    /// when it grows large.
    cache: HashMap<(String, u32, bool), Layout<Brush>>,
}

impl Default for TextContext {
    fn default() -> Self {
        Self::new()
    }
}

impl TextContext {
    pub fn new() -> Self {
        Self {
            fonts: FontContext::new(),
            layout: LayoutContext::new(),
            cache: HashMap::new(),
        }
    }

    /// Lend the font DB and layout scratch buffers together — for
    /// `parley::PlainEditor` (see [`crate::textedit`]).
    pub fn parts(&mut self) -> (&mut FontContext, &mut LayoutContext<Brush>) {
        (&mut self.fonts, &mut self.layout)
    }

    /// A memoized single-line layout with no wrapping. `size` is in px.
    /// Repeated strings (labels, numbers, tool names) are laid out once
    /// and reused every frame — parley shaping is the bulk of per-frame
    /// text cost.
    fn build(&mut self, text: &str, size: f32, bold: bool) -> &Layout<Brush> {
        let key = (text.to_owned(), size.to_bits(), bold);
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= 1024 {
                self.cache.clear();
            }
            let mut builder = self.layout.ranged_builder(&mut self.fonts, text, 1.0, true);
            builder.push_default(StyleProperty::FontSize(size));
            builder.push_default(StyleProperty::LineHeight(
                parley::LineHeight::FontSizeRelative(1.3),
            ));
            builder.push_default(GenericFamily::SystemUi);
            builder.push_default(StyleProperty::FontWeight(if bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            }));
            builder.push_default(StyleProperty::Brush(Brush::Solid(Color::WHITE)));
            let mut layout = builder.build(text);
            layout.break_all_lines(None);
            self.cache.insert(key.clone(), layout);
        }
        self.cache.get(&key).expect("just inserted")
    }

    /// Advance width of `text` at `size`, in logical px.
    pub fn measure(&mut self, text: &str, size: f32) -> f64 {
        self.build(text, size, false).width() as f64
    }

    /// Lay `text` out wrapped to `wrap_width` px, `line_height` as a multiple
    /// of the font size. The caller keeps the [`Layout`] for drawing and for
    /// hit-testing / selection.
    pub fn wrap(
        &mut self,
        text: &str,
        size: f32,
        color: Color,
        wrap_width: f32,
        line_height: f32,
    ) -> Layout<Brush> {
        let mut builder = self.layout.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(
            line_height,
        )));
        builder.push_default(GenericFamily::SystemUi);
        builder.push_default(StyleProperty::Brush(Brush::Solid(color)));
        let mut layout = builder.build(text);
        layout.break_all_lines(Some(wrap_width));
        layout
    }

    /// Emit `layout`'s glyph runs into `scene` under `transform` (applied
    /// after parley's own per-run positioning).
    fn emit(scene: &mut Scene, layout: &Layout<Brush>, color: Color, transform: Affine) {
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let mut gx = glyph_run.offset();
                let gy = glyph_run.baseline();
                let run = glyph_run.run();
                let font = run.font();
                let font_size = run.font_size();
                let coords = run.normalized_coords();
                let synthesis = run.synthesis();
                let glyph_xform = synthesis
                    .skew()
                    .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
                scene
                    .draw_glyphs(font)
                    .brush(&Brush::Solid(color))
                    .hint(true)
                    .transform(transform)
                    .glyph_transform(glyph_xform)
                    .font_size(font_size)
                    .normalized_coords(coords)
                    .draw(
                        Fill::NonZero,
                        glyph_run.glyphs().map(|g| {
                            let x = gx + g.x;
                            let y = gy - g.y;
                            gx += g.advance;
                            Glyph {
                                id: g.id as u32,
                                x,
                                y,
                            }
                        }),
                    );
            }
        }
    }

    /// Draw an already-built [`Layout`] with its top-left corner at `(x, y)`.
    pub fn draw_layout(
        &self,
        scene: &mut Scene,
        layout: &Layout<Brush>,
        color: Color,
        x: f64,
        y: f64,
    ) {
        Self::emit(scene, layout, color, Affine::translate((x, y)));
    }

    /// Draw `text` at `size` in `color`, with its left edge at `x` and its
    /// alphabetic baseline at `y`.
    pub fn draw(&mut self, scene: &mut Scene, text: &str, size: f32, color: Color, x: f64, y: f64) {
        self.draw_weighted(scene, text, size, color, x, y, false);
    }

    /// Like [`Self::draw`] but bold.
    pub fn draw_bold(
        &mut self,
        scene: &mut Scene,
        text: &str,
        size: f32,
        color: Color,
        x: f64,
        y: f64,
    ) {
        self.draw_weighted(scene, text, size, color, x, y, true);
    }

    fn draw_weighted(
        &mut self,
        scene: &mut Scene,
        text: &str,
        size: f32,
        color: Color,
        x: f64,
        y: f64,
        bold: bool,
    ) {
        let layout = self.build(text, size, bold);
        // parley positions runs from the layout origin; shift so the first
        // baseline lands on `y`.
        let first_baseline = layout
            .lines()
            .next()
            .map(|l| l.metrics().baseline)
            .unwrap_or(0.0);
        Self::emit(
            scene,
            layout,
            color,
            Affine::translate((x, y - first_baseline as f64)),
        );
    }

    /// Draw `text` as a column of upright glyphs, each centred on `cx`
    /// and stacked `row_h` px apart starting with its baseline at `y`.
    /// One layout, one `draw_glyphs` call — for the vertical ruler.
    pub fn draw_column(
        &mut self,
        scene: &mut Scene,
        text: &str,
        size: f32,
        color: Color,
        cx: f64,
        y: f64,
        row_h: f64,
    ) {
        let layout = self.build(text, size, false);
        for line in layout.lines() {
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                    continue;
                };
                let run = glyph_run.run();
                let font = run.font();
                let font_size = run.font_size();
                let coords = run.normalized_coords();
                let step = row_h as f32;
                let mut row = 0.0f32;
                scene
                    .draw_glyphs(font)
                    .brush(&Brush::Solid(color))
                    .hint(true)
                    .transform(Affine::translate((cx, y)))
                    .font_size(font_size)
                    .normalized_coords(coords)
                    .draw(
                        Fill::NonZero,
                        glyph_run.glyphs().map(|g| {
                            let gy = row;
                            row += step;
                            Glyph {
                                id: g.id as u32,
                                x: -g.advance * 0.5,
                                y: gy,
                            }
                        }),
                    );
            }
        }
    }
}
