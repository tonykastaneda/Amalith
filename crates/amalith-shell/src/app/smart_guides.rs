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
/// `guides` are ruler-guide positions (each tagged with the single axis it
/// runs fixed on) — checked alongside object edges/centers for alignment,
/// but never fed into spacing (an infinitely long guide has no width to
/// measure a gap against, and its perpendicular extent is unbounded, so it
/// would otherwise "overlap" every row and contaminate every gap match).
fn move_snap(
    moved: Rect,
    candidates: &[Rect],
    tol: f64,
    alignment: bool,
    spacing: bool,
    guides: &[(Axis, f64)],
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
            for &(guide_axis, target) in guides {
                if guide_axis != axis {
                    continue;
                }
                for origin in [lo, (lo + hi) * 0.5, hi] {
                    offer(target - origin, SmartGuideHit::AlignEdge { axis, value: target });
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

/// Drops an [`SmartGuideHit::AlignEdge`] on an axis its caller isn't
/// actually moving along — [`App::sg_scale_snap`]'s edge-handle case,
/// where `move_snap` still checks both axes generically but only one
/// (or neither, for a corner vs. an edge handle) should ever be applied
/// or even displayed. Non-`AlignEdge`/non-`Multiple` hits pass through
/// untouched; a single-axis leftover unwraps out of `Multiple`.
fn mask_axis_hit(hit: Option<SmartGuideHit>, changes_x: bool, changes_y: bool) -> Option<SmartGuideHit> {
    match hit {
        Some(SmartGuideHit::Multiple(hits)) => {
            let mut kept: Vec<_> = hits
                .into_iter()
                .filter(|h| match h {
                    SmartGuideHit::AlignEdge { axis: Axis::X, .. } => changes_x,
                    SmartGuideHit::AlignEdge { axis: Axis::Y, .. } => changes_y,
                    _ => true,
                })
                .collect();
            match kept.len() {
                0 => None,
                1 => kept.pop(),
                _ => Some(SmartGuideHit::Multiple(kept)),
            }
        }
        other => other,
    }
}

/// The extra single-axis point targets [`App::sg_scale_snap`] offers
/// beyond plain edge/center alignment: matching each candidate's own
/// width/height (Illustrator's "matching dimensions" cue), plus this same
/// shape's own untouched-so-far dimension (a corner/edge landing there
/// yields a perfect square or circle). Expressed as `(Axis, value)` pairs
/// — the same shape ruler guides use — so they ride through `move_snap`'s
/// existing single-axis alignment path without touching spacing.
fn scale_dimension_guides(handle: Handle, bounds: Rect, candidates: &[Rect]) -> Vec<(Axis, f64)> {
    let changes_x = !matches!(handle, Handle::N | Handle::S);
    let changes_y = !matches!(handle, Handle::E | Handle::W);
    let left = matches!(handle, Handle::Nw | Handle::W | Handle::Sw);
    let top = matches!(handle, Handle::Nw | Handle::N | Handle::Ne);
    // The opposite corner/edge stays put; the dragged one lands at that
    // fixed coordinate plus (or minus, depending on which side is fixed)
    // the target size.
    let fixed_x = if left { bounds.x1 } else { bounds.x0 };
    let fixed_y = if top { bounds.y1 } else { bounds.y0 };
    let sign_x = if left { -1.0 } else { 1.0 };
    let sign_y = if top { -1.0 } else { 1.0 };
    let mut guides = Vec::new();
    for r in candidates {
        if changes_x {
            guides.push((Axis::X, fixed_x + sign_x * r.width()));
        }
        if changes_y {
            guides.push((Axis::Y, fixed_y + sign_y * r.height()));
        }
    }
    if changes_x {
        guides.push((Axis::X, fixed_x + sign_x * bounds.height()));
    }
    if changes_y {
        guides.push((Axis::Y, fixed_y + sign_y * bounds.width()));
    }
    guides
}

/// Snaps a Shear-tool angle (already computed and clamped to its own
/// `(-90°, 90°)` domain — see `Drag::ShearTool`'s own comment on why it
/// can't use a plain full-circle `angle_to`) to the user's construction-
/// angle list. Reusing `construction_snap` directly isn't an option: that
/// solver measures a point's direction from an origin over the full
/// circle, but a shear angle already *is* an angle, wrapped into a half-
/// turn (a shear axis, like a reflection axis, repeats every 180°) rather
/// than derived from one. Each preset angle is folded into that same
/// domain before comparing.
fn shear_angle_snap(raw_deg: f64, angles: &[f64], tol_deg: f64) -> Option<f64> {
    angles
        .iter()
        .copied()
        .filter(|a| a.is_finite())
        .map(|a| ((a + 90.0).rem_euclid(180.0)) - 90.0)
        .map(|a| (a, (a - raw_deg).abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|&(_, err)| err <= tol_deg)
        .map(|(a, _)| a)
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

/// View ▸ Snap to Grid's own math: the nearest grid intersection,
/// `spacing` canonical px apart. A non-finite or non-positive spacing is a
/// no-op rather than a divide-by-zero/NaN result.
fn nearest_grid_point(p: Point, spacing: f64) -> Point {
    if !spacing.is_finite() || spacing <= 0.0 {
        return p;
    }
    Point::new((p.x / spacing).round() * spacing, (p.y / spacing).round() * spacing)
}

/// View ▸ Snap to Pixel's own math: the nearest whole document unit (this
/// app's canonical px), independent of zoom — matching Illustrator's own
/// "Align to Pixel Grid" rounding.
fn nearest_pixel_point(p: Point) -> Point {
    Point::new(p.x.round(), p.y.round())
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
        self.sg_point_candidate(cursor_doc, &[], false)
    }

    /// The best point-shaped candidate (anchor/endpoint/path) near `p`,
    /// excluding the exact anchors in `exclude`. Anchor-level, not object-
    /// level: dragging one anchor of a path can still land on a *sibling*
    /// anchor, its object's own bounding-box center, or another of its own
    /// segments — Illustrator doesn't blind a whole object just because
    /// one of its points is being moved, only the point being dragged onto
    /// itself is excluded. Shared by hover and the point-drag snap below.
    /// `points_only` restricts this to the anchor scan alone — real
    /// Illustrator's Snap to Point (its own View-menu toggle, independent
    /// of Smart Guides entirely) only ever finds anchor points, never a
    /// bounding-box center or a bare path segment; `false` is the fuller
    /// scan Smart Guides itself uses.
    fn sg_point_candidate(&self, p: Point, exclude: &[(ObjectId, usize)], points_only: bool) -> Option<SmartGuideHit> {
        let doc = self.doc.editor.document();
        let tol = self.sg_tolerance_doc();
        let best = anchors::anchors_within(doc, p, tol)
            .into_iter()
            .filter(|(id, n, _)| !exclude.contains(&(*id, *n)))
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
        if points_only {
            return None;
        }
        // A nearby object's bounding-box center — "center".
        let visible = self.visible_doc_rect();
        let center_hit = select::visible_top_level_bounds(doc, visible, &[])
            .into_iter()
            .map(|(_, b)| b.center())
            .filter(|c| (*c - p).hypot() <= tol)
            .min_by(|a, b| (*a - p).hypot2().partial_cmp(&(*b - p).hypot2()).unwrap());
        if let Some(c) = center_hit {
            return Some(SmartGuideHit::Center { point: c });
        }
        let paths: Vec<_> = anchors::path_leaves(doc)
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
    /// actually *land*, not just show a label. `exclude` keeps an anchor
    /// from "snapping" onto itself while it's the thing being dragged
    /// (sibling anchors on the same path, and that path's own center/
    /// other segments, stay fair game) — unless that's exactly the point
    /// (Join deliberately passes an empty exclude list).
    /// Arbitration when nothing above matches: View ▸ Snap to Grid, then
    /// View ▸ Snap to Pixel, in that order — both independent of the
    /// Smart Guides master switch entirely, same as real Illustrator
    /// keeps them as their own View-menu systems, not Smart Guides
    /// sub-features.
    pub(in crate::app) fn sg_grid_pixel_fallback(&self, p: Point) -> Point {
        if self.settings.snap_to_grid {
            nearest_grid_point(p, self.settings.grid_spacing)
        } else if self.settings.snap_to_pixel {
            nearest_pixel_point(p)
        } else {
            p
        }
    }

    pub(in crate::app) fn sg_point_snap(
        &self,
        cursor_doc: Point,
        exclude: &[(ObjectId, usize)],
    ) -> (Point, Option<SmartGuideHit>) {
        // Snap to Point is its own View-menu toggle, independent of the
        // Smart Guides master switch — but real Illustrator's version of
        // it only ever finds anchor points, never a bounding-box center
        // or bare path segment the way Smart Guides itself does, so it
        // gets the restricted scan when Smart Guides isn't also on.
        if self.settings.smart_guides_enabled {
            if let Some(hit) = self.sg_point_candidate(cursor_doc, exclude, false) {
                return (hit.point().unwrap_or(cursor_doc), Some(hit));
            }
        } else if self.settings.snap_to_point {
            if let Some(hit) = self.sg_point_candidate(cursor_doc, exclude, true) {
                return (hit.point().unwrap_or(cursor_doc), Some(hit));
            }
        }
        (self.sg_grid_pixel_fallback(cursor_doc), None)
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
        let moved = bounds + raw_delta;
        if self.settings.smart_guides_enabled {
            let candidates = self.sg_alignment_bounds(exclude, &[]);
            let (adjustment, hit) = move_snap(
                moved,
                &candidates,
                self.sg_tolerance_doc(),
                self.settings.sg_alignment_guides,
                self.settings.sg_spacing_guides,
                &self.sg_guide_targets(),
            );
            if hit.is_some() {
                return (raw_delta + adjustment, hit);
            }
        }
        // No Smart Guide match (or Smart Guides is off entirely) — Snap
        // to Grid/Pixel are their own View-menu systems, so they still
        // get a say, snapping the moved bounds' own top-left corner.
        let corner = self.sg_grid_pixel_fallback(moved.origin());
        (raw_delta + (corner - moved.origin()), None)
    }

    /// [`Self::sg_move_snap`]'s artboard-manipulation counterpart: an
    /// artboard being dragged is excluded from the artboard-rect
    /// candidates (it can't snap to its own pre-drag position) but every
    /// other artboard and every object still counts.
    pub(in crate::app) fn sg_artboard_move_snap(
        &self,
        raw_delta: Vec2,
        bounds: Rect,
        exclude_artboard: amalith_core::ArtboardId,
    ) -> (Vec2, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled {
            return (raw_delta, None);
        }
        let moved = bounds + raw_delta;
        let candidates = self.sg_alignment_bounds(&[], &[exclude_artboard]);
        let (adjustment, hit) = move_snap(
            moved,
            &candidates,
            self.sg_tolerance_doc(),
            self.settings.sg_alignment_guides,
            self.settings.sg_spacing_guides,
            &self.sg_guide_targets(),
        );
        (raw_delta + adjustment, hit)
    }

    /// [`Self::sg_scale_snap`]'s artboard-manipulation counterpart, for
    /// dragging an artboard's own resize handle.
    pub(in crate::app) fn sg_artboard_resize_snap(
        &self,
        handle: Handle,
        pointer: Point,
        bounds: Rect,
        exclude_artboard: amalith_core::ArtboardId,
    ) -> (Point, Option<SmartGuideHit>) {
        self.sg_scale_snap(handle, pointer, bounds, &[], &[exclude_artboard])
    }

    /// Drag-time snap for a bounding-box Scale handle: `pointer` is the raw
    /// document-space cursor position that `handles::scaled_transform`
    /// would otherwise use directly as the dragged corner/edge's new
    /// position; `bounds` is the shape's own bounds *before* this drag (the
    /// fixed opposite corner/edge lives on it). Snaps only the axis/axes
    /// that handle actually moves — an edge handle (N/S/E/W) must never
    /// pick up a cross-axis nudge from a candidate that only lines up on
    /// the axis it doesn't control, or the opposite edge would silently
    /// drift. Besides ordinary edge/center alignment, offers two more
    /// point targets per axis, expressed the same way ruler guides are (a
    /// single-axis value, so they can't contaminate the other axis or
    /// spacing): matching another candidate's width/height (Illustrator's
    /// "matching dimensions" cue), and matching *this* shape's own other,
    /// unchanged dimension (a corner/edge landing exactly there yields a
    /// perfect square or circle from a rectangle/ellipse).
    /// `exclude_artboards` is for [`Self::sg_artboard_resize_snap`]'s own
    /// reuse of this exact axis-masking, so a resized artboard can't snap
    /// to its own (pre-drag) rect.
    pub(in crate::app) fn sg_scale_snap(
        &self,
        handle: Handle,
        pointer: Point,
        bounds: Rect,
        exclude: &[ObjectId],
        exclude_artboards: &[amalith_core::ArtboardId],
    ) -> (Point, Option<SmartGuideHit>) {
        let changes_x = !matches!(handle, Handle::N | Handle::S);
        let changes_y = !matches!(handle, Handle::E | Handle::W);
        if self.settings.smart_guides_enabled && self.settings.sg_alignment_guides {
            let candidates = self.sg_alignment_bounds(exclude, exclude_artboards);
            let mut guides = self.sg_guide_targets();
            guides.extend(scale_dimension_guides(handle, bounds, &candidates));
            let moved = Rect::new(pointer.x, pointer.y, pointer.x, pointer.y);
            let (d, hit) = move_snap(moved, &candidates, self.sg_tolerance_doc(), true, false, &guides);
            if let Some(hit) = mask_axis_hit(hit, changes_x, changes_y) {
                let mut snapped = pointer;
                if changes_x {
                    snapped.x += d.x;
                }
                if changes_y {
                    snapped.y += d.y;
                }
                return (snapped, Some(hit));
            }
        }
        // No Smart Guide match — Snap to Grid/Pixel still get a say,
        // masked to the same axis/axes this handle actually moves.
        let fallback = self.sg_grid_pixel_fallback(pointer);
        let mut snapped = pointer;
        if changes_x {
            snapped.x = fallback.x;
        }
        if changes_y {
            snapped.y = fallback.y;
        }
        (snapped, None)
    }

    /// Alignment-candidate rects for [`Self::sg_move_snap`] and the Pen
    /// tool's alignment fallback: object bounds — scoped to the current
    /// isolation group when isolated, the whole document otherwise,
    /// matching Illustrator hiding the rest of the document as alignment
    /// noise once you've drilled into a group — plus every artboard's own
    /// rect and its bleed-expanded rect, which participate regardless of
    /// isolation depth (artboards are document-level framing, not content).
    /// `exclude_artboards` keeps an artboard being moved/resized from
    /// snapping to its own (pre-drag) rect.
    fn sg_alignment_bounds(&self, exclude: &[ObjectId], exclude_artboards: &[amalith_core::ArtboardId]) -> Vec<Rect> {
        let doc = self.doc.editor.document();
        let visible = self.visible_doc_rect();
        let mut out: Vec<Rect> = match self.isolation_root() {
            Some(root) => select::bounds_within(doc, root, visible, exclude),
            None => select::visible_top_level_bounds(doc, visible, exclude),
        }
        .into_iter()
        .map(|(_, b)| b)
        .collect();
        let bleed = doc.settings.bleed;
        let overlaps = |a: Rect, b: Rect| a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0;
        for ab in doc.artboards().iter().filter(|ab| !exclude_artboards.contains(&ab.id)) {
            let r = convert::rect(ab.rect);
            if overlaps(r, visible) {
                out.push(r);
            }
            let bled = Rect::new(
                r.x0 - bleed.left,
                r.y0 - bleed.top,
                r.x1 + bleed.right,
                r.y1 + bleed.bottom,
            );
            if bled != r && overlaps(bled, visible) {
                out.push(bled);
            }
        }
        out
    }

    /// Ruler-guide positions as alignment targets, one axis each — empty
    /// while guides are hidden (View ▸ Hide Guides), same as Illustrator
    /// stops snapping to guides you can't see.
    fn sg_guide_targets(&self) -> Vec<(Axis, f64)> {
        if self.guides_hidden {
            return Vec::new();
        }
        self.doc
            .editor
            .document()
            .guides()
            .iter()
            .map(|g| match g.orient {
                amalith_core::GuideOrient::Vertical => (Axis::X, g.pos),
                amalith_core::GuideOrient::Horizontal => (Axis::Y, g.pos),
            })
            .collect()
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

    /// [`Self::sg_construction_snap`], but the candidate angle list is the
    /// user's own construction angles *plus* every nearby visible object's
    /// current rotation angle — so Rotate/Reflect can snap to line up with
    /// another object's existing orientation, not just a fixed preset
    /// list. `exclude` keeps the object(s) actually being transformed out
    /// of their own candidate list (their pre-drag angle would otherwise
    /// always be a trivial, uninteresting match right at the start of the
    /// drag).
    pub(in crate::app) fn sg_construction_snap_with_object_angles(
        &self,
        from: Point,
        p: Point,
        exclude: &[ObjectId],
    ) -> (Point, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled || !self.settings.sg_construction_guides {
            return (p, None);
        }
        let mut angles = self.settings.sg_angles.to_vec();
        angles.extend(self.nearby_rotation_angles(exclude));
        construction_snap(from, p, &angles, self.sg_tolerance_doc()).map_or((p, None), |(p, h)| (p, Some(h)))
    }

    /// Every nearby visible object's own current rotation angle (degrees),
    /// decomposed from its world transform — scoped to the isolation
    /// group when isolated, same as every other alignment-candidate scan.
    /// A near-zero-scale (degenerate) transform contributes nothing: its
    /// rotation isn't meaningfully defined.
    fn nearby_rotation_angles(&self, exclude: &[ObjectId]) -> Vec<f64> {
        let doc = self.doc.editor.document();
        let visible = self.visible_doc_rect();
        let candidates = match self.isolation_root() {
            Some(root) => select::bounds_within(doc, root, visible, exclude),
            None => select::visible_top_level_bounds(doc, visible, exclude),
        };
        candidates
            .into_iter()
            .filter_map(|(id, _)| {
                let m = convert::affine(doc.world_transform(id)).as_coeffs();
                let scale = (m[0] * m[0] + m[1] * m[1]).sqrt();
                (scale > 1e-9).then(|| (-m[1]).atan2(m[0]).to_degrees())
            })
            .collect()
    }

    /// [`Self::sg_construction_snap`]'s Shear-tool counterpart: `raw_deg`
    /// is the already-clamped `(-90°, 90°)` shear angle; `pivot` is only
    /// used to place the resulting reference ray. The tolerance is a fixed
    /// couple of degrees rather than a doc-space/zoom-derived one — an
    /// angle match isn't a spatial distance, the same way Shift's own 45°
    /// lock here is zoom-independent too.
    pub(in crate::app) fn sg_shear_snap(&self, pivot: Point, raw_deg: f64) -> (f64, Option<SmartGuideHit>) {
        if !self.settings.smart_guides_enabled || !self.settings.sg_construction_guides {
            return (raw_deg, None);
        }
        const TOL_DEG: f64 = 1.5;
        match shear_angle_snap(raw_deg, &self.settings.sg_angles, TOL_DEG) {
            Some(degrees) => (degrees, Some(SmartGuideHit::ConstructionAngle { from: pivot, degrees })),
            None => (raw_deg, None),
        }
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
                let mut targets = self.sg_alignment_bounds(&[], &[]);
                targets.extend(
                    self.pen
                        .iter()
                        .map(|a| Rect::new(a.point.x, a.point.y, a.point.x, a.point.y)),
                );
                let (d, hit) = move_snap(
                    Rect::new(p.x, p.y, p.x, p.y),
                    &targets,
                    tol,
                    true,
                    false,
                    &self.sg_guide_targets(),
                );
                if hit.is_some() {
                    return (p + d, hit);
                }
            }
        }
        let (p2, hit2) = from.map_or((p, None), |from| self.sg_construction_snap(from, p));
        if hit2.is_some() {
            return (p2, hit2);
        }
        // Nothing above matched — Snap to Grid/Pixel still get the final
        // say, same as every other point-placement path.
        (self.sg_grid_pixel_fallback(p), None)
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
            Drag::MoveHandle { object, anchor, last_doc, .. } => {
                let anchor_pt = anchors::anchors_of(self.doc.editor.document(), object)
                    .into_iter()
                    .find(|(n, _)| *n == anchor)
                    .map(|(_, p)| p)?;
                let d = last_doc - anchor_pt;
                Some(format!("L: {:.1} pt  {:.1}°", d.hypot(), (-d.y).atan2(d.x).to_degrees()))
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
    fn shear_angle_snap_matches_a_preset_folded_into_the_half_turn_domain() {
        // 0/45/90/135 fold to 0/45/90/-45 within (-90, 90] — 90 and -45
        // (135 folded) are both directly reachable presets in that domain.
        assert_eq!(shear_angle_snap(0.3, &[0., 45., 90., 135.], 1.5), Some(0.));
        assert_eq!(shear_angle_snap(44.6, &[0., 45., 90., 135.], 1.5), Some(45.));
        assert_eq!(shear_angle_snap(-44.7, &[0., 45., 90., 135.], 1.5), Some(-45.));
        // Outside tolerance of every preset: no match.
        assert_eq!(shear_angle_snap(20.0, &[0., 45.], 1.5), None);
        // Non-finite presets are ignored, not propagated as NaN matches.
        assert_eq!(shear_angle_snap(0.0, &[f64::NAN, f64::INFINITY, 0.0], 1.5), Some(0.));
    }

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
            &[],
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
                let (d, hit) = move_snap(map(moved), &candidates.map(map), 4., false, true, &[]);
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
            move_snap(moved, &candidates, 4., true, true, &[]).0,
            Vec2::new(-1., 0.)
        );
    }

    #[test]
    fn spacing_centers_between_neighbors_but_ignores_unrelated_rows() {
        let neighbors = [Rect::new(0., 0., 10., 10.), Rect::new(60., 0., 70., 10.)];
        assert_eq!(
            move_snap(Rect::new(29., 0., 39., 10.), &neighbors, 4., false, true, &[]).0,
            Vec2::new(1., 0.)
        );
        assert!(
            move_snap(Rect::new(29., 100., 39., 110.), &neighbors, 4., false, true, &[])
                .1
                .is_none()
        );
    }

    #[test]
    fn ruler_guides_align_on_their_own_axis_only_and_stay_out_of_spacing() {
        let moved = Rect::new(1., 1., 11., 11.);
        // A vertical guide at x=0 should pull the moved rect's left edge
        // onto it; a "guide" is single-axis, so it must never also offer
        // itself as a Y-axis target.
        let (d, hit) = move_snap(moved, &[], 4., true, false, &[(Axis::X, 0.)]);
        assert_eq!(d, Vec2::new(-1., 0.));
        assert!(matches!(
            hit,
            Some(SmartGuideHit::Multiple(h))
                if matches!(h.as_slice(), [SmartGuideHit::AlignEdge { axis: Axis::X, value }] if *value == 0.)
        ));

        // Neither axis matches within tolerance — no hit, no NaN/garbage
        // from treating the guide's infinite extent as real geometry.
        let (d, hit) = move_snap(moved, &[], 4., true, false, &[(Axis::Y, 500.)]);
        assert_eq!(d, Vec2::ZERO);
        assert!(hit.is_none());

        // Spacing must never see a guide as a zero-width neighbor object.
        let (d, hit) = move_snap(moved, &[], 4., false, true, &[(Axis::X, 0.), (Axis::Y, 1.)]);
        assert_eq!(d, Vec2::ZERO);
        assert!(hit.is_none());
    }

    #[test]
    fn nearest_grid_point_rounds_to_the_nearest_intersection() {
        assert_eq!(nearest_grid_point(Point::new(23., 41.), 10.), Point::new(20., 40.));
        assert_eq!(nearest_grid_point(Point::new(-23., -41.), 10.), Point::new(-20., -40.));
        assert_eq!(nearest_grid_point(Point::new(25., 0.), 10.), Point::new(30., 0.), "exact half rounds away from zero, matching f64::round");
        // Malformed spacing is a no-op, not a divide-by-zero/NaN result.
        assert_eq!(nearest_grid_point(Point::new(23., 41.), 0.), Point::new(23., 41.));
        assert_eq!(nearest_grid_point(Point::new(23., 41.), -5.), Point::new(23., 41.));
        assert_eq!(nearest_grid_point(Point::new(23., 41.), f64::NAN), Point::new(23., 41.));
    }

    #[test]
    fn nearest_pixel_point_rounds_to_the_nearest_whole_unit() {
        assert_eq!(nearest_pixel_point(Point::new(23.4, 41.6)), Point::new(23., 42.));
        assert_eq!(nearest_pixel_point(Point::new(-23.4, -41.6)), Point::new(-23., -42.));
    }

    #[test]
    fn scale_dimension_guides_matches_a_candidates_size_from_the_fixed_corner() {
        // Dragging the Se handle: Nw stays fixed, so the target x/y are
        // measured forward (+) from bounds.x0/y0.
        let bounds = Rect::new(0., 0., 10., 10.);
        let candidates = [Rect::new(100., 100., 140., 130.)]; // width 40, height 30
        let guides = scale_dimension_guides(Handle::Se, bounds, &candidates);
        assert!(guides.contains(&(Axis::X, 40.)), "matches the candidate's width from the fixed left edge");
        assert!(guides.contains(&(Axis::Y, 30.)), "matches the candidate's height from the fixed top edge");
        // Square/circle self-cue: this shape's own other dimension (both
        // 10 here, so both self-cues coincide with each other but not
        // with the candidate's).
        assert!(guides.contains(&(Axis::X, 10.)));
        assert!(guides.contains(&(Axis::Y, 10.)));
    }

    #[test]
    fn scale_dimension_guides_flips_sign_for_the_opposite_fixed_corner() {
        // Dragging the Nw handle: Se (x1,y1) stays fixed, so growing the
        // shape means moving the dragged corner *backward* (-) from it.
        let bounds = Rect::new(0., 0., 10., 10.);
        let candidates = [Rect::new(100., 100., 140., 130.)];
        let guides = scale_dimension_guides(Handle::Nw, bounds, &candidates);
        assert!(guides.contains(&(Axis::X, 10. - 40.)));
        assert!(guides.contains(&(Axis::Y, 10. - 30.)));
    }

    #[test]
    fn scale_dimension_guides_edge_handle_only_offers_its_own_axis() {
        // An E handle only moves x — no Y-axis guide should ever appear,
        // matching an edge handle's own axis restriction elsewhere.
        let bounds = Rect::new(0., 0., 10., 20.);
        let candidates = [Rect::new(100., 100., 140., 130.)];
        let guides = scale_dimension_guides(Handle::E, bounds, &candidates);
        assert!(guides.iter().all(|(axis, _)| *axis == Axis::X));
        // Square cue: width should be able to match this shape's own
        // (unchanged) height of 20, landing at x = 0 + 20 = 20.
        assert!(guides.contains(&(Axis::X, 20.)));
    }

    #[test]
    fn mask_axis_hit_drops_the_inactive_axis_so_an_edge_handle_cant_drift_sideways() {
        let both = Some(SmartGuideHit::Multiple(vec![
            SmartGuideHit::AlignEdge { axis: Axis::X, value: 5. },
            SmartGuideHit::AlignEdge { axis: Axis::Y, value: 9. },
        ]));
        // An E/W edge handle only moves X — the Y match must disappear
        // entirely, not just go unapplied, so no misleading line is drawn.
        assert!(matches!(
            mask_axis_hit(both.clone(), true, false),
            Some(SmartGuideHit::AlignEdge { axis: Axis::X, value }) if value == 5.
        ));
        // An N/S edge handle only moves Y.
        assert!(matches!(
            mask_axis_hit(both.clone(), false, true),
            Some(SmartGuideHit::AlignEdge { axis: Axis::Y, value }) if value == 9.
        ));
        // A corner handle keeps both.
        assert!(matches!(mask_axis_hit(both, true, true), Some(SmartGuideHit::Multiple(h)) if h.len() == 2));
        // Neither axis active (shouldn't happen for a real handle, but must not panic): nothing survives.
        let both = Some(SmartGuideHit::Multiple(vec![
            SmartGuideHit::AlignEdge { axis: Axis::X, value: 5. },
            SmartGuideHit::AlignEdge { axis: Axis::Y, value: 9. },
        ]));
        assert!(mask_axis_hit(both, false, false).is_none());
        // Non-alignment hits and "no hit" pass straight through.
        assert!(mask_axis_hit(None, true, true).is_none());
        let spacing = Some(SmartGuideHit::Spacing { gap_a: (Point::ZERO, Point::ZERO), gap_b: (Point::ZERO, Point::ZERO), px: 4. });
        assert!(matches!(mask_axis_hit(spacing, false, false), Some(SmartGuideHit::Spacing { .. })));
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
