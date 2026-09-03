//! The main document view: canvas, rails, tab strip, app bar, context
//! bar, and the drag/flyout overlays layered over them.

use super::super::*;

/// How a ruler guide line should be drawn this frame.
#[derive(Clone, Copy, PartialEq)]
pub(in crate::app) enum GuideMark {
    /// A committed, unselected guide.
    Idle,
    /// The guide currently being dragged out / repositioned.
    Active,
    /// A selected guide (click or marquee).
    Selected,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::app) fn paint_main(
    scene: &mut Scene,
    text: &mut TextContext,
    dock: &DockModel,
    doc: &Document,
    view: &CanvasView,
    theme: &Theme,
    selection: &[ObjectId],
    active_tool: Tool,
    active_slot: panels::PaintSlot,
    picker: Option<crate::picker::Picker>,
    representative: Option<amalith_core::Appearance>,
    fill_mixed: bool,
    stroke_mixed: bool,
    cur_fill: amalith_core::Paint,
    cur_stroke: amalith_core::Paint,
    pointer: Point,
    drag_preview: Option<DragPreview<'_>>,
    draw_shape: Option<(Tool, Rect)>,
    artboard_ghost: Option<Rect>,
    artboard_handles: Option<[Point; 4]>,
    pen_preview: Option<PenPreview<'_>>,
    anchor_view: Option<AnchorView<'_>>,
    marquee: Option<Rect>,
    width: f64,
    height: f64,
    redock_preview: Option<&(RailSide, DropTarget)>,
    status: Option<&str>,
    expanded: &std::collections::HashSet<ObjectId>,
    cur_weight: f64,
    cur_opacity: f32,
    renaming: Option<(panels::RenameId, &str)>,
    selected_layer: Option<LayerId>,
    selected_artboard: Option<ArtboardId>,
    active_artboard: Option<ArtboardId>,
    newdoc_form: Option<&newdoc::NewDocForm>,
    tab_labels: &[String],
    active_tab: usize,
    cursor_glyph: Option<(Tool, PenHint, bool)>,
    anchor_sel_len: usize,
    zoom_cursor: Option<bool>,
    cursor_mode: CanvasCursor,
    shape_tool: Tool,
    shape_flyout: Option<Rect>,
    stroke_popover: bool,
    stroke_style: StrokeStyle,
    stroke_flyout: stroke_panel::Layout,
    editing_text: Option<ObjectId>,
    text_style: amalith_core::TextStyle,
    text_align: amalith_core::TextAlign,
    text_paragraph: amalith_core::Paragraph,
    text_editing: bool,
    font_families: &[String],
    layer_query: &str,
    layer_search_focused: bool,
    color_mode: panels::ColorSpace,
    recent: &[amalith_core::Color],
    images: &std::collections::HashMap<AssetId, crate::lod::ImageLods>,
    xform_ref: amalith_core::RefPoint,
    xform_constrain: bool,
    xform_edit: Option<(panels::transform::XformField, &str)>,
    align_to: amalith_commands::AlignTo,
    align_to_menu: bool,
    align_spacing: Option<f64>,
    align_spacing_edit: Option<&str>,
    key_object: Option<ObjectId>,
    panel_scroll: &std::collections::HashMap<PanelId, f64>,
    cull_inset: f64,
    show_cull: bool,
    rulers: bool,
    // Rotate tool: the reference point (document space) to mark, if any.
    rotate_pivot: Option<Point>,
    // Ruler guides to draw: (orientation, doc coord, how to mark it).
    guide_lines: &[(amalith_core::GuideOrient, f64, GuideMark)],
    // Outline (wireframe) view active.
    outline_mode: bool,
) {
    scene.fill(
        Fill::NonZero,
        ID,
        theme.bg,
        None,
        &Rect::new(0.0, 0.0, width, height),
    );

    // Canvas fills the gap between whatever rails are present.
    let left_x = if dock.left.is_empty() {
        0.0
    } else {
        rail_rect_for(RailSide::Left, dock.left.width as f64, width, height).x1
    };
    let right_x = if dock.right.is_empty() {
        width
    } else {
        rail_rect_for(RailSide::Right, dock.right.width as f64, width, height).x0
    };
    // Full canvas region between the rails; the rulers (when on) sit in a
    // strip along its top / left, and content is inset to match
    // `App::canvas_viewport`.
    let full = Rect::new(left_x, CHROME_TOP, right_x.max(left_x), height);
    let viewport = if rulers {
        Rect::new(full.x0 + rulers::THICK, full.y0 + rulers::THICK, full.x1, full.y1)
    } else {
        full
    };
    canvas::paint(
        scene,
        doc,
        view,
        viewport,
        theme,
        text,
        selection,
        drag_preview,
        draw_shape,
        artboard_ghost,
        artboard_handles,
        active_tool == Tool::Artboard,
        pen_preview,
        anchor_view,
        editing_text,
        images,
        key_object,
        cull_inset,
        show_cull,
        active_artboard,
        outline_mode,
    );

    if let Some(m) = marquee {
        scene.fill(Fill::NonZero, ID, theme.marquee_fill, None, &m);
        scene.stroke(&Stroke::new(1.0), ID, theme.accent, None, &m);
    }

    // Ruler guides — full-canvas lines, clipped to the viewport.
    if !guide_lines.is_empty() {
        use amalith_core::GuideOrient;
        scene.push_clip_layer(Fill::NonZero, ID, &viewport);
        let vt = view.to_screen();
        // Idle / active: neon cyan (matches the Rotate reference point).
        // Selected: blue.
        let cyan = vello::peniko::Color::from_rgb8(0x00, 0xff, 0xff);
        let blue = vello::peniko::Color::from_rgb8(0x4f, 0x80, 0xff);
        for &(orient, pos, mark) in guide_lines {
            let (col, wt) = match mark {
                GuideMark::Idle => (cyan.with_alpha(0.8), 1.0),
                GuideMark::Active => (cyan, 1.4),
                GuideMark::Selected => (blue, 1.6),
            };
            let line = match orient {
                GuideOrient::Horizontal => {
                    let y = (vt * Point::new(0.0, pos)).y.round() + 0.5;
                    vello::kurbo::Line::new((viewport.x0, y), (viewport.x1, y))
                }
                GuideOrient::Vertical => {
                    let x = (vt * Point::new(pos, 0.0)).x.round() + 0.5;
                    vello::kurbo::Line::new((x, viewport.y0), (x, viewport.y1))
                }
            };
            scene.stroke(&Stroke::new(wt), ID, col, None, &line);
        }
        scene.pop_layer();
    }

    // Rotate tool: a registration-mark reference point (circle + crosshair)
    // at the pivot the next drag turns around.
    if let Some(pv) = rotate_pivot {
        let c = view.to_screen() * pv;
        if viewport.contains(c) {
            let r = 5.0;
            // Neon cyan, over a white halo so it reads on any background.
            let ink = vello::peniko::Color::from_rgb8(0x00, 0xff, 0xff);
            let halo = vello::peniko::Color::WHITE;
            let ring = vello::kurbo::Circle::new(c, r);
            let h = vello::kurbo::Line::new(
                Point::new(c.x - r - 3.0, c.y),
                Point::new(c.x + r + 3.0, c.y),
            );
            let v = vello::kurbo::Line::new(
                Point::new(c.x, c.y - r - 3.0),
                Point::new(c.x, c.y + r + 3.0),
            );
            for (col, w) in [(halo, 3.5), (ink, 1.5)] {
                scene.stroke(&Stroke::new(w), ID, col, None, &ring);
                scene.stroke(&Stroke::new(w), ID, col, None, &h);
                scene.stroke(&Stroke::new(w), ID, col, None, &v);
            }
        }
    }

    // Rulers (when on) are drawn by `App::paint_rulers` after this fn
    // returns — their static layer is cached across frames.

    // Document-tab strip (canvas x-span, between options bar and canvas).
    let tab_strip = tab_bar_rect(left_x, right_x);
    scene.fill(Fill::NonZero, ID, theme.app_bar, None, &tab_strip);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(tab_strip.x0, tab_strip.y1 - 1.0, tab_strip.x1, tab_strip.y1),
    );
    for (i, (whole, close)) in layout_tabs(text, tab_labels, tab_strip).into_iter().enumerate() {
        let is_active = i == active_tab;
        if is_active {
            scene.fill(Fill::NonZero, ID, theme.strip_active, None, &whole);
            scene.fill(
                Fill::NonZero,
                ID,
                theme.accent,
                None,
                &Rect::new(whole.x0, whole.y1 - 2.0, whole.x1, whole.y1),
            );
        }
        // Close ×.
        let xc = close.center();
        let cc = if is_active { theme.text } else { theme.text_dim };
        let mut xg = BezPath::new();
        xg.move_to((xc.x - 4.0, xc.y - 4.0));
        xg.line_to((xc.x + 4.0, xc.y + 4.0));
        xg.move_to((xc.x + 4.0, xc.y - 4.0));
        xg.line_to((xc.x - 4.0, xc.y + 4.0));
        scene.stroke(&Stroke::new(1.3), ID, cc, None, &xg);
        text.draw(
            scene,
            &tab_labels[i],
            12.6,
            if is_active { theme.text } else { theme.text_dim },
            close.x1 + 6.0,
            tab_strip.y0 + TAB_BAR_H * 0.5 + 4.0,
        );
        // Divider between tabs.
        if i + 1 < tab_labels.len() {
            scene.fill(
                Fill::NonZero,
                ID,
                theme.border,
                None,
                &Rect::new(whole.x1, tab_strip.y0 + 5.0, whole.x1 + 1.0, tab_strip.y1 - 5.0),
            );
        }
    }

    for side in [RailSide::Left, RailSide::Right] {
        let rail = dock.rail(side);
        let is_preview_target = redock_preview.is_some_and(|(s, _)| *s == side);
        if rail.is_empty() && !is_preview_target {
            continue;
        }
        let rect = rail_rect_for(side, rail.width as f64, width, height);
        let laid = build_rail_layout(rail, theme, text, rect);
        if !rail.is_empty() {
            chrome::paint(scene, &laid, theme, text, &tab_label);
            let ctx = panels::Ctx {
                theme,
                doc,
                selection,
                active_tool,
                pointer,
                representative,
                fill_mixed,
                stroke_mixed,
                active_slot,
                picker,
                cur_fill,
                cur_stroke,
                shape_tool,
                expanded,
                renaming,
                selected_layer,
                selected_artboard,
                text_style: text_style.clone(),
                text_align,
                text_paragraph,
                text_editing,
                font_families,
                layer_query,
                layer_search_focused,
                layer_scroll: panel_scroll.get(&PanelId("layers")).copied().unwrap_or(0.0),
                color_mode,
                recent,
                xform_ref,
                xform_constrain,
                xform_edit,
                align_to,
                align_spacing,
                align_spacing_edit,
                key_object,
            };
            for area in &laid.areas {
                if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                    // Clip to the real body so a panel shorter than its
                    // content spills nothing past the splitter; the panel
                    // itself is drawn into a body slid up by its scroll.
                    let (pbody, scroll) = panels::scrolled_body(
                        pid,
                        area.body,
                        panel_scroll.get(&pid).copied().unwrap_or(0.0),
                    );
                    scene.push_clip_layer(Fill::NonZero, ID, &area.body);
                    panels::paint(scene, text, pid, pbody, &ctx);
                    panels::paint_scrollbar(scene, area.body, pid, scroll, theme);
                    scene.pop_layer();
                }
            }
            // Bar on the canvas-facing edge — the whole-rail resize handle.
            scene.fill(
                Fill::NonZero,
                ID,
                theme.splitter,
                None,
                &rail_edge_bar(side, rect),
            );
        }
        if let Some((_, target)) = redock_preview.filter(|(s, _)| *s == side) {
            chrome::paint_drop(scene, target, &laid, rect, theme);
        }
    }

    // Context / control bar — a strip of self-contained segments; which
    // ones appear is decided per-segment by `applies(ctx)`. See the
    // `context_bar` module.
    let text_ctx = text_editing
        || (!selection.is_empty()
            && selection.iter().all(|id| {
                matches!(
                    doc.object(*id).map(|o| &o.kind),
                    Some(amalith_core::ObjectKind::Text(_))
                )
            }));
    let cbar = context_bar::Ctx {
        theme,
        selection_len: selection.len(),
        text_context: text_ctx,
        representative,
        fill_mixed,
        stroke_mixed,
        active_slot,
        cur_weight,
        cur_opacity,
        stroke_open: stroke_popover,
        text_style: text_style.clone(),
        anchor_sel_len,
        xform: super::super::selection_xform(doc, selection, xform_ref),
        xform_constrain,
        xform_edit,
        pointer,
        align_to,
        align_to_menu,
    };
    context_bar::paint(scene, text, opt_bar_rect(width), &cbar);

    // The Stroke flyout hangs off the options bar's "Stroke" link.
    if stroke_popover {
        let shown_weight = representative.map(|a| a.stroke_width).unwrap_or(cur_weight);
        stroke_panel::paint(scene, text, theme, &stroke_flyout, &stroke_style, shown_weight);
    }

    // The active tool's on-document glyph, standing in for the OS cursor.
    if let Some((tool, hint, over_selectable)) = cursor_glyph {
        let sz = 30.0;
        let (hx, hy) = cursor_hotspot(tool);
        let x0 = pointer.x - sz * hx;
        let y0 = pointer.y - sz * hy;
        let box_ = Rect::new(x0, y0, x0 + sz, y0 + sz);
        let src = match tool {
            Tool::DirectSelect => icons::CURSOR_DIRECT_SELECT_SVG,
            Tool::Pen if hint == PenHint::Closing => icons::CURSOR_PEN_CLOSING_SVG,
            Tool::Pen => icons::CURSOR_PEN_DRAWING_SVG,
            _ => icons::CURSOR_SELECT_SVG,
        };
        icons::draw_cursor(scene, src, box_);
        // A small filled square below-right of the arrow's tip — "this is
        // selectable". Kept clear of the arrow body.
        if tool == Tool::Select && over_selectable {
            let c = vello::kurbo::Point::new(x0 + sz * 0.88, y0 + sz * 0.9);
            let a = 3.2;
            let ink = vello::peniko::Color::from_rgb8(0x1a, 0x1a, 0x1a);
            let halo = vello::peniko::Color::WHITE;
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                halo,
                None,
                &Rect::new(c.x - a - 1.2, c.y - a - 1.2, c.x + a + 1.2, c.y + a + 1.2),
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                ink,
                None,
                &Rect::new(c.x - a, c.y - a, c.x + a, c.y + a),
            );
        }
        // A small "+" badge when a click would insert an anchor.
        if tool == Tool::Pen && hint == PenHint::AddPoint {
            use vello::kurbo::{Line, Stroke};
            let c = vello::kurbo::Point::new(x0 + sz * 0.78, y0 + sz * 0.30);
            let a = 4.0;
            let ink = vello::peniko::Color::from_rgb8(0x1a, 0x1a, 0x1a);
            let halo = vello::peniko::Color::WHITE;
            for (col, w) in [(halo, 4.0), (ink, 2.0)] {
                scene.stroke(&Stroke::new(w), Affine::IDENTITY, col, None,
                    &Line::new((c.x - a, c.y), (c.x + a, c.y)));
                scene.stroke(&Stroke::new(w), Affine::IDENTITY, col, None,
                    &Line::new((c.x, c.y - a), (c.x, c.y + a)));
            }
        }
    }
    if let Some(plus) = zoom_cursor {
        icons::draw_magnifier(scene, pointer, plus);
    }
    // Transform-handle hover cursors (scale double-arrow / rotate).
    {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        match cursor_mode {
            CanvasCursor::FitUp => icons::draw_fit_up_cursor(scene, pointer),
            CanvasCursor::ScaleNS => icons::draw_scale_cursor(scene, pointer, FRAC_PI_2),
            CanvasCursor::ScaleEW => icons::draw_scale_cursor(scene, pointer, 0.0),
            CanvasCursor::ScaleNWSE => icons::draw_scale_cursor(scene, pointer, FRAC_PI_4),
            CanvasCursor::ScaleNESW => {
                icons::draw_scale_cursor(scene, pointer, 3.0 * FRAC_PI_4)
            }
            CanvasCursor::Rotate(grip) => {
                // Handle::ALL index (0 = Nw, then N, Ne, E, Se, S, Sw, W)
                // → the outward direction that grip sits on. The arc
                // bulges out that way, leaving its gap + both arrowheads
                // facing the selection.
                let angle = [
                    -3.0 * FRAC_PI_4,
                    -FRAC_PI_2,
                    -FRAC_PI_4,
                    0.0,
                    FRAC_PI_4,
                    FRAC_PI_2,
                    3.0 * FRAC_PI_4,
                    PI,
                ][grip as usize % 8];
                icons::draw_rotate_cursor(scene, pointer, angle);
            }
            CanvasCursor::ThreadPort => {
                // Select arrow + a small "linked frames" badge.
                let sz = 30.0;
                let (hx, hy) = cursor_hotspot(Tool::Select);
                let x0 = pointer.x - sz * hx;
                let y0 = pointer.y - sz * hy;
                icons::draw_cursor(
                    scene,
                    icons::CURSOR_SELECT_SVG,
                    Rect::new(x0, y0, x0 + sz, y0 + sz),
                );
                use vello::kurbo::Stroke;
                let ink = vello::peniko::Color::from_rgb8(0x1a, 0x1a, 0x1a);
                let paper = vello::peniko::Color::WHITE;
                let bx = x0 + sz * 0.58;
                let by = y0 + sz * 0.34;
                // two overlapping frames
                for (dx, dy) in [(4.0, 4.0), (0.0, 0.0)] {
                    let r = Rect::new(bx + dx, by + dy, bx + dx + 7.0, by + dy + 7.0);
                    scene.fill(Fill::NonZero, ID, paper, None, &r);
                    scene.stroke(&Stroke::new(1.2), ID, ink, None, &r);
                }
            }
            CanvasCursor::LoadedText => {
                // A little page-of-text glyph at the pointer.
                use vello::kurbo::{Line, Stroke};
                let ink = vello::peniko::Color::from_rgb8(0x1a, 0x1a, 0x1a);
                let paper = vello::peniko::Color::WHITE;
                let x0 = pointer.x + 2.0;
                let y0 = pointer.y + 2.0;
                let page = Rect::new(x0, y0, x0 + 17.0, y0 + 21.0);
                scene.fill(Fill::NonZero, ID, paper, None, &page);
                scene.stroke(&Stroke::new(1.5), ID, ink, None, &page);
                for i in 0..4 {
                    let ly = y0 + 5.0 + i as f64 * 4.0;
                    scene.stroke(
                        &Stroke::new(1.5),
                        ID,
                        ink,
                        None,
                        &Line::new(
                            vello::kurbo::Point::new(x0 + 3.0, ly),
                            vello::kurbo::Point::new(x0 + 14.0 - (i % 2) as f64 * 4.0, ly),
                        ),
                    );
                }
            }
            _ => {}
        }
    }

    // Primitive flyout.
    if let Some(anchor) = shape_flyout {
        let last = shape_flyout_cell(anchor, panels::tools::SHAPE_TOOLS.len() - 1);
        let bg = Rect::new(anchor.x1 + 4.0, anchor.y0 - 3.0, last.x1 + 3.0, last.y1 + 3.0);
        scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &bg);
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &bg);
        for (i, t) in panels::tools::SHAPE_TOOLS.iter().enumerate() {
            let c = shape_flyout_cell(anchor, i);
            let on = *t == shape_tool;
            if on {
                scene.fill(Fill::NonZero, ID, theme.accent, None, &c);
            } else if c.contains(pointer) {
                scene.fill(Fill::NonZero, ID, theme.accent.with_alpha(0.14), None, &c);
            }
            let col = if on {
                Color::from_rgb8(0xff, 0xff, 0xff)
            } else {
                theme.text_dim
            };
            icons::draw(scene, t.icon(), Rect::from_center_size(c.center(), (22.0, 22.0)), col);
        }
    }

    // Top app bar (drawn last so nothing bleeds over it). macOS keeps the
    // traffic lights floating over its left end. On Windows APP_BAR_H is 0
    // — the native title bar and menu bar own this space — so skip it.
    if APP_BAR_H > 0.0 {
        let bar = Rect::new(0.0, 0.0, width, APP_BAR_H);
        scene.fill(Fill::NonZero, ID, theme.app_bar, None, &bar);
        scene.fill(
            Fill::NonZero,
            ID,
            theme.border,
            None,
            &Rect::new(0.0, APP_BAR_H - 1.0, width, APP_BAR_H),
        );
        // The name sits in this strip only where the OS title bar is
        // hidden (macOS). Elsewhere the native title bar already shows it.
        #[cfg(target_os = "macos")]
        {
            let name = "Amalith Ver. Alpha";
            let tw = text.measure(name, 12.5);
            text.draw(
                scene,
                name,
                12.5,
                Color::from_rgb8(0xcd, 0xcd, 0xcd),
                (width - tw) * 0.5,
                APP_BAR_H * 0.5 + 4.5,
            );
        }
        if let Some(status) = status {
            let sw = text.measure(status, 11.5);
            text.draw(
                scene,
                status,
                11.5,
                Color::from_rgb8(0x9a, 0x9a, 0x9a),
                width - sw - 12.0,
                APP_BAR_H * 0.5 + 4.0,
            );
        }
    }

    // The New Document modal sits over everything.
    if let Some(form) = newdoc_form {
        newdoc::paint(scene, text, theme, Rect::new(0.0, 0.0, width, height), form);
    }
}
