//! The document canvas: renders an `amalith_core::Document` with vello in
//! the region between the rails. No editing yet — just pan / zoom / paint.

use std::collections::HashMap;

use amalith_core::{Document, LineCap, LineJoin, ObjectId, ObjectKind, StrokeAlign, StrokeStyle};
use vello::kurbo::{Affine, BezPath, Cap, Join, Point, Rect, Shape, Stroke, Vec2};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::handles::{self, Handle};
use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;
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
    /// Live anchor drag: `(selected anchors, document-space delta)`.
    pub anchors: Option<(&'a [(ObjectId, usize)], amalith_core::Vec2)>,
}

/// Anchor markers for the Direct Selection tool.
#[derive(Clone, Copy)]
pub struct AnchorView<'a> {
    pub selected: &'a [(ObjectId, usize)],
    /// Paths whose anchors to display.
    pub paths: &'a [ObjectId],
}

/// In-progress Pen path preview (all points in document space).
#[derive(Clone, Copy)]
pub struct PenPreview<'a> {
    pub anchors: &'a [Point],
    pub hover: Point,
    /// The cursor is close enough to the first anchor to close the path.
    pub near_close: bool,
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
    draw_shape: Option<(Tool, Rect)>,
    artboard_ghost: Option<Rect>,
    artboard_handles: Option<[Point; 4]>,
    // Artboard tool active — lighten the pasteboard, like main.
    artboard_mode: bool,
    pen: Option<PenPreview<'_>>,
    anchor_view: Option<AnchorView<'_>>,
) {
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &viewport);

    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        if artboard_mode {
            theme.pasteboard
        } else {
            theme.canvas_bg
        },
        None,
        &viewport,
    );

    let vt = view.to_screen();

    for (i, ab) in doc.artboards().iter().enumerate() {
        let r = vt.transform_rect_bbox(convert::rect(ab.rect));
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgb8(0x1c, 0x1c, 0x1c),
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
            theme.artboard_border,
            None,
            &r,
        );
        text.draw(
            scene,
            &format!("{:02} - {}", i + 1, ab.name),
            11.0,
            theme.artboard_label,
            r.x0,
            r.y0 - 6.0,
        );
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

    // Artboard tool: live rect outline + resize handles.
    if let Some(g) = artboard_ghost {
        let r = vt.transform_rect_bbox(g);
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            theme.select_blue,
            None,
            &r,
        );
    }
    if let Some(q) = artboard_handles {
        let mut outline = BezPath::new();
        outline.move_to(q[0]);
        for p in &q[1..] {
            outline.line_to(*p);
        }
        outline.close_path();
        // Dashed light-blue selected-artboard border, like main.
        scene.stroke(
            &Stroke::new(1.25).with_dashes(0.0, [4.0, 3.0]),
            Affine::IDENTITY,
            Color::from_rgb8(0x6e, 0xbf, 0xff),
            None,
            &outline,
        );
        let handle_fill = Color::from_rgb8(0xe1, 0xf1, 0xff);
        let handle_border = Color::from_rgb8(0x2d, 0x8b, 0xf2);
        for h in handles::Handle::ALL {
            let sq = Rect::from_center_size(handles::handle_pos(q, h), (8.0, 8.0));
            scene.fill(Fill::NonZero, Affine::IDENTITY, handle_fill, None, &sq);
            scene.stroke(
                &Stroke::new(1.25),
                Affine::IDENTITY,
                handle_border,
                None,
                &sq,
            );
        }
    }

    if let Some((tool, r_doc)) = draw_shape {
        let r = vt.transform_rect_bbox(r_doc);
        let fill = theme.select_blue.with_alpha(0.12);
        let stroke = Stroke::new(1.0);
        match tool {
            Tool::Ellipse => {
                let e = vello::kurbo::Ellipse::from_rect(r);
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &e);
                scene.stroke(&stroke, Affine::IDENTITY, theme.select_blue, None, &e);
            }
            Tool::Rectangle | Tool::Select | Tool::Pen => {
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &r);
                scene.stroke(&stroke, Affine::IDENTITY, theme.select_blue, None, &r);
            }
            _ => {
                let p = shape_preview_path(tool, r);
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &p);
                scene.stroke(&stroke, Affine::IDENTITY, theme.select_blue, None, &p);
            }
        }
    }

    // Pen: in-progress path.
    if let Some(pen) = pen {
        if let Some((&first, rest)) = pen.anchors.split_first() {
            let mut path = BezPath::new();
            path.move_to(vt * first);
            for &a in rest {
                path.line_to(vt * a);
            }
            path.line_to(vt * pen.hover);
            scene.stroke(
                &Stroke::new(1.5),
                Affine::IDENTITY,
                theme.select_blue,
                None,
                &path,
            );
            let white = Color::from_rgb8(0xff, 0xff, 0xff);
            for (i, &a) in pen.anchors.iter().enumerate() {
                let hot = i == 0 && pen.near_close;
                let sz = if hot { 9.0 } else { 6.0 };
                let sq = Rect::from_center_size(vt * a, (sz, sz));
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    if hot { theme.select_blue } else { white },
                    None,
                    &sq,
                );
                scene.stroke(
                    &Stroke::new(1.25),
                    Affine::IDENTITY,
                    theme.select_blue,
                    None,
                    &sq,
                );
            }
        }
    }

    // Selection box + transform handles. Direct Selection replaces these
    // with the path contour + node markers below, matching Illustrator's
    // white arrow (no bounding box, no scale/rotate handles).
    if !selection.is_empty() && anchor_view.is_none() {
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
                &Stroke::new(1.25),
                Affine::IDENTITY,
                theme.select_blue,
                None,
                &path,
            );
            let white = Color::from_rgb8(0xff, 0xff, 0xff);
            for h in Handle::ALL {
                let sq = Rect::from_center_size(handles::handle_pos(q, h), (8.0, 8.0));
                scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &sq);
                scene.stroke(
                    &Stroke::new(1.25),
                    Affine::IDENTITY,
                    theme.select_blue,
                    None,
                    &sq,
                );
            }
            let center = Rect::from_center_size(handles::quad_center(q), (6.0, 6.0));
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.select_blue,
                None,
                &center,
            );
        }
    }

    // Direct Selection: contour highlight + anchor markers.
    if let Some(av) = anchor_view {
        // Document-space drag delta for the selected anchors.
        let dv = drag
            .and_then(|d| d.anchors)
            .map(|(_, dv)| Vec2::new(dv.x, dv.y))
            .unwrap_or(Vec2::ZERO);
        let core_dv = amalith_core::Vec2::new(dv.x, dv.y);

        // Outline every selected path (deformed live by any of its
        // anchors currently being dragged).
        for &id in av.paths {
            let idxs: Vec<usize> = av
                .selected
                .iter()
                .filter(|(o, _)| *o == id)
                .map(|(_, i)| *i)
                .collect();
            if let Some(ObjectKind::Path(pd)) = doc.object(id).map(|o| &o.kind) {
                let g = crate::anchors::deformed(&pd.geometry, &idxs, core_dv);
                let m = vt * convert::affine(doc.world_transform(id));
                // Bake the view transform into the geometry and stroke in
                // screen space, so the contour stays a hairline at any
                // zoom instead of scaling with it.
                let screen = m * convert::bez_path(&g);
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    theme.select_blue,
                    None,
                    &screen,
                );
            }
        }

        // Anchor markers. When some of a path's anchors are individually
        // selected (a click or a marquee), the rest go hollow —
        // Illustrator's white arrow. With none selected the path is just
        // "shown", and every anchor is solid blue.
        let white = Color::from_rgb8(0xff, 0xff, 0xff);
        for &id in av.paths {
            let any_sel = av.selected.iter().any(|(o, _)| *o == id);
            for (idx, pos) in crate::anchors::anchors_of(doc, id) {
                let sel = av.selected.contains(&(id, idx));
                let doc_pos = if sel { pos + dv } else { pos };
                let sq = Rect::from_center_size(vt * doc_pos, (7.0, 7.0));
                if sel || !any_sel {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.select_blue, None, &sq);
                } else {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &sq);
                    scene.stroke(
                        &Stroke::new(1.25),
                        Affine::IDENTITY,
                        theme.select_blue,
                        None,
                        &sq,
                    );
                }
            }
        }
    }

    scene.pop_layer();
}

/// Screen-space outline for a primitive shape tool's rubber-band preview.
fn shape_preview_path(tool: Tool, r: Rect) -> BezPath {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let (cx, cy) = (r.center().x, r.center().y);
    let (rx, ry) = (r.width() * 0.5, r.height() * 0.5);
    let mut p = BezPath::new();
    match tool {
        Tool::RoundedRect => {
            return vello::kurbo::RoundedRect::new(
                r.x0,
                r.y0,
                r.x1,
                r.y1,
                r.width().min(r.height()) * 0.18,
            )
            .to_path(0.1);
        }
        Tool::Polygon => {
            for i in 0..6 {
                let a = -FRAC_PI_2 + i as f64 * TAU / 6.0;
                let pt = (cx + rx * a.cos(), cy + ry * a.sin());
                if i == 0 {
                    p.move_to(pt);
                } else {
                    p.line_to(pt);
                }
            }
        }
        Tool::Star => {
            for i in 0..10 {
                let a = -FRAC_PI_2 + i as f64 * PI / 5.0;
                let k = if i % 2 == 0 { 1.0 } else { 0.45 };
                let pt = (cx + rx * k * a.cos(), cy + ry * k * a.sin());
                if i == 0 {
                    p.move_to(pt);
                } else {
                    p.line_to(pt);
                }
            }
        }
        _ => {}
    }
    p.close_path();
    p
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

fn kurbo_cap(c: LineCap) -> Cap {
    match c {
        LineCap::Butt => Cap::Butt,
        LineCap::Round => Cap::Round,
        LineCap::Square => Cap::Square,
    }
}

fn kurbo_join(j: LineJoin) -> Join {
    match j {
        LineJoin::Miter => Join::Miter,
        LineJoin::Round => Join::Round,
        LineJoin::Bevel => Join::Bevel,
    }
}

/// A vello `Stroke` for `width` under `style` — cap, join, miter limit,
/// and dash pattern. Alignment is applied separately by [`stroke_path`].
fn stroke_spec(width: f64, style: &StrokeStyle) -> Stroke {
    let mut s = Stroke::new(width)
        .with_caps(kurbo_cap(style.cap))
        .with_join(kurbo_join(style.join))
        .with_miter_limit(style.miter_limit.max(1.0));
    if let Some(pattern) = style.dash_pattern() {
        s = s.with_dashes(style.dash_offset, pattern);
    }
    s
}

/// Stroke `bp` (object-local space; `m` maps it to the screen) at `width`
/// under `style`. For a closed path with `align` set to Inside / Outside,
/// a double-width stroke is clipped to the wanted side of the outline.
fn stroke_path(
    scene: &mut Scene,
    m: Affine,
    color: Color,
    bp: &BezPath,
    width: f64,
    style: &StrokeStyle,
    closed: bool,
) {
    if !closed || style.align == StrokeAlign::Center || width <= 0.0 {
        scene.stroke(&stroke_spec(width, style), m, color, None, bp);
        return;
    }
    let wide = stroke_spec(width * 2.0, style);
    match style.align {
        StrokeAlign::Inside => {
            scene.push_clip_layer(Fill::NonZero, m, bp);
            scene.stroke(&wide, m, color, None, bp);
            scene.pop_layer();
        }
        StrokeAlign::Outside => {
            let pad = width + 2.0;
            let outer = bp.bounding_box().inflate(pad, pad);
            let mut ring = BezPath::new();
            ring.extend(outer.path_elements(0.1));
            ring.extend(bp.elements().iter().copied());
            scene.push_clip_layer(Fill::EvenOdd, m, &ring);
            scene.stroke(&wide, m, color, None, bp);
            scene.pop_layer();
        }
        StrokeAlign::Center => unreachable!(),
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
    let style = obj.appearance.stroke_style;
    let paint_path = |scene: &mut Scene, bp: &vello::kurbo::BezPath| {
        // Open paths (a line, an unclosed pen path) don't get a fill.
        let closed = bp
            .elements()
            .iter()
            .any(|e| matches!(e, vello::kurbo::PathEl::ClosePath));
        if closed {
            if let Some(c) = fill {
                scene.fill(Fill::NonZero, m, c, None, bp);
            }
        }
        if let Some(c) = stroke {
            stroke_path(scene, m, c, bp, sw, &style, closed);
        }
    };

    match &obj.kind {
        ObjectKind::Path(pd) => {
            // Live anchor drag: deform this path's geometry.
            let idxs: Vec<usize> = drag
                .and_then(|d| d.anchors)
                .map(|(sel, _)| {
                    sel.iter()
                        .filter(|(o, _)| *o == id)
                        .map(|(_, i)| *i)
                        .collect()
                })
                .unwrap_or_default();
            if let (false, Some((_, dv))) = (idxs.is_empty(), drag.and_then(|d| d.anchors)) {
                let g = crate::anchors::deformed(&pd.geometry, &idxs, dv);
                paint_path(scene, &convert::bez_path(&g));
            } else {
                paint_path(scene, &convert::bez_path(&pd.geometry));
            }
        }
        ObjectKind::CompoundPath(cp) => {
            for sub in &cp.subpaths {
                paint_path(scene, &convert::bez_path(sub));
            }
        }
        ObjectKind::Group(g) => {
            // `m` already folds in this group's own transform (committed
            // or previewed) plus any drag offset — children render
            // relative to it. Dropping it here is what made a
            // scaled/rotated/moved group snap back on release.
            let child_drag = if replacement.is_some() { None } else { drag };
            for &child in &g.children {
                paint_object(scene, doc, child, m, child_drag);
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
