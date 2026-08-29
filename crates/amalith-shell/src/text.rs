//! Text: parley for layout, vello for glyph rasterization.
//!
//! One [`TextContext`] per thread, kept alive for the whole app — it caches
//! the system font set and parley's scratch buffers. `measure` gives an
//! advance width (used to size tabs); `draw` pushes a positioned glyph run
//! into a vello scene.

use parley::{
    FontContext, GenericFamily, Layout, LayoutContext, PositionedLayoutItem, StyleProperty,
};
use vello::kurbo::Affine;
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

/// Owns the font database and parley's reusable layout buffers.
pub struct TextContext {
    fonts: FontContext,
    layout: LayoutContext<Brush>,
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
        }
    }

    /// Lay a single line out with no wrapping. `size` is in px.
    fn build(&mut self, text: &str, size: f32, color: Color) -> Layout<Brush> {
        let mut builder = self.layout.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(size));
        builder.push_default(StyleProperty::LineHeight(
            parley::LineHeight::FontSizeRelative(1.3),
        ));
        builder.push_default(GenericFamily::SystemUi);
        builder.push_default(StyleProperty::Brush(Brush::Solid(color)));
        let mut layout = builder.build(text);
        layout.break_all_lines(None);
        layout
    }

    /// Advance width of `text` at `size`, in logical px.
    pub fn measure(&mut self, text: &str, size: f32) -> f64 {
        self.build(text, size, Color::WHITE).width() as f64
    }

    /// Draw `text` at `size` in `color`, with its left edge at `x` and its
    /// alphabetic baseline at `y`.
    pub fn draw(&mut self, scene: &mut Scene, text: &str, size: f32, color: Color, x: f64, y: f64) {
        let layout = self.build(text, size, color);
        // parley positions runs from the layout origin; shift so the first
        // baseline lands on `y`.
        let first_baseline = layout
            .lines()
            .next()
            .map(|l| l.metrics().baseline)
            .unwrap_or(0.0);
        let transform = Affine::translate((x, y - first_baseline as f64));

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
}
