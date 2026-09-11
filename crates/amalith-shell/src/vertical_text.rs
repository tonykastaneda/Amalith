//! Vertical Type Tool: top-to-bottom, right-to-left column layout with
//! upright glyphs — Illustrator's Vertical Type Tool applied to any
//! script, not just CJK.
//!
//! Parley (this app's shaping engine — see [`crate::text`]) has no
//! vertical-writing-mode support at all: no vertical line-breaking, no
//! vertical caret navigation. This module uses it only as a *shaper* —
//! one unwrapped horizontal layout gives real glyph ids, per-cluster
//! advances, and cluster byte ranges — and does all the actual column
//! placement, wrapping-by-height, hit-testing, and caret-rect math
//! itself. [`crate::textedit::TextEdit`] still owns the underlying
//! `parley::PlainEditor` for text storage and logical-order caret
//! movement (insert/delete/select-all/etc. don't care about layout
//! direction); only its point-based hit-testing and its Up/Down/Left/
//! Right key handling are swapped for a `vertical` object — see there.
//!
//! Known v1 simplifications: caret/selection indexes by shaping
//! *cluster* (parley's own atomic-unit-of-text, which already merges
//! ligatures/combining marks reasonably), not by explicit user-facing
//! "character" in every possible script; a fully blank paragraph (two
//! consecutive returns) collapses into its neighboring column rather
//! than reserving its own blank column; a glyph's baseline within its
//! row is a fixed `row_h * 0.8` fraction rather than derived from the
//! font's real ascent metric.

use std::ops::Range;

use amalith_core::{ArcLengthPath, PathTextData, TextData, TextStyle};
use parley::{FontData, FontFamily, FontFamilyName, FontStyle as ParleyFontStyle, FontWeight, PositionedLayoutItem, StyleProperty};
use vello::kurbo::{Affine, Point, Rect};
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

use crate::convert;
use crate::text::TextContext;

/// One glyph belonging to a cluster, position relative to the cluster's
/// own origin (usually just one glyph at `(0, 0)`).
#[derive(Clone)]
struct VGlyph {
    id: u32,
    dx: f32,
    dy: f32,
}

/// One "row" of a column: a single parley shaping cluster.
#[derive(Clone)]
struct VCluster {
    font: FontData,
    font_size: f32,
    coords: Vec<i16>,
    glyph_xform: Option<Affine>,
    glyphs: Vec<VGlyph>,
    advance: f32,
    byte_range: Range<usize>,
}

/// One top-to-bottom column of clusters. `start` is the byte offset the
/// column begins at even when it has no clusters yet (an empty trailing
/// column right after a forced break) — needed so hit-testing/caret
/// placement still has somewhere sane to land.
struct VColumn {
    x: f64,
    start: usize,
    clusters: Vec<VCluster>,
}

/// A fully laid-out vertical text block, ready to draw / hit-test /
/// place a caret in. Column 0 sits at local `x = 0`; column 1 at
/// `x = -col_w`, and so on — right-to-left, matching vertical-writing
/// convention. Local `y` grows downward from each column's own top.
pub struct VerticalLayout {
    columns: Vec<VColumn>,
    row_h: f64,
    col_w: f64,
}

/// Shapes `content` into columns. `wrap_h` is `None` for point text (a
/// single unwrapped column; only an explicit `'\n'` starts a new one) or
/// `Some(box_height)` for area text (a column also breaks once adding
/// another row would exceed `box_height`).
pub fn layout(tcx: &mut TextContext, content: &str, style: &TextStyle, wrap_h: Option<f64>) -> VerticalLayout {
    let row_h = style.leading.unwrap_or(style.size * 1.2);
    let col_w = style.size;
    if content.is_empty() {
        return VerticalLayout {
            columns: vec![VColumn { x: 0.0, start: 0, clusters: Vec::new() }],
            row_h,
            col_w,
        };
    }

    let mut parley_layout = shape(tcx, content, style);
    // No wrapping at the shaper level — explicit `'\n'`s still produce
    // separate parley "lines" even unwrapped, which is exactly the
    // paragraph-break granularity this needs; height-based column
    // splitting happens below, entirely outside parley.
    parley_layout.break_all_lines(None);

    let mut columns: Vec<VColumn> = Vec::new();
    let mut cur = VColumn { x: 0.0, start: 0, clusters: Vec::new() };
    let mut cur_h = 0.0f64;

    for line in parley_layout.lines() {
        for cluster in line_clusters(&line) {
            if let Some(h) = wrap_h {
                if !cur.clusters.is_empty() && cur_h + row_h > h + 0.5 {
                    let next_start = cluster.byte_range.start;
                    columns.push(std::mem::replace(
                        &mut cur,
                        VColumn { x: 0.0, start: next_start, clusters: Vec::new() },
                    ));
                    cur_h = 0.0;
                }
            }
            if cur.clusters.is_empty() {
                cur.start = cluster.byte_range.start;
            }
            cur_h += row_h;
            cur.clusters.push(cluster);
        }
        // A manual return always starts a fresh column, even if the
        // current one isn't full — matching real vertical typesetting.
        if !cur.clusters.is_empty() {
            columns.push(std::mem::replace(
                &mut cur,
                VColumn { x: 0.0, start: line.text_range().end, clusters: Vec::new() },
            ));
            cur_h = 0.0;
        }
    }
    if !cur.clusters.is_empty() || columns.is_empty() {
        columns.push(cur);
    }
    for (i, col) in columns.iter_mut().enumerate() {
        col.x = -(i as f64) * col_w;
    }
    VerticalLayout { columns, row_h, col_w }
}

/// Shapes `content` into an unwrapped parley layout — the shared first
/// step behind both column layout ([`layout`]) and path layout
/// ([`flat_clusters`]).
fn shape(tcx: &mut TextContext, content: &str, style: &TextStyle) -> parley::Layout<Brush> {
    let (fonts, lcx) = tcx.parts();
    let mut b = lcx.ranged_builder(fonts, content, 1.0, true);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(std::borrow::Cow::Owned(
        vec![FontFamilyName::Named(std::borrow::Cow::Owned(style.family.clone()))],
    ))));
    b.push_default(StyleProperty::FontSize(style.size as f32));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(style.weight as f32)));
    b.push_default(StyleProperty::FontStyle(if style.italic {
        ParleyFontStyle::Italic
    } else {
        ParleyFontStyle::Normal
    }));
    b.push_default(StyleProperty::LetterSpacing(
        (style.tracking / 1000.0 * style.size) as f32,
    ));
    b.build(content)
}

/// Every real (non-hard-break) cluster of one parley `Line`, in logical
/// order.
fn line_clusters(line: &parley::layout::Line<'_, Brush>) -> Vec<VCluster> {
    let mut out = Vec::new();
    for item in line.items() {
        let PositionedLayoutItem::GlyphRun(glyph_run) = item else { continue };
        let run = glyph_run.run();
        let font = run.font().clone();
        let font_size = run.font_size();
        let coords = run.normalized_coords().to_vec();
        let glyph_xform = run
            .synthesis()
            .skew()
            .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
        for cluster in run.clusters() {
            if cluster.is_hard_line_break() {
                continue;
            }
            let byte_range = cluster.text_range();
            let glyphs: Vec<VGlyph> = cluster
                .glyphs()
                .map(|g| VGlyph { id: g.id, dx: g.x, dy: g.y })
                .collect();
            if glyphs.is_empty() {
                continue;
            }
            out.push(VCluster {
                font: font.clone(),
                font_size,
                coords: coords.clone(),
                glyph_xform,
                glyphs,
                advance: cluster.advance(),
                byte_range,
            });
        }
    }
    out
}

/// `content`'s clusters as one flat, unwrapped sequence — for type on a
/// path, which walks a single reading line along the curve rather than
/// stacking columns. The second element is `true` when `content` has a
/// second manual paragraph (a `'\n'`) that this ignores — the same
/// "everything past the first line is overflow" rule
/// `pathtext::paint_path_text` applies to horizontal path text.
pub(crate) fn flat_clusters(tcx: &mut TextContext, content: &str, style: &TextStyle) -> (Vec<VCluster>, bool) {
    if content.is_empty() {
        return (Vec::new(), false);
    }
    let mut parley_layout = shape(tcx, content, style);
    parley_layout.break_all_lines(None);
    let mut lines = parley_layout.lines();
    let first = lines.next().map(|l| line_clusters(&l)).unwrap_or_default();
    let overflow = lines.next().is_some();
    (first, overflow)
}

impl VerticalLayout {
    /// Total width of the laid-out block (all columns), local space.
    pub fn width(&self) -> f64 {
        self.columns.len() as f64 * self.col_w
    }

    /// `(x, bottom_y)` for each column's own baseline guide — Illustrator
    /// draws a vertical line down a vertical-text column's center
    /// (`x`), the same role a horizontal line under each line of
    /// horizontal text plays, from the column's top (`y = 0`) down to
    /// its own content's bottom edge.
    pub fn column_guides(&self) -> Vec<(f64, f64)> {
        self.columns
            .iter()
            .map(|c| (c.x, c.clusters.len().max(1) as f64 * self.row_h))
            .collect()
    }

    /// Height of the tallest column, local space.
    pub fn height(&self) -> f64 {
        self.columns
            .iter()
            .map(|c| c.clusters.len().max(1) as f64 * self.row_h)
            .fold(0.0, f64::max)
    }

    /// Local bounding box (column 0 at `x = 0`, extending negative for
    /// further columns; `y` from `0` down). Each column's glyphs are
    /// *centered* on its own `x` (see `draw`), so the box's right edge
    /// sits at `col_w / 2`, not `col_w` — using the full `col_w` here
    /// shifted the whole box half a column to the right of the actual
    /// drawn glyphs.
    pub fn bounds(&self) -> Rect {
        Rect::new(
            -self.width() + self.col_w * 0.5,
            0.0,
            self.col_w * 0.5,
            self.height().max(self.row_h),
        )
    }

    fn locate(&self, byte: usize) -> (usize, usize) {
        for (ci, col) in self.columns.iter().enumerate() {
            if col.clusters.is_empty() {
                continue;
            }
            let col_start = col.clusters[0].byte_range.start;
            let col_end = col.clusters.last().unwrap().byte_range.end;
            if byte < col_start {
                return (ci, 0);
            }
            if byte <= col_end {
                for (ri, c) in col.clusters.iter().enumerate() {
                    if byte < c.byte_range.end || ri == col.clusters.len() - 1 {
                        return (ci, ri);
                    }
                }
            }
        }
        let last = self.columns.len().saturating_sub(1);
        (last, self.columns[last].clusters.len().saturating_sub(1))
    }

    /// `(column, row)` of the cluster containing (or nearest to) `byte`
    /// — for the arrow-key "jump to the same row in the next/previous
    /// column" navigation `TextEdit::key` needs for vertical text.
    pub fn column_row_of(&self, byte: usize) -> (usize, usize) {
        self.locate(byte)
    }

    /// Byte offset of the start of the cluster at `(column, row)`,
    /// clamped into range — the reverse of [`Self::column_row_of`].
    pub fn byte_at(&self, column: usize, row: usize) -> usize {
        if self.columns.is_empty() {
            return 0;
        }
        let col = &self.columns[column.min(self.columns.len() - 1)];
        if col.clusters.is_empty() {
            return col.start;
        }
        col.clusters[row.min(col.clusters.len() - 1)].byte_range.start
    }

    /// Byte range spanning all of column `column` — for Home/End acting
    /// as "start/end of the current column" on vertical text.
    pub fn column_bounds(&self, column: usize) -> Range<usize> {
        if self.columns.is_empty() {
            return 0..0;
        }
        let col = &self.columns[column.min(self.columns.len() - 1)];
        match (col.clusters.first(), col.clusters.last()) {
            (Some(f), Some(l)) => f.byte_range.start..l.byte_range.end,
            _ => col.start..col.start,
        }
    }

    fn row_rect(&self, col: &VColumn, row: usize) -> Rect {
        let y0 = row as f64 * self.row_h;
        Rect::new(col.x - self.col_w * 0.5, y0, col.x + self.col_w * 0.5, y0 + self.row_h)
    }

    /// A thin caret bar, local space — a horizontal line between rows
    /// (vertical writing's caret is perpendicular to horizontal
    /// writing's), positioned above or below the cluster at `byte`
    /// depending on whether `byte` is that cluster's start or end.
    pub fn caret_rect(&self, byte: usize) -> Rect {
        let (ci, ri) = self.locate(byte);
        let col = &self.columns[ci];
        if col.clusters.is_empty() {
            return Rect::new(col.x - self.col_w * 0.5, -0.75, col.x + self.col_w * 0.5, 0.75);
        }
        let c = &col.clusters[ri];
        let y = ri as f64 * self.row_h + if byte >= c.byte_range.end { self.row_h } else { 0.0 };
        Rect::new(col.x - self.col_w * 0.5, y - 0.75, col.x + self.col_w * 0.5, y + 0.75)
    }

    /// One highlight rect per selected row (possibly spanning several
    /// columns), local space.
    pub fn selection_rects(&self, range: Range<usize>) -> Vec<Rect> {
        if range.start == range.end {
            return Vec::new();
        }
        let mut rects = Vec::new();
        for col in &self.columns {
            for (ri, c) in col.clusters.iter().enumerate() {
                if c.byte_range.start < range.end && c.byte_range.end > range.start {
                    rects.push(self.row_rect(col, ri));
                }
            }
        }
        rects
    }

    /// Local-space point → nearest byte offset, for click-to-place-caret
    /// and drag-select.
    pub fn hit_test(&self, p: Point) -> usize {
        if self.columns.is_empty() {
            return 0;
        }
        let ci = self
            .columns
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                (a.x - p.x).abs().partial_cmp(&(b.x - p.x).abs()).unwrap()
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
        let col = &self.columns[ci];
        if col.clusters.is_empty() {
            return col.start;
        }
        let row = ((p.y / self.row_h).floor().max(0.0) as usize).min(col.clusters.len() - 1);
        let c = &col.clusters[row];
        let row_top = row as f64 * self.row_h;
        let frac = ((p.y - row_top) / self.row_h).clamp(0.0, 1.0);
        if frac > 0.5 {
            c.byte_range.end
        } else {
            c.byte_range.start
        }
    }

    /// Draws every column's glyphs, upright, under `xf`.
    pub fn draw(&self, scene: &mut Scene, xf: Affine, color: Color) {
        for col in &self.columns {
            for (ri, c) in col.clusters.iter().enumerate() {
                let y = ri as f64 * self.row_h + self.row_h * 0.8;
                let cx = col.x - c.advance as f64 * 0.5;
                scene
                    .draw_glyphs(&c.font)
                    .brush(&Brush::Solid(color))
                    .hint(false)
                    .transform(xf)
                    .glyph_transform(c.glyph_xform)
                    .font_size(c.font_size)
                    .normalized_coords(&c.coords)
                    .draw(
                        Fill::NonZero,
                        c.glyphs.iter().map(|g| vello::Glyph {
                            id: g.id,
                            x: (cx + g.dx as f64) as f32,
                            y: (y - g.dy as f64) as f32,
                        }),
                    );
            }
        }
    }
}

/// Vertical Type on a Path: each character stays upright *relative to
/// the curve* — its own reading axis (top-to-bottom) follows the
/// tangent, the same relationship `pathtext::paint_path_text` gives a
/// horizontal character (its reading axis, left-to-right, follows the
/// tangent there). Concretely that's the same per-glyph
/// `translate(p) · rotate(angle) · translate(-local)` construction as
/// the horizontal version, with an extra `-90°` in the rotation because
/// a vertical glyph's "along the path" coordinate is its local *y* (row
/// position) rather than local *x* (advance position) — swapping which
/// local axis lines up with the tangent needs that quarter turn.
///
/// `pt.align`'s ascent/descender/baseline meaning is a horizontal-only
/// concept (it picks which part of a glyph's *height* rides the path);
/// there's no equally obvious analogue for a vertical glyph's *width*,
/// so v1 always centers each character's width on the path curve
/// itself, regardless of `pt.align`. Returns whether any character fell
/// past the end bracket (unshown) or the content has a second manual
/// paragraph — same overflow contract as the horizontal version.
pub fn paint_path_text_vertical(
    scene: &mut Scene,
    tcx: &mut TextContext,
    td: &TextData,
    pt: &PathTextData,
    arc: &ArcLengthPath,
    rel_xf: amalith_core::Affine,
    view_xf: Affine,
    color: Color,
) -> bool {
    if td.content.is_empty() {
        return false;
    }
    let path_xf = view_xf * convert::affine(rel_xf);
    let row_h = td.style.leading.unwrap_or(td.style.size * 1.2);
    let (clusters, mut overflow) = flat_clusters(tcx, &td.content, &td.style);
    let span = (pt.end - pt.start).max(0.0);
    let content_extent = clusters.len() as f64 * row_h;
    let offset = crate::pathtext::paragraph_offset(td.align, span, content_extent);

    for (i, c) in clusters.iter().enumerate() {
        let along = offset + i as f64 * row_h + row_h * 0.5;
        let leading_edge = along - row_h * 0.5;
        let trailing_edge = along + row_h * 0.5;
        if trailing_edge > span {
            overflow = true;
            continue;
        }
        if leading_edge < 0.0 {
            continue;
        }
        let distance = if pt.flip { pt.end - along } else { pt.start + along };
        let (p, mut angle) = arc.point_and_tangent(distance);
        if pt.flip {
            angle += std::f64::consts::PI;
        }
        let rot = angle - std::f64::consts::FRAC_PI_2;
        let perp_center = c.advance as f64 * 0.5;
        for g in &c.glyphs {
            // `local`'s x is the perpendicular-to-path axis (glyph width
            // centering + baseline shift, mirroring how the horizontal
            // version's `shift` lands in its own perpendicular axis, y);
            // its y is the along-path axis's fine (usually zero)
            // intra-cluster offset — the coarse position already came
            // from `distance` above.
            let perp = g.dx as f64 - perp_center - td.style.baseline_shift;
            let along_fine = -(g.dy as f64);
            let local = Point::new(perp, along_fine);
            let glyph_xf = path_xf
                * Affine::translate(convert::point(p).to_vec2())
                * Affine::rotate(rot)
                * Affine::translate(-local.to_vec2());
            scene
                .draw_glyphs(&c.font)
                .brush(&Brush::Solid(color))
                .hint(false)
                .transform(glyph_xf)
                .glyph_transform(c.glyph_xform)
                .font_size(c.font_size)
                .normalized_coords(&c.coords)
                .draw(Fill::NonZero, std::iter::once(Glyph { id: g.id, x: 0.0, y: 0.0 }));
        }
    }
    overflow
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        TextStyle { size: 20.0, leading: Some(24.0), ..TextStyle::default() }
    }

    #[test]
    fn short_text_fits_in_one_column() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "Hi", &style(), None);
        assert_eq!(l.columns.len(), 1, "no wrap height, no explicit break -> a single column");
    }

    /// Every column's glyphs are drawn *centered* on its own `x` (see
    /// `draw`) — column 0 spans roughly `[-col_w/2, col_w/2]`, not
    /// `[0, col_w]`. `bounds()` used to place the box's right edge at
    /// `col_w` instead of `col_w / 2`, shifting the whole selection box
    /// half a column to the right of the actual drawn text.
    #[test]
    fn bounds_right_edge_sits_at_half_a_column_not_a_whole_one() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "Hi", &style(), None);
        let b = l.bounds();
        assert_eq!(b.x1, l.col_w * 0.5, "the box's right edge must match column 0's real right edge");
        assert_eq!(b.x0, -l.width() + l.col_w * 0.5);
    }

    #[test]
    fn flat_clusters_is_a_single_unbroken_sequence_for_path_text() {
        let mut tcx = TextContext::new();
        let (clusters, overflow) = flat_clusters(&mut tcx, "Hello", &style());
        assert_eq!(clusters.len(), 5, "one cluster per character, no column splitting");
        assert!(!overflow);
        // Logical order: byte ranges strictly increase.
        for w in clusters.windows(2) {
            assert!(w[0].byte_range.start < w[1].byte_range.start);
        }
    }

    #[test]
    fn flat_clusters_flags_overflow_past_the_first_manual_paragraph() {
        let mut tcx = TextContext::new();
        let (clusters, overflow) = flat_clusters(&mut tcx, "AB\nCD", &style());
        assert_eq!(clusters.len(), 2, "only the first line's clusters are returned");
        assert!(overflow, "content after the first manual paragraph break is flagged as overflow");
    }

    #[test]
    fn a_tall_string_wraps_into_a_second_column_under_a_short_box() {
        let mut tcx = TextContext::new();
        // 5 rows at row_h=24 need 120px; a 60px box only fits ~2 rows.
        let l = layout(&mut tcx, "Lorem", &style(), Some(60.0));
        assert!(l.columns.len() >= 2, "a short wrap height must force at least one column break");
    }

    #[test]
    fn an_explicit_newline_always_starts_a_new_column() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "A\nB", &style(), None);
        assert_eq!(l.columns.len(), 2, "a manual return starts a fresh column even with room to spare");
    }

    #[test]
    fn columns_are_ordered_right_to_left() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "A\nB\nC", &style(), None);
        assert_eq!(l.columns.len(), 3);
        assert_eq!(l.columns[0].x, 0.0);
        assert!(l.columns[1].x < l.columns[0].x, "column 1 sits to the left of column 0");
        assert!(l.columns[2].x < l.columns[1].x, "column 2 sits further left still");
    }

    #[test]
    fn column_guides_span_each_columns_own_content_at_its_own_x() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "AB\nC", &style(), None);
        let guides = l.column_guides();
        assert_eq!(guides.len(), 2);
        assert_eq!(guides[0].0, 0.0, "column 0's guide runs down x=0");
        assert_eq!(guides[0].1, 2.0 * l.row_h, "column 0 ('AB') is 2 rows tall");
        assert_eq!(guides[1].0, -l.col_w, "column 1's guide sits one column-width to the left");
        assert_eq!(guides[1].1, 1.0 * l.row_h, "column 1 ('C') is 1 row tall");
    }

    #[test]
    fn locate_and_byte_at_round_trip() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "AB\nCD", &style(), None);
        let (ci, ri) = l.column_row_of(3); // 'C', the first char of the 2nd column
        let back = l.byte_at(ci, ri);
        assert_eq!(back, 3);
    }

    #[test]
    fn hit_test_picks_the_nearest_column_and_row() {
        let mut tcx = TextContext::new();
        let l = layout(&mut tcx, "A\nB", &style(), None);
        // Column 1 ('B') sits at x = -col_w; well inside it, first row.
        let byte = l.hit_test(Point::new(l.columns[1].x, 2.0));
        assert_eq!(byte, 2, "'B' starts right after the newline at byte 2");
    }
}
