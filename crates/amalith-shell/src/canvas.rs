//! The document canvas: renders an `amalith_core::Document` with vello in
//! the region between the rails. No editing yet — just pan / zoom / paint.

use amalith_core::{Document, ObjectId, ObjectKind};
use vello::kurbo::{Affine, Point, Rect, Stroke, Vec2};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::convert;
use crate::text::TextContext;
use crate::theme::Theme;

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

/// Live drag preview: `ids` are drawn offset by `delta` (document space).
#[derive(Clone, Copy)]
pub struct DragPreview<'a> {
    pub ids: &'a [ObjectId],
    pub delta: Vec2,
}

impl DragPreview<'_> {
    fn offset_for(&self, id: ObjectId) -> Affine {
        if self.ids.contains(&id) {
            Affine::translate(self.delta)
        } else {
            Affine::IDENTITY
        }
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

    // Selection outlines (blue box round each selected object's bounds).
    for &id in selection {
        if let Some(b) = crate::select::object_bbox(doc, id) {
            let off = drag.map_or(Affine::IDENTITY, |d| d.offset_for(id));
            let screen = (vt * off).transform_rect_bbox(b);
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                theme.drop_line,
                None,
                &screen,
            );
        }
    }

    scene.pop_layer();
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
    let off = drag.map_or(Affine::IDENTITY, |d| d.offset_for(id));
    let m = vt * off * convert::affine(obj.transform);
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
            for &child in &g.children {
                paint_object(scene, doc, child, vt * off, drag);
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
