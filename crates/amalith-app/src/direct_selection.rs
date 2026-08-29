use amalith_commands::{Command, CommandError, Editor};
use amalith_core::{
    geom, geom::BezPath, Affine, Document, ObjectId, ObjectKind, ObjectParent, Point, Rect, Vec2,
};
use std::borrow::Cow;
use std::collections::HashSet;

enum Drag {
    Move {
        start: Point,
    },
    Marquee {
        start: Point,
        add: bool,
        current: Point,
    },
}

/// Illustrator-style white-arrow selection for individual path anchors.
pub(crate) struct DirectSelectionTool {
    pub(crate) active: bool,
    pub(crate) selected: HashSet<(ObjectId, usize)>,
    drag: Option<Drag>,
    preview_delta: Vec2,
}

impl Default for DirectSelectionTool {
    fn default() -> Self {
        Self {
            active: false,
            selected: HashSet::new(),
            drag: None,
            preview_delta: Vec2::ZERO,
        }
    }
}

impl DirectSelectionTool {
    pub(crate) fn set_active(&mut self, active: bool) {
        self.active = active;
        self.cancel_drag();
    }

    pub(crate) fn press(&mut self, document: &Document, point: Point, hit_radius: f64, add: bool) {
        if let Some(anchor) = topmost_anchor_at(document, point, hit_radius) {
            if add {
                if !self.selected.insert(anchor) {
                    self.selected.remove(&anchor);
                }
                self.drag = None;
            } else {
                if !self.selected.contains(&anchor) {
                    self.selected.clear();
                    self.selected.insert(anchor);
                }
                self.drag = Some(Drag::Move { start: point });
            }
        } else if let Some((id, a, b)) = topmost_segment_at(document, point, hit_radius) {
            // Clicking a contour segment selects the two anchors that
            // bound that edge, matching Illustrator's white-arrow
            // click-on-a-line.
            let anchors = [(id, a), (id, b)];
            if add {
                let all_selected = anchors.iter().all(|anchor| self.selected.contains(anchor));
                if all_selected {
                    for anchor in &anchors {
                        self.selected.remove(anchor);
                    }
                    self.drag = None;
                } else {
                    self.selected.extend(anchors);
                    self.drag = Some(Drag::Move { start: point });
                }
            } else {
                let already = anchors.iter().all(|anchor| self.selected.contains(anchor))
                    && self.selected.len() == 2;
                if !already {
                    self.selected.clear();
                    self.selected.extend(anchors);
                }
                self.drag = Some(Drag::Move { start: point });
            }
        } else if let Some(id) = topmost_path_at(document, point) {
            // Clicking the body of a shape (not a single node) selects
            // every anchor on that path — Illustrator's white-arrow
            // click-in-the-middle.
            let anchors = path_anchors(document, id);
            if add {
                let all_selected = !anchors.is_empty()
                    && anchors.iter().all(|anchor| self.selected.contains(anchor));
                if all_selected {
                    for anchor in &anchors {
                        self.selected.remove(anchor);
                    }
                    self.drag = None;
                } else {
                    self.selected.extend(anchors);
                    self.drag = Some(Drag::Move { start: point });
                }
            } else {
                let already = !anchors.is_empty()
                    && anchors.iter().all(|anchor| self.selected.contains(anchor));
                if !already {
                    self.selected.clear();
                    self.selected.extend(anchors);
                }
                self.drag = Some(Drag::Move { start: point });
            }
        } else {
            if !add {
                self.selected.clear();
            }
            self.drag = Some(Drag::Marquee {
                start: point,
                add,
                current: point,
            });
        }
        self.preview_delta = Vec2::ZERO;
    }

    pub(crate) fn drag(&mut self, point: Point) {
        match &mut self.drag {
            Some(Drag::Move { start }) => self.preview_delta = point - *start,
            Some(Drag::Marquee { current, .. }) => *current = point,
            None => {}
        }
    }

    pub(crate) fn finish_drag(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        let drag = self.drag.take();
        let delta = std::mem::replace(&mut self.preview_delta, Vec2::ZERO);
        match drag {
            Some(Drag::Move { .. }) if delta != Vec2::ZERO && !self.selected.is_empty() => {
                editor.execute(Command::MoveAnchors {
                    anchors: self.selected.iter().copied().collect(),
                    delta,
                })?;
                Ok(true)
            }
            Some(Drag::Marquee {
                start,
                add,
                current,
            }) => {
                let marquee = normalized_rect(start, current);
                if marquee.width() <= 0.0 || marquee.height() <= 0.0 {
                    if !add {
                        self.selected.clear();
                    }
                    return Ok(false);
                }
                if !add {
                    self.selected.clear();
                }
                for layer in editor.document().layers() {
                    for id in path_descendants(editor.document(), ObjectParent::Layer(layer.id)) {
                        let Some(object) = editor.document().object(id) else {
                            continue;
                        };
                        let ObjectKind::Path(path) = &object.kind else {
                            continue;
                        };
                        let transform = editor.document().world_transform(id);
                        for index in geom::anchor_indices(&path.geometry) {
                            let Some(position) = geom::anchor_position(&path.geometry, index)
                            else {
                                continue;
                            };
                            if marquee.contains(transform * position) {
                                self.selected.insert((id, index));
                            }
                        }
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
        self.preview_delta = Vec2::ZERO;
    }

    pub(crate) fn marquee_rect(&self) -> Option<Rect> {
        match self.drag {
            Some(Drag::Marquee { start, current, .. }) => Some(normalized_rect(start, current)),
            _ => None,
        }
    }

    /// Whether this object is the active node-edit target. The canvas uses
    /// this to draw Illustrator's blue editable-path overlay without
    /// replacing the object's real fill or stroke.
    pub(crate) fn has_selected_anchor_on(&self, id: ObjectId) -> bool {
        self.selected.iter().any(|&(object_id, _)| object_id == id)
    }

    pub(crate) fn display_anchor_position(
        &self,
        document: &Document,
        id: ObjectId,
        index: usize,
    ) -> Option<Point> {
        let object = document.object(id)?;
        let ObjectKind::Path(path) = &object.kind else {
            return None;
        };
        let position = document.world_transform(id) * geom::anchor_position(&path.geometry, index)?;
        (self.selected.contains(&(id, index)) && matches!(self.drag, Some(Drag::Move { .. })))
            .then_some(position + self.preview_delta)
            .or(Some(position))
    }

    /// The geometry `id`'s path should actually render with right now —
    /// `geometry` itself, live-deformed by whichever of its anchors are
    /// selected and mid-drag, so the shape reacts in real time instead of
    /// staying frozen until the drag commits (only the little anchor
    /// marker used to move live; the fill/stroke didn't). Mirrors
    /// `Command::MoveAnchors`'s compile step exactly — one shared delta
    /// applied to every selected anchor on this object — just computed
    /// for display instead of for the command engine. Borrows `geometry`
    /// unchanged (no clone) whenever there's nothing to preview, which is
    /// every path on every frame except the one actually being dragged.
    pub(crate) fn preview_geometry<'a>(
        &self,
        id: ObjectId,
        geometry: &'a BezPath,
    ) -> Cow<'a, BezPath> {
        if !matches!(self.drag, Some(Drag::Move { .. })) || self.preview_delta == Vec2::ZERO {
            return Cow::Borrowed(geometry);
        }
        let indices: Vec<usize> = self
            .selected
            .iter()
            .filter(|&&(object_id, _)| object_id == id)
            .map(|&(_, index)| index)
            .collect();
        if indices.is_empty() {
            return Cow::Borrowed(geometry);
        }
        let mut preview = geometry.clone();
        for index in indices {
            geom::translate_anchor(&mut preview, index, self.preview_delta);
        }
        Cow::Owned(preview)
    }

    pub(crate) fn retain_existing(&mut self, document: &Document) {
        self.selected.retain(|&(id, index)| {
            document.object(id).is_some_and(|object| {
                matches!(&object.kind, ObjectKind::Path(path) if geom::anchor_position(&path.geometry, index).is_some())
            })
        });
        if self.selected.is_empty() {
            self.cancel_drag();
        }
    }
}

/// The topmost path segment whose contour lies within `hit_radius` of
/// `point`, as the two editable anchors that bound that segment.
fn topmost_segment_at(
    document: &Document,
    point: Point,
    hit_radius: f64,
) -> Option<(ObjectId, usize, usize)> {
    document.layers().iter().rev().find_map(|layer| {
        if !layer.visible {
            return None;
        }
        path_descendants(document, ObjectParent::Layer(layer.id))
            .into_iter()
            .rev()
            .find_map(|id| {
                let object = document.object(id)?;
                let ObjectKind::Path(path) = &object.kind else {
                    return None;
                };
                if !object.visible {
                    return None;
                }
                let transform = document.world_transform(id);
                contour_segments(&path.geometry)
                    .into_iter()
                    .find_map(|(from, to, edges)| {
                        edges
                            .iter()
                            .any(|&(a, b)| {
                                distance_to_segment(point, transform * a, transform * b)
                                    <= hit_radius
                            })
                            .then_some((id, from, to))
                    })
            })
    })
}

fn contour_segments(path: &geom::BezPath) -> Vec<(usize, usize, Vec<(Point, Point)>)> {
    let mut result = Vec::new();
    let mut start_idx = 0usize;
    let mut start_pt = Point::ZERO;
    let mut last_idx = 0usize;
    let mut last_pt = Point::ZERO;
    let mut started = false;

    let push_segment = |from: usize, to: usize, mini: geom::BezPath, out: &mut Vec<_>| {
        let mut edges = Vec::new();
        for poly in geom::flattened_points(&mini, 0.5) {
            for pair in poly.windows(2) {
                edges.push((pair[0], pair[1]));
            }
        }
        if !edges.is_empty() {
            out.push((from, to, edges));
        }
    };

    for (index, element) in path.elements().iter().enumerate() {
        match *element {
            geom::PathEl::MoveTo(point) => {
                start_idx = index;
                start_pt = point;
                last_idx = index;
                last_pt = point;
                started = true;
            }
            geom::PathEl::LineTo(point)
            | geom::PathEl::QuadTo(_, point)
            | geom::PathEl::CurveTo(_, _, point) => {
                let mut mini = geom::BezPath::new();
                mini.move_to(last_pt);
                mini.push(*element);
                push_segment(last_idx, index, mini, &mut result);
                last_idx = index;
                last_pt = point;
            }
            geom::PathEl::ClosePath if started => {
                if (last_pt - start_pt).hypot() > 1e-9 {
                    let mut mini = geom::BezPath::new();
                    mini.move_to(last_pt);
                    mini.line_to(start_pt);
                    push_segment(last_idx, start_idx, mini, &mut result);
                }
            }
            geom::PathEl::ClosePath => {}
        }
    }
    result
}

fn distance_to_segment(point: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let ap = point - a;
    let len2 = ab.dot(ab);
    if len2 <= f64::EPSILON {
        return ap.hypot();
    }
    let t = (ap.dot(ab) / len2).clamp(0.0, 1.0);
    (ap - ab * t).hypot()
}

/// The topmost visible path whose document-space bounds contain `point`.
/// Used when a white-arrow click misses every anchor but lands inside a
/// shape, so the click can select that path's full node set.
fn topmost_path_at(document: &Document, point: Point) -> Option<ObjectId> {
    document.layers().iter().rev().find_map(|layer| {
        if !layer.visible {
            return None;
        }
        path_descendants(document, ObjectParent::Layer(layer.id))
            .into_iter()
            .rev()
            .find(|&id| {
                document.object(id).is_some_and(|object| {
                    object.visible
                        && matches!(object.kind, ObjectKind::Path(_))
                        && document
                            .bounds_of(id)
                            .is_some_and(|bounds| bounds.contains(point))
                })
            })
    })
}

fn path_anchors(document: &Document, id: ObjectId) -> Vec<(ObjectId, usize)> {
    let Some(object) = document.object(id) else {
        return Vec::new();
    };
    let ObjectKind::Path(path) = &object.kind else {
        return Vec::new();
    };
    geom::anchor_indices(&path.geometry)
        .into_iter()
        .map(|index| (id, index))
        .collect()
}

/// Finds the topmost path anchor within `hit_radius` document units.
/// Callers derive that radius from a fixed screen-space target (currently
/// 6px) so picking remains comfortable at every zoom level.
pub(crate) fn topmost_anchor_at(
    document: &Document,
    point: Point,
    hit_radius: f64,
) -> Option<(ObjectId, usize)> {
    let radius_squared = hit_radius * hit_radius;
    document.layers().iter().rev().find_map(|layer| {
        if !layer.visible {
            return None;
        }
        path_descendants(document, ObjectParent::Layer(layer.id))
            .into_iter()
            .rev()
            .find_map(|id| {
                let object = document.object(id)?;
                let ObjectKind::Path(path) = &object.kind else {
                    return None;
                };
                if !object.visible {
                    return None;
                }
                let transform: Affine = document.world_transform(id);
                geom::anchor_indices(&path.geometry)
                    .into_iter()
                    .filter_map(|index| {
                        let position = transform * geom::anchor_position(&path.geometry, index)?;
                        let delta = position - point;
                        (delta.x * delta.x + delta.y * delta.y <= radius_squared)
                            .then_some((index, delta.x * delta.x + delta.y * delta.y))
                    })
                    .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
                    .map(|(index, _)| (id, index))
            })
    })
}

/// The rendered leaf paths under `parent`, in paint order. Direct selection
/// operates on those leaves even when their parent group is what the black
/// arrow selected, matching the canvas renderer's group expansion.
fn path_descendants(document: &Document, parent: ObjectParent) -> Vec<ObjectId> {
    let mut paths = Vec::new();
    for &id in document.children_of(parent) {
        let Some(object) = document.object(id) else {
            continue;
        };
        match &object.kind {
            ObjectKind::Path(_) => paths.push(id),
            ObjectKind::Group(_) => {
                paths.extend(path_descendants(document, ObjectParent::Group(id)));
            }
            _ => {}
        }
    }
    paths
}

fn normalized_rect(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_commands::{Command, CommandOutcome};
    use amalith_core::{Document, ObjectKind};

    fn editor_with_rect() -> (Editor, ObjectId) {
        let mut editor = Editor::new(Document::new("Test"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        (editor, object)
    }

    #[test]
    fn preview_geometry_deforms_live_while_dragging_a_selected_anchor() {
        // Regression test: only the little anchor marker used to move
        // live during a node drag — the shape's actual fill/stroke kept
        // rendering the committed geometry until the drag committed, so
        // the shape looked frozen and then snapped instead of reacting
        // in real time.
        let (editor, object) = editor_with_rect();
        let ObjectKind::Path(path) = &editor.document().object(object).unwrap().kind else {
            unreachable!()
        };
        let committed = path.geometry.clone();

        let mut tool = DirectSelectionTool::default();
        let moved_index = geom::anchor_indices(&committed)[0];
        tool.selected.insert((object, moved_index));
        let delta = Vec2::new(3.0, -4.0);
        // Set drag state directly rather than through `press`'s hit-test
        // — this test is about `preview_geometry`'s math, not hit-testing.
        tool.drag = Some(Drag::Move {
            start: Point::new(0.0, 0.0),
        });
        tool.preview_delta = delta;

        let preview = tool.preview_geometry(object, &committed);
        assert_ne!(preview.as_ref(), &committed);

        let original_anchor = geom::anchor_position(&committed, moved_index).unwrap();
        let preview_anchor = geom::anchor_position(&preview, moved_index).unwrap();
        assert_eq!(preview_anchor, original_anchor + delta);

        // A different, unselected anchor on the same path must not move.
        let other_index = geom::anchor_indices(&committed)[2];
        assert_eq!(
            geom::anchor_position(&preview, other_index),
            geom::anchor_position(&committed, other_index)
        );
    }

    #[test]
    fn preview_geometry_borrows_unchanged_when_nothing_is_being_dragged() {
        let (editor, object) = editor_with_rect();
        let ObjectKind::Path(path) = &editor.document().object(object).unwrap().kind else {
            unreachable!()
        };
        let geometry = path.geometry.clone();

        let tool = DirectSelectionTool::default();
        let preview = tool.preview_geometry(object, &geometry);
        assert!(matches!(preview, Cow::Borrowed(_)));
    }

    #[test]
    fn finds_anchors_nested_inside_a_group() {
        let (mut editor, child) = editor_with_rect();
        editor
            .execute(Command::Group {
                ids: vec![child],
                name: Some("Grouped rectangle".into()),
            })
            .unwrap();
        let ObjectKind::Path(path) = &editor.document().object(child).unwrap().kind else {
            unreachable!()
        };
        let index = geom::anchor_indices(&path.geometry)[0];
        let point = editor.document().world_transform(child)
            * geom::anchor_position(&path.geometry, index).unwrap();

        assert_eq!(
            topmost_anchor_at(editor.document(), point, 0.1),
            Some((child, index))
        );
    }

    #[test]
    fn clicking_a_path_body_selects_every_anchor() {
        let (editor, object) = editor_with_rect();
        let ObjectKind::Path(path) = &editor.document().object(object).unwrap().kind else {
            unreachable!()
        };
        let all = geom::anchor_indices(&path.geometry);
        assert_eq!(all.len(), 4);

        let mut tool = DirectSelectionTool::default();
        tool.press(editor.document(), Point::new(5.0, 5.0), 0.1, false);

        let selected: HashSet<usize> = tool
            .selected
            .iter()
            .filter(|(id, _)| *id == object)
            .map(|(_, index)| *index)
            .collect();
        assert_eq!(selected, all.iter().copied().collect());
    }

    #[test]
    fn clicking_a_contour_segment_selects_its_two_endpoints() {
        let (editor, object) = editor_with_rect();
        let ObjectKind::Path(path) = &editor.document().object(object).unwrap().kind else {
            unreachable!()
        };
        let anchors = geom::anchor_indices(&path.geometry);
        assert_eq!(anchors.len(), 4);
        // Midpoint of the top edge (0,0)-(10,0).
        let mut tool = DirectSelectionTool::default();
        tool.press(editor.document(), Point::new(5.0, 0.0), 0.5, false);
        let selected: HashSet<usize> = tool
            .selected
            .iter()
            .filter(|(id, _)| *id == object)
            .map(|(_, index)| *index)
            .collect();
        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&anchors[0]));
        assert!(selected.contains(&anchors[1]));
    }

    #[test]
    fn clicking_empty_space_does_not_select_anchors() {
        let (editor, object) = editor_with_rect();
        let mut tool = DirectSelectionTool::default();
        tool.press(editor.document(), Point::new(50.0, 50.0), 0.1, false);
        assert!(
            !tool.has_selected_anchor_on(object),
            "a miss should start a marquee, not select the path"
        );
        assert!(matches!(tool.drag, Some(Drag::Marquee { .. })));
    }
}
