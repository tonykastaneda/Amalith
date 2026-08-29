//! The document canvas: renders an `amalith_core::Document` with vello in
//! the region between the rails. No editing yet — just pan / zoom / paint.

use std::collections::HashMap;

use amalith_core::{Document, ObjectId, ObjectKind};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke, Vec2};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::handles::{self, Handle};
use crate::text::TextContext;
use crate::theme::Theme;
use crate::{convert, select};

/// Pan (screen px) and zoom for the document view.
#[derive(Clone, Copy, Debug)]
pub struct CanvasView {
    pub pan: Vec2,
    pub zoom: f64,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            pan: Vec2::new(80.0, 80.0),
            zoom: 0.5,
        }
    }
}

impl CanvasView {
    /// Document space → screen space.
    pub fn to_screen(&self) -> Affine {
        Affine::translate(self.pan) * Affine::scale(self.zoom)
    }

    /// Multiply zoom by `factor`, keeping the document point under `pivot`
    /// (screen px) fixed.
    pub fn zoom_at(&mut self, factor: f64, pivot: Point) {
        let new_zoom = (self.zoom * factor).clamp(0.02, 64.0);
        let k = new_zoom / self.zoom;
        self.pan = pivot.to_vec2() + (self.pan - pivot.to_vec2()) * k;
        self.zoom = new_zoom;
    }
}

/// Live drag preview.
///
/// - A move offsets the dragged objects by `delta` (document space).
/// - A duplicate (`dup`) keeps the originals put and draws a copy at
///   `delta`.
/// - A scale/rotate supplies `xf`: a full replacement transform per
///   dragged object, which wins over `delta`/`dup`.
#[derive(Clone, Copy)]
pub struct DragPreview<'a> {
    pub ids: &'a [ObjectId],
    pub delta: Vec2,
    pub dup: bool,
    pub xf: Option<&'a HashMap<ObjectId, Affine>>,
}

impl DragPreview<'_> {
    /// Transform for a dragged object's own rendering (translate only).
    fn object_offset(&self, id: ObjectId) -> Affine {
        if !self.dup && self.ids.contains(&id) {
            Affine::translate(self.delta)
        } else {
            Affine::IDENTITY
        }
    }

    /// Full replacement transform for `id` during a scale/rotate.
    fn replacement(&self, id: ObjectId) -> Option<Affine> {
        self.xf.and_then(|m| m.get(&id).copied())
    }

    fn is_dragged(&self, id: ObjectId) -> bool {
        self.ids.contains(&id)
    }
}

/// Paint the document into `viewport` (the screen rect between the rails).
#[allow(clippy::too_many_arguments)]
pub fn paint(
    scene: &mut Scene,
    doc: &Document,
    view: &CanvasView,
    viewport: Rect,
    theme: &Theme,
    text: &mut TextContext,
    selection: &[ObjectId],
    drag: Option<DragPreview<'_>>,
) {
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &viewport);

    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.canvas_bg,
        None,
        &viewport,
    );

    let vt = view.to_screen();

    for ab in doc.artboards() {
        let r = vt.transform_rect_bbox(convert::rect(ab.rect));
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgb8(0x12, 0x12, 0x12),
            None,
            &r.with_origin(Point::new(r.x0 + 3.0, r.y0 + 3.0)),
        );
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgb8(0xff, 0xff, 0xff),
            None,
            &r,
        );
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            Color::from_rgb8(0x00, 0x00, 0x00),
            None,
            &r,
        );
        text.draw(scene, &ab.name, 11.0, theme.text_dim, r.x0, r.y0 - 6.0);
    }

    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in &layer.children {
            paint_object(scene, doc, id, vt, drag);
        }
    }

    // Duplicate drag: draw the copy-to-be at the offset, full opacity —
    // the originals stay put underneath and the blue outline marks it.
    if let Some(d) = drag.filter(|d| d.dup) {
        for &id in d.ids {
            paint_object(scene, doc, id, vt * Affine::translate(d.delta), None);
        }
    }

    // Selection box + transform handles.
    if !selection.is_empty() {
        // The oriented box, in screen px, transformed by any live
        // scale/rotate preview.
        let quad = select::selection_quad(doc, selection).map(|q| {
            let extra = match drag {
                Some(d) if d.xf.is_some() => xf_for_quad(doc, selection, d),
                Some(d) if d.is_dragged(selection[0]) => Affine::translate(d.delta),
                _ => Affine::IDENTITY,
            };
            q.map(|p| vt * extra * p)
        });
        if let Some(q) = quad {
            let mut path = BezPath::new();
            path.move_to(q[0]);
            for p in &q[1..] {
                path.line_to(*p);
            }
            path.close_path();
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                theme.drop_line,
                None,
                &path,
            );
            for h in Handle::ALL {
                let c = handles::handle_pos(q, h);
                let sq = Rect::from_center_size(c, (7.0, 7.0));
                scene.fill(Fill::NonZero, Affine::IDENTITY, theme.canvas_bg, None, &sq);
                scene.stroke(
                    &Stroke::new(1.0),
                    Affine::IDENTITY,
                    theme.drop_line,
                    None,
                    &sq,
                );
            }
        }
    }

    scene.pop_layer();
}

/// The extra affine to apply to the whole selection quad given a
/// scale/rotate preview — take the first dragged object's replacement
/// relative to its start transform.
fn xf_for_quad(doc: &Document, selection: &[ObjectId], d: DragPreview<'_>) -> Affine {
    let id = selection[0];
    match (d.replacement(id), doc.object(id)) {
        (Some(new), Some(obj)) => new * convert::affine(obj.transform).inverse(),
        _ => Affine::IDENTITY,
    }
}

fn paint_object(
    scene: &mut Scene,
    doc: &Document,
    id: ObjectId,
    vt: Affine,
    drag: Option<DragPreview<'_>>,
) {
    let Some(obj) = doc.object(id) else {
        return;
    };
    if !obj.visible {
        return;
    }
    let off = drag.map_or(Affine::IDENTITY, |d| d.object_offset(id));
    let replacement = drag.and_then(|d| d.replacement(id));
    let m = match replacement {
        Some(a) => vt * a,
        None => vt * off * convert::affine(obj.transform),
    };
    let fill = obj.appearance.fill.color().map(convert::color);
    let stroke = obj.appearance.stroke.color().map(convert::color);
    let sw = obj.appearance.stroke_width;
    let paint_path = |scene: &mut Scene, bp: &vello::kurbo::BezPath| {
        if let Some(c) = fill {
            scene.fill(Fill::NonZero, m, c, None, bp);
        }
        if let Some(c) = stroke {
            scene.stroke(&Stroke::new(sw), m, c, None, bp);
        }
    };

    match &obj.kind {
        ObjectKind::Path(pd) => paint_path(scene, &convert::bez_path(&pd.geometry)),
        ObjectKind::CompoundPath(cp) => {
            for sub in &cp.subpaths {
                paint_path(scene, &convert::bez_path(sub));
            }
        }
        ObjectKind::Group(g) => {
            let (child_vt, child_drag) = match replacement {
                Some(a) => (vt * a, None),
                None => (vt * off, drag),
            };
            for &child in &g.children {
                paint_object(scene, doc, child, child_vt, child_drag);
            }
        }
        other => {
            if let Some(b) = other.own_local_bounds() {
                scene.stroke(
                    &Stroke::new(1.0),
                    m,
                    Color::from_rgb8(0x88, 0x88, 0x88),
                    None,
                    &convert::rect(b),
                );
            }
        }
    }
}
