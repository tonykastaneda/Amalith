//! The main document view: canvas, rails, tab strip, app bar, context
//! bar, and the drag/flyout overlays layered over them.

use super::super::*;

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
    newdoc_form: Option<&newdoc::NewDocForm>,
    tab_labels: &[String],
    active_tab: usize,
    cursor_glyph: Option<(Tool, PenHint)>,
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
    text_editing: bool,
    font_families: &[String],
    layer_query: &str,
    layer_search_focused: bool,
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
    let viewport = Rect::new(left_x, CHROME_TOP, right_x.max(left_x), height);
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
    );

    if let Some(m) = marquee {
        scene.fill(Fill::NonZero, ID, theme.marquee_fill, None, &m);
        scene.stroke(&Stroke::new(1.0), ID, theme.accent, None, &m);
    }

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
                text_editing,
                font_families,
                layer_query,
                layer_search_focused,
            };
            for area in &laid.areas {
                if let Some(pid) = area.tabs.get(area.active).map(|t| t.panel) {
                    // Clip to the body so a panel dragged shorter than its
                    // content spills nothing past the splitter — matches
                    // the floating-panel path.
                    scene.push_clip_layer(Fill::NonZero, ID, &area.body);
                    panels::paint(scene, text, pid, area.body, &ctx);
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
        active_slot,
        cur_weight,
        cur_opacity,
        stroke_open: stroke_popover,
        text_style: text_style.clone(),
        anchor_sel_len,
    };
    context_bar::paint(scene, text, opt_bar_rect(width), &cbar);

    // The Stroke flyout hangs off the options bar's "Stroke" link.
    if stroke_popover {
        let shown_weight = representative.map(|a| a.stroke_width).unwrap_or(cur_weight);
        stroke_panel::paint(scene, text, theme, &stroke_flyout, &stroke_style, shown_weight);
    }

    // The active tool's on-document glyph, standing in for the OS cursor.
    if let Some((tool, hint)) = cursor_glyph {
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
