//! Text on a Path: resolving the followed path's flattened geometry into
//! an [`amalith_core::ArcLengthPath`], painting glyphs curved along it,
//! and the start/end/center bracket handles' hit-testing and drag math —
//! kept here rather than in `handles.rs` (the selection-box transform
//! handles) since every function needs `ArcLengthPath`, and the two
//! don't otherwise share anything. A plain `ObjectKind::Path` only for
//! v1 (a `CompoundPath` spine is a later addition; there's no single
//! unambiguous "the outline" to walk once a shape has more than one
//! subpath).

use amalith_core::{
    ArcLengthPath, Document, ObjectId, ObjectKind, PathTextAlign, PathTextData, TextAlign, TextData,
};
use parley::layout::PositionedLayoutItem;
use vello::kurbo::{Affine as VAffine, Point as VPoint};
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

use crate::convert;
use crate::text::TextContext;
use crate::textedit::td_layout;

/// Flattening tolerance for a followed path, in local px. Matches the
/// tolerance used elsewhere in the shell for this kind of polyline
/// approximation (e.g. `app/isolation.rs`'s spine-hover line).
const FLATTEN_TOLERANCE: f64 = 0.05;

/// The followed path's arc-length table, plus the transform from *its*
/// local space into the text object's own local space — composed with
/// the text object's usual accumulated paint transform, this lands
/// glyphs correctly no matter where in the group hierarchy the path and
/// the text object each sit. `None` if the path is gone, isn't a plain
/// `Path`, or is too short to have a direction.
pub fn resolve(
    doc: &Document,
    text_id: ObjectId,
    pt: &PathTextData,
) -> Option<(ArcLengthPath, amalith_core::Affine)> {
    if let ObjectKind::Text(td) = &doc.object(text_id)?.kind {
        if let Some(pd) = &td.path_geometry {
            let points = pd.flattened_points(FLATTEN_TOLERANCE).into_iter().next()?;
            let arc = ArcLengthPath::new(&points, pd.subpaths().first()?.closed);
            return (arc.total_length() > 1e-6).then_some((arc, amalith_core::Affine::IDENTITY));
        }
    }
    let path_obj = doc.object(pt.path)?;
    let ObjectKind::Path(pd) = &path_obj.kind else {
        return None;
    };
    let points = pd.flattened_points(FLATTEN_TOLERANCE).into_iter().next()?;
    if points.len() < 2 {
        return None;
    }
    let closed = pd.subpaths().first().is_some_and(|s| s.closed);
    let text_world = doc.world_transform(text_id);
    let path_world = doc.world_transform(pt.path);
    Some((
        ArcLengthPath::new(&points, closed),
        text_world.inverse() * path_world,
    ))
}

/// Net vertical shift (local px, +down) that brings the glyph run's
/// `align`-designated line onto the path curve, given font metrics whose
/// baseline currently sits there.
fn align_shift(ascent: f32, descent: f32, align: PathTextAlign) -> f64 {
    match align {
        PathTextAlign::Baseline => 0.0,
        PathTextAlign::Ascender => ascent as f64,
        PathTextAlign::Descender => -(descent as f64),
        PathTextAlign::Center => (ascent as f64 - descent as f64) / 2.0,
    }
}

/// Offset the shaped line inside the start/end brackets using the same
/// paragraph alignment Illustrator applies to type on a path.
pub fn paragraph_offset(align: TextAlign, span: f64, width: f64) -> f64 {
    let spare = (span - width).max(0.0);
    match align {
        TextAlign::Center | TextAlign::JustifyCenter => spare / 2.0,
        TextAlign::End | TextAlign::JustifyRight => spare,
        _ => 0.0,
    }
}

/// Lay out `td` (a `TextKind::Path` object) and draw its glyphs curved
/// along `arc`, one `scene.draw_glyphs()` call per glyph — vello has no
/// per-glyph transform within a single batched run, so a shared straight
/// baseline (the normal, fast `draw_glyph_runs` path) isn't an option
/// here. Returns whether any glyph fell past the end bracket (unshown) —
/// the overflow indicator is the caller's job, matching area text's tab.
pub fn paint_path_text(
    scene: &mut Scene,
    tcx: &mut TextContext,
    td: &TextData,
    pt: &PathTextData,
    arc: &ArcLengthPath,
    rel_xf: amalith_core::Affine,
    view_xf: VAffine,
    color: Color,
) -> bool {
    if td.content.is_empty() {
        return false;
    }
    let path_xf = view_xf * convert::affine(rel_xf);
    let layout = td_layout(tcx, td);
    let offset = paragraph_offset(
        td.align,
        (pt.end - pt.start).max(0.0),
        layout
            .lines()
            .next()
            .map(|line| line.metrics().advance as f64)
            .unwrap_or(0.0),
    );
    let mut overflow = false;
    for (line_index, line) in layout.lines().enumerate() {
        if line_index > 0 {
            overflow = true;
            break;
        }
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let mut gx = run.offset();
            let gy = run.baseline();
            let r = run.run();
            let font = r.font();
            let size = r.font_size();
            let coords = r.normalized_coords();
            let metrics = r.metrics();
            let y_shift =
                align_shift(metrics.ascent, metrics.descent, pt.align) - td.style.baseline_shift;
            for g in run.glyphs() {
                let x = (gx + g.x) as f64;
                let y = (gy - g.y) as f64;
                let advance = g.advance as f64;
                let center = gx as f64 + advance / 2.0;
                gx += g.advance;
                let line_distance = offset + center;
                let distance = if pt.flip {
                    pt.end - line_distance
                } else {
                    pt.start + line_distance
                };
                let leading_edge = line_distance - advance / 2.0;
                let trailing_edge = line_distance + advance / 2.0;
                if trailing_edge > pt.end - pt.start {
                    overflow = true;
                    continue;
                }
                if leading_edge < 0.0 {
                    continue;
                }
                let (p, mut angle) = arc.point_and_tangent(distance);
                let shift = y_shift;
                if pt.flip {
                    angle += std::f64::consts::PI;
                }
                let glyph_xf = path_xf
                    * VAffine::translate((p.x, p.y))
                    * VAffine::rotate(angle)
                    * VAffine::translate((0.0, shift))
                    * VAffine::translate((-center, -(gy as f64)));
                scene
                    .draw_glyphs(font)
                    .brush(&Brush::Solid(color))
                    .hint(false)
                    .transform(glyph_xf)
                    .font_size(size)
                    .normalized_coords(coords)
                    .draw(
                        Fill::NonZero,
                        std::iter::once(Glyph {
                            id: g.id as u32,
                            x: x as f32,
                            y: y as f32,
                        }),
                    );
            }
        }
    }
    overflow
}

/// Extract the same placed glyph outlines used on screen for PDF export,
/// Create Outlines, and bounds. Glyph offsets stay in the shaped run's
/// coordinate system so combining marks are not stripped of their offsets.
pub fn outline_path_text(
    tcx: &mut TextContext,
    td: &TextData,
    pt: &PathTextData,
    arc: &ArcLengthPath,
    rel: amalith_core::Affine,
) -> amalith_core::geom::BezPath {
    use skrifa::{
        instance::{LocationRef, Size},
        outline::{DrawSettings, OutlinePen},
        GlyphId, MetadataProvider,
    };
    let mut out = amalith_core::geom::BezPath::new();
    let layout = td_layout(tcx, td);
    let offset = paragraph_offset(
        td.align,
        pt.end - pt.start,
        layout
            .lines()
            .next()
            .map(|l| l.metrics().advance as f64)
            .unwrap_or(0.0),
    );
    for line in layout.lines().take(1) {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let r = run.run();
            let font = r.font();
            let Ok(font_ref) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else {
                continue;
            };
            let glyphs = font_ref.outline_glyphs();
            let loc: Vec<_> = r
                .normalized_coords()
                .iter()
                .map(|&c| skrifa::instance::NormalizedCoord::from_bits(c))
                .collect();
            let metrics = r.metrics();
            let shift =
                align_shift(metrics.ascent, metrics.descent, pt.align) - td.style.baseline_shift;
            let mut x = run.offset() as f64;
            for g in run.glyphs() {
                let advance = g.advance as f64;
                let center = x + advance / 2.0;
                let distance = if pt.flip {
                    pt.end - offset - center
                } else {
                    pt.start + offset + center
                };
                let fits = offset + x >= 0.0 && offset + x + advance <= pt.end - pt.start + 1e-6;
                x += advance;
                if !fits {
                    continue;
                }
                let Some(glyph) = glyphs.get(GlyphId::new(g.id as u32)) else {
                    continue;
                };
                let (p, angle) = arc.point_and_tangent(distance);
                let pose = rel
                    * amalith_core::Affine::translate((p.x, p.y))
                    * amalith_core::Affine::rotate(
                        angle + if pt.flip { std::f64::consts::PI } else { 0.0 },
                    )
                    * amalith_core::Affine::translate((
                        g.x as f64 - advance / 2.0,
                        shift - g.y as f64,
                    ))
                    * amalith_core::Affine::scale_non_uniform(1.0, -1.0);
                let mut sink = crate::textedit::OutlineSink {
                    path: &mut out,
                    xf: pose,
                    started: false,
                };
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

/// Union of the owned curve and placed glyph outlines in text-local
/// coordinates. This includes baseline shifts and combining marks, without
/// extending the selection to an unrelated straight text layout.
pub fn text_bounds(
    tcx: &mut TextContext,
    td: &TextData,
    pt: &PathTextData,
    arc: &ArcLengthPath,
    rel_xf: amalith_core::Affine,
) -> amalith_core::Rect {
    use amalith_core::geom::Shape;
    let outline = outline_path_text(tcx, td, pt, arc, rel_xf);
    let geometry = td
        .path_geometry
        .as_ref()
        .map(|p| rel_xf.transform_rect_bbox(p.local_bounds()));
    match (geometry, outline.is_empty()) {
        (Some(path), false) => path.union(outline.bounding_box()),
        (Some(path), true) => path,
        (None, false) => outline.bounding_box(),
        (None, true) => amalith_core::Rect::ZERO,
    }
}

// ---------------------------------------------------------- bracket handles

/// Which bracket handle a Selection-tool drag grabbed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bracket {
    Start,
    End,
    Center,
}

/// Gesture state stores a continuous distance, so crossing the closed-path
/// seam (even repeatedly) never switches to another lap mid-drag.
#[derive(Clone, Debug)]
pub struct BracketDrag {
    pub original: PathTextData,
    pub which: Bracket,
    press_distance: f64,
    pointer_distance: f64,
    pub pointer: amalith_core::Point,
    side: bool,
    dead_zone: f64,
}

impl BracketDrag {
    pub fn new(
        arc: &ArcLengthPath,
        original: PathTextData,
        which: Bracket,
        pointer: amalith_core::Point,
        dead_zone: f64,
    ) -> Self {
        let reference = match which {
            Bracket::Start => original.start,
            Bracket::End => original.end,
            Bracket::Center => (original.start + original.end) / 2.0,
        };
        let distance = nearest_unwrapped(arc, pointer, reference);
        Self {
            original,
            which,
            press_distance: distance,
            pointer_distance: distance,
            pointer,
            side: original.flip,
            dead_zone,
        }
    }

    pub fn update(&mut self, arc: &ArcLengthPath, pointer: amalith_core::Point) {
        self.pointer_distance = nearest_unwrapped(arc, pointer, self.pointer_distance);
        self.pointer = pointer;
        let (p, angle) = arc.point_and_tangent(self.pointer_distance);
        let offset = angle.cos() * (pointer.y - p.y) - angle.sin() * (pointer.x - p.x);
        if offset.abs() > self.dead_zone {
            self.side = offset > 0.0;
        }
    }

    pub fn values(&self, arc: &ArcLengthPath, lock_flip: bool) -> PathTextData {
        let mut pt = self.original;
        let delta = self.pointer_distance - self.press_distance;
        let total = arc.total_length();
        let gap = MIN_SPAN.min(total);
        match self.which {
            Bracket::Start => {
                let lower = if arc.is_closed() { pt.end - total } else { 0.0 };
                pt.start = (pt.start + delta).clamp(lower, (pt.end - gap).max(lower));
            }
            Bracket::End => {
                let upper = if arc.is_closed() {
                    pt.start + total
                } else {
                    total
                };
                pt.end = (pt.end + delta).clamp((pt.start + gap).min(upper), upper);
            }
            Bracket::Center => {
                let delta = if arc.is_closed() {
                    delta
                } else {
                    delta.clamp(-pt.start, (total - pt.end).max(-pt.start))
                };
                pt.start += delta;
                pt.end += delta;
                if !lock_flip {
                    pt.flip = self.side;
                }
            }
        }
        pt
    }
}

/// One screen-space bracket. Painting and hit-testing use these exact
/// endpoints; start/end stems separate on opposite sides at a closed seam.
pub struct ScreenBracket {
    pub which: Bracket,
    pub base: VPoint,
    pub tip: VPoint,
    pub tangent: vello::kurbo::Vec2,
}

/// Perpendicular riser length (screen px) for the bracket ticks: tall
/// enough to clear the glyphs regardless of font size, with the original
/// fixed 24px as a floor for small text.
pub fn bracket_stem_len(font_px: f64) -> f64 {
    (font_px * 1.2).max(24.0)
}

pub fn screen_brackets(arc: &ArcLengthPath, pt: &PathTextData, xf: VAffine, stem: f64) -> Vec<ScreenBracket> {
    // Closed paths use the curve's visual center to choose the outside.
    // A fixed start/end normal breaks at the seam and pointed the end
    // bracket into circles.
    let closed_center = arc.is_closed().then(|| {
        let total = arc.total_length();
        let sum = (0..32).fold(VPoint::ORIGIN.to_vec2(), |sum, sample| {
            let (point, _) = arc.point_and_tangent(total * sample as f64 / 32.0);
            sum + (xf * convert::point(point)).to_vec2()
        });
        VPoint::new(sum.x / 32.0, sum.y / 32.0)
    });
    let coincident_ends = arc.is_closed()
        && (pt.end - pt.start)
            .rem_euclid(arc.total_length())
            .min((pt.start - pt.end).rem_euclid(arc.total_length()))
            < 1e-6;
    [Bracket::Start, Bracket::End, Bracket::Center]
        .into_iter()
        .map(|which| {
            let distance = match which {
                Bracket::Start => pt.start,
                Bracket::End => pt.end,
                Bracket::Center => (pt.start + pt.end) / 2.0,
            };
            let (p, angle) = arc.point_and_tangent(distance);
            let tangent =
                (xf * VPoint::new(angle.cos(), angle.sin()) - xf * VPoint::ORIGIN).normalize();
            let normal =
                (xf * VPoint::new(-angle.sin(), angle.cos()) - xf * VPoint::ORIGIN).normalize();
            // A brand-new full-loop selection has start, end (and their
            // midpoint, Center) all sitting on the very same curve point.
            // Nudging only the tip left the three stems fanning out of one
            // shared base — a crossing "X" right where they meet. Shifting
            // the whole stem (base *and* tip) sideways instead gives Start
            // and End daylight between them from the ground up — a small
            // fixed gap, not tied to the (now font-scaled) stem length, so
            // the pair reads as two handles sitting right next to each
            // other rather than spread apart.
            let lateral = if coincident_ends {
                match which {
                    Bracket::Start => 4.0,
                    Bracket::End => -4.0,
                    Bracket::Center => 0.0,
                }
            } else {
                0.0
            };
            let base = xf * convert::point(p) + tangent * lateral;
            let direction = if let Some(center) = closed_center {
                let outward = if normal.dot(base - center) >= 0.0 {
                    1.0
                } else {
                    -1.0
                };
                outward * if pt.flip { -1.0 } else { 1.0 }
            } else {
                (if which == Bracket::End { 1.0 } else { -1.0 }) * if pt.flip { -1.0 } else { 1.0 }
            };
            ScreenBracket {
                which,
                base,
                tip: base + normal * direction * stem,
                tangent,
            }
        })
        .collect()
}

/// A start-to-end span shorter than this (local px) isn't allowed — keeps
/// a degenerate drag from swallowing all the text.
pub const MIN_SPAN: f64 = 4.0;

/// Grab radius for a bracket handle, screen px — matches
/// [`crate::handles::hit_handle`]'s.
pub const HANDLE_GRAB: f64 = 9.0;

/// Keep distances continuous when a closed path crosses its seam.
pub fn nearest_unwrapped(arc: &ArcLengthPath, point: amalith_core::Point, reference: f64) -> f64 {
    let d = arc.nearest_distance(point);
    let total = arc.total_length();
    if arc.is_closed() && total > 0.0 {
        d + ((reference - d) / total).round() * total
    } else {
        d
    }
}

/// Convert a document-space point into the followed path's own local
/// space — the bracket-drag handlers above all work in path-local
/// coordinates (what `ArcLengthPath` understands), but the pointer comes
/// in as a document-space point like every other drag in the shell.
pub fn to_path_local(doc: &Document, text_id: ObjectId, doc_point: VPoint) -> amalith_core::Point {
    let path_id = match doc.object(text_id).map(|o| &o.kind) {
        Some(ObjectKind::Text(td)) if td.path_geometry.is_none() => match td.kind {
            amalith_core::TextKind::Path(pt) => pt.path,
            _ => text_id,
        },
        _ => text_id,
    };
    let path_world = doc.world_transform(path_id);
    path_world.inverse() * amalith_core::Point::new(doc_point.x, doc_point.y)
}

/// Which bracket (if any) `screen_pointer` is within grab distance of.
/// `view_xf` is the text object's own accumulated screen transform (the
/// same `m` every other paint / hit-test in the shell uses) — bracket
/// points are in the *path's* local space, so it's composed with `rel_xf`
/// first, same as [`paint_path_text`].
pub fn hit_bracket(
    arc: &ArcLengthPath,
    pt: &PathTextData,
    rel_xf: amalith_core::Affine,
    view_xf: VAffine,
    screen_pointer: VPoint,
    stem: f64,
) -> Option<Bracket> {
    let path_xf = view_xf * convert::affine(rel_xf);
    screen_brackets(arc, pt, path_xf, stem)
        .into_iter()
        .filter_map(|handle| {
            let axis = handle.tip - handle.base;
            // Keep the near-path junction out of the grab zone so coincident
            // start/end stems are independently reachable by their tips.
            let t = ((screen_pointer - handle.base).dot(axis) / axis.hypot2()).clamp(0.35, 1.0);
            let distance = (screen_pointer - (handle.base + axis * t)).hypot();
            (distance <= HANDLE_GRAB).then_some((handle.which, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(which, _)| which)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotated_bracket_tip_is_clickable() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::new(0.0, 0.0),
                amalith_core::Point::new(100.0, 0.0),
            ],
            false,
        );
        let pt = PathTextData {
            path: ObjectId::new(),
            start: 0.0,
            end: 100.0,
            align: PathTextAlign::Baseline,
            flip: false,
        };
        let xf = VAffine::rotate(std::f64::consts::FRAC_PI_2);
        assert_eq!(
            hit_bracket(
                &arc,
                &pt,
                amalith_core::Affine::IDENTITY,
                xf,
                VPoint::new(24.0, 50.0),
                24.0
            ),
            Some(Bracket::Center)
        );
        assert_eq!(
            hit_bracket(
                &arc,
                &pt,
                amalith_core::Affine::IDENTITY,
                xf,
                VPoint::new(38.0, 50.0),
                24.0
            ),
            None
        );
    }

    #[test]
    fn reflected_center_handle_stays_on_the_text_side() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::ORIGIN,
                amalith_core::Point::new(100.0, 0.0),
            ],
            false,
        );
        let pt = PathTextData {
            path: ObjectId::new(),
            start: 0.0,
            end: 100.0,
            align: PathTextAlign::Baseline,
            flip: false,
        };
        let xf = VAffine::scale_non_uniform(-2.0, 3.0);
        let center = screen_brackets(&arc, &pt, xf, 24.0)
            .into_iter()
            .find(|h| h.which == Bracket::Center)
            .unwrap();
        assert_eq!(center.tip, VPoint::new(-100.0, -24.0));
        assert_eq!(
            hit_bracket(&arc, &pt, amalith_core::Affine::IDENTITY, xf, center.tip, 24.0),
            Some(Bracket::Center)
        );
    }

    #[test]
    fn closed_path_drag_crosses_seam_without_jumping_a_lap() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::new(0.0, 0.0),
                amalith_core::Point::new(100.0, 0.0),
                amalith_core::Point::new(100.0, 100.0),
                amalith_core::Point::new(0.0, 100.0),
            ],
            true,
        );
        assert_eq!(
            nearest_unwrapped(&arc, amalith_core::Point::new(5.0, 0.0), 395.0),
            405.0
        );
    }

    #[test]
    fn paragraph_alignment_uses_the_space_between_brackets() {
        assert_eq!(paragraph_offset(TextAlign::Start, 100.0, 40.0), 0.0);
        assert_eq!(paragraph_offset(TextAlign::Center, 100.0, 40.0), 30.0);
        assert_eq!(paragraph_offset(TextAlign::End, 100.0, 40.0), 60.0);
        assert_eq!(paragraph_offset(TextAlign::Center, 20.0, 40.0), 0.0);
    }

    #[test]
    fn command_drag_keeps_the_original_side() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::new(0.0, 0.0),
                amalith_core::Point::new(100.0, 0.0),
            ],
            false,
        );
        let original = PathTextData {
            path: ObjectId::new(),
            start: 10.0,
            end: 70.0,
            align: PathTextAlign::Baseline,
            flip: false,
        };
        let mut drag = BracketDrag::new(
            &arc,
            original,
            Bracket::Center,
            amalith_core::Point::new(40.0, -18.0),
            4.0,
        );
        drag.update(&arc, amalith_core::Point::new(50.0, 20.0));
        assert!(drag.values(&arc, false).flip);
        let result = drag.values(&arc, true);
        assert!(!result.flip);
        assert_eq!((result.start, result.end), (20.0, 80.0));
    }

    #[test]
    fn along_path_drag_does_not_flip_in_the_neutral_band() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::new(0.0, 0.0),
                amalith_core::Point::new(100.0, 0.0),
            ],
            false,
        );
        let original = PathTextData {
            path: ObjectId::new(),
            start: 10.0,
            end: 70.0,
            align: PathTextAlign::Baseline,
            flip: true,
        };
        let mut drag = BracketDrag::new(
            &arc,
            original,
            Bracket::Center,
            amalith_core::Point::new(40.0, 18.0),
            4.0,
        );
        drag.update(&arc, amalith_core::Point::new(50.0, 1.0));
        assert!(drag.values(&arc, false).flip);
        drag.update(&arc, amalith_core::Point::new(50.0, -8.0));
        assert!(!drag.values(&arc, false).flip);
    }

    #[test]
    fn coincident_closed_brackets_have_independent_targets() {
        let arc = ArcLengthPath::new(
            &[
                amalith_core::Point::new(0.0, 0.0),
                amalith_core::Point::new(100.0, 0.0),
                amalith_core::Point::new(100.0, 100.0),
                amalith_core::Point::new(0.0, 100.0),
            ],
            true,
        );
        let original = PathTextData {
            path: ObjectId::new(),
            start: 0.0,
            end: 400.0,
            align: PathTextAlign::Baseline,
            flip: false,
        };
        let brackets = screen_brackets(&arc, &original, VAffine::IDENTITY, 24.0);
        let start = brackets
            .iter()
            .find(|handle| handle.which == Bracket::Start)
            .unwrap();
        let end = brackets
            .iter()
            .find(|handle| handle.which == Bracket::End)
            .unwrap();
        assert_ne!(start.tip, end.tip);
        let center = VPoint::new(50.0, 50.0);
        assert!((start.tip - center).hypot() > (start.base - center).hypot());
        assert!((end.tip - center).hypot() > (end.base - center).hypot());
        assert_eq!(
            hit_bracket(
                &arc,
                &original,
                amalith_core::Affine::IDENTITY,
                VAffine::IDENTITY,
                start.tip,
                24.0
            ),
            Some(Bracket::Start)
        );
        assert_eq!(
            hit_bracket(
                &arc,
                &original,
                amalith_core::Affine::IDENTITY,
                VAffine::IDENTITY,
                end.tip,
                24.0
            ),
            Some(Bracket::End)
        );
        let mut drag = BracketDrag::new(
            &arc,
            original,
            Bracket::Center,
            arc.point_and_tangent(200.0).0,
            4.0,
        );
        assert_eq!(drag.values(&arc, true), original);
        for distance in (225..=1025).step_by(25) {
            drag.update(&arc, arc.point_and_tangent(distance as f64).0);
        }
        let moved = drag.values(&arc, true);
        assert_eq!((moved.start, moved.end), (825.0, 1225.0));
    }
}
