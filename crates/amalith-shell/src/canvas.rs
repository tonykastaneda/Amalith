//! The document canvas: renders an `amalith_core::Document` with vello in
//! the region between the rails. No editing yet — just pan / zoom / paint.

use std::collections::HashMap;

use amalith_core::{
    ArtboardId, AssetId, Document, LineCap, LineJoin, ObjectId, ObjectKind, StrokeAlign,
    StrokeStyle,
};
use vello::kurbo::{Affine, BezPath, Cap, Circle, Join, Line, Point, Rect, Shape, Stroke, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

use crate::handles::{self, Handle};
use crate::lod::ImageLods;
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

/// Floor / ceiling for view zoom (0.1% … 25600%). LOD pick is separate.
pub const ZOOM_MIN: f64 = 0.001;
pub const ZOOM_MAX: f64 = 256.0;

/// Hairline colour for Outline (wireframe) view — near-black, reads on the
/// white artboard.
const OUTLINE_INK: Color = Color::from_rgb8(0x20, 0x20, 0x20);

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
        let new_zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let k = new_zoom / self.zoom;
        self.pan = pivot.to_vec2() + (self.pan - pivot.to_vec2()) * k;
        self.zoom = new_zoom;
    }
}

/// Tab / status zoom, with extra decimals when you're far out.
pub fn zoom_percent_label(zoom: f64) -> String {
    let p = zoom * 100.0;
    if p >= 10.0 {
        format!("{p:.0}%")
    } else if p >= 1.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.2}%")
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
    /// Live handle drag: `(object, anchor ordinal, side, local-space delta)`.
    pub handle: Option<(ObjectId, usize, amalith_core::HandleSide, amalith_core::Vec2)>,
    /// Live area-text-box resize: each frame re-wraps at its previewed
    /// width/height (document px) with its origin shifted by `origin_delta`.
    pub text_boxes: &'a [TextBoxPreview],
}

/// One area-text box being resized by a Selection-tool handle drag.
#[derive(Clone, Copy)]
pub struct TextBoxPreview {
    pub id: ObjectId,
    pub width: f64,
    pub height: f64,
    pub origin_delta: Vec2,
}

/// Anchor markers for the Direct Selection tool.
#[derive(Clone, Copy)]
pub struct AnchorView<'a> {
    pub selected: &'a [(ObjectId, usize)],
    /// Paths whose anchors to display.
    pub paths: &'a [ObjectId],
    /// A read-only "hold Space to see the nodes" peek from the Selection
    /// tool — keep the bounding box drawn underneath.
    pub peek: bool,
    /// The anchor the pointer is over, drawn enlarged as a "you'll edit
    /// this one" affordance.
    pub hover: Option<(ObjectId, usize)>,
}

/// One placed anchor of an in-progress Pen path, in document space.
/// `handle_in` / `handle_out` are absolute control points; `None` means
/// that side of the anchor is a straight segment.
#[derive(Clone, Copy)]
pub struct PenAnchor {
    pub point: Point,
    pub handle_in: Option<Point>,
    pub handle_out: Option<Point>,
    pub mode: amalith_core::HandleMode,
}

/// In-progress Pen path preview (all points in document space).
#[derive(Clone, Copy)]
pub struct PenPreview<'a> {
    pub anchors: &'a [PenAnchor],
    pub hover: Point,
    /// The cursor is close enough to the first anchor to close the path.
    pub near_close: bool,
    /// The appearance a commit would give the path — drawn live under the
    /// blue guide so the path looks finished before you end it.
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    pub stroke_w: f64,
    pub style: StrokeStyle,
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

/// Default inset from the canvas viewport where objects start to cull.
/// The live value is `Settings::cull_inset` (Preferences ▸ Debug).
pub const CULL_INSET: f64 = 48.0;

/// Screen-space rect used for object / image culling, inset from `viewport`
/// by `inset` logical px so the boundary sits on the canvas, not under the
/// rails.
pub fn cull_rect(viewport: Rect, inset: f64) -> Rect {
    let i = inset.max(0.0);
    Rect::new(
        viewport.x0 + i,
        viewport.y0 + i,
        (viewport.x1 - i).max(viewport.x0 + i + 1.0),
        (viewport.y1 - i).max(viewport.y0 + i + 1.0),
    )
}

/// Paint the document into `viewport` (the screen rect between the rails).
#[allow(clippy::too_many_arguments)]
/// Render just the document content inside `src` (document space) into a
/// fresh transparent [`Scene`] sized `src` × `scale` pixels — no chrome,
/// no artboard frame, no selection. Used by Export for Screens.
pub fn export_scene(
    doc: &Document,
    src: Rect,
    scale: f64,
    bg: Option<Color>,
    images: &HashMap<AssetId, ImageLods>,
    outline: bool,
    text: &mut TextContext,
) -> Scene {
    let mut scene = Scene::new();
    let px = Rect::new(
        0.0,
        0.0,
        (src.width() * scale).max(1.0),
        (src.height() * scale).max(1.0),
    );
    let vt = Affine::scale(scale) * Affine::translate((-src.x0, -src.y0));
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &px);
    if let Some(c) = bg {
        scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &px);
    }
    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in &layer.children {
            paint_object(&mut scene, doc, id, vt, scale, px, None, text, None, images, outline);
        }
    }
    scene.pop_layer();
    scene
}

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
    // The text object open in the Type tool — drawn live by the shell
    // overlay, so its committed content is skipped here.
    editing_text: Option<ObjectId>,
    images: &HashMap<AssetId, ImageLods>,
    key_object: Option<ObjectId>,
    cull_inset: f64,
    show_cull: bool,
    active_artboard: Option<ArtboardId>,
    // Outline (wireframe) view — objects render as hairline contours only.
    outline: bool,
    // Isolation mode: dim everything, then repaint this group on top.
    isolate: Option<ObjectId>,
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
    let cull = cull_rect(viewport, cull_inset);

    for (i, ab) in doc.artboards().iter().enumerate() {
        let r = vt.transform_rect_bbox(convert::rect(ab.rect));
        let active = active_artboard == Some(ab.id);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgb8(0x1c, 0x1c, 0x1c),
            None,
            &r.with_origin(Point::new(r.x0 + 3.0, r.y0 + 3.0)),
        );
        // Paper: the artboard's background fill; transparent (the default)
        // still shows as white paper on the canvas, like Illustrator — the
        // fill only changes the exported background.
        let paper = ab
            .fill
            .map(|c| Color::new([c.r, c.g, c.b, c.a]))
            .unwrap_or(Color::from_rgb8(0xff, 0xff, 0xff));
        scene.fill(Fill::NonZero, Affine::IDENTITY, paper, None, &r);
        // Thin black outline on every artboard (Illustrator); the active
        // one gets a slightly heavier line.
        scene.stroke(
            &Stroke::new(if active { 1.6 } else { 1.0 }),
            Affine::IDENTITY,
            Color::from_rgb8(0x00, 0x00, 0x00),
            None,
            &r,
        );
        // Name label above the artboard. The active one reads brighter
        // and bold; same size as the rest.
        let name = format!("{:02} - {}", i + 1, ab.name);
        if active {
            text.draw_bold(scene, &name, 11.0, theme.text, r.x0, r.y0 - 6.0);
        } else {
            text.draw(scene, &name, 11.0, theme.artboard_label, r.x0, r.y0 - 6.0);
        }
    }

    for layer in doc.layers() {
        if !layer.visible {
            continue;
        }
        for &id in &layer.children {
            paint_object(
                scene,
                doc,
                id,
                vt,
                view.zoom,
                cull,
                drag,
                text,
                editing_text,
                images,
                outline,
            );
        }
    }

    // Isolation mode: scrim the whole canvas, then repaint the isolated
    // group at full strength so only its contents are editable-looking.
    if let Some(root) = isolate.filter(|id| doc.object(*id).is_some()) {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.canvas_bg.with_alpha(0.62),
            None,
            &viewport,
        );
        let parent_m = match doc.object(root).map(|o| o.parent) {
            Some(amalith_core::ObjectParent::Group(g)) => {
                vt * convert::affine(doc.world_transform(g))
            }
            _ => vt,
        };
        paint_object(
            scene, doc, root, parent_m, view.zoom, cull, drag, text, editing_text, images, outline,
        );
    }

    // Duplicate drag: draw the copy-to-be at the offset, full opacity —
    // the originals stay put underneath and the blue outline marks it.
    if let Some(d) = drag.filter(|d| d.dup) {
        for &id in d.ids {
            paint_object(
                scene,
                doc,
                id,
                vt * Affine::translate(d.delta),
                view.zoom,
                cull,
                None,
                text,
                editing_text,
                images,
                outline,
            );
        }
    }

    // Artboard tool: live rect outline + resize handles.
    if let Some(g) = artboard_ghost {
        let r = vt.transform_rect_bbox(g);
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            theme.accent,
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
        let fill = theme.accent.with_alpha(0.12);
        let stroke = Stroke::new(1.0);
        match tool {
            Tool::Ellipse => {
                let e = vello::kurbo::Ellipse::from_rect(r);
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &e);
                scene.stroke(&stroke, Affine::IDENTITY, theme.accent, None, &e);
            }
            Tool::Rectangle | Tool::Select | Tool::Pen => {
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &r);
                scene.stroke(&stroke, Affine::IDENTITY, theme.accent, None, &r);
            }
            Tool::Text => {
                // Area-text box: dashed outline, no fill.
                scene.stroke(
                    &Stroke::new(1.0).with_dashes(0.0, [4.0, 3.0]),
                    Affine::IDENTITY,
                    theme.accent,
                    None,
                    &r,
                );
            }
            Tool::Line => {
                // `r_doc` carries the endpoints in its corners, not a bbox.
                let a = vt * Point::new(r_doc.x0, r_doc.y0);
                let b = vt * Point::new(r_doc.x1, r_doc.y1);
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    theme.accent,
                    None,
                    &Line::new(a, b),
                );
            }
            _ => {
                let p = shape_preview_path(tool, r);
                scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &p);
                scene.stroke(&stroke, Affine::IDENTITY, theme.accent, None, &p);
            }
        }
    }

    // Pen: in-progress path (real bezier segments, matching how
    // `amalith_core::subpaths_to_bezpath` will flatten the committed path).
    if let Some(pen) = pen {
        if let Some((first, rest)) = pen.anchors.split_first() {
            let seg = |path: &mut BezPath, a: &PenAnchor, b: &PenAnchor| {
                match (a.handle_out, b.handle_in) {
                    (None, None) => path.line_to(vt * b.point),
                    _ => path.curve_to(
                        vt * a.handle_out.unwrap_or(a.point),
                        vt * b.handle_in.unwrap_or(b.point),
                        vt * b.point,
                    ),
                }
            };
            // The path through the anchors already placed. This is what a
            // commit would produce, so it carries the live fill + real
            // stroke — it does *not* extend to the pen cursor.
            let mut solid = BezPath::new();
            solid.move_to(vt * first.point);
            let mut prev = first;
            for a in rest {
                seg(&mut solid, prev, a);
                prev = a;
            }
            if pen.near_close {
                solid.close_path();
            }
            if let Some(c) = pen.fill {
                scene.fill(Fill::NonZero, Affine::IDENTITY, c, None, &solid);
            }
            if let Some(c) = pen.stroke {
                scene.stroke(
                    &stroke_spec(pen.stroke_w, &pen.style, view.zoom.max(1e-6)),
                    Affine::IDENTITY,
                    c,
                    None,
                    &solid,
                );
            }

            // The blue guide adds the rubber-band segment out to the
            // cursor — a hairline, never the object's stroke weight.
            let mut guide = solid.clone();
            if !pen.near_close {
                let cursor = PenAnchor {
                    point: pen.hover,
                    handle_in: None,
                    handle_out: None,
                    mode: amalith_core::HandleMode::Corner,
                };
                seg(&mut guide, prev, &cursor);
            }
            scene.stroke(&Stroke::new(1.5), Affine::IDENTITY, theme.accent, None, &guide);

            let white = Color::from_rgb8(0xff, 0xff, 0xff);
            // Handle sticks + round handle dots.
            for a in pen.anchors {
                let ps = vt * a.point;
                for h in [a.handle_in, a.handle_out].into_iter().flatten() {
                    let hs = vt * h;
                    scene.stroke(
                        &Stroke::new(1.0),
                        Affine::IDENTITY,
                        theme.accent,
                        None,
                        &Line::new(ps, hs),
                    );
                    let dot = Circle::new(hs, 3.0);
                    scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &dot);
                    scene.stroke(&Stroke::new(1.25), Affine::IDENTITY, theme.accent, None, &dot);
                }
            }
            // Square anchor markers.
            for (i, a) in pen.anchors.iter().enumerate() {
                let hot = i == 0 && pen.near_close;
                let sz = if hot { 9.0 } else { 6.0 };
                let sq = Rect::from_center_size(vt * a.point, (sz, sz));
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    if hot { theme.accent } else { white },
                    None,
                    &sq,
                );
                scene.stroke(&Stroke::new(1.25), Affine::IDENTITY, theme.accent, None, &sq);
            }
        }
    }

    // Selection box + transform handles. Direct Selection replaces these
    // with the path contour + node markers below, matching Illustrator's
    // white arrow (no bounding box, no scale/rotate handles).
    if !selection.is_empty() && anchor_view.map_or(true, |av| av.peek) {
        // The oriented box, in screen px, transformed by any live
        // scale/rotate preview. During a text-box resize it follows the
        // union of the previewed frames.
        let text_box_quad = drag
            .map(|d| d.text_boxes)
            .filter(|tbs| !tbs.is_empty())
            .and_then(|tbs| {
                let mut acc: Option<Rect> = None;
                for tb in tbs {
                    let c = doc.object(tb.id)?.transform.as_coeffs();
                    let (x0, y0) = (c[4] + tb.origin_delta.x, c[5] + tb.origin_delta.y);
                    let r = Rect::new(x0, y0, x0 + tb.width, y0 + tb.height);
                    acc = Some(acc.map_or(r, |a| a.union(r)));
                }
                let r = acc?;
                Some(
                    [
                        Point::new(r.x0, r.y0),
                        Point::new(r.x1, r.y0),
                        Point::new(r.x1, r.y1),
                        Point::new(r.x0, r.y1),
                    ]
                    .map(|p| vt * p),
                )
            });
        let quad = text_box_quad.or_else(|| {
            select::selection_quad(doc, selection).map(|q| {
                let extra = match drag {
                    Some(d) if d.xf.is_some() => xf_for_quad(doc, selection, d),
                    Some(d) if d.is_dragged(selection[0]) => Affine::translate(d.delta),
                    _ => Affine::IDENTITY,
                };
                q.map(|p| vt * extra * p)
            })
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
                theme.accent,
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
                    theme.accent,
                    None,
                    &sq,
                );
            }
            // Text-thread ports + the auto-fit tab on a single selected
            // area-text frame.
            if selection.len() == 1 {
                if let Some(ObjectKind::Text(td)) = doc.object(selection[0]).map(|o| &o.kind) {
                    let is_area = matches!(td.kind, amalith_core::TextKind::Area { .. });
                    // Point/Area convert widget: a dot off the right edge.
                    // Filled = area, hollow = point. Double-click toggles.
                    {
                        let redge = Point::new(
                            (q[1].x + q[2].x) * 0.5,
                            (q[1].y + q[2].y) * 0.5,
                        );
                        let dot_c = Point::new(redge.x + 16.0, redge.y);
                        scene.stroke(
                            &Stroke::new(1.25),
                            Affine::IDENTITY,
                            theme.accent,
                            None,
                            &Line::new(redge, dot_c),
                        );
                        let dot = vello::kurbo::Circle::new(dot_c, 4.5);
                        let paper = Color::from_rgb8(0xff, 0xff, 0xff);
                        scene.fill(
                            Fill::NonZero,
                            Affine::IDENTITY,
                            if is_area { theme.accent } else { paper },
                            None,
                            &dot,
                        );
                        scene.stroke(
                            &Stroke::new(1.25),
                            Affine::IDENTITY,
                            if is_area { paper } else { theme.accent },
                            None,
                            &dot,
                        );
                    }
                    if is_area {
                        // Auto-fit tab: bottom-edge centre, on a short
                        // stem below the box. Double-click snaps the box
                        // height to the text. A filled accent square so it
                        // reads distinct from the hollow scale handles.
                        let edge = Point::new(
                            (q[2].x + q[3].x) * 0.5,
                            (q[2].y + q[3].y) * 0.5,
                        );
                        let bmid = Point::new(edge.x, edge.y + 22.0);
                        scene.stroke(
                            &Stroke::new(1.25),
                            Affine::IDENTITY,
                            theme.accent,
                            None,
                            &Line::new(edge, bmid),
                        );
                        let tab = Rect::from_center_size(bmid, (9.0, 9.0));
                        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &tab);
                        scene.stroke(
                            &Stroke::new(1.25),
                            Affine::IDENTITY,
                            Color::from_rgb8(0xff, 0xff, 0xff),
                            None,
                            &tab,
                        );
                        let white = Color::from_rgb8(0xff, 0xff, 0xff);
                        let red = Color::from_rgb8(0xd0, 0x30, 0x30);
                        let port = |scene: &mut Scene, c: Point, fill: Color, border: Color| {
                            let r = Rect::from_center_size(c, (11.0, 11.0));
                            scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &r);
                            scene.stroke(&Stroke::new(1.25), Affine::IDENTITY, border, None, &r);
                        };
                        let overset = crate::thread::frame_overset(doc, text, selection[0]);
                        // Ports sit just off the corner so they don't cover
                        // the scale handle there (Illustrator).
                        // In-port (top-left) — only when text flows in.
                        if td.thread_prev.is_some() {
                            port(
                                scene,
                                Point::new(q[0].x - 2.0, q[0].y - 17.0),
                                theme.accent,
                                theme.accent,
                            );
                        }
                        // Out-port (bottom-right), lifted above the handle.
                        let op = Point::new(q[2].x + 2.0, q[2].y - 17.0);
                        if td.thread_next.is_some() {
                            port(scene, op, theme.accent, theme.accent);
                        } else if overset {
                            port(scene, op, white, red);
                            let pl = Stroke::new(1.5);
                            scene.stroke(
                                &pl,
                                Affine::IDENTITY,
                                red,
                                None,
                                &Line::new(
                                    Point::new(op.x - 3.0, op.y),
                                    Point::new(op.x + 3.0, op.y),
                                ),
                            );
                            scene.stroke(
                                &pl,
                                Affine::IDENTITY,
                                red,
                                None,
                                &Line::new(
                                    Point::new(op.x, op.y - 3.0),
                                    Point::new(op.x, op.y + 3.0),
                                ),
                            );
                        } else {
                            port(scene, op, white, theme.accent);
                        }
                    }
                }
            }

            // In a multi-selection the union box doesn't show where each
            // text frame is — outline every selected area-text frame in
            // its own right so its bounds are always visible.
            if selection.len() >= 2 {
                let baby = Color::from_rgb8(0x8f, 0xc2, 0xf5);
                for &id in selection {
                    let Some(ObjectKind::Text(t)) = doc.object(id).map(|o| &o.kind) else {
                        continue;
                    };
                    if !matches!(t.kind, amalith_core::TextKind::Area { .. }) {
                        continue;
                    }
                    // Follow a live resize preview, else the committed box.
                    let fq = if let Some(tb) =
                        drag.and_then(|d| d.text_boxes.iter().copied().find(|tb| tb.id == id))
                    {
                        let c = doc.object(id).map(|o| o.transform.as_coeffs()).unwrap();
                        let (x0, y0) = (c[4] + tb.origin_delta.x, c[5] + tb.origin_delta.y);
                        Some([
                            Point::new(x0, y0),
                            Point::new(x0 + tb.width, y0),
                            Point::new(x0 + tb.width, y0 + tb.height),
                            Point::new(x0, y0 + tb.height),
                        ])
                    } else {
                        select::selection_quad(doc, &[id])
                    };
                    if let Some(fq) = fq {
                        let fq = fq.map(|p| vt * p);
                        let mut p = BezPath::new();
                        p.move_to(fq[0]);
                        for c in &fq[1..] {
                            p.line_to(*c);
                        }
                        p.close_path();
                        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, baby, None, &p);
                    }
                }
                for &id in selection {
                    let Some(ObjectKind::Text(t)) = doc.object(id).map(|o| &o.kind) else {
                        continue;
                    };
                    let Some(next) = t.thread_next else { continue };
                    if !selection.contains(&next) {
                        continue;
                    }
                    let (Some(qa), Some(qb)) = (
                        select::selection_quad(doc, &[id]),
                        select::selection_quad(doc, &[next]),
                    ) else {
                        continue;
                    };
                    let seg = Line::new(
                        vt * qa[2] + Vec2::new(2.0, -17.0),
                        vt * qb[0] + Vec2::new(-2.0, -17.0),
                    );
                    scene.stroke(
                        &Stroke::new(5.0),
                        Affine::IDENTITY,
                        Color::from_rgb8(0xff, 0xff, 0xff).with_alpha(0.9),
                        None,
                        &seg,
                    );
                    scene.stroke(&Stroke::new(3.0), Affine::IDENTITY, baby, None, &seg);
                }
            }
        }

        // Baseline guide under every visible line of a selected text
        // object (Illustrator draws these while the frame is selected).
        for &id in selection {
            let Some(obj) = doc.object(id) else { continue };
            let ObjectKind::Text(td) = &obj.kind else { continue };
            // A threaded downstream frame shows its slice of the story.
            let owned: Option<amalith_core::TextData> = if td.is_threaded() {
                crate::thread::head(doc, id).map(|head| {
                    let story = match doc.object(head).map(|o| &o.kind) {
                        Some(ObjectKind::Text(h)) => h.content.clone(),
                        _ => String::new(),
                    };
                    let start = crate::thread::slices(doc, head, text)
                        .get(&id)
                        .map_or(0, |s| s.start.min(story.len()));
                    let mut f = td.clone();
                    f.content = story[start..].to_string();
                    f
                })
            } else {
                None
            };
            // Follow a live resize: re-wrap at the previewed size / origin.
            let tb = drag.and_then(|d| d.text_boxes.iter().copied().find(|tb| tb.id == id));
            let mut td = owned.unwrap_or_else(|| td.clone());
            if let Some(tb) = tb {
                td.kind = amalith_core::TextKind::Area {
                    width: tb.width,
                    height: Some(tb.height),
                };
            }
            if td.content.is_empty() {
                continue;
            }
            let (frame_w, frame_h, area) = match td.kind {
                amalith_core::TextKind::Area { width, height } => (width, height, true),
                amalith_core::TextKind::Point => (0.0, None, false),
            };
            let mut m = vt * convert::affine(doc.world_transform(id));
            if let Some(tb) = tb {
                m *= Affine::translate(tb.origin_delta);
            }
            let guide = theme.accent.with_alpha(0.55);
            let layout = crate::textedit::td_layout(text, &td);
            let dx = if area {
                0.0
            } else {
                crate::textedit::point_align_dx(td.align, layout.width())
            };
            for line in layout.lines() {
                let lm = line.metrics();
                let by = lm.baseline as f64;
                // Don't draw guides for lines clipped past a fixed-height box.
                if frame_h.is_some_and(|h| by > h + 1.0) {
                    break;
                }
                let (x0, x1) = if area {
                    (0.0, frame_w)
                } else {
                    let x = dx + lm.offset as f64;
                    (x, x + (lm.advance - lm.trailing_whitespace) as f64)
                };
                scene.stroke(
                    &Stroke::new(0.75),
                    Affine::IDENTITY,
                    guide,
                    None,
                    &Line::new(m * Point::new(x0, by), m * Point::new(x1, by)),
                );
            }
        }

        if let Some(kid) = key_object {
            if let Some(q) = select::selection_quad(doc, &[kid]) {
                let extra = match drag {
                    Some(d) if d.xf.is_some() => xf_for_quad(doc, &[kid], d),
                    Some(d) if d.is_dragged(kid) => Affine::translate(d.delta),
                    _ => Affine::IDENTITY,
                };
                let q = q.map(|p| vt * extra * p);
                let mut path = BezPath::new();
                path.move_to(q[0]);
                for p in &q[1..] {
                    path.line_to(*p);
                }
                path.close_path();
                scene.stroke(
                    &Stroke::new(2.5),
                    Affine::IDENTITY,
                    theme.accent,
                    None,
                    &path,
                );
            }
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

        let hdrag = drag.and_then(|d| d.handle);
        let white = Color::from_rgb8(0xff, 0xff, 0xff);

        // Outline every selected path, deformed live by an anchor drag or a
        // handle drag in progress.
        for &id in av.paths {
            let Some(ObjectKind::Path(pd)) = doc.object(id).map(|o| &o.kind) else {
                continue;
            };
            let m = vt * convert::affine(doc.world_transform(id));
            let preview_pd = hdrag
                .filter(|&(o, ..)| o == id)
                .map(|(_, n, side, hd)| crate::anchors::deformed_handle(pd, n, side, hd));
            let g = if let Some(ppd) = &preview_pd {
                ppd.geometry.clone()
            } else {
                let idxs: Vec<usize> = av
                    .selected
                    .iter()
                    .filter(|(o, _)| *o == id)
                    .map(|(_, i)| *i)
                    .collect();
                crate::anchors::deformed(pd, &idxs, core_dv)
            };
            // Bake the view transform into the geometry and stroke in
            // screen space, so the contour stays a hairline at any zoom.
            let screen = m * convert::bez_path(&g);
            scene.stroke(&Stroke::new(1.5), Affine::IDENTITY, theme.accent, None, &screen);

            // Bezier handle sticks + round dots for selected anchors.
            let src = preview_pd.as_ref().map(|p| p.subpaths()).unwrap_or(pd.subpaths());
            for (n, a) in src.iter().flat_map(|s| s.anchors.iter()).enumerate() {
                if !av.selected.contains(&(id, n)) {
                    continue;
                }
                let anchor_shift = if preview_pd.is_none() { core_dv } else { amalith_core::Vec2::ZERO };
                let ap = m * convert::point(amalith_core::geom::Point::new(
                    a.point.x + anchor_shift.x,
                    a.point.y + anchor_shift.y,
                ));
                for h in [a.handle_in, a.handle_out].into_iter().flatten() {
                    let hp = m * convert::point(amalith_core::geom::Point::new(
                        h.x + anchor_shift.x,
                        h.y + anchor_shift.y,
                    ));
                    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.accent, None,
                        &vello::kurbo::Line::new(ap, hp));
                    let dot = vello::kurbo::Circle::new(hp, 3.0);
                    scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &dot);
                    scene.stroke(&Stroke::new(1.25), Affine::IDENTITY, theme.accent, None, &dot);
                }
            }
        }

        // Anchor markers. When some of a path's anchors are individually
        // selected (a click or a marquee), the rest go hollow —
        // Illustrator's white arrow. With none selected the path is just
        // "shown", and every anchor is solid blue.
        for &id in av.paths {
            let any_sel = av.selected.iter().any(|(o, _)| *o == id);
            for (idx, pos) in crate::anchors::anchors_of(doc, id) {
                let sel = av.selected.contains(&(id, idx));
                let moved = sel && hdrag.is_none_or(|(o, ..)| o != id);
                let doc_pos = if moved { pos + dv } else { pos };
                // The hovered node swells so it's clear which one a click
                // (or a rotation) will act on.
                let s = if av.hover == Some((id, idx)) { 10.0 } else { 7.0 };
                let sq = Rect::from_center_size(vt * doc_pos, (s, s));
                if sel || !any_sel {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &sq);
                } else {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, white, None, &sq);
                    scene.stroke(
                        &Stroke::new(1.25),
                        Affine::IDENTITY,
                        theme.accent,
                        None,
                        &sq,
                    );
                }
            }
        }
    }

    // Debug (Preferences ▸ Debug ▸ Show Cull Outline): the dashed line is
    // the cull threshold — objects whose bounds fully leave it are not
    // drawn, and their rasters are not decoded.
    if show_cull {
        scene.stroke(
            &Stroke::new(1.5).with_dashes(0.0, [8.0, 6.0]),
            Affine::IDENTITY,
            Color::from_rgb8(0xff, 0x3b, 0x8a),
            None,
            &cull,
        );
        text.draw(
            scene,
            "cull",
            11.0,
            Color::from_rgb8(0xff, 0x3b, 0x8a),
            cull.x0 + 6.0,
            cull.y0 + 14.0,
        );
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

/// A vello `Stroke` for a document-space `width` under `style`. `px` is
/// the uniform screen scale (view zoom) — object scale is *not* folded
/// in, so a 10px stroke stays 10px after squash/stretch (Illustrator
/// with Scale Strokes & Effects off).
fn stroke_spec(width: f64, style: &StrokeStyle, px: f64) -> Stroke {
    let mut s = Stroke::new(width * px)
        .with_caps(kurbo_cap(style.cap))
        .with_join(kurbo_join(style.join))
        .with_miter_limit(style.miter_limit.max(1.0));
    if let Some(pattern) = style.dash_pattern() {
        s = s.with_dashes(
            style.dash_offset * px,
            pattern.into_iter().map(|d| d * px),
        );
    }
    s
}

/// Stroke `bp` (object-local space; `m` maps it to the screen) at a
/// document-space `width`. The path is baked into screen space and
/// stroked with `Affine::IDENTITY` so object scale cannot fatten or
/// squash the envelope. `zoom` is the view's uniform scale.
#[allow(clippy::too_many_arguments)]
fn stroke_path(
    scene: &mut Scene,
    m: Affine,
    brush: vello::peniko::BrushRef<'_>,
    // Brush transform for a gradient stroke, already composed with `m`
    // (strokes are baked to world space here, so the brush must be too).
    // `None` for a solid colour.
    brush_xf: Option<Affine>,
    bp: &BezPath,
    width: f64,
    style: &StrokeStyle,
    closed: bool,
    zoom: f64,
) {
    let px = zoom.max(1e-6);
    let baked = m * bp;
    if !closed || style.align == StrokeAlign::Center || width <= 0.0 {
        scene.stroke(
            &stroke_spec(width, style, px),
            Affine::IDENTITY,
            brush,
            brush_xf,
            &baked,
        );
        return;
    }
    let wide = stroke_spec(width * 2.0, style, px);
    match style.align {
        StrokeAlign::Inside => {
            scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &baked);
            scene.stroke(&wide, Affine::IDENTITY, brush, brush_xf, &baked);
            scene.pop_layer();
        }
        StrokeAlign::Outside => {
            let pad = width * px + 2.0;
            let outer = baked.bounding_box().inflate(pad, pad);
            let mut ring = BezPath::new();
            ring.extend(outer.path_elements(0.1));
            ring.extend(baked.elements().iter().copied());
            scene.push_clip_layer(Fill::EvenOdd, Affine::IDENTITY, &ring);
            scene.stroke(&wide, Affine::IDENTITY, brush, brush_xf, &baked);
            scene.pop_layer();
        }
        StrokeAlign::Center => unreachable!(),
    }
}

/// Paints a freeform gradient fill: a soft radial "blob" per colour point
/// (opaque at its centre, fading to transparent by `spread`), layered over
/// a flat backstop of the points' average colour so a spot outside every
/// point's reach doesn't show through as a hole. This isn't a true
/// weighted-distance diffusion — that needs a per-pixel compute pass — but
/// it's a fully vector-native approximation: every layer is a real peniko
/// radial gradient, so it stays crisp at any zoom and exports as real
/// gradients, not a baked raster.
fn paint_freeform_fill(
    scene: &mut Scene,
    m: Affine,
    bbox_xf: Affine,
    g: &amalith_core::Gradient,
    bp: &vello::kurbo::BezPath,
) {
    if g.points.is_empty() {
        return;
    }
    scene.push_clip_layer(Fill::NonZero, m, bp);
    // The object's local bounds (== the unit square under `bbox_xf`)
    // fully contain the path, so filling it inside the clip above paints
    // exactly the path's shape.
    let unit_xf = m * bbox_xf;
    let unit_rect = Rect::new(0.0, 0.0, 1.0, 1.0);

    let n = g.points.len() as f32;
    let mut avg = [0.0f32; 4];
    for p in &g.points {
        let c = p.effective_color();
        avg[0] += c.r;
        avg[1] += c.g;
        avg[2] += c.b;
        avg[3] += c.a;
    }
    let backstop = Color::new([avg[0] / n, avg[1] / n, avg[2] / n, avg[3] / n]);
    scene.fill(Fill::NonZero, unit_xf, backstop, None, &unit_rect);

    for p in &g.points {
        let c = convert::color(p.effective_color());
        let outer = (p.spread * 2.2).max(0.02) as f32;
        let transparent = c.with_alpha(0.0);
        let blob = vello::peniko::Gradient::new_radial((p.pos[0], p.pos[1]), outer)
            .with_extend(vello::peniko::Extend::Pad)
            .with_stops(
                [
                    vello::peniko::ColorStop::from((0.0, c)),
                    vello::peniko::ColorStop::from((1.0, transparent)),
                ]
                .as_slice(),
            );
        scene.fill(Fill::NonZero, unit_xf, &blob, None, &unit_rect);
    }
    scene.pop_layer();
}

#[allow(clippy::too_many_arguments)]
fn paint_object(
    scene: &mut Scene,
    doc: &Document,
    id: ObjectId,
    vt: Affine,
    zoom: f64,
    viewport: Rect,
    drag: Option<DragPreview<'_>>,
    text: &mut TextContext,
    editing_text: Option<ObjectId>,
    images: &HashMap<AssetId, ImageLods>,
    outline: bool,
) {
    let Some(obj) = doc.object(id) else {
        return;
    };
    if !obj.visible {
        return;
    }
    let off = drag.map_or(Affine::IDENTITY, |d| d.object_offset(id));
    let replacement = drag.and_then(|d| d.replacement(id));
    // Same cull as the egui-ui branch: skip anything whose bounds miss
    // the canvas. Off-screen copies of a huge raster must not enter
    // Vello's image atlas.
    if replacement.is_none() {
        if let Some(b) = doc.bounds_of(id) {
            let screen = (vt * off).transform_rect_bbox(convert::rect(b));
            if !overlaps(screen, viewport) {
                return;
            }
        }
    }
    let m = match replacement {
        Some(a) => vt * a,
        None => vt * off * convert::affine(obj.transform),
    };
    let fill = obj.appearance.fill.color().map(convert::color);
    let stroke = obj.appearance.stroke.color().map(convert::color);
    let sw = obj.appearance.stroke_width;
    let style = obj.appearance.stroke_style;
    // Gradient paints (fill / stroke) resolved against the document pool.
    // The gradient is defined in bounding-box unit space; `bbox_xf` maps
    // that unit square onto this object's own local bounds so the gradient
    // rides with the object (SVG `objectBoundingBox`). A radial gradient's
    // `aspect`/`rotation` are an *additional* unit-space squish applied
    // before that mapping (see `convert::radial_squish`), so each paint
    // gets its own brush transform rather than a single shared one.
    let bbox_xf = obj.kind.own_local_bounds().map(|b| {
        let r = convert::rect(b);
        Affine::translate((r.x0, r.y0))
            * Affine::scale_non_uniform(r.width().max(1e-6), r.height().max(1e-6))
    });
    let resolve_grad = |paint: amalith_core::Paint| -> Option<(vello::peniko::Gradient, Affine)> {
        let gid = paint.gradient_id()?;
        let g = doc.gradient(gid)?;
        let bbox = bbox_xf?;
        Some((convert::peniko_gradient(g), bbox * convert::radial_squish(g)))
    };
    // A freeform *fill* is a composite of per-point layers (paint_freeform_
    // fill below), not a single peniko::Gradient — resolve_grad's
    // placeholder is only used when that's not possible (no local bounds)
    // or on a stroke (Illustrator restricts freeform to fills; a freeform
    // stroke keeps the single-gradient placeholder rather than needing a
    // stroke-shaped clip for the layered composite).
    let fill_freeform = match obj.appearance.fill {
        amalith_core::Paint::Gradient(gid) => doc
            .gradient(gid)
            .filter(|g| g.kind == amalith_core::GradientKind::Freeform),
        _ => None,
    };
    let fill_grad = if fill_freeform.is_some() {
        None
    } else {
        resolve_grad(obj.appearance.fill)
    };
    let stroke_grad = resolve_grad(obj.appearance.stroke);
    let paint_path = |scene: &mut Scene, bp: &vello::kurbo::BezPath| {
        // Outline (wireframe) view: hairline contour only, no fill/stroke.
        if outline {
            scene.stroke(
                &Stroke::new(1.0),
                Affine::IDENTITY,
                OUTLINE_INK,
                None,
                &(m * bp.clone()),
            );
            return;
        }
        let closed = bp
            .elements()
            .iter()
            .any(|e| matches!(e, vello::kurbo::PathEl::ClosePath));
        // An open path still fills — the fill closes the contour
        // implicitly (Illustrator / SVG), while the stroke stays open.
        if let (Some(g), Some(bbox)) = (fill_freeform, bbox_xf) {
            paint_freeform_fill(scene, m, bbox, g, bp);
        } else if let Some((g, xf)) = &fill_grad {
            scene.fill(Fill::NonZero, m, g, Some(*xf), bp);
        } else if let Some(c) = fill {
            scene.fill(Fill::NonZero, m, c, None, bp);
        }
        if let Some((g, xf)) = &stroke_grad {
            // Strokes bake to world space here (transform = IDENTITY), so
            // the brush transform must be composed with `m` too.
            stroke_path(scene, m, g.into(), Some(m * *xf), bp, sw, &style, closed, zoom);
        } else if let Some(c) = stroke {
            stroke_path(scene, m, c.into(), None, bp, sw, &style, closed, zoom);
        }
    };

    match &obj.kind {
        ObjectKind::Path(pd) => {
            // Live anchor drag / handle drag: deform this path's geometry.
            let idxs: Vec<usize> = drag
                .and_then(|d| d.anchors)
                .map(|(sel, _)| {
                    sel.iter()
                        .filter(|(o, _)| *o == id)
                        .map(|(_, i)| *i)
                        .collect()
                })
                .unwrap_or_default();
            let hdrag = drag.and_then(|d| d.handle).filter(|&(o, ..)| o == id);
            if let Some((_, n, side, hd)) = hdrag {
                let g = crate::anchors::deformed_handle(pd, n, side, hd);
                paint_path(scene, &convert::bez_path(&g.geometry));
            } else if let (false, Some((_, dv))) =
                (idxs.is_empty(), drag.and_then(|d| d.anchors))
            {
                let g = crate::anchors::deformed(pd, &idxs, dv);
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

            // Clip group: the `clip` child (always a shape) masks the rest.
            let clip = g.clip.filter(|cid| g.children.contains(cid));
            let clipped = clip.and_then(|cid| {
                let cobj = doc.object(cid)?;
                let cm = m * convert::affine(cobj.transform);
                let bez = match &cobj.kind {
                    ObjectKind::Path(pd) => convert::bez_path(&pd.geometry),
                    ObjectKind::CompoundPath(cp) => {
                        let mut b = BezPath::new();
                        for sub in &cp.subpaths {
                            b.extend(convert::bez_path(sub));
                        }
                        b
                    }
                    _ => return None,
                };
                scene.push_clip_layer(Fill::NonZero, cm, &bez);
                Some(())
            });

            for &child in &g.children {
                if Some(child) == clip {
                    continue; // the mask itself isn't drawn
                }
                paint_object(
                    scene,
                    doc,
                    child,
                    m,
                    zoom,
                    viewport,
                    child_drag,
                    text,
                    editing_text,
                    images,
                    outline,
                );
            }
            if clipped.is_some() {
                scene.pop_layer();
            }
        }
        ObjectKind::Text(td) => {
            // Live text stays solid in Outline view — only text that's been
            // expanded to paths (now a Path / CompoundPath) wireframes.
            // The object open in the Type tool is drawn live by the shell.
            if Some(id) != editing_text {
                let color = fill.unwrap_or(Color::from_rgb8(0, 0, 0));
                // Live area-text-box resize: re-wrap at the previewed frame
                // size and shift the origin, without touching the document.
                if let Some(tb) = drag
                    .map(|d| d.text_boxes)
                    .and_then(|tbs| tbs.iter().copied().find(|tb| tb.id == id))
                {
                    let mut preview = td.clone();
                    preview.kind = amalith_core::TextKind::Area {
                        width: tb.width,
                        height: Some(tb.height),
                    };
                    let pm = m * Affine::translate(tb.origin_delta);
                    crate::textedit::paint_text_data(scene, text, &preview, pm, color);
                } else if td.is_threaded() {
                    // Threaded frame: show the story's overflow starting at
                    // this frame's byte offset, clipped to its box.
                    if let Some(head) = crate::thread::head(doc, id) {
                        let story = match doc.object(head).map(|o| &o.kind) {
                            Some(ObjectKind::Text(h)) => h.content.clone(),
                            _ => String::new(),
                        };
                        let slices = crate::thread::slices(doc, head, text);
                        if let Some(sl) = slices.get(&id) {
                            let start = sl.start.min(story.len());
                            let mut frame = td.clone();
                            frame.content = story[start..].to_string();
                            crate::textedit::paint_text_data(scene, text, &frame, m, color);
                        }
                    }
                } else {
                    crate::textedit::paint_text_data(scene, text, td, m, color);
                }
            }
        }
        ObjectKind::Image(img) => {
            if outline {
                // Wireframe: a crossed box, like Illustrator.
                if let Some(b) = obj.kind.own_local_bounds() {
                    let r = convert::rect(b);
                    let quad = [
                        Point::new(r.x0, r.y0),
                        Point::new(r.x1, r.y0),
                        Point::new(r.x1, r.y1),
                        Point::new(r.x0, r.y1),
                    ]
                    .map(|p| m * p);
                    let mut bp = BezPath::new();
                    bp.move_to(quad[0]);
                    for p in &quad[1..] {
                        bp.line_to(*p);
                    }
                    bp.close_path();
                    bp.move_to(quad[0]);
                    bp.line_to(quad[2]);
                    bp.move_to(quad[1]);
                    bp.line_to(quad[3]);
                    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, OUTLINE_INK, None, &bp);
                }
                return;
            }
            // Screen-space long side of the object (zoom × native size).
            // Native bounds stay full-res; only the GPU copy is swapped.
            let cover = {
                let sb = m.transform_rect_bbox(convert::rect(img.local_bounds));
                sb.width().max(sb.height())
            };
            if let Some(gpu) = images.get(&img.asset).and_then(|l| l.pick(cover)) {
                paint_raster(scene, m, gpu, img.local_bounds);
            } else if let Some(b) = obj.kind.own_local_bounds() {
                let r = convert::rect(b);
                scene.fill(
                    Fill::NonZero,
                    m,
                    Color::from_rgb8(0x3a, 0x3a, 0x3c),
                    None,
                    &r,
                );
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

fn overlaps(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
}

/// Draw a raster so `img`'s pixel box fills `local_bounds` under `m`.
/// Native document size comes from `local_bounds`; `img` may be a
/// downsampled GPU copy (Vello's atlas is 8192²).
fn paint_raster(
    scene: &mut Scene,
    m: Affine,
    img: &ImageData,
    local_bounds: amalith_core::Rect,
) {
    if img.width == 0 || img.height == 0 {
        return;
    }
    let b = convert::rect(local_bounds);
    if b.width() <= 0.0 || b.height() <= 0.0 {
        return;
    }
    let xf = m
        * Affine::translate((b.x0, b.y0))
        * Affine::scale_non_uniform(
            b.width() / img.width as f64,
            b.height() / img.height as f64,
        );
    scene.draw_image(img, xf);
}

/// A decoded raster: native document size plus a GPU image that fits
/// Vello's 8192² atlas.
pub struct Raster {
    pub native_w: u32,
    pub native_h: u32,
    pub gpu: ImageData,
}

/// Vello's image atlas is a square of at most 8192. Larger rasters are
/// downsampled for the GPU; object bounds stay at native pixels.
pub const GPU_ATLAS_MAX: u32 = 8192;

#[cfg(test)]
fn atlas_fit(w: u32, h: u32) -> (u32, u32) {
    fit_side(w, h, GPU_ATLAS_MAX)
}

fn fit_side(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (1, 1);
    }
    if w <= max_side && h <= max_side {
        return (w, h);
    }
    let s = max_side as f64 / w.max(h) as f64;
    (
        ((w as f64 * s).floor() as u32).max(1),
        ((h as f64 * s).floor() as u32).max(1),
    )
}

fn cap_rgba(mut rgba: image::RgbaImage, max_side: u32) -> image::RgbaImage {
    let (nw, nh) = fit_side(rgba.width(), rgba.height(), max_side);
    if nw != rgba.width() || nh != rgba.height() {
        rgba = image::imageops::resize(&rgba, nw, nh, image::imageops::FilterType::Triangle);
    }
    rgba
}

fn rgba_to_gpu(rgba: image::RgbaImage) -> ImageData {
    let (width, height) = rgba.dimensions();
    ImageData {
        data: Blob::from(rgba.into_raw()),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width,
        height,
    }
}

/// Header-only dimensions. Does not decode pixels.
pub fn raster_dimensions(path: &std::path::Path) -> Option<(u32, u32)> {
    #[cfg(target_os = "macos")]
    {
        if let Some(d) = crate::imageio::pixel_size(path) {
            return Some(d);
        }
    }
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Header-only dimensions of encoded bytes.
pub fn raster_dimensions_bytes(bytes: &[u8]) -> Option<(u32, u32)> {
    #[cfg(target_os = "macos")]
    {
        if let Some(d) = crate::imageio::pixel_size_bytes(bytes) {
            return Some(d);
        }
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Decode `path` to a GPU image whose longest side is at most `max_side`.
/// On macOS this is an ImageIO thumbnail (no full-res decode).
pub fn decode_path_max_side(path: &std::path::Path, max_side: u32) -> Option<ImageData> {
    #[cfg(target_os = "macos")]
    {
        if let Some(img) = crate::imageio::thumbnail_path(path, max_side) {
            return Some(img);
        }
    }
    let rgba = image::open(path).ok()?.to_rgba8();
    Some(rgba_to_gpu(cap_rgba(rgba, max_side)))
}

/// Decode encoded bytes to a GPU image whose longest side is at most `max_side`.
pub fn decode_bytes_max_side(bytes: &[u8], max_side: u32) -> Option<ImageData> {
    #[cfg(target_os = "macos")]
    {
        if let Some(img) = crate::imageio::thumbnail_bytes(bytes, max_side) {
            return Some(img);
        }
    }
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    Some(rgba_to_gpu(cap_rgba(rgba, max_side)))
}

/// Decode PNG/JPEG bytes into a vello image (GPU-capped) plus native size.
pub fn decode_raster_bytes(bytes: &[u8]) -> Option<Raster> {
    let (native_w, native_h) = raster_dimensions_bytes(bytes)?;
    let gpu = decode_bytes_max_side(bytes, GPU_ATLAS_MAX)?;
    Some(Raster {
        native_w,
        native_h,
        gpu,
    })
}

/// Decode a raster file. PNG/JPEG via the `image` crate; on macOS, ImageIO
/// covers HEIC and anything else Preview can open (iMessage attachments).
pub fn decode_raster_path(path: &std::path::Path) -> Option<Raster> {
    let (native_w, native_h) = raster_dimensions(path)?;
    let gpu = decode_path_max_side(path, GPU_ATLAS_MAX)?;
    Some(Raster {
        native_w,
        native_h,
        gpu,
    })
}

/// True for the raster types File ▸ Place and drop currently accept.
pub fn is_raster_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "heic" | "heif" | "tif" | "tiff" | "gif" | "webp" | "bmp")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_fit_caps_the_long_side_and_keeps_aspect() {
        let (w, h) = atlas_fit(10913, 4071);
        assert!(w <= GPU_ATLAS_MAX && h <= GPU_ATLAS_MAX);
        assert_eq!(w, GPU_ATLAS_MAX);
        let native = 10913.0 / 4071.0;
        let gpu = w as f64 / h as f64;
        assert!((native - gpu).abs() < 0.01);
    }

    #[test]
    fn atlas_fit_leaves_small_images_alone() {
        assert_eq!(atlas_fit(800, 600), (800, 600));
    }

    #[test]
    fn zoom_at_clamps_to_range() {
        let mut v = CanvasView {
            pan: Vec2::ZERO,
            zoom: 1.0,
        };
        v.zoom_at(1e9, Point::ZERO);
        assert_eq!(v.zoom, ZOOM_MAX);
        v.zoom_at(1e-12, Point::ZERO);
        assert_eq!(v.zoom, ZOOM_MIN);
    }

    #[test]
    fn zoom_percent_label_keeps_decimals_when_far_out() {
        assert_eq!(zoom_percent_label(1.0), "100%");
        assert_eq!(zoom_percent_label(0.05), "5.0%");
        assert_eq!(zoom_percent_label(0.001), "0.10%");
    }
}
