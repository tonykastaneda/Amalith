//! Smart Guides (⌘U): the constant background hover/drag assist —
//! "anchor"/"path"/"endpoint" labels, snap-to-alignment while moving,
//! angle-locked construction guides for the Pen tool, a hover highlight,
//! and a reference guide while scaling/rotating.
//!
//! One engine, several thin consumers: [`App::sg_hover_scan`] answers
//! "what's under the cursor right now" for pure hover (no drag), and
//! [`App::sg_move_snap`] answers the same question while additionally
//! nudging the live drag position onto whatever it finds — that's the
//! difference between just *labeling* an endpoint and actually *landing*
//! on it. Every candidate category is gated by its own `Settings.sg_*`
//! bool; the whole engine is gated by `Settings.smart_guides_enabled`.
//! Label visibility never changes the geometry used for point snapping.

use super::*;
use vello::kurbo::{ParamCurve, ParamCurveNearest, Vec2};

/// Guide ink, independent of selection and theme accent colors.
pub(in crate::app) const SMART_GUIDE_INK: Color = Color::from_rgb8(0xff, 0x00, 0xcc);

/// What Smart Guides currently has the pointer over / snapped to — drives
/// both the label/guide-line text and the drawn overlay.
#[derive(Clone, Debug)]
pub(in crate::app) enum SmartGuideHit {
    Multiple(Vec<SmartGuideHit>),
    /// A visible Bézier control point on the in-progress Pen path.
    Handle {
        point: Point,
    },
    /// A plain (non-endpoint) anchor — "anchor".
    Anchor {
        point: Point,
    },
    /// An open subpath's free end — "endpoint" (Illustrator's own wording
    /// for exactly this case, confirmed from the reference video).
    Endpoint {
        point: Point,
    },
    /// A bounding-box center — "center".
    Center {
        point: Point,
    },
    /// The nearest point on a bare path segment — "path".
    Path {
        point: Point,
    },
    /// Two paths crossing — "intersect".
    Intersection {
        point: Point,
    },
    /// A dashed alignment line to another object's edge/center, on one
    /// axis. `value` is the document-space X (or Y) coordinate the guide
    /// runs along; the painter converts to screen space and spans the
    /// whole viewport.
    AlignEdge {
        axis: Axis,
        value: f64,
    },
    /// A preset-angle construction ray from the Pen tool's last anchor.
    ConstructionAngle {
        from: Point,
        degrees: f64,
    },
    /// A reference guide at the pre-drag angle/size while transforming.
    TransformReference {
        center: Point,
        angle: f64,
    },
    /// Two matching gaps either side of the dragged object.
    Spacing {
        gap_a: (Point, Point),
        gap_b: (Point, Point),
        px: f64,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::app) enum Axis {
    X,
    Y,
}

impl SmartGuideHit {
    /// The plain hover-label text, for the unboxed pink labels
    /// (Anchor/Path Labels). `None` for hits that don't use this style
    /// (measurement/construction/spacing all draw their own text).
    pub(in crate::app) fn label(&self) -> Option<&'static str> {
        match self {
            SmartGuideHit::Handle { .. } => Some("handle"),
            SmartGuideHit::Anchor { .. } => Some("anchor"),
            SmartGuideHit::Endpoint { .. } => Some("endpoint"),
            SmartGuideHit::Center { .. } => Some("center"),
            SmartGuideHit::Path { .. } => Some("path"),
            SmartGuideHit::Intersection { .. } => Some("intersect"),
            _ => None,
        }
    }

    /// Document-space target for a plain-label hit.
    pub(in crate::app) fn point(&self) -> Option<Point> {
        match self {
            SmartGuideHit::Handle { point }
            | SmartGuideHit::Anchor { point }
            | SmartGuideHit::Endpoint { point }
            | SmartGuideHit::Center { point }
            | SmartGuideHit::Path { point }
            | SmartGuideHit::Intersection { point } => Some(*point),
            _ => None,
        }
    }
}

/// Live Pen handles are not in the document yet. Resolve their hover cue
/// separately from snap targets so a hover does not change the next anchor.
fn pen_handle_hover(anchors: &[PenAnchor], cursor: Point, tolerance: f64) -> Option<SmartGuideHit> {
    anchors
        .iter()
        .flat_map(|a| {
            [a.handle_in, a.handle_out]
                .into_iter()
                .flatten()
                .filter(move |p| (*p - a.point).hypot2() > 1e-18)
        })
        .filter(|p| (*p - cursor).hypot2() <= tolerance * tolerance)
        .min_by(|a, b| (*a - cursor).hypot2().total_cmp(&(*b - cursor).hypot2()))
        .map(|point| SmartGuideHit::Handle { point })
}

fn pen_anchor_hit(anchors: &[PenAnchor], p: Point, tol: f64) -> Option<SmartGuideHit> {
    anchors
        .iter()
        .map(|a| a.point)
        .filter(|q| (*q - p).hypot2() <= tol * tol)
        .min_by(|a, b| (*a - p).hypot2().total_cmp(&(*b - p).hypot2()))
        .map(|point| SmartGuideHit::Anchor { point })
}

/// Preserve the incoming curve when the user returns to the last Pen anchor
/// to start the next segment without its outgoing direction handle.
pub(in crate::app) fn finish_pen_tangent(anchors: &mut [PenAnchor], p: Point, tol: f64) -> bool {
    let Some(last) = anchors.last_mut() else {
        return false;
    };
    if (last.point - p).hypot() > tol {
        return false;
    }
    last.handle_out = None;
    last.mode = amalith_core::HandleMode::Corner;
    true
}

fn pen_path(anchors: &[PenAnchor]) -> vello::kurbo::BezPath {
    let mut path = vello::kurbo::BezPath::new();
    if let Some(a) = anchors.first() {
        path.move_to(a.point);
    }
    for pair in anchors.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if a.handle_out.is_some() || b.handle_in.is_some() {
            path.curve_to(
                a.handle_out.unwrap_or(a.point),
                b.handle_in.unwrap_or(b.point),
                b.point,
            );
        } else {
            path.line_to(b.point);
        }
    }
    path
}

/// Snap independently on both axes. Spacing only compares objects whose
/// perpendicular extents overlap, so unrelated rows cannot attract a drag.
fn move_snap(
    moved: Rect,
    candidates: &[Rect],
    tol: f64,
    alignment: bool,
    spacing: bool,
) -> (Vec2, Option<SmartGuideHit>) {
    let mut delta = Vec2::ZERO;
    let mut hits = Vec::new();
    for axis in [Axis::X, Axis::Y] {
        let interval = |r: Rect| {
            if axis == Axis::X {
                (r.x0, r.x1)
            } else {
                (r.y0, r.y1)
            }
        };
        let cross = |r: Rect| {
            if axis == Axis::X {
                (r.y0, r.y1)
            } else {
                (r.x0, r.x1)
            }
        };
        let (lo, hi) = interval(moved);
        let mut best: Option<(f64, SmartGuideHit)> = None;
        let mut offer = |d: f64, hit: SmartGuideHit| {
            if d.abs() <= tol && best.as_ref().is_none_or(|(old, _)| d.abs() < old.abs()) {
                best = Some((d, hit));
            }
        };
        if alignment {
            for &r in candidates {
                let (a, b) = interval(r);
                for target in [a, (a + b) * 0.5, b] {
                    for origin in [lo, (lo + hi) * 0.5, hi] {
                        offer(
                            target - origin,
                            SmartGuideHit::AlignEdge {
                                axis,
                                value: target,
                            },
                        );
                    }
                }
            }
        }
        if spacing {
            let (c0, c1) = cross(moved);
            let mut row: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|r| {
                    let (a, b) = cross(*r);
                    a <= c1 && b >= c0
                })
                .collect();
            row.sort_by(|a, b| interval(*a).0.total_cmp(&interval(*b).0));
            let point = |v: f64| {
                if axis == Axis::X {
                    Point::new(v, moved.center().y)
                } else {
                    Point::new(moved.center().x, v)
                }
            };
            let left: Vec<_> = row
                .iter()
                .copied()
                .filter(|r| interval(*r).1 <= lo)
                .collect();
            let right: Vec<_> = row
                .iter()
                .copied()
                .filter(|r| interval(*r).0 >= hi)
                .collect();
            if let (Some(l), Some(r)) = (left.last(), right.first()) {
                let l = interval(*l).1;
                let r = interval(*r).0;
                let d = ((r - hi) - (lo - l)) * 0.5;
                offer(
                    d,
                    SmartGuideHit::Spacing {
                        gap_a: (point(l), point(lo + d)),
                        gap_b: (point(hi + d), point(r)),
                        px: lo + d - l,
                    },
                );
            }
            if right.len() >= 2 {
                let (a, b) = interval(right[0]);
                let c = interval(right[1]).0;
                let gap = c - b;
                let d = a - hi - gap;
                if gap >= 0.0 {
                    offer(
                        d,
                        SmartGuideHit::Spacing {
                            gap_a: (point(hi + d), point(a)),
                            gap_b: (point(b), point(c)),
                            px: gap,
                        },
                    );
                }
            }
            if left.len() >= 2 {
                let (a, b) = interval(left[left.len() - 1]);
                let c = interval(left[left.len() - 2]).1;
                let gap = a - c;
                let d = b + gap - lo;
                if gap >= 0.0 {
                    offer(
                        d,
                        SmartGuideHit::Spacing {
                            gap_a: (point(c), point(a)),
                            gap_b: (point(b), point(lo + d)),
                            px: gap,
                        },
                    );
                }
            }
        }
        if let Some((d, hit)) = best {
            if axis == Axis::X {
                delta.x = d;
            } else {
                delta.y = d;
            }
            hits.push(hit);
        }
    }
    for hit in &mut hits {
        if let SmartGuideHit::Spacing { gap_a, gap_b, .. } = hit {
            let offset = if gap_a.0.y == gap_a.1.y {
                Vec2::new(0.0, delta.y)
            } else {
                Vec2::new(delta.x, 0.0)
            };
            gap_a.0 += offset;
            gap_a.1 += offset;
            gap_b.0 += offset;
            gap_b.1 += offset;
        }
    }
    (
        delta,
        (!hits.is_empty()).then_some(SmartGuideHit::Multiple(hits)),
    )
}

fn construction_snap(
    from: Point,
    p: Point,
    angles: &[f64],
    tol: f64,
) -> Option<(Point, SmartGuideHit)> {
    let d = p - from;
    if d.hypot() <= tol {
        return None;
    }
    angles
        .iter()
        .copied()
        .filter(|a| a.is_finite())
        .filter_map(|degrees| {
            let a = degrees.to_radians();
            let dir = Vec2::new(a.cos(), -a.sin());
            let projection = dir * d.dot(dir);
            let error = (d - projection).hypot();
            (error <= tol).then_some((
                error,
                from + projection,
                SmartGuideHit::ConstructionAngle {
                    from,
                    degrees: (-projection.y).atan2(projection.x).to_degrees(),
                },
            ))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, p, hit)| (p, hit))
}

/// Curve projection is exact to the nearest solver's accuracy. Intersections
/// use a flattened local neighborhood with at most 0.01 logical-pixel error.
fn path_snap(paths: &[vello::kurbo::BezPath], p: Point, tol: f64) -> Option<SmartGuideHit> {
    use vello::kurbo::{Line, PathEl};
    let mut nearest = None;
    let mut lines = Vec::new();
    for path in paths {
        let mut near_path = false;
        for segment in path.segments() {
            let near = segment.nearest(p, 1e-6);
            if near.distance_sq <= tol * tol {
                near_path = true;
                if nearest.as_ref().is_none_or(|(d, _)| near.distance_sq < *d) {
                    nearest = Some((near.distance_sq, segment.eval(near.t)));
                }
            }
        }
        if !near_path {
            continue;
        }
        let mut first = Point::ZERO;
        let mut last = Point::ZERO;
        vello::kurbo::flatten(path.iter(), (tol * 0.0025).max(1e-9), |el| {
            let end = match el {
                PathEl::MoveTo(q) => {
                    first = q;
                    last = q;
                    return;
                }
                PathEl::LineTo(q) => q,
                PathEl::ClosePath => first,
                _ => return,
            };
            let line = Line::new(last, end);
            if (end - last).hypot2() > 1e-18 && line.nearest(p, 1e-6).distance_sq <= tol * tol {
                lines.push(line);
            }
            last = end;
        });
    }
    let mut intersection = None;
    for (i, a) in lines.iter().enumerate() {
        for b in &lines[i + 1..] {
            // Adjacent pieces meet at a flattening vertex, not a crossing.
            if [a.p0, a.p1].iter().any(|p| *p == b.p0 || *p == b.p1) {
                continue;
            }
            let core = |p: Point| amalith_core::Point::new(p.x, p.y);
            if let Some(q) = amalith_core::geom::segment_intersection(
                core(a.p0),
                core(a.p1),
                core(b.p0),
                core(b.p1),
            ) {
                let q = Point::new(q.x, q.y);
                let d = (q - p).hypot2();
                if d <= tol * tol && intersection.as_ref().is_none_or(|(old, _)| d < *old) {
                    intersection = Some((d, q));
                }
            }
        }
    }
    intersection
        .map(|(_, point)| SmartGuideHit::Intersection { point })
        .or_else(|| nearest.map(|(_, point)| SmartGuideHit::Path { point }))
}

impl App {
    /// Document-space snap tolerance for the current zoom.
    fn sg_tolerance_doc(&self) -> f64 {
        self.settings.sg_tolerance / self.doc.view.zoom.max(1e-6)
    }

    /// Pure hover resolution — no drag, no snapping, just "what's the
    /// pointer over." Anchor/Path Labels + Object Highlighting.
    pub(in crate::app) fn sg_hover_scan(&self, cursor_doc: Point) -> Option<SmartGuideHit> {
        if !self.settings.smart_guides_enabled || !self.settings.sg_anchor_path_labels {
            return None;
        }
        self.sg_point_candidate(cursor_doc, &[])
    }

    /// The best point-shaped candidate (anchor/endpoint/path) near `p`,
    /// excluding anchors on `exclude_objects`. Shared by hover and the
    /// point-drag snap below.
    fn sg_point_candidate(&self, p: Point, exclude_objects: &[ObjectId]) -> Option<SmartGuideHit> {
        let doc = self.doc.editor.document();
        let tol = self.sg_tolerance_doc();
        let best = anchors::anchors_within(doc, p, tol)
            .into_iter()
            .filter(|(id, ..)| !exclude_objects.contains(id))
            .min_by(|a, b| (a.2 - p).hypot2().partial_cmp(&(b.2 - p).hypot2()).unwrap());
        if let Some((id, n, ap)) = best {
            let is_end = doc
                .object(id)
                .and_then(|o| o.kind.path_data())
                .is_some_and(|pd| amalith_core::anchor_is_open_endpoint(pd.subpaths(), n));
            return Some(if is_end {
                SmartGuideHit::Endpoint { point: ap }
            } else {
                SmartGuideHit::Anchor { point: ap }
            });
        }
        // A nearby object's bounding-box center — "center".
        let visible = self.visible_doc_rect();
        let center_hit = select::visible_top_level_bounds(doc, visible, exclude_objects)
            .into_iter()
            .map(|(_, b)| b.center())
            .filter(|c| (*c - p).hypot() <= tol)
            .min_by(|a, b| (*a - p).hypot2().partial_cmp(&(*b - p).hypot2()).unwrap());
        if let Some(c) = center_hit {
            return Some(SmartGuideHit::Center { point: c });
        }
        let leaves: Vec<ObjectId> = anchors::path_leaves(doc)
            .into_iter()
            .filter(|id| !exclude_objects.contains(id))
            .collect();
        let paths: Vec<_> = leaves
            .into_iter()
            .filter_map(|id| {
                let pd = doc.object(id)?.kind.path_data()?;
                Some(convert::affine(doc.world_transform(id)) * convert::bez_path(&pd.geometry))
            })
            .collect();
        path_snap(&paths, p, tol)
    }

    /// Drag-time point snap (moving a single anchor/handle, or the Join
    /// tool): scans, and if a hit is within tolerance, returns its exact
    /// point in place of the raw cursor — this is what makes the drag
    /// actually *land*, not just show a label. `exclude_objects` keeps an
    /// anchor from "snapping" to its own object's other anchors while
    /// it's the thing being dragged... unless that's exactly the point
    /// (Join deliberately passes an empty exclude list).
    pub(in crate::app) fn sg_point_snap(
        &self,
        cursor_doc: Point,
        exclude_objects: &[ObjectId],
    ) -> (Point, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled {
            return (cursor_doc, None);
        }
        match self.sg_point_candidate(cursor_doc, exclude_objects) {
            Some(hit) => {
                let p = hit.point().unwrap_or(cursor_doc);
                (p, Some(hit))
            }
            None => (cursor_doc, None),
        }
    }

    /// Drag-time snap for moving a whole selection: `raw_delta` is the
    /// unsnapped document-space translation since press; `bounds` is the
    /// selection's own bounding box *before* this delta. Returns a
    /// (possibly axis-nudged) delta plus whatever alignment/spacing it
    /// found, independently on X and Y.
    pub(in crate::app) fn sg_move_snap(
        &self,
        raw_delta: Vec2,
        bounds: Rect,
        exclude: &[ObjectId],
    ) -> (Vec2, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled {
            return (raw_delta, None);
        }
        let moved = bounds + raw_delta;
        let candidates: Vec<_> = select::visible_top_level_bounds(
            self.doc.editor.document(),
            self.visible_doc_rect(),
            exclude,
        )
        .into_iter()
        .map(|(_, b)| b)
        .collect();
        let (adjustment, hit) = move_snap(
            moved,
            &candidates,
            self.sg_tolerance_doc(),
            self.settings.sg_alignment_guides,
            self.settings.sg_spacing_guides,
        );
        (raw_delta + adjustment, hit)
    }

    pub(in crate::app) fn sg_construction_snap(
        &self,
        from: Point,
        p: Point,
    ) -> (Point, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled || !self.settings.sg_construction_guides {
            return (p, None);
        }
        construction_snap(from, p, &self.settings.sg_angles, self.sg_tolerance_doc())
            .map_or((p, None), |(p, h)| (p, Some(h)))
    }

    /// The same resolution is used for the Pen preview and its committed click.
    pub(in crate::app) fn sg_pen_snap(&self, p: Point) -> (Point, Option<SmartGuideHit>) {
        let from = self.pen.last().map(|a| a.point);
        if self.shift_down {
            return (constrained(from, p, true), None);
        }
        let tol = self.sg_tolerance_doc();
        if self.settings.smart_guides_enabled {
            if let Some(hit) = pen_anchor_hit(&self.pen, p, tol) {
                return (hit.point().unwrap(), Some(hit));
            }
        }
        let (point, hit) = self.sg_point_snap(p, &[]);
        if hit.is_some() {
            return (point, hit);
        }
        if self.settings.smart_guides_enabled {
            if let Some(hit) = path_snap(&[pen_path(&self.pen)], p, tol) {
                return (hit.point().unwrap(), Some(hit));
            }
            if self.settings.sg_alignment_guides {
                let doc = self.doc.editor.document();
                let mut targets: Vec<_> =
                    select::visible_top_level_bounds(doc, self.visible_doc_rect(), &[])
                        .into_iter()
                        .map(|(_, r)| r)
                        .collect();
                targets.extend(
                    self.pen
                        .iter()
                        .map(|a| Rect::new(a.point.x, a.point.y, a.point.x, a.point.y)),
                );
                let (d, hit) = move_snap(Rect::new(p.x, p.y, p.x, p.y), &targets, tol, true, false);
                if hit.is_some() {
                    return (p + d, hit);
                }
            }
        }
        from.map_or((p, None), |from| self.sg_construction_snap(from, p))
    }

    pub(in crate::app) fn sg_measurement_text(&self) -> Option<String> {
        let transform = match &self.drag {
            Drag::Scale {
                start_xf, preview, ..
            }
            | Drag::ScaleTool {
                start_xf, preview, ..
            }
            | Drag::Rotate {
                start_xf, preview, ..
            }
            | Drag::RotateTool {
                start_xf, preview, ..
            }
            | Drag::ShearTool {
                start_xf, preview, ..
            } => start_xf.iter().find_map(|(id, start)| {
                if start.determinant().abs() < 1e-12 {
                    return None;
                }
                preview.get(id).map(|p| (*p * start.inverse()).as_coeffs())
            }),
            _ => None,
        };
        if self.settings.sg_transform_tools {
            if let Some(m) = transform {
                return Some(match self.drag {
                    Drag::Rotate { .. } | Drag::RotateTool { .. } => {
                        format!("Rotate: {:.1}°", -m[1].atan2(m[0]).to_degrees())
                    }
                    Drag::ShearTool { .. } => format!("Shear: {:.1}°", m[2].atan().to_degrees()),
                    _ => format!("X: {:.1}%  Y: {:.1}%", m[0] * 100.0, m[3] * 100.0),
                });
            }
        }
        if !self.settings.sg_measurement_labels {
            return None;
        }
        match self.drag {
            Drag::MoveObjects {
                start_doc,
                last_doc,
                ..
            }
            | Drag::MoveAnchors {
                start_doc,
                last_doc,
                ..
            }
            | Drag::MoveArtboard {
                start_doc,
                last_doc,
                ..
            } => {
                let d = if self.shift_down {
                    snap8(last_doc - start_doc)
                } else {
                    last_doc - start_doc
                };
                Some(format!("ΔX: {:.1} pt  ΔY: {:.1} pt", d.x, d.y))
            }
            Drag::DrawShape {
                tool: Tool::Line,
                start_doc,
                cur_doc,
            } => {
                let d = constrained(Some(start_doc), cur_doc, self.shift_down) - start_doc;
                Some(format!(
                    "L: {:.1} pt  {:.1}°",
                    d.hypot(),
                    (-d.y).atan2(d.x).to_degrees()
                ))
            }
            Drag::DrawShape {
                start_doc, cur_doc, ..
            }
            | Drag::DrawArtboard { start_doc, cur_doc }
            | Drag::DrawText { start_doc, cur_doc } => {
                let rect = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
                Some(format!(
                    "W: {:.1} pt  H: {:.1} pt",
                    rect.width(),
                    rect.height()
                ))
            }
            Drag::None if self.active_tool == Tool::Pen => {
                let p = self.sg_pen_snap(self.doc_point(self.pointer)).0;
                Some(format!("X: {:.1} pt  Y: {:.1} pt", p.x, p.y))
            }
            _ => None,
        }
    }

    /// Only label handles the node overlay actually exposes: selected anchors
    /// on visible node paths, plus the live Pen path's control points.
    fn sg_handle_hover(&self, p: Point) -> Option<SmartGuideHit> {
        if !self.settings.smart_guides_enabled || !self.settings.sg_anchor_path_labels {
            return None;
        }
        let tol = self.sg_tolerance_doc();
        if self.active_tool == Tool::Pen {
            if let Some(hit) = pen_handle_hover(&self.pen, p, tol) {
                return Some(hit);
            }
        }
        let doc = self.doc.editor.document();
        let visible = anchors::path_leaves(doc);
        let mut handles = Vec::new();
        for id in self
            .node_paths()
            .into_iter()
            .filter(|id| visible.contains(id))
        {
            handles.extend(
                anchors::handles_of(doc, id)
                    .into_iter()
                    .filter(|(n, _, _)| self.doc.anchor_sel.contains(&(id, *n)))
                    .map(|(_, _, p)| p),
            );
        }
        handles
            .into_iter()
            .filter(|q| (*q - p).hypot2() <= tol * tol)
            .min_by(|a, b| (*a - p).hypot2().total_cmp(&(*b - p).hypot2()))
            .map(|point| SmartGuideHit::Handle { point })
    }

    /// Object Highlighting: the path directly under the cursor, hover-only.
    fn sg_hovered_path_at(&self, cursor_doc: Point) -> Option<ObjectId> {
        if !self.settings.smart_guides_enabled || !self.settings.sg_object_highlighting {
            return None;
        }
        let doc = self.doc.editor.document();
        let ids = anchors::path_leaves(doc);
        anchors::segment_at(doc, &ids, cursor_doc, self.sg_tolerance_doc()).map(|(id, _, _)| id)
    }

    /// Unconditional per-pointer-move refresh (no drag active) — Anchor/
    /// Path Labels + Object Highlighting. Called from `on_cursor_move`
    /// right alongside `refresh_tooltip`.
    pub(in crate::app) fn refresh_smart_guides(&mut self) {
        if !matches!(self.drag, Drag::None) {
            self.smart_guide_hit = None;
            self.sg_hovered_path = None;
            return;
        }
        let had_hit = self.smart_guide_hit.is_some() || self.sg_hovered_path.is_some();
        if self.pointer_win != self.main_id
            || !self.canvas_viewport().contains(self.pointer)
            || self.prefs.is_some()
            || self.ctx_menu.is_some()
            || self.picker.is_some()
        {
            self.smart_guide_hit = None;
            self.sg_hovered_path = None;
            if had_hit {
                self.request_main_redraw();
            }
            return;
        }
        let dp = self.doc_point(self.pointer);
        self.smart_guide_hit = self.sg_handle_hover(dp).or_else(|| {
            if self.active_tool == Tool::Pen {
                self.sg_pen_snap(dp).1
            } else {
                self.sg_hover_scan(dp)
            }
        });
        if had_hit {
            self.request_main_redraw();
        }
        self.sg_hovered_path = self.sg_hovered_path_at(dp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::BezPath;

    #[test]
    fn live_pen_geometry_remains_available_for_labels_and_corner_transition() {
        let mut anchors = [
            PenAnchor {
                point: Point::ZERO,
                handle_in: None,
                handle_out: None,
                mode: amalith_core::HandleMode::Corner,
            },
            PenAnchor {
                point: Point::new(100., 0.),
                handle_in: Some(Point::new(75., 25.)),
                handle_out: Some(Point::new(125., -25.)),
                mode: amalith_core::HandleMode::Smooth,
            },
        ];
        let curve = pen_path(&anchors);
        let mid = curve.segments().next().unwrap().eval(0.5);
        assert!(matches!(
            path_snap(&[curve.clone()], mid, 4.),
            Some(SmartGuideHit::Path { .. })
        ));
        assert_eq!(
            pen_anchor_hit(&anchors, Point::new(2., 0.), 4.)
                .unwrap()
                .label(),
            Some("anchor")
        );
        assert!(!finish_pen_tangent(&mut anchors, Point::new(50., 50.), 4.));
        assert!(finish_pen_tangent(&mut anchors, Point::new(100., 0.), 4.));
        assert!(anchors[1].handle_out.is_none());
        assert_eq!(anchors[1].handle_in, Some(Point::new(75., 25.)));
        assert_eq!(
            pen_path(&anchors),
            curve,
            "the already drawn curve must not change"
        );
    }

    #[test]
    fn live_pen_handles_get_hover_labels_before_the_path_is_committed() {
        let anchors = [
            PenAnchor {
                point: Point::ZERO,
                handle_in: None,
                handle_out: None,
                mode: amalith_core::HandleMode::Corner,
            },
            PenAnchor {
                point: Point::new(100., 100.),
                handle_in: Some(Point::new(80., 120.)),
                handle_out: Some(Point::new(120., 80.)),
                mode: amalith_core::HandleMode::Smooth,
            },
        ];
        for zoom in [0.5, 1., 4.] {
            for handle in [
                anchors[1].handle_in.unwrap(),
                anchors[1].handle_out.unwrap(),
            ] {
                let hit = pen_handle_hover(&anchors, handle + Vec2::new(3. / zoom, 0.), 4. / zoom)
                    .unwrap();
                assert_eq!(hit.label(), Some("handle"));
                assert_eq!(hit.point(), Some(handle));
                assert!(
                    pen_handle_hover(&anchors, handle + Vec2::new(5. / zoom, 0.), 4. / zoom)
                        .is_none()
                );
            }
        }
        assert!(pen_handle_hover(&anchors, Point::ZERO, 4.).is_none());
        assert!(pen_handle_hover(&[], Point::ZERO, 4.).is_none());
    }

    #[test]
    fn alignment_reaches_remote_objects_and_reports_both_axes() {
        let (d, hit) = move_snap(
            Rect::new(1., 2., 11., 12.),
            &[
                Rect::new(0., 100., 10., 110.),
                Rect::new(100., 0., 110., 10.),
            ],
            4.,
            true,
            false,
        );
        assert_eq!(d, Vec2::new(-1., -2.));
        assert!(matches!(hit,Some(SmartGuideHit::Multiple(h)) if h.len()==2));
    }

    #[test]
    fn spacing_snaps_in_each_direction_independent_of_alignment() {
        let moved = Rect::new(1., 0., 11., 10.);
        let candidates = [Rect::new(30., 0., 40., 10.), Rect::new(60., 0., 70., 10.)];
        for swap in [false, true] {
            for flip in [false, true] {
                let map = |r: Rect| {
                    let r = if flip {
                        Rect::new(-r.x1, r.y0, -r.x0, r.y1)
                    } else {
                        r
                    };
                    if swap {
                        Rect::new(r.y0, r.x0, r.y1, r.x1)
                    } else {
                        r
                    }
                };
                let (d, hit) = move_snap(map(moved), &candidates.map(map), 4., false, true);
                let amount = if flip { 1. } else { -1. };
                assert_eq!(
                    d,
                    if swap {
                        Vec2::new(0., amount)
                    } else {
                        Vec2::new(amount, 0.)
                    }
                );
                assert!(hit.is_some());
            }
        }
        // Y alignment must not suppress an independent X spacing match.
        assert_eq!(
            move_snap(moved, &candidates, 4., true, true).0,
            Vec2::new(-1., 0.)
        );
    }

    #[test]
    fn spacing_centers_between_neighbors_but_ignores_unrelated_rows() {
        let neighbors = [Rect::new(0., 0., 10., 10.), Rect::new(60., 0., 70., 10.)];
        assert_eq!(
            move_snap(Rect::new(29., 0., 39., 10.), &neighbors, 4., false, true).0,
            Vec2::new(1., 0.)
        );
        assert!(
            move_snap(Rect::new(29., 100., 39., 110.), &neighbors, 4., false, true)
                .1
                .is_none()
        );
    }

    #[test]
    fn construction_is_bidirectional_and_respects_screen_tolerance_at_every_zoom() {
        for zoom in [0.1, 1., 10.] {
            for sign in [-1., 1.] {
                let p = Point::new(sign * 100. / zoom, 3. / zoom);
                let (snapped, _) = construction_snap(Point::ZERO, p, &[0.], 4. / zoom).unwrap();
                assert!((snapped.y).abs() < 1e-10);
                assert!((snapped.x - p.x).abs() < 1e-10);
                assert!(
                    construction_snap(
                        Point::ZERO,
                        Point::new(sign * 1000. / zoom, 5. / zoom),
                        &[0.],
                        4. / zoom
                    )
                    .is_none()
                );
            }
        }
        assert!(
            construction_snap(
                Point::ZERO,
                Point::new(100., 1.),
                &[f64::NAN, f64::INFINITY],
                4.
            )
            .is_none()
        );
    }

    #[test]
    fn path_snap_projects_to_transformed_curve_and_finds_crossing() {
        let mut line = BezPath::new();
        line.move_to((0., 0.));
        line.line_to((100., 0.));
        let p = Point::new(50., 2.);
        assert_eq!(
            path_snap(&[line.clone()], p, 4.).unwrap().point(),
            Some(Point::new(50., 0.))
        );
        let mut vertical = BezPath::new();
        vertical.move_to((50., -20.));
        vertical.line_to((50., 20.));
        assert!(
            matches!(path_snap(&[line,vertical],Point::new(51.,1.),4.),Some(SmartGuideHit::Intersection { point }) if (point-Point::new(50.,0.)).hypot()<1e-8)
        );
        let mut curve = BezPath::new();
        curve.move_to((0., 0.));
        curve.curve_to((0., 10.), (10., 10.), (10., 0.));
        let curve = Affine::translate((100., 20.)) * curve;
        let hit = path_snap(&[curve], Point::new(105., 29.), 4.)
            .unwrap()
            .point()
            .unwrap();
        assert!((hit - Point::new(105., 27.5)).hypot() < 1e-5);
    }
}
