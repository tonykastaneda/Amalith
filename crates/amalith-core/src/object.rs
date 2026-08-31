//! Objects: the drawable/groupable content of a document.
//!
//! An [`Object`] is Amalith's analogue of Inkscape's `SPItem` — but unlike
//! `SPItem`, it is a plain data value with no XML repr shadowing it. There
//! is exactly one tree (see [`crate::document`] for how objects attach to
//! layers and groups), not an XML repr tree plus a parallel item tree kept
//! in sync. That sync problem is a large fraction of `SPObject`'s
//! complexity in Inkscape; Amalith has no reason to take it on, since the
//! native format is not "serialized DOM" (see `DESIGN.md`).
use crate::appearance::Appearance;
use crate::geom::{Affine, BezPath, PathEl, Point, Rect};
use crate::ids::{AssetId, LayerId, ObjectId};
use serde::{Deserialize, Serialize};

/// Where an object lives in the ownership tree.
///
/// Every object has exactly one parent: either a layer (top-level within
/// that layer) or another object that is a [`ObjectKind::Group`]. This
/// field is a cache of the edge already recorded in the parent's
/// child-order list (`Layer::children` or the group's own children); it
/// exists so callers can answer "who owns this object" and "what is this
/// object's world transform" in O(depth) instead of a full tree scan.
/// Kept in sync exclusively by the raw mutation methods on [`crate::Document`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObjectParent {
    Layer(LayerId),
    Group(ObjectId),
}

/// How an anchor's two bezier handles are kept related while editing.
///
/// This is *editing intent*, not geometry — [`subpaths_to_bezpath`]
/// ignores it. Tools consult it to decide whether moving one handle
/// should drag its partner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HandleMode {
    /// Handles move independently — Illustrator's "corner point".
    #[default]
    Corner,
    /// Handles stay 180° opposed; lengths independent — a "smooth point".
    Smooth,
    /// Handles stay 180° opposed *and* equal length — a "symmetric point".
    Symmetric,
}

/// One anchor of a [`Subpath`], with optional cubic bezier handles, in the
/// object's local coordinate space (before `Object::transform`).
///
/// Handle positions are absolute (not relative to `point`), matching how
/// [`crate::geom::translate_anchor`] already moves an anchor and its
/// controls together. `None` on a side means the segment on that side is a
/// straight line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub point: Point,
    pub handle_in: Option<Point>,
    pub handle_out: Option<Point>,
    #[serde(default)]
    pub mode: HandleMode,
}

impl Anchor {
    /// A plain corner anchor with no handles.
    pub fn corner(point: Point) -> Self {
        Self {
            point,
            handle_in: None,
            handle_out: None,
            mode: HandleMode::Corner,
        }
    }
}

/// A single open or closed contour: an ordered run of [`Anchor`]s.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Subpath {
    pub anchors: Vec<Anchor>,
    pub closed: bool,
}

/// A path built from Bezier segments, in the object's local coordinate
/// space (before `Object::transform` is applied).
///
/// `subpaths` is the editable truth (anchors + handles + per-anchor
/// [`HandleMode`]); `geometry` is a flattened kurbo cache kept in sync on
/// every mutation. Rendering, hit-testing, bounds and SVG export all read
/// `geometry` — treat it as read-only and mutate through
/// [`Self::edit_subpaths`] or the constructors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "PathDataRepr")]
pub struct PathData {
    subpaths: Vec<Subpath>,
    pub geometry: BezPath,
}

/// On-disk shape of [`PathData`]. Files written before the anchor model
/// carried only `geometry`; `subpaths` is derived from it on load. Newer
/// files carry both and `subpaths` wins.
#[derive(Deserialize)]
#[serde(untagged)]
enum PathDataRepr {
    /// Newer files: anchors are authoritative. Any `geometry` key present
    /// alongside is ignored and rebuilt.
    Structured { subpaths: Vec<Subpath> },
    /// Pre-anchor-model files: only a flat `geometry` path.
    Legacy { geometry: BezPath },
}

impl From<PathDataRepr> for PathData {
    fn from(repr: PathDataRepr) -> Self {
        match repr {
            PathDataRepr::Structured { subpaths } => Self::from_subpaths(subpaths),
            PathDataRepr::Legacy { geometry } => Self::from_bezpath(geometry),
        }
    }
}

/// Flattens `subpaths` to a kurbo [`BezPath`]. The closing edge of a
/// closed contour is left to `close_path()` when it is straight, and
/// emitted explicitly when it is curved — so our native rectangle /
/// ellipse constructors round-trip byte-identically.
pub fn subpaths_to_bezpath(subpaths: &[Subpath]) -> BezPath {
    let mut out = BezPath::new();
    for sp in subpaths {
        let n = sp.anchors.len();
        if n == 0 {
            continue;
        }
        out.move_to(sp.anchors[0].point);
        let segments = if sp.closed { n } else { n - 1 };
        for i in 0..segments {
            let a = &sp.anchors[i];
            let b = &sp.anchors[(i + 1) % n];
            match (a.handle_out, b.handle_in) {
                (None, None) => {
                    // Let close_path() draw the straight wrap edge.
                    if !(sp.closed && i == n - 1) {
                        out.line_to(b.point);
                    }
                }
                _ => out.curve_to(
                    a.handle_out.unwrap_or(a.point),
                    b.handle_in.unwrap_or(b.point),
                    b.point,
                ),
            }
        }
        if sp.closed {
            out.close_path();
        }
    }
    out
}

/// Derives an anchor model from an arbitrary kurbo [`BezPath`]. Quadratic
/// segments are degree-elevated to cubics. Per-anchor [`HandleMode`] is
/// inferred: an anchor whose two handles are ~colinear becomes `Smooth`
/// (or `Symmetric` when their lengths also match), otherwise `Corner`.
pub fn bezpath_to_subpaths(path: &BezPath) -> Vec<Subpath> {
    const EPS: f64 = 1e-6;
    let mut out: Vec<Subpath> = Vec::new();
    let mut cur: Option<Subpath> = None;
    for el in path.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                if let Some(sp) = cur.take() {
                    out.push(sp);
                }
                cur = Some(Subpath {
                    anchors: vec![Anchor::corner(p)],
                    closed: false,
                });
            }
            PathEl::LineTo(p) => {
                if let Some(sp) = cur.as_mut() {
                    sp.anchors.push(Anchor::corner(p));
                }
            }
            PathEl::QuadTo(c, p) => {
                if let Some(sp) = cur.as_mut() {
                    if let Some(a) = sp.anchors.last_mut() {
                        a.handle_out = Some(a.point + (c - a.point) * (2.0 / 3.0));
                    }
                    sp.anchors.push(Anchor {
                        point: p,
                        handle_in: Some(p + (c - p) * (2.0 / 3.0)),
                        handle_out: None,
                        mode: HandleMode::Corner,
                    });
                }
            }
            PathEl::CurveTo(p1, p2, p) => {
                if let Some(sp) = cur.as_mut() {
                    if let Some(a) = sp.anchors.last_mut() {
                        a.handle_out = Some(p1);
                    }
                    sp.anchors.push(Anchor {
                        point: p,
                        handle_in: Some(p2),
                        handle_out: None,
                        mode: HandleMode::Corner,
                    });
                }
            }
            PathEl::ClosePath => {
                if let Some(sp) = cur.as_mut() {
                    sp.closed = true;
                    // A trailing anchor coincident with the start is the
                    // seam of a merged closed contour — fold it back in.
                    if sp.anchors.len() > 1 {
                        let tail = *sp.anchors.last().unwrap();
                        if (tail.point - sp.anchors[0].point).hypot() < EPS {
                            sp.anchors.pop();
                            sp.anchors[0].handle_in = tail.handle_in;
                        }
                    }
                }
            }
        }
    }
    if let Some(sp) = cur.take() {
        out.push(sp);
    }
    for sp in &mut out {
        let n = sp.anchors.len();
        for i in 0..n {
            let a = sp.anchors[i];
            let (Some(hin), Some(hout)) = (a.handle_in, a.handle_out) else {
                continue;
            };
            let din = a.point - hin;
            let dout = hout - a.point;
            let (lin, lout) = (din.hypot(), dout.hypot());
            if lin < EPS || lout < EPS {
                continue;
            }
            let cross = din.x * dout.y - din.y * dout.x;
            let dot = din.dot(dout);
            if cross.abs() < EPS * lin * lout && dot > 0.0 {
                sp.anchors[i].mode = if (lin - lout).abs() < EPS * lin.max(lout) {
                    HandleMode::Symmetric
                } else {
                    HandleMode::Smooth
                };
            }
        }
    }
    out
}

impl PathData {
    /// Builds a path from an anchor model, deriving the `geometry` cache.
    pub fn from_subpaths(subpaths: Vec<Subpath>) -> Self {
        let geometry = subpaths_to_bezpath(&subpaths);
        Self { subpaths, geometry }
    }

    /// Wraps an existing kurbo path, deriving the anchor model from it.
    /// The `geometry` is kept verbatim (not re-flattened) so callers that
    /// only render / export see no change.
    pub fn from_bezpath(geometry: BezPath) -> Self {
        let subpaths = bezpath_to_subpaths(&geometry);
        Self { subpaths, geometry }
    }

    /// The editable anchor model.
    pub fn subpaths(&self) -> &[Subpath] {
        &self.subpaths
    }

    /// Mutates the anchor model, then rebuilds the `geometry` cache.
    pub fn edit_subpaths(&mut self, f: impl FnOnce(&mut Vec<Subpath>)) {
        f(&mut self.subpaths);
        self.geometry = subpaths_to_bezpath(&self.subpaths);
    }


    /// Builds a closed axis-aligned rectangle path in local space.
    ///
    /// This is the minimum path construction needed for Milestone 0.1
    /// ("press M, drag rectangle"): a rectangle is just a path with four
    /// corner points and a close segment, not a distinct primitive type.
    pub fn rectangle(rect: Rect) -> Self {
        let mut path = BezPath::new();
        path.move_to((rect.x0, rect.y0));
        path.line_to((rect.x1, rect.y0));
        path.line_to((rect.x1, rect.y1));
        path.line_to((rect.x0, rect.y1));
        path.close_path();
        Self::from_bezpath(path)
    }

    /// Builds a closed ellipse path in local space using four cubic Bézier
    /// segments (the standard kappa approximation).
    pub fn ellipse(rect: Rect) -> Self {
        let cx = (rect.x0 + rect.x1) * 0.5;
        let cy = (rect.y0 + rect.y1) * 0.5;
        let rx = (rect.x1 - rect.x0) * 0.5;
        let ry = (rect.y1 - rect.y0) * 0.5;
        let k = 0.552_284_749_830_793_6;
        let kx = rx * k;
        let ky = ry * k;
        let mut path = BezPath::new();
        path.move_to((cx + rx, cy));
        path.curve_to((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry));
        path.curve_to((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy));
        path.curve_to((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry));
        path.curve_to((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy));
        path.close_path();
        Self::from_bezpath(path)
    }

    /// A closed straight-sided path through `points`.
    pub fn polygon(points: &[crate::geom::Point]) -> Self {
        let mut path = BezPath::new();
        if let Some(first) = points.first() {
            path.move_to((first.x, first.y));
            for point in &points[1..] {
                path.line_to((point.x, point.y));
            }
            path.close_path();
        }
        Self::from_bezpath(path)
    }

    /// An open straight-sided path through `points`.
    ///
    /// Unlike [`Self::polygon`], this deliberately leaves the final segment
    /// open. It is the native representation used by the Pen tool for an
    /// unfinished/open path, rather than a closed shape with an invisible
    /// closing edge.
    pub fn polyline(points: &[crate::geom::Point]) -> Self {
        let mut path = BezPath::new();
        if let Some(first) = points.first() {
            path.move_to((first.x, first.y));
            for point in &points[1..] {
                path.line_to((point.x, point.y));
            }
        }
        Self::from_bezpath(path)
    }

    /// A rounded rectangle with cubic Bézier corners.
    pub fn rounded_rectangle(rect: Rect, radius: f64) -> Self {
        let radius = radius
            .min(rect.width().abs() * 0.5)
            .min(rect.height().abs() * 0.5);
        let k = radius * 0.552_284_749_830_793_6;
        let mut path = BezPath::new();
        path.move_to((rect.x0 + radius, rect.y0));
        path.line_to((rect.x1 - radius, rect.y0));
        path.curve_to(
            (rect.x1 - radius + k, rect.y0),
            (rect.x1, rect.y0 + radius - k),
            (rect.x1, rect.y0 + radius),
        );
        path.line_to((rect.x1, rect.y1 - radius));
        path.curve_to(
            (rect.x1, rect.y1 - radius + k),
            (rect.x1 - radius + k, rect.y1),
            (rect.x1 - radius, rect.y1),
        );
        path.line_to((rect.x0 + radius, rect.y1));
        path.curve_to(
            (rect.x0 + radius - k, rect.y1),
            (rect.x0, rect.y1 - radius + k),
            (rect.x0, rect.y1 - radius),
        );
        path.line_to((rect.x0, rect.y0 + radius));
        path.curve_to(
            (rect.x0, rect.y0 + radius - k),
            (rect.x0 + radius - k, rect.y0),
            (rect.x0 + radius, rect.y0),
        );
        path.close_path();
        Self::from_bezpath(path)
    }

    pub fn local_bounds(&self) -> Rect {
        crate::geom::bez_path_bounds(&self.geometry)
    }

    /// Returns a polyline approximation of every subpath in local space.
    pub fn flattened_points(&self, tolerance: f64) -> Vec<Vec<crate::geom::Point>> {
        crate::geom::flattened_points(&self.geometry, tolerance)
    }
}

/// An ordered collection of child objects, composited together.
///
/// Children are stored as an ordered `Vec<ObjectId>`; index 0 is the
/// bottom of the group's local stacking order, matching
/// [`crate::Layer::children`]'s convention (see `document.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GroupData {
    pub children: Vec<ObjectId>,
}

/// One or more subpaths treated as a single fillable shape (even/odd or
/// nonzero winding across all subpaths) — e.g. a letter "O" as one object.
///
/// Stub: geometry only, enough to have real bounds; fill-rule and boolean
/// composition of subpaths come with the Pathfinder/boolean-ops subsystem.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CompoundPathData {
    pub subpaths: Vec<BezPath>,
}

impl CompoundPathData {
    pub fn local_bounds(&self) -> Option<Rect> {
        self.subpaths
            .iter()
            .map(crate::geom::bez_path_bounds)
            .reduce(|a, b| a.union(b))
    }
}

/// Point type auto-sizes to its content and only wraps on explicit
/// newlines. Area type wraps to `width`; `height` `None` grows downward.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TextKind {
    Point,
    Area { width: f64, height: Option<f64> },
}

/// Horizontal alignment of the text block against its anchor / box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
    Justify,
}

/// OpenType vertical position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextPosition {
    #[default]
    Normal,
    Superscript,
    Subscript,
}

/// One text object's typography. v1 applies to the whole object — there are
/// no per-character runs yet. Fonts are referenced portably (family name +
/// weight + italic) and resolved against the system font set on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub family: String,
    /// CSS weight, 100..=900.
    pub weight: u16,
    pub italic: bool,
    /// Font size in local px (= pt at 1:1).
    pub size: f64,
    /// Line height in px. `None` = auto (≈ 1.2 × size).
    pub leading: Option<f64>,
    /// Tracking, in thousandths of an em (Illustrator's unit).
    pub tracking: f64,
    pub underline: bool,
    pub strikethrough: bool,
    pub small_caps: bool,
    pub position: TextPosition,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            family: "Helvetica".into(),
            weight: 400,
            italic: false,
            size: 24.0,
            leading: None,
            tracking: 0.0,
            underline: false,
            strikethrough: false,
            small_caps: false,
            position: TextPosition::Normal,
        }
    }
}

/// A text object. `local_bounds` is recomputed from the laid-out text by
/// the shell after every content / style / box change (the core has no
/// typography engine).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextData {
    pub content: String,
    pub kind: TextKind,
    pub style: TextStyle,
    pub align: TextAlign,
    pub local_bounds: Rect,
}

impl Default for TextData {
    fn default() -> Self {
        Self {
            content: String::new(),
            kind: TextKind::Point,
            style: TextStyle::default(),
            align: TextAlign::Start,
            local_bounds: Rect::ZERO,
        }
    }
}

/// Stub image object: references a (linked or embedded) [`AssetId`] plus an
/// explicit local bounds box, standing in for real raster decode/placement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageData {
    pub asset: AssetId,
    pub local_bounds: Rect,
}

/// Stub symbol instance: references a definition object by [`ObjectId`]
/// (the definition itself is an ordinary object, typically a group, held
/// outside the visible layer tree) plus an explicit local bounds box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolData {
    pub definition: ObjectId,
    pub local_bounds: Rect,
}

/// The kind-specific payload of an [`Object`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObjectKind {
    Path(PathData),
    Group(GroupData),
    Text(TextData),
    Image(ImageData),
    CompoundPath(CompoundPathData),
    Symbol(SymbolData),
}

impl ObjectKind {
    /// Geometry-only bounds in the object's own local space, ignoring its
    /// `transform`. `None` for an empty group/compound path.
    ///
    /// Groups are handled by the document (bounds require recursing
    /// through children's own transforms), so this returns `None` for
    /// `Group` — see [`crate::Document::bounds_of`].
    pub fn own_local_bounds(&self) -> Option<Rect> {
        match self {
            ObjectKind::Path(p) => Some(p.local_bounds()),
            ObjectKind::CompoundPath(cp) => cp.local_bounds(),
            ObjectKind::Text(t) => Some(t.local_bounds),
            ObjectKind::Image(i) => Some(i.local_bounds),
            ObjectKind::Symbol(s) => Some(s.local_bounds),
            ObjectKind::Group(_) => None,
        }
    }
}

/// A drawable or groupable object: a path, group, text frame, image, etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub id: ObjectId,
    pub name: Option<String>,
    /// Maps this object's local coordinate space into its parent's space
    /// (the owning layer's space, or the owning group's space).
    pub transform: Affine,
    pub visible: bool,
    pub locked: bool,
    pub parent: ObjectParent,
    pub kind: ObjectKind,
    /// Fill and stroke. `#[serde(default)]` so a `.amalith` file saved
    /// before this field existed still loads (with the default
    /// appearance) instead of failing to parse.
    #[serde(default)]
    pub appearance: Appearance,
}

impl Object {
    pub fn new(id: ObjectId, parent: ObjectParent, kind: ObjectKind) -> Self {
        Self {
            id,
            name: None,
            transform: Affine::IDENTITY,
            visible: true,
            locked: false,
            parent,
            kind,
            appearance: Appearance::default(),
        }
    }

    /// Convenience constructor for a rectangle path object.
    pub fn rectangle(id: ObjectId, parent: ObjectParent, rect: Rect) -> Self {
        Self::new(id, parent, ObjectKind::Path(PathData::rectangle(rect)))
    }

    pub fn is_group(&self) -> bool {
        matches!(self.kind, ObjectKind::Group(_))
    }
}

#[cfg(test)]
mod path_data_tests {
    use super::*;

    /// Every native constructor must round-trip through the anchor model
    /// with a byte-identical `geometry`, so Stage 1 is behaviour-neutral.
    #[test]
    fn constructors_roundtrip_geometry_verbatim() {
        let r = Rect::new(10.0, 20.0, 90.0, 70.0);
        for pd in [
            PathData::rectangle(r),
            PathData::ellipse(r),
            PathData::rounded_rectangle(r, 12.0),
            PathData::polygon(&[
                Point::new(0.0, 0.0),
                Point::new(30.0, 5.0),
                Point::new(15.0, 40.0),
            ]),
            PathData::polyline(&[
                Point::new(0.0, 0.0),
                Point::new(30.0, 5.0),
                Point::new(15.0, 40.0),
            ]),
        ] {
            let rebuilt = subpaths_to_bezpath(pd.subpaths());
            assert_eq!(
                pd.geometry.elements(),
                rebuilt.elements(),
                "geometry cache diverged from its own subpaths"
            );
        }
    }

    #[test]
    fn rectangle_is_four_corner_anchors() {
        let pd = PathData::rectangle(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(pd.subpaths().len(), 1);
        let sp = &pd.subpaths()[0];
        assert!(sp.closed);
        assert_eq!(sp.anchors.len(), 4);
        assert!(sp
            .anchors
            .iter()
            .all(|a| a.mode == HandleMode::Corner && a.handle_in.is_none()));
    }

    #[test]
    fn ellipse_anchors_are_smooth() {
        let pd = PathData::ellipse(Rect::new(0.0, 0.0, 100.0, 60.0));
        let sp = &pd.subpaths()[0];
        assert_eq!(sp.anchors.len(), 4);
        assert!(sp
            .anchors
            .iter()
            .all(|a| a.handle_in.is_some() && a.handle_out.is_some()));
        assert!(sp
            .anchors
            .iter()
            .all(|a| matches!(a.mode, HandleMode::Smooth | HandleMode::Symmetric)));
    }

    #[test]
    fn legacy_geometry_only_json_still_loads() {
        // A pre-anchor-model artwork blob is just `{ "geometry": <bezpath> }`.
        let reference = PathData::rectangle(Rect::new(1.0, 2.0, 3.0, 4.0));
        let wrapped = serde_json::json!({
            "geometry": serde_json::to_value(&reference.geometry).unwrap(),
        });
        let loaded: PathData = serde_json::from_value(wrapped).unwrap();
        assert_eq!(loaded.geometry.elements(), reference.geometry.elements());
        assert_eq!(loaded.subpaths().len(), 1);
        assert_eq!(loaded.subpaths()[0].anchors.len(), 4);
    }

    #[test]
    fn structured_json_roundtrips() {
        let pd = PathData::ellipse(Rect::new(0.0, 0.0, 40.0, 30.0));
        let json = serde_json::to_string(&pd).unwrap();
        let back: PathData = serde_json::from_str(&json).unwrap();
        // Anchor structure is preserved exactly; geometry may differ by a
        // float ULP because it is re-derived from the parsed anchors.
        assert_eq!(back.subpaths().len(), pd.subpaths().len());
        assert_eq!(back.subpaths()[0].anchors.len(), pd.subpaths()[0].anchors.len());
        assert_eq!(back.subpaths()[0].closed, pd.subpaths()[0].closed);
        for (a, b) in back.subpaths()[0]
            .anchors
            .iter()
            .zip(&pd.subpaths()[0].anchors)
        {
            assert!((a.point - b.point).hypot() < 1e-9);
            assert_eq!(a.mode, b.mode);
        }
        assert_eq!(back.geometry.elements().len(), pd.geometry.elements().len());
    }
}
