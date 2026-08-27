use amalith_commands::{Command, CommandError, Editor};
use amalith_core::{geom, Affine, Document, ObjectId, ObjectKind, ObjectParent, Point, Rect, Vec2};
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
                    for &id in editor.document().children_of(ObjectParent::Layer(layer.id)) {
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
        document
            .children_of(ObjectParent::Layer(layer.id))
            .iter()
            .rev()
            .find_map(|&id| {
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

fn normalized_rect(a: Point, b: Point) -> Rect {
    Rect::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}
