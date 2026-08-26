use amalith_commands::{Command, CommandError, Editor};
use amalith_core::{Affine, Document, ObjectId, ObjectKind, ObjectParent, Point, Rect, Vec2};

use crate::artboard_tool::Handle;
use std::collections::HashSet;

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
        start_transform: Affine,
        preview_transform: Affine,
    },
    Rotate {
        center: Point,
        start_angle: f64,
        start_transform: Affine,
        preview_transform: Affine,
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
            if !self.selected.contains(&id) {
                self.selected.clear();
                self.selected.insert(id);
            }
            self.drag = Some(if duplicate {
                Drag::Duplicate { start: point }
            } else {
                Drag::Move { start: point }
            });
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

    pub(crate) fn begin_scale(&mut self, document: &Document, handle: Handle) {
        let Some(id) = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()
        else {
            return;
        };
        let Some(object) = document.object(id) else {
            return;
        };
        let Some(start_bounds) = document.bounds_of(object.id) else {
            return;
        };
        self.preview_delta = Vec2::ZERO;
        self.drag = Some(Drag::Scale {
            handle,
            start_bounds,
            start_transform: object.transform,
            preview_transform: object.transform,
        });
    }

    pub(crate) fn begin_rotate(&mut self, document: &Document, pointer: Point) {
        let Some(id) = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()
        else {
            return;
        };
        let Some(object) = document.object(id) else {
            return;
        };
        let Some(quad) = object_quad(document, object.id, object.transform) else {
            return;
        };
        let center = quad_center(quad);
        self.preview_delta = Vec2::ZERO;
        self.drag = Some(Drag::Rotate {
            center,
            start_angle: (pointer.y - center.y).atan2(pointer.x - center.x),
            start_transform: object.transform,
            preview_transform: object.transform,
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
                start_transform,
                preview_transform,
            }) => {
                let (transform, _) = scaled_transform(
                    *start_bounds,
                    *start_transform,
                    *handle,
                    point,
                    uniform,
                    from_center,
                );
                *preview_transform = transform;
            }
            Some(Drag::Rotate {
                center,
                start_angle,
                start_transform,
                preview_transform,
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
                *preview_transform = rotate_about_center * *start_transform;
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
                start_transform,
                preview_transform,
                ..
            }) if preview_transform != start_transform => {
                editor.execute(Command::SetTransform {
                    object,
                    transform: preview_transform,
                })?;
                Ok(true)
            }
            Some(Drag::Rotate {
                start_transform,
                preview_transform,
                ..
            }) if preview_transform != start_transform => {
                editor.execute(Command::SetTransform {
                    object,
                    transform: preview_transform,
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
                    self.selected.clear();
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
                preview_transform, ..
            })
            | Some(Drag::Rotate {
                preview_transform, ..
            }) => Some(*preview_transform),
            Some(Drag::Move { .. }) => {
                Some(Affine::translate(self.preview_delta) * document.world_transform(id))
            }
            Some(Drag::Duplicate { .. }) => Some(document.world_transform(id)),
            Some(Drag::Marquee { .. }) => Some(document.world_transform(id)),
            None => Some(document.world_transform(id)),
        }
    }

    pub(crate) fn selected_quad(&self, document: &Document) -> Option<[Point; 4]> {
        let id = (self.selected.len() == 1)
            .then(|| self.selected.iter().next().copied())
            .flatten()?;
        self.display_quad(document, id)
    }

    pub(crate) fn selected_intersects(&self, document: &Document, visible: Rect) -> bool {
        self.selected
            .iter()
            .filter_map(|id| document.bounds_of(*id))
            .any(|bounds| rects_intersect(bounds, visible))
    }

    pub(crate) fn selected_union_bounds(&self, document: &Document) -> Option<Rect> {
        self.selected
            .iter()
            .filter_map(|id| document.bounds_of(*id))
            .reduce(|a, b| a.union(b))
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
