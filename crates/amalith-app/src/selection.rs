use amalith_commands::{Command, CommandError, Editor};
use amalith_core::{Affine, Document, ObjectId, ObjectKind, ObjectParent, Point, Rect, Vec2};

use crate::artboard_tool::Handle;
use std::collections::{HashMap, HashSet};

const MIN_SIZE: f64 = 1.0;

enum Drag {
    Move {
        start: Point,
    },
    Duplicate {
        start: Point,
    },
    Marquee {
        start: Point,
        add: bool,
        current: Point,
    },
    Scale {
        handle: Handle,
        start_bounds: Rect,
        start_transforms: HashMap<ObjectId, Affine>,
        preview_transforms: HashMap<ObjectId, Affine>,
    },
    Rotate {
        center: Point,
        start_angle: f64,
        start_transforms: HashMap<ObjectId, Affine>,
        preview_transforms: HashMap<ObjectId, Affine>,
    },
}

pub(crate) struct SelectionTool {
    pub(crate) active: bool,
    pub(crate) selected: HashSet<ObjectId>,
    drag: Option<Drag>,
    preview_delta: Vec2,
}

impl Default for SelectionTool {
    fn default() -> Self {
        Self {
            active: true,
            selected: HashSet::new(),
            drag: None,
            preview_delta: Vec2::ZERO,
        }
    }
}

impl SelectionTool {
    pub(crate) fn set_active(&mut self, active: bool) {
        self.active = active;
        self.cancel_drag();
    }

    pub(crate) fn press(
        &mut self,
        document: &Document,
        point: Point,
        visible: Rect,
        duplicate: bool,
        add: bool,
    ) {
        if let Some(id) = topmost_path_at(document, point, visible) {
            if add {
                if !self.selected.insert(id) {
                    self.selected.remove(&id);
                }
                self.drag = None;
            } else {
                // Clicking an already-selected object moves the complete
                // selection.  Only replace the selection when the hit object
                // was not selected (Alt duplication remains single-source).
                if duplicate || !self.selected.contains(&id) {
                    self.selected.clear();
                    self.selected.insert(id);
                }
                self.drag = Some(if duplicate {
                    Drag::Duplicate { start: point }
                } else {
                    Drag::Move { start: point }
                });
            }
        } else {
            // The union/oriented selection box is also draggable between
            // objects.  Main's handle/rotation hit tests run first, so this
            // path only handles its interior.
            let inside_selection_box = !add
                && !duplicate
                && self
                    .selected_quad(document)
                    .is_some_and(|quad| point_in_convex_quad(point, quad));
            if inside_selection_box {
                self.drag = Some(Drag::Move { start: point });
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
        }
        self.preview_delta = Vec2::ZERO;
    }

    pub(crate) fn begin_scale(&mut self, document: &Document, handle: Handle) {
        let Some(start_bounds) = self.selected_union_bounds(document) else {
            return;
        };
        let start_transforms: HashMap<_, _> = self
            .selected
            .iter()
            .filter_map(|id| document.object(*id).map(|object| (*id, object.transform)))
            .collect();
        if start_transforms.is_empty() {
            return;
        }
        self.preview_delta = Vec2::ZERO;
        self.drag = Some(Drag::Scale {
            handle,
            start_bounds,
            preview_transforms: start_transforms.clone(),
            start_transforms,
        });
    }

    pub(crate) fn begin_rotate(&mut self, document: &Document, pointer: Point) {
        let Some(bounds) = self.selected_union_bounds(document) else {
            return;
        };
        let center = bounds.center();
        let start_transforms: HashMap<_, _> = self
            .selected
            .iter()
            .filter_map(|id| document.object(*id).map(|object| (*id, object.transform)))
            .collect();
        if start_transforms.is_empty() {
            return;
        }
        self.preview_delta = Vec2::ZERO;
        self.drag = Some(Drag::Rotate {
            center,
            start_angle: (pointer.y - center.y).atan2(pointer.x - center.x),
            preview_transforms: start_transforms.clone(),
            start_transforms,
        });
    }

    pub(crate) fn drag(&mut self, point: Point, uniform: bool, from_center: bool) {
        match &mut self.drag {
            Some(Drag::Move { start }) => self.preview_delta = point - *start,
            Some(Drag::Duplicate { start }) => self.preview_delta = point - *start,
            Some(Drag::Marquee { current, .. }) => *current = point,
            Some(Drag::Scale {
                handle,
                start_bounds,
                start_transforms,
                preview_transforms,
            }) => {
                let (scale, _) = scaled_transform(
                    *start_bounds,
                    Affine::IDENTITY,
                    *handle,
                    point,
                    uniform,
                    from_center,
                );
                for (id, start) in start_transforms.iter() {
                    preview_transforms.insert(*id, scale * *start);
                }
            }
            Some(Drag::Rotate {
                center,
                start_angle,
                start_transforms,
                preview_transforms,
            }) => {
                let current_angle = (point.y - center.y).atan2(point.x - center.x);
                let mut theta = current_angle - *start_angle;
                if uniform {
                    let increment = std::f64::consts::FRAC_PI_4;
                    theta = (theta / increment).round() * increment;
                }
                let rotate_about_center = Affine::translate((center.x, center.y))
                    * Affine::rotate(theta)
                    * Affine::translate((-center.x, -center.y));
                for (id, start) in start_transforms.iter() {
                    preview_transforms.insert(*id, rotate_about_center * *start);
                }
            }
            None => {}
        }
    }

    pub(crate) fn cancel_drag(&mut self) {
        self.drag = None;
        self.preview_delta = Vec2::ZERO;
    }

    pub(crate) fn finish_drag(&mut self, editor: &mut Editor) -> Result<bool, CommandError> {
        let drag = self.drag.take();
        let delta = std::mem::replace(&mut self.preview_delta, Vec2::ZERO);
        let object = self.selected.iter().next().copied();
        if object.is_none() && !matches!(drag, Some(Drag::Marquee { .. })) {
            return Ok(false);
        }
        let object = object.unwrap_or_default();
        match drag {
            Some(Drag::Move { .. }) if delta != Vec2::ZERO => {
                let objects = self.selected.iter().copied().collect();
                editor.execute(Command::MoveObjects { objects, delta })?;
                Ok(true)
            }
            Some(Drag::Duplicate { .. }) if delta != Vec2::ZERO => {
                let outcome = editor.execute(Command::DuplicateObject { object, delta })?;
                if let amalith_commands::CommandOutcome::Object(copy) = outcome {
                    self.selected.clear();
                    self.selected.insert(copy);
                }
                Ok(true)
            }
            Some(Drag::Scale {
                start_transforms,
                preview_transforms,
                ..
            }) if preview_transforms != start_transforms => {
                editor.execute(Command::SetTransforms {
                    items: preview_transforms.into_iter().collect(),
                })?;
                Ok(true)
            }
            Some(Drag::Rotate {
                start_transforms,
                preview_transforms,
                ..
            }) if preview_transforms != start_transforms => {
                editor.execute(Command::SetTransforms {
                    items: preview_transforms.into_iter().collect(),
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
                } else {
                    if !add {
                        self.selected.clear();
                    }
                    for layer in editor.document().layers() {
                        for &id in editor.document().children_of(ObjectParent::Layer(layer.id)) {
                            let is_path = editor
                                .document()
                                .object(id)
                                .is_some_and(|object| matches!(object.kind, ObjectKind::Path(_)));
                            if editor
                                .document()
                                .bounds_of(id)
                                .is_some_and(|bounds| is_path && rects_intersect(bounds, marquee))
                            {
                                self.selected.insert(id);
                            }
                        }
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn display_transform(&self, document: &Document, id: ObjectId) -> Option<Affine> {
        document.object(id)?;
        if !self.selected.contains(&id) {
            return Some(document.world_transform(id));
        }
        match &self.drag {
            Some(Drag::Scale {
                preview_transforms, ..
            })
            | Some(Drag::Rotate {
                preview_transforms, ..
            }) => preview_transforms.get(&id).copied(),
            Some(Drag::Move { .. }) => {
                Some(Affine::translate(self.preview_delta) * document.world_transform(id))
            }
            Some(Drag::Duplicate { .. }) => Some(document.world_transform(id)),
            Some(Drag::Marquee { .. }) => Some(document.world_transform(id)),
            None => Some(document.world_transform(id)),
        }
    }

    pub(crate) fn selected_quad(&self, document: &Document) -> Option<[Point; 4]> {
        if let Some(id) = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()
        {
            return self.display_quad(document, id);
        }
        let bounds = self.selected_union_bounds(document)?;
        Some([
            Point::new(bounds.x0, bounds.y0),
            Point::new(bounds.x1, bounds.y0),
            Point::new(bounds.x1, bounds.y1),
            Point::new(bounds.x0, bounds.y1),
        ])
    }

    pub(crate) fn selected_intersects(&self, document: &Document, visible: Rect) -> bool {
        self.selected
            .iter()
            .filter_map(|id| self.display_bounds(document, *id))
            .any(|bounds| rects_intersect(bounds, visible))
    }

    pub(crate) fn selected_union_bounds(&self, document: &Document) -> Option<Rect> {
        self.selected
            .iter()
            .filter_map(|id| self.display_bounds(document, *id))
            .reduce(|a, b| a.union(b))
    }

    fn display_bounds(&self, document: &Document, id: ObjectId) -> Option<Rect> {
        let object = document.object(id)?;
        let local = object.kind.own_local_bounds()?;
        Some(
            self.display_transform(document, id)?
                .transform_rect_bbox(local),
        )
    }

    pub(crate) fn display_quad(&self, document: &Document, id: ObjectId) -> Option<[Point; 4]> {
        object_quad(document, id, self.display_transform(document, id)?)
    }

    pub(crate) fn duplicate_preview_quad(&self, document: &Document) -> Option<[Point; 4]> {
        let id = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()?;
        if !matches!(&self.drag, Some(Drag::Duplicate { .. })) {
            return None;
        }
        object_quad(
            document,
            id,
            Affine::translate(self.preview_delta) * document.world_transform(id),
        )
    }

    pub(crate) fn is_duplicate_drag(&self) -> bool {
        matches!(&self.drag, Some(Drag::Duplicate { .. }))
    }

    pub(crate) fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    pub(crate) fn duplicate_preview_bounds(&self, document: &Document) -> Option<Rect> {
        let id = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()?;
        self.is_duplicate_drag().then_some(()).and_then(|_| {
            document
                .bounds_of(id)
                .map(|bounds| bounds + self.preview_delta)
        })
    }

    pub(crate) fn retain_existing(&mut self, document: &Document) {
        if self
            .selected
            .iter()
            .any(|id| document.object(*id).is_none())
        {
            self.selected.retain(|id| document.object(*id).is_some());
            if self.selected.is_empty() {
                self.cancel_drag();
            }
        }
    }

    pub(crate) fn marquee_rect(&self) -> Option<Rect> {
        match self.drag {
            Some(Drag::Marquee { start, current, .. }) => Some(normalized_rect(start, current)),
            _ => None,
        }
    }
}

fn object_quad(document: &Document, id: ObjectId, transform: Affine) -> Option<[Point; 4]> {
    let local = document.object(id)?.kind.own_local_bounds()?;
    Some([
        transform * Point::new(local.x0, local.y0),
        transform * Point::new(local.x1, local.y0),
        transform * Point::new(local.x1, local.y1),
        transform * Point::new(local.x0, local.y1),
    ])
}

fn quad_center(quad: [Point; 4]) -> Point {
    Point::new(
        quad.iter().map(|point| point.x).sum::<f64>() / 4.0,
        quad.iter().map(|point| point.y).sum::<f64>() / 4.0,
    )
}

fn scaled_transform(
    bounds: Rect,
    old_transform: Affine,
    handle: Handle,
    pointer: Point,
    uniform: bool,
    from_center: bool,
) -> (Affine, Rect) {
    let center = bounds.center();
    let changes_x = matches!(
        handle,
        Handle::NorthWest
            | Handle::NorthEast
            | Handle::East
            | Handle::SouthEast
            | Handle::SouthWest
            | Handle::West
    );
    let changes_y = matches!(
        handle,
        Handle::NorthWest
            | Handle::North
            | Handle::NorthEast
            | Handle::SouthEast
            | Handle::South
            | Handle::SouthWest
    );
    let left = matches!(handle, Handle::NorthWest | Handle::SouthWest | Handle::West);
    let top = matches!(
        handle,
        Handle::NorthWest | Handle::North | Handle::NorthEast
    );

    let mut sx = if changes_x {
        if from_center {
            if left {
                (center.x - pointer.x) * 2.0 / bounds.width()
            } else {
                (pointer.x - center.x) * 2.0 / bounds.width()
            }
        } else if left {
            (bounds.x1 - pointer.x) / bounds.width()
        } else {
            (pointer.x - bounds.x0) / bounds.width()
        }
    } else {
        1.0
    };
    let mut sy = if changes_y {
        if from_center {
            if top {
                (center.y - pointer.y) * 2.0 / bounds.height()
            } else {
                (pointer.y - center.y) * 2.0 / bounds.height()
            }
        } else if top {
            (bounds.y1 - pointer.y) / bounds.height()
        } else {
            (pointer.y - bounds.y0) / bounds.height()
        }
    } else {
        1.0
    };
    sx = sx.max(MIN_SIZE / bounds.width());
    sy = sy.max(MIN_SIZE / bounds.height());
    if uniform {
        let scale = if changes_x && changes_y {
            sx.max(sy)
        } else if changes_x {
            sx
        } else {
            sy
        }
        .max((MIN_SIZE / bounds.width()).max(MIN_SIZE / bounds.height()));
        sx = scale;
        sy = scale;
    }

    let pivot = if from_center {
        center
    } else {
        Point::new(
            if left {
                bounds.x1
            } else if changes_x {
                bounds.x0
            } else {
                center.x
            },
            if top {
                bounds.y1
            } else if changes_y {
                bounds.y0
            } else {
                center.y
            },
        )
    };
    let scale_about_pivot = Affine::translate((pivot.x, pivot.y))
        * Affine::scale_non_uniform(sx, sy)
        * Affine::translate((-pivot.x, -pivot.y));
    (
        scale_about_pivot * old_transform,
        scale_about_pivot.transform_rect_bbox(bounds),
    )
}

pub(crate) fn topmost_path_at(
    document: &Document,
    point: Point,
    visible: Rect,
) -> Option<ObjectId> {
    document.layers().iter().rev().find_map(|layer| {
        if !layer.visible {
            return None;
        }
        document
            .children_of(ObjectParent::Layer(layer.id))
            .iter()
            .rev()
            .find_map(|&id| {
                let object = document.object(id)?;
                (object.visible
                    && matches!(object.kind, ObjectKind::Path(_))
                    && document.bounds_of(id).is_some_and(|bounds| {
                        rects_intersect(bounds, visible) && bounds.contains(point)
                    }))
                .then_some(id)
            })
    })
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

fn point_in_convex_quad(point: Point, quad: [Point; 4]) -> bool {
    let mut has_positive = false;
    let mut has_negative = false;
    for index in 0..4 {
        let edge = quad[(index + 1) % 4] - quad[index];
        let to_point = point - quad[index];
        let cross = edge.x * to_point.y - edge.y * to_point.x;
        has_positive |= cross > 0.0;
        has_negative |= cross < 0.0;
    }
    !(has_positive && has_negative)
}

fn normalized_rect(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_commands::CommandOutcome;
    use amalith_core::Document;

    fn editor_with_layer() -> (Editor, amalith_core::LayerId) {
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
        (editor, layer)
    }

    #[test]
    fn hit_test_selects_topmost_path() {
        let (mut editor, layer) = editor_with_layer();
        let first = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 50.0),
                name: None,
            })
            .unwrap();
        let second = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(10.0, 10.0, 60.0, 60.0),
                name: None,
            })
            .unwrap();
        let CommandOutcome::Object(first) = first else {
            unreachable!()
        };
        let CommandOutcome::Object(second) = second else {
            unreachable!()
        };

        assert_eq!(
            topmost_path_at(
                editor.document(),
                Point::new(20.0, 20.0),
                Rect::new(-100.0, -100.0, 100.0, 100.0),
            ),
            Some(second)
        );
        assert_ne!(
            topmost_path_at(
                editor.document(),
                Point::new(20.0, 20.0),
                Rect::new(-100.0, -100.0, 100.0, 100.0),
            ),
            Some(first)
        );
    }

    #[test]
    fn hit_test_culls_objects_outside_visible_rect() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(500.0, 500.0, 550.0, 550.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };

        assert_eq!(
            topmost_path_at(
                editor.document(),
                Point::new(525.0, 525.0),
                Rect::new(0.0, 0.0, 100.0, 100.0),
            ),
            None
        );
        assert_eq!(
            topmost_path_at(
                editor.document(),
                Point::new(525.0, 525.0),
                Rect::new(450.0, 450.0, 600.0, 600.0),
            ),
            Some(object)
        );
    }

    #[test]
    fn drag_commits_one_move_and_undo_restores_bounds() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let original = editor.document().bounds_of(object).unwrap();
        let mut selection = SelectionTool::default();
        selection.press(
            editor.document(),
            Point::new(10.0, 10.0),
            Rect::new(-100.0, -100.0, 100.0, 100.0),
            false,
            false,
        );
        selection.drag(Point::new(25.0, 4.0), false, false);

        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_eq!(
            editor.document().bounds_of(object),
            Some(original + Vec2::new(15.0, -6.0))
        );

        editor.undo().unwrap();
        assert_eq!(editor.document().bounds_of(object), Some(original));
    }

    #[test]
    fn alt_drag_duplicates_and_selects_copy() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(original) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let mut selection = SelectionTool::default();
        selection.press(
            editor.document(),
            Point::new(10.0, 10.0),
            Rect::new(-100.0, -100.0, 100.0, 100.0),
            true,
            false,
        );
        selection.drag(Point::new(25.0, 4.0), false, true);

        assert!(selection.finish_drag(&mut editor).unwrap());
        let copy = *selection.selected.iter().next().unwrap();
        assert_ne!(copy, original);
        assert_eq!(
            editor.document().bounds_of(original),
            Some(Rect::new(0.0, 0.0, 50.0, 30.0))
        );
        assert_eq!(
            editor.document().bounds_of(copy),
            Some(Rect::new(15.0, -6.0, 65.0, 24.0))
        );

        editor.undo().unwrap();
        assert!(editor.document().object(copy).is_none());
    }

    #[test]
    fn marquee_selects_intersections_and_shift_adds() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(first) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let CommandOutcome::Object(second) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(30.0, 0.0, 50.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let visible = Rect::new(-100.0, -100.0, 100.0, 100.0);
        let mut selection = SelectionTool::default();
        selection.press(
            editor.document(),
            Point::new(-10.0, -10.0),
            visible,
            false,
            false,
        );
        selection.drag(Point::new(35.0, 25.0), false, false);
        selection.finish_drag(&mut editor).unwrap();
        assert_eq!(selection.selected.len(), 2);
        assert!(selection.selected.contains(&first));
        assert!(selection.selected.contains(&second));

        selection.press(
            editor.document(),
            Point::new(70.0, 70.0),
            visible,
            false,
            false,
        );
        selection.drag(Point::new(80.0, 80.0), false, false);
        selection.finish_drag(&mut editor).unwrap();
        assert!(selection.selected.is_empty());

        selection.press(
            editor.document(),
            Point::new(5.0, 5.0),
            visible,
            false,
            false,
        );
        selection.finish_drag(&mut editor).unwrap();
        assert_eq!(selection.selected.len(), 1);
        assert!(selection.selected.contains(&first));

        selection.press(
            editor.document(),
            Point::new(25.0, -5.0),
            visible,
            false,
            true,
        );
        selection.drag(Point::new(55.0, 25.0), false, false);
        selection.finish_drag(&mut editor).unwrap();
        assert!(selection.selected.contains(&first));
        assert!(selection.selected.contains(&second));
    }

    #[test]
    fn shift_click_toggles_individual_objects() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(first) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let CommandOutcome::Object(second) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(30.0, 0.0, 50.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let visible = Rect::new(-100.0, -100.0, 100.0, 100.0);
        let mut selection = SelectionTool::default();

        selection.press(
            editor.document(),
            Point::new(5.0, 5.0),
            visible,
            false,
            false,
        );
        selection.finish_drag(&mut editor).unwrap();
        selection.press(
            editor.document(),
            Point::new(35.0, 5.0),
            visible,
            false,
            true,
        );
        selection.finish_drag(&mut editor).unwrap();
        assert_eq!(selection.selected.len(), 2);

        selection.press(
            editor.document(),
            Point::new(5.0, 5.0),
            visible,
            false,
            true,
        );
        selection.finish_drag(&mut editor).unwrap();
        assert_eq!(selection.selected.len(), 1);
        assert!(selection.selected.contains(&second));
        assert!(!selection.selected.contains(&first));
    }

    #[test]
    fn moving_selected_object_keeps_multi_selection() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(first) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let CommandOutcome::Object(second) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(30.0, 0.0, 50.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let visible = Rect::new(-100.0, -100.0, 100.0, 100.0);
        let mut selection = SelectionTool::default();
        selection.selected.extend([first, second]);

        selection.press(
            editor.document(),
            Point::new(5.0, 5.0),
            visible,
            false,
            false,
        );
        selection.drag(Point::new(15.0, 10.0), false, false);
        assert_eq!(
            selection.selected_union_bounds(editor.document()),
            Some(Rect::new(10.0, 5.0, 60.0, 25.0))
        );
        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_eq!(selection.selected.len(), 2);
        assert_eq!(
            editor.document().bounds_of(first),
            Some(Rect::new(10.0, 5.0, 30.0, 25.0))
        );
        assert_eq!(
            editor.document().bounds_of(second),
            Some(Rect::new(40.0, 5.0, 60.0, 25.0))
        );

        // Empty space between selected objects is still inside the union box.
        selection.press(
            editor.document(),
            Point::new(35.0, 15.0),
            visible,
            false,
            false,
        );
        assert!(matches!(selection.drag, Some(Drag::Move { .. })));
    }

    #[test]
    fn clicking_unselected_object_replaces_selection_before_move() {
        let (mut editor, layer) = editor_with_layer();
        let make = |editor: &mut Editor, rect| {
            let CommandOutcome::Object(id) = editor
                .execute(Command::CreateRect {
                    layer,
                    rect,
                    name: None,
                })
                .unwrap()
            else {
                unreachable!()
            };
            id
        };
        let first = make(&mut editor, Rect::new(0.0, 0.0, 20.0, 20.0));
        let second = make(&mut editor, Rect::new(30.0, 0.0, 50.0, 20.0));
        let third = make(&mut editor, Rect::new(60.0, 0.0, 80.0, 20.0));
        let mut selection = SelectionTool::default();
        selection.selected.extend([first, second]);
        selection.press(
            editor.document(),
            Point::new(65.0, 5.0),
            Rect::new(-100.0, -100.0, 100.0, 100.0),
            false,
            false,
        );
        assert_eq!(selection.selected.len(), 1);
        assert!(selection.selected.contains(&third));
    }

    #[test]
    fn multi_transform_scales_and_rotates_as_one_undo_group() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(first) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let CommandOutcome::Object(second) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(30.0, 0.0, 50.0, 20.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let first_transform = editor.document().object(first).unwrap().transform;
        let second_transform = editor.document().object(second).unwrap().transform;
        let mut selection = SelectionTool::default();
        selection.selected.insert(first);
        selection.selected.insert(second);

        selection.begin_scale(editor.document(), Handle::East);
        selection.drag(Point::new(100.0, 10.0), false, false);
        assert_eq!(
            selection.selected_union_bounds(editor.document()),
            Some(Rect::new(0.0, 0.0, 100.0, 20.0))
        );
        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_eq!(
            editor.document().bounds_of(first),
            Some(Rect::new(0.0, 0.0, 40.0, 20.0))
        );
        assert_eq!(
            editor.document().bounds_of(second),
            Some(Rect::new(60.0, 0.0, 100.0, 20.0))
        );

        selection.begin_rotate(editor.document(), Point::new(75.0, 10.0));
        selection.drag(Point::new(50.0, 35.0), false, false);
        assert_ne!(
            selection.selected_union_bounds(editor.document()),
            Some(Rect::new(0.0, 0.0, 100.0, 20.0))
        );
        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_ne!(
            editor.document().object(first).unwrap().transform,
            first_transform
        );
        assert_ne!(
            editor.document().object(second).unwrap().transform,
            second_transform
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.document().bounds_of(first),
            Some(Rect::new(0.0, 0.0, 40.0, 20.0))
        );
        assert_eq!(
            editor.document().bounds_of(second),
            Some(Rect::new(60.0, 0.0, 100.0, 20.0))
        );
        editor.undo().unwrap();
        assert_eq!(
            editor.document().bounds_of(first),
            Some(Rect::new(0.0, 0.0, 20.0, 20.0))
        );
        assert_eq!(
            editor.document().bounds_of(second),
            Some(Rect::new(30.0, 0.0, 50.0, 20.0))
        );
    }

    #[test]
    fn side_handle_keeps_opposite_edge_fixed() {
        let bounds = Rect::new(10.0, 20.0, 60.0, 50.0);
        let (_, resized) = scaled_transform(
            bounds,
            Affine::IDENTITY,
            Handle::East,
            Point::new(110.0, 35.0),
            false,
            false,
        );

        assert_eq!(resized.x0, bounds.x0);
        assert_eq!(resized.x1, 110.0);
        assert_eq!(resized.y0, bounds.y0);
        assert_eq!(resized.y1, bounds.y1);
    }

    #[test]
    fn shift_keeps_start_aspect_ratio() {
        let bounds = Rect::new(0.0, 0.0, 50.0, 20.0);
        let (_, resized) = scaled_transform(
            bounds,
            Affine::IDENTITY,
            Handle::SouthEast,
            Point::new(100.0, 30.0),
            true,
            false,
        );

        assert!((resized.width() / resized.height() - 2.5).abs() < 1e-9);
    }

    #[test]
    fn alt_scales_about_center() {
        let bounds = Rect::new(0.0, 10.0, 50.0, 40.0);
        let (_, resized) = scaled_transform(
            bounds,
            Affine::IDENTITY,
            Handle::East,
            Point::new(75.0, 25.0),
            false,
            true,
        );

        assert_eq!(resized.center(), bounds.center());
        assert_eq!(resized.width(), 100.0);
        assert_eq!(resized.height(), bounds.height());
    }

    #[test]
    fn handle_drag_commits_transform_and_undo_restores_it() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let original = editor.document().object(object).unwrap().transform;
        let mut selection = SelectionTool::default();
        selection.selected.insert(object);
        selection.begin_scale(editor.document(), Handle::East);
        selection.drag(Point::new(100.0, 15.0), false, false);

        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_ne!(
            editor.document().object(object).unwrap().transform,
            original
        );

        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(object).unwrap().transform,
            original
        );
    }

    #[test]
    fn rotation_ninety_degrees_stays_about_center() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let mut selection = SelectionTool::default();
        selection.selected.insert(object);
        selection.begin_rotate(editor.document(), Point::new(50.0, 15.0));
        selection.drag(Point::new(25.0, 40.0), false, false);

        let quad = selection.selected_quad(editor.document()).unwrap();
        let center = quad_center(quad);
        assert!((center.x - 25.0).abs() < 1e-9);
        assert!((center.y - 15.0).abs() < 1e-9);
        assert!((quad[0].x - 40.0).abs() < 1e-9);
        assert!((quad[0].y + 10.0).abs() < 1e-9);
    }

    #[test]
    fn shift_snaps_rotation_to_forty_five_degrees() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let mut selection = SelectionTool::default();
        selection.selected.insert(object);
        let center = Point::new(25.0, 15.0);
        selection.begin_rotate(editor.document(), Point::new(50.0, 15.0));
        let angle = 60_f64.to_radians();
        selection.drag(
            Point::new(center.x + 25.0 * angle.cos(), center.y + 25.0 * angle.sin()),
            true,
            false,
        );

        let transform = selection
            .display_transform(editor.document(), object)
            .unwrap();
        let expected = Affine::translate((center.x, center.y))
            * Affine::rotate(std::f64::consts::FRAC_PI_4)
            * Affine::translate((-center.x, -center.y));
        for (actual, expected) in transform.as_coeffs().into_iter().zip(expected.as_coeffs()) {
            assert!((actual - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn rotate_commit_is_undoable() {
        let (mut editor, layer) = editor_with_layer();
        let CommandOutcome::Object(object) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 50.0, 30.0),
                name: None,
            })
            .unwrap()
        else {
            unreachable!()
        };
        let original = editor.document().object(object).unwrap().transform;
        let mut selection = SelectionTool::default();
        selection.selected.insert(object);
        selection.begin_rotate(editor.document(), Point::new(50.0, 15.0));
        selection.drag(Point::new(25.0, 40.0), false, false);

        assert!(selection.finish_drag(&mut editor).unwrap());
        assert_ne!(
            editor.document().object(object).unwrap().transform,
            original
        );
        editor.undo().unwrap();
        assert_eq!(
            editor.document().object(object).unwrap().transform,
            original
        );
    }
}
