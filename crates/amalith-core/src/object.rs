//! Objects: the drawable/groupable content of a document.
//!
//! An [`Object`] is Amalith's analogue of Inkscape's `SPItem` — but unlike
//! `SPItem`, it is a plain data value with no XML repr shadowing it. There
//! is exactly one tree (see [`crate::document`] for how objects attach to
//! layers and groups), not an XML repr tree plus a parallel item tree kept
//! in sync. That sync problem is a large fraction of `SPObject`'s
//! complexity in Inkscape; Amalith has no reason to take it on, since the
//! native format is not "serialized DOM" (see `DESIGN.md`).
use crate::geom::{Affine, BezPath, PathEl, Rect};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectParent {
    Layer(LayerId),
    Group(ObjectId),
}

/// A path built from Bezier segments, in the object's local coordinate
/// space (before `Object::transform` is applied).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathData {
    pub geometry: BezPath,
}

impl PathData {
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
        Self { geometry: path }
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
        Self { geometry: path }
    }

    pub fn local_bounds(&self) -> Rect {
        crate::geom::bez_path_bounds(&self.geometry)
    }

    /// Returns a polyline approximation of every subpath in local space.
    pub fn flattened_points(&self, tolerance: f64) -> Vec<Vec<crate::geom::Point>> {
        let mut paths: Vec<Vec<crate::geom::Point>> = Vec::new();
        let mut current: Option<Vec<crate::geom::Point>> = None;
        crate::geom::flatten(&self.geometry, tolerance, |element| match element {
            PathEl::MoveTo(point) => {
                if let Some(points) = current.take() {
                    if points.len() >= 2 {
                        paths.push(points);
                    }
                }
                current = Some(vec![point]);
            }
            PathEl::LineTo(point) => {
                if let Some(points) = current.as_mut() {
                    points.push(point);
                }
            }
            PathEl::ClosePath => {
                if let Some(points) = current.take() {
                    if points.len() >= 2 {
                        paths.push(points);
                    }
                }
            }
            PathEl::QuadTo(_, _) | PathEl::CurveTo(_, _, _) => {}
        });
        if let Some(points) = current {
            if points.len() >= 2 {
                paths.push(points);
            }
        }
        paths
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

/// Stub text object: enough identity, transform, and an explicit bounds box
/// to participate in layout/selection, without a typography engine yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextData {
    pub content: String,
    /// Local-space bounds, set explicitly until real text layout exists.
    pub local_bounds: Rect,
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
