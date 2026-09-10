//! Join tool: Illustrator-parity endpoint-connect ("drag one open path's
//! free endpoint onto another's") and overlap-trim ("scrub across two
//! open paths' terminal segments where they cross") in one drag-based
//! canvas tool. Canvas-wide (unlike Width), not scoped to the current
//! selection — Illustrator's own Join tool works on whatever you drag
//! onto, selected or not.

use super::*;
use vello::kurbo::{CubicBez, Line, ParamCurve, ParamCurveNearest, PathSeg};

pub(in crate::app) const JOIN_ENDPOINT_GRAB: f64 = 8.0;
/// Screen-px search radius, around the pointer, for a *second* open
/// endpoint once a drag is armed — generous, matching how forgiving
/// Illustrator's own scrub feels.
pub(in crate::app) const JOIN_OVERLAP_SEARCH: f64 = 40.0;

/// One free endpoint of an open subpath, in screen space — the Join
/// tool's universe of hit-test targets.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct JoinCandidate {
    pub object: ObjectId,
    pub subpath: usize,
    /// The free end is that subpath's last anchor (`true`) or first
    /// anchor (`false`).
    pub at_end: bool,
    /// Flat anchor ordinal, for `Command::JoinAnchors`.
    pub anchor: usize,
    pub point: Point,
}

/// What a live `Drag::JoinScrub` is currently over.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) enum JoinTarget {
    /// Commits as `Command::JoinAnchors`.
    Endpoint { object: ObjectId, anchor: usize, point: Point },
    /// Commits as `Command::TrimAndJoinPaths`. `point` is the shared
    /// screen-space intersection, for the live preview dot.
    Overlap { self_trim: JoinTrim, other_trim: JoinTrim, point: Point },
}

impl App {
    /// Every open-path free endpoint in the visible document, in screen
    /// space.
    pub(in crate::app) fn join_candidates(&self) -> Vec<JoinCandidate> {
        let doc = self.doc.editor.document();
        let to_screen = self.doc.view.to_screen();
        let mut out = Vec::new();
        for id in anchors::path_leaves(doc) {
            let Some(pd) = doc.object(id).and_then(|o| o.kind.path_data()) else { continue };
            let m = to_screen * convert::affine(doc.world_transform(id));
            let mut offset = 0usize;
            for (si, sp) in pd.subpaths().iter().enumerate() {
                let n = sp.anchors.len();
                if !sp.closed && n >= 1 {
                    let ends: &[(usize, bool)] =
                        if n == 1 { &[(0, false)] } else { &[(0, false), (n - 1, true)] };
                    for &(local, at_end) in ends {
                        out.push(JoinCandidate {
                            object: id,
                            subpath: si,
                            at_end,
                            anchor: offset + local,
                            point: m * convert::point(sp.anchors[local].point),
                        });
                    }
                }
                offset += n;
            }
        }
        out
    }

    /// Nearest candidate to `p` (screen px) within `radius`, excluding one
    /// anchor (a live drag's own origin).
    fn join_nearest(
        &self,
        p: Point,
        radius: f64,
        exclude: Option<(ObjectId, usize)>,
    ) -> Option<JoinCandidate> {
        self.join_candidates()
            .into_iter()
            .filter(|c| Some((c.object, c.anchor)) != exclude)
            .map(|c| {
                let d = (c.point - p).hypot();
                (c, d)
            })
            .filter(|&(_, d)| d <= radius)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(c, _)| c)
    }

    /// Press: arms the drag from the nearest open endpoint within grab
    /// radius. Returns whether the press was handled.
    pub(in crate::app) fn join_tool_press(&mut self) -> bool {
        let Some(hit) = self.join_nearest(self.pointer, JOIN_ENDPOINT_GRAB, None) else {
            return false;
        };
        self.drag = Drag::JoinScrub {
            from: (hit.object, hit.anchor),
            path: vec![self.doc_point(self.pointer)],
            target: None,
        };
        self.request_main_redraw();
        true
    }

    /// Live move: recomputes what the current pointer is over.
    pub(in crate::app) fn join_tool_move(&self, from: (ObjectId, usize)) -> Option<JoinTarget> {
        if let Some(hit) = self.join_nearest(self.pointer, JOIN_ENDPOINT_GRAB, Some(from)) {
            return Some(JoinTarget::Endpoint { object: hit.object, anchor: hit.anchor, point: hit.point });
        }
        self.join_overlap_target(from)
    }

    /// Looks for a crossing between `from`'s own terminal segment and the
    /// terminal segment of whichever *other* open endpoint is nearest the
    /// current pointer within `JOIN_OVERLAP_SEARCH` — deliberately scoped
    /// to just those two candidate segments, not a general intersection
    /// search.
    fn join_overlap_target(&self, from: (ObjectId, usize)) -> Option<JoinTarget> {
        let from_c = self.join_candidates().into_iter().find(|c| (c.object, c.anchor) == from)?;
        let other = self
            .join_nearest(self.pointer, JOIN_OVERLAP_SEARCH, Some(from))
            .filter(|c| c.object != from_c.object || c.subpath != from_c.subpath)?;

        let doc = self.doc.editor.document();
        let to_screen = self.doc.view.to_screen();
        let from_m = to_screen * convert::affine(doc.world_transform(from_c.object));
        let other_m = to_screen * convert::affine(doc.world_transform(other.object));
        let from_seg = terminal_seg(doc, from_c.object, from_c.subpath, from_c.at_end)?;
        let other_seg = terminal_seg(doc, other.object, other.subpath, other.at_end)?;

        // Sample each (short, bounded) terminal segment to a small screen-
        // space polyline, find where the two cross, then refine onto the
        // real curves with `ParamCurveNearest`.
        const STEPS: usize = 16;
        let sample = |seg: &PathSeg, m: Affine| -> Vec<Point> {
            (0..=STEPS).map(|i| m * seg.eval(i as f64 / STEPS as f64)).collect()
        };
        let fp = sample(&from_seg, from_m);
        let op = sample(&other_seg, other_m);

        // `segment_intersection` is `amalith_core` geometry (kurbo 0.11);
        // everything here is screen-space vello kurbo (0.13) — cross the
        // boundary at this one call, like `convert` does everywhere else.
        let mut hit: Option<Point> = None;
        'search: for w1 in fp.windows(2) {
            for w2 in op.windows(2) {
                if let Some(p) = amalith_core::geom::segment_intersection(
                    convert::point_to_core(w1[0]),
                    convert::point_to_core(w1[1]),
                    convert::point_to_core(w2[0]),
                    convert::point_to_core(w2[1]),
                ) {
                    hit = Some(convert::point(p));
                    break 'search;
                }
            }
        }
        let screen_p = hit?;

        // `nearest()`'s own `t` (0..=1 along the segment's stored anchor-
        // index order, low to high) is already exactly `trim_to_split`'s
        // own convention — no direction flip needed regardless of
        // `at_end`, since `terminal_seg` always builds its curve in that
        // same stored order.
        let near_self = from_seg.nearest(from_m.inverse() * screen_p, 0.05);
        let near_other = other_seg.nearest(other_m.inverse() * screen_p, 0.05);

        Some(JoinTarget::Overlap {
            self_trim: JoinTrim { object: from_c.object, subpath: from_c.subpath, at_end: from_c.at_end, t: near_self.t },
            other_trim: JoinTrim { object: other.object, subpath: other.subpath, at_end: other.at_end, t: near_other.t },
            point: screen_p,
        })
    }

    /// Commits the finished drag as one undo step. No-op if `target` is
    /// `None` (dragged to nowhere joinable).
    pub(in crate::app) fn commit_join(&mut self, from: (ObjectId, usize), target: Option<JoinTarget>) {
        let _ = match target {
            Some(JoinTarget::Endpoint { object, anchor, .. }) => self
                .doc.editor
                .execute(Command::JoinAnchors { anchor_a: from, anchor_b: (object, anchor) }),
            Some(JoinTarget::Overlap { self_trim, other_trim, .. }) => self
                .doc.editor
                .execute(Command::TrimAndJoinPaths { a: self_trim, b: other_trim }),
            None => return,
        };
        // Ordinals are stale either way after a join.
        self.doc.anchor_sel.clear();
        self.prune_selection();
        self.request_main_redraw();
    }
}

/// `id`'s terminal segment at `subpath`/`at_end` (the last segment if
/// `at_end`, else the first) as a single curve in `id`'s own local space,
/// built in the same low-to-high anchor-index order
/// `insert_anchor`/`trim_to_split` expect regardless of `at_end`.
fn terminal_seg(doc: &amalith_core::Document, id: ObjectId, subpath: usize, at_end: bool) -> Option<PathSeg> {
    let pd = doc.object(id)?.kind.path_data()?;
    let sp = pd.subpaths().get(subpath)?;
    let n = sp.anchors.len();
    if n < 2 {
        return None;
    }
    let (a, b) = if at_end { (sp.anchors[n - 2], sp.anchors[n - 1]) } else { (sp.anchors[0], sp.anchors[1]) };
    let (pa, pb) = (convert::point(a.point), convert::point(b.point));
    Some(match (a.handle_out, b.handle_in) {
        (None, None) => PathSeg::Line(Line::new(pa, pb)),
        (ha, hb) => PathSeg::Cubic(CubicBez::new(
            pa,
            ha.map_or(pa, convert::point),
            hb.map_or(pb, convert::point),
            pb,
        )),
    })
}
