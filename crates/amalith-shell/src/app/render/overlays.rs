//! Overlay painters drawn on top of everything: the font dropdown and
//! the hover tooltip.

use crate::metrics::px as ui_px;

use super::super::*;
use vello::kurbo::Line;

impl App {
    pub(in crate::app) fn paint_font_menu(&mut self) {
        let Some(m) = &self.font_menu else {
            return;
        };
        let outer = Self::font_menu_rect(m);
        let th = &self.theme;
        self.content.fill(
            Fill::NonZero,
            ID,
            th.bg,
            None,
            &outer.to_rounded_rect(ui_px(4.0)),
        );
        self.content
            .stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &outer.to_rounded_rect(ui_px(4.0)));
        self.content
            .push_clip_layer(Fill::NonZero, ID, &outer);
        let cur = match m.kind {
            panels::FontMenu::Family => self.active_text_style().family,
            panels::FontMenu::Style => {
                let s = self.active_text_style();
                panels::character::face_label(s.weight, s.italic)
            }
            panels::FontMenu::Size => {
                format!("{}", self.active_text_style().size.round() as i64)
            }
        };
        let header = m.header_h(Self::metric_fm_row());
        let items = m.matches();
        for (i, label) in items.iter().enumerate() {
            let y = outer.y0 + ui_px(3.0) + header + i as f64 * Self::metric_fm_row() - m.scroll;
            if y + Self::metric_fm_row() < outer.y0 || y > outer.y1 {
                continue;
            }
            let row = Rect::new(outer.x0, y, outer.x1, y + Self::metric_fm_row());
            let hot = row.contains(self.pointer);
            if hot {
                self.content
                    .fill(Fill::NonZero, ID, th.strip_bg, None, &row);
            }
            let sel = *label == cur;
            self.text.draw(
                &mut self.content,
                label,
                12.0,
                if sel { th.accent } else { th.text },
                row.x0 + ui_px(10.0),
                row.center().y + ui_px(4.0),
            );
        }
        // The type-to-filter row, drawn last so scrolled entries can't
        // bleed over it.
        if header > 0.0 {
            let hrow = Rect::new(
                outer.x0,
                outer.y0 + ui_px(3.0),
                outer.x1,
                outer.y0 + ui_px(3.0) + Self::metric_fm_row(),
            );
            self.content.fill(Fill::NonZero, ID, th.strip_bg, None, &hrow);
            self.content.fill(
                Fill::NonZero,
                ID,
                th.border,
                None,
                &Rect::new(outer.x0, hrow.y1, outer.x1, hrow.y1 + 1.0),
            );
            let qx = hrow.x0 + ui_px(10.0);
            let qw = self.text.measure(&m.query, 12.0);
            self.text.draw(
                &mut self.content,
                &m.query,
                12.0,
                th.text,
                qx,
                hrow.center().y + ui_px(4.0),
            );
            self.content.fill(
                Fill::NonZero,
                ID,
                th.accent,
                None,
                &Rect::new(qx + qw + 1.0, hrow.y0 + ui_px(4.0), qx + qw + ui_px(2.4), hrow.y1 - ui_px(4.0)),
            );
        }
        self.content.pop_layer();
    }

    pub(in crate::app) fn paint_ruler_menu(&mut self) {
        let Some(anchor) = self.ruler_menu else {
            return;
        };
        let fly = Self::ruler_menu_rect(anchor);
        let th = &self.theme;
        self.content
            .fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(ui_px(4.0)));
        self.content.stroke(
            &Stroke::new(ui_px(1.0)),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(ui_px(4.0)),
        );
        let cur = self.doc.editor.document().settings.default_unit;
        let mut y = fly.y0 + Self::metric_rm_pad();
        for unit in amalith_core::Unit::ALL {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::metric_rm_row());
            if row.contains(self.pointer) {
                self.content
                    .fill(Fill::NonZero, ID, th.strip_bg, None, &row);
            }
            let on = unit == cur;
            if on {
                self.text.draw(
                    &mut self.content,
                    "✓",
                    12.0,
                    th.accent,
                    row.x0 + ui_px(10.0),
                    row.center().y + ui_px(4.0),
                );
            }
            self.text.draw(
                &mut self.content,
                unit.label(),
                12.5,
                if on { th.accent } else { th.text },
                row.x0 + ui_px(28.0),
                row.center().y + ui_px(4.5),
            );
            y += Self::metric_rm_row();
        }
    }

    /// The isolation-mode breadcrumb bar across the top of the canvas.
    pub(in crate::app) fn paint_isolation_bar(&mut self) {
        self.iso_bar.clear();
        if self.isolation.is_empty() {
            return;
        }
        let crumbs = self.isolation_crumbs();
        let region = self.canvas_region();
        let inset = if self.rulers { crate::rulers::THICK } else { 0.0 };
        let bar = Rect::new(region.x0 + inset, region.y0 + inset, region.x1, region.y0 + inset + ui_px(24.0));
        let th = &self.theme;
        self.content.fill(Fill::NonZero, ID, th.strip_bg, None, &bar);
        self.content.fill(
            Fill::NonZero,
            ID,
            th.border,
            None,
            &Rect::new(bar.x0, bar.y1, bar.x1, bar.y1 + 1.0),
        );
        // "<" back arrow.
        let arrow = Rect::new(bar.x0 + ui_px(4.0), bar.y0, bar.x0 + ui_px(22.0), bar.y1);
        {
            use vello::kurbo::BezPath;
            let cy = bar.center().y;
            let mut p = BezPath::new();
            p.move_to((arrow.x0 + ui_px(11.0), cy - ui_px(4.0)));
            p.line_to((arrow.x0 + ui_px(6.0), cy));
            p.line_to((arrow.x0 + ui_px(11.0), cy + ui_px(4.0)));
            self.content
                .stroke(&Stroke::new(ui_px(1.5)), ID, th.text, None, &p);
        }
        self.iso_bar.push((arrow, self.isolation.len() - 1));

        let mut x = arrow.x1 + ui_px(6.0);
        for (i, label) in crumbs.iter().enumerate() {
            if i > 0 {
                self.text.draw(&mut self.content, "›", 12.0, self.theme.text_dim, x, bar.center().y + ui_px(4.0));
                x += ui_px(12.0);
            }
            let w = self.text.measure(label, 12.5);
            let last = i == crumbs.len() - 1;
            let col = if last { self.theme.text } else { self.theme.text_dim };
            self.text.draw(&mut self.content, label, 12.5, col, x, bar.center().y + ui_px(4.5));
            // crumb 0 = owning layer; crumbs 1.. map to isolation depth i.
            if i >= 1 {
                self.iso_bar
                    .push((Rect::new(x - ui_px(3.0), bar.y0, x + w + ui_px(3.0), bar.y1), i));
            }
            x += w + ui_px(8.0);
        }
    }

    /// Blue contour plus start / end / center brackets for one selected
    /// path-text object. The contour remains visible while the Type tool
    /// edits the text, but is only selection UI: the curve's original fill
    /// and stroke stay removed from the artwork and exports.
    pub(in crate::app) fn paint_path_text_brackets(&mut self) {
        let selecting = matches!(self.active_tool, Tool::Select | Tool::DirectSelect);
        let editing = self
            .text_edit
            .as_ref()
            .is_some_and(|te| self.doc.selection.as_slice() == [te.object]);
        if !selecting && !editing {
            return;
        }
        let [id] = self.doc.selection[..] else { return };
        let doc = self.doc.editor.document();
        let Some(amalith_core::ObjectKind::Text(td)) = doc.object(id).map(|o| &o.kind) else {
            return;
        };
        let amalith_core::TextKind::Path(pt) = &td.kind else {
            return;
        };
        let Some((arc, rel_xf)) = pathtext::resolve(doc, id, pt) else {
            return;
        };
        let live = match &self.drag {
            Drag::PathTextBracket { object, edit } if *object == id => edit.values(&arc, self.cmd_down),
            _ => *pt,
        };
        let m = self.doc.view.to_screen() * convert::affine(doc.world_transform(id)) * convert::affine(rel_xf);
        let accent = self.theme.accent;

        // Use the original Bézier geometry rather than the flattened
        // arc-length table, keeping circles and curves visually smooth.
        // Bake the transform into the geometry and stroke in screen space,
        // exactly like normal Direct Selection contours, so zooming or an
        // object scale never makes this line thicker.
        if let Some(path) = td.path_geometry.as_ref() {
            let screen = (self.doc.view.to_screen()
                * convert::affine(doc.world_transform(id)))
                * convert::bez_path(&path.geometry);
            self.content.stroke(
                &Stroke::new(1.5),
                ID,
                accent,
                None,
                &screen,
            );
        } else if let Some(amalith_core::ObjectKind::Path(path)) =
            doc.object(pt.path).map(|object| &object.kind)
        {
            let screen = (self.doc.view.to_screen()
                * convert::affine(doc.world_transform(pt.path)))
                * convert::bez_path(&path.geometry);
            self.content.stroke(
                &Stroke::new(1.5),
                ID,
                accent,
                None,
                &screen,
            );
        }

        // Illustrator hides range brackets while the insertion caret is
        // active. Keep the curve itself visible so its shape remains clear.
        if !selecting || self.text_edit.as_ref().is_some_and(|te| te.object == id) {
            return;
        }
        let layout = crate::textedit::td_layout(&mut self.text, td);
        let overflow = layout.lines().count() > 1 || layout.lines().next().is_some_and(|line| line.metrics().advance as f64 > live.end - live.start);
        // The bracket's riser (perpendicular to the path, base → tip)
        // reads as taller than the glyphs it brackets, rescaling with font
        // size / zoom instead of a fixed screen size. The crossbar at the
        // tip (parallel to the path) stays a small fixed width — that's
        // the part that must NOT grow, or it reads as wide instead of tall.
        let mc = m.as_coeffs();
        let screen_scale = (mc[0] * mc[0] + mc[1] * mc[1]).sqrt();
        let font_px = td.style.size * screen_scale;
        let stem = pathtext::bracket_stem_len(font_px);
        let cap_half = 5.0;
        for handle in pathtext::screen_brackets(&arc, &live, m, stem) {
            self.content.stroke(&Stroke::new(1.5), ID, accent, None, &Line::new(handle.base, handle.tip));
            if overflow && handle.which == pathtext::Bracket::End {
                // Lifted clear of the bracket tick (out along its own
                // outward direction) so it doesn't sit on top of the
                // draggable handle and steal clicks meant for it — same
                // size/style as the Area-text overset out-port.
                let dir = (handle.tip - handle.base).normalize();
                let p = handle.tip + dir * 16.0;
                let white = vello::peniko::Color::from_rgb8(0xff, 0xff, 0xff);
                let red = vello::peniko::Color::from_rgb8(208, 48, 48);
                self.content.stroke(&Stroke::new(1.5), ID, accent, None, &Line::new(handle.tip, p));
                let r = Rect::from_center_size(p, (11.0, 11.0));
                self.content.fill(Fill::NonZero, ID, white, None, &r);
                self.content.stroke(&Stroke::new(1.25), ID, red, None, &r);
                self.content.stroke(&Stroke::new(1.5), ID, red, None, &Line::new((p.x - 3.0, p.y), (p.x + 3.0, p.y)));
                self.content.stroke(&Stroke::new(1.5), ID, red, None, &Line::new((p.x, p.y - 3.0), (p.x, p.y + 3.0)));
                continue;
            }
            let cap = handle.tangent * cap_half;
            self.content.stroke(&Stroke::new(1.5), ID, accent, None, &Line::new(handle.tip - cap, handle.tip + cap));
        }
    }

    /// Width-tool handles for the single selected path: a small diamond
    /// at each width point's on-path location — dragging it only slides
    /// the point along the path — plus its left/right half-widths as two
    /// independently draggable dots on a line through it, perpendicular
    /// to the path. Grab radius for all three matches
    /// `width_tool::WIDTH_HANDLE_GRAB`.
    pub(in crate::app) fn paint_width_points(&mut self) {
        if self.active_tool != Tool::Width {
            return;
        }
        let Some(t) = self.width_target() else { return };
        let live = match &self.drag {
            Drag::WidthPoint { object, points, .. } if *object == t.id => points.as_slice(),
            _ => t.points.as_slice(),
        };
        let accent = self.theme.accent;
        let white = vello::peniko::Color::from_rgb8(0xff, 0xff, 0xff);
        for wp in live {
            let (center, left_p, right_p) = width_tool::width_handle_points(&t, wp);
            self.content.stroke(&Stroke::new(1.25), ID, accent, None, &Line::new(left_p, right_p));
            let dot = |c: Point| vello::kurbo::Circle::new(c, 3.0);
            self.content.fill(Fill::NonZero, ID, accent, None, &dot(left_p));
            self.content.fill(Fill::NonZero, ID, accent, None, &dot(right_p));
            // The draggable on-path handle: a filled diamond.
            let d = 5.0;
            let mut diamond = BezPath::new();
            diamond.move_to((center.x, center.y - d));
            diamond.line_to((center.x + d, center.y));
            diamond.line_to((center.x, center.y + d));
            diamond.line_to((center.x - d, center.y));
            diamond.close_path();
            self.content.fill(Fill::NonZero, ID, white, None, &diamond);
            self.content.stroke(&Stroke::new(1.25), ID, accent, None, &diamond);
        }
    }

    /// Join-tool live preview: a rubber-band line from the drag's origin
    /// endpoint to wherever it's currently over — a snapped endpoint, an
    /// overlap crossing, or (while over nothing joinable) the bare
    /// pointer — plus a small dot marking the current target.
    pub(in crate::app) fn paint_join_preview(&mut self) {
        if self.active_tool != Tool::Join {
            return;
        }
        let Drag::JoinScrub { from, target, .. } = &self.drag else { return };
        let Some(from_c) = self.join_candidates().into_iter().find(|c| (c.object, c.anchor) == *from) else {
            return;
        };
        let accent = self.theme.accent;
        let end = match target {
            Some(join_tool::JoinTarget::Endpoint { point, .. }) => *point,
            Some(join_tool::JoinTarget::Overlap { point, .. }) => *point,
            None => self.pointer,
        };
        self.content.stroke(&Stroke::new(1.5), ID, accent, None, &Line::new(from_c.point, end));
        self.content.fill(Fill::NonZero, ID, accent, None, &vello::kurbo::Circle::new(end, 4.0));
    }

    /// Highlights the hovered face, and every face swept this drag, over
    /// a Shape Builder selection — plain drag in the theme accent
    /// (what a release would merge), Alt-drag in `shape_builder::ERASE_INK`
    /// (what a release would delete instead).
    pub(in crate::app) fn paint_shape_builder_preview(&mut self) {
        if self.active_tool != Tool::ShapeBuilder {
            return;
        }
        // Keeps the region cache fresh for plain hovering, not just after
        // the first press — a no-op re-check when the selection hasn't
        // changed since it was last built.
        self.shape_builder_cache();
        let (erase, touched): (bool, Vec<usize>) = match &self.drag {
            Drag::ShapeBuilderDrag { erase, touched } => (*erase, touched.clone()),
            _ => (self.alt_down, Vec::new()),
        };
        let hovered = self.shape_builder_face_at(self.doc_point(self.pointer));
        let Some(cache) = self.shape_builder.as_ref() else { return };
        let to_screen = self.doc.view.to_screen();
        let color = if erase { shape_builder::ERASE_INK } else { self.theme.accent };
        for &i in &touched {
            let path = to_screen * cache.faces[i].contour.clone();
            self.content.fill(Fill::NonZero, ID, color.with_alpha(0.45), None, &path);
            self.content.stroke(&Stroke::new(1.5), ID, color, None, &path);
        }
        if let Some(h) = hovered {
            if !touched.contains(&h) {
                let path = to_screen * cache.faces[h].contour.clone();
                self.content.fill(Fill::NonZero, ID, color.with_alpha(0.25), None, &path);
                self.content.stroke(&Stroke::new(1.0), ID, color, None, &path);
            }
        }
    }

    /// A hollow circle tracking the brush's current size at the pointer,
    /// always shown while the tool is active — plus, mid-drag, a filled
    /// preview of the stroke swept so far, in the same red used for
    /// Shape Builder's own erase mode.
    pub(in crate::app) fn paint_eraser_preview(&mut self) {
        if self.active_tool != Tool::Eraser {
            return;
        }
        let ink = shape_builder::ERASE_INK;
        if let Drag::EraserStroke { path } = &self.drag {
            if let Some(area) = self.eraser_brush_area(path) {
                let to_screen = self.doc.view.to_screen();
                let shape = to_screen * convert::bez_path(&area.geometry);
                self.content.fill(Fill::NonZero, ID, ink.with_alpha(0.35), None, &shape);
                self.content.stroke(&Stroke::new(1.25), ID, ink, None, &shape);
            }
        }
        let r = self.eraser_size * 0.5;
        self.content.stroke(&Stroke::new(1.0), ID, ink, None, &vello::kurbo::Circle::new(self.pointer, r));
    }

    /// All guide geometry is clipped to the canvas, never over panels or menus.
    pub(in crate::app) fn paint_smart_guides(&mut self) {
        if !self.settings.smart_guides_enabled || self.pointer_win != self.main_id
            || self.prefs.is_some() || self.ctx_menu.is_some() || self.palette.is_some() {
            return;
        }
        let viewport = self.canvas_viewport();
        if !viewport.contains(self.pointer) && matches!(self.drag, Drag::None) { return; }
        let to_screen = self.doc.view.to_screen();
        let ink = smart_guides::SMART_GUIDE_INK;
        let (wl,hl) = self.main_logical_size().unwrap_or((1280.0,800.0));
        self.content.push_clip_layer(Fill::NonZero, ID, &viewport);
        if self.settings.sg_object_highlighting && matches!(self.drag, Drag::None) {
            if let Some(id) = self.sg_hovered_path {
                let doc = self.doc.editor.document();
                if let Some(pd) = doc.object(id).and_then(|o| o.kind.path_data()) {
                    // Transform the real path, retaining MoveTo boundaries; never
                    // connect separate contours with a synthetic straight line.
                    let path = to_screen * convert::affine(doc.world_transform(id)) * convert::bez_path(&pd.geometry);
                    // Illustrator tints this outline with the hovered
                    // object's own layer color, not a fixed Smart Guides
                    // pink — falls back to the fixed ink if the object's
                    // layer somehow can't be found (shouldn't happen for
                    // anything actually paintable).
                    let highlight = self
                        .owning_layer(id)
                        .and_then(|lid| doc.layer(lid))
                        .map_or(ink, |l| convert::color(l.color.rgb()));
                    self.content.stroke(&Stroke::new(1.0), ID, highlight.with_alpha(0.65), None, &path);
                }
            }
        }
        if let Some(hit) = self.smart_guide_hit.clone() { self.paint_smart_guide_hit(hit); }
        if let Some(label) = self.sg_measurement_text() {
            draw_tooltip(&mut self.content, &mut self.text, &self.theme, &label, self.pointer + Vec2::new(16.0,24.0), wl,hl);
        }
        self.content.pop_layer();
    }

    fn paint_smart_guide_hit(&mut self, hit: smart_guides::SmartGuideHit) {
        use smart_guides::{SmartGuideHit as Hit, Axis};
        let to_screen = self.doc.view.to_screen();
        let ink = smart_guides::SMART_GUIDE_INK;
        let viewport = self.canvas_viewport();
        let (wl,hl) = self.main_logical_size().unwrap_or((1280.0,800.0));
        let dash = Stroke::new(1.0).with_dashes(0.0, [4.0,3.0]);
        match hit {
            Hit::Multiple(hits) => for hit in hits { self.paint_smart_guide_hit(hit); },
            Hit::AlignEdge { axis, value } => {
                let (a,b) = match axis {
                    Axis::X => { let x = (to_screen*Point::new(value,0.0)).x; (Point::new(x,viewport.y0),Point::new(x,viewport.y1)) },
                    Axis::Y => { let y = (to_screen*Point::new(0.0,value)).y; (Point::new(viewport.x0,y),Point::new(viewport.x1,y)) },
                };
                self.content.stroke(&dash,ID,ink,None,&Line::new(a,b));
            }
            Hit::ConstructionAngle { from, degrees } => {
                let sp = to_screen*from;
                let dir = Vec2::new(degrees.to_radians().cos(),-degrees.to_radians().sin());
                self.content.stroke(&dash,ID,ink,None,&Line::new(sp,sp+dir*(wl+hl)));
                draw_tooltip(&mut self.content,&mut self.text,&self.theme,&format!("{degrees:.1}°"),self.pointer+Vec2::new(12.0,-24.0),wl,hl);
            }
            Hit::Spacing { gap_a, gap_b, px } => {
                for (a,b) in [gap_a,gap_b] {
                    let a = to_screen*a; let b = to_screen*b;
                    self.content.stroke(&dash,ID,ink,None,&Line::new(a,b));
                    let tick = if (b.x-a.x).abs() >= (b.y-a.y).abs() { Vec2::new(0.0,3.0) } else { Vec2::new(3.0,0.0) };
                    for p in [a,b] { self.content.stroke(&Stroke::new(1.0),ID,ink,None,&Line::new(p-tick,p+tick)); }
                }
                draw_tooltip(&mut self.content,&mut self.text,&self.theme,&format!("{px:.1} pt"),to_screen*gap_a.0.midpoint(gap_a.1),wl,hl);
            }
            Hit::TransformReference { center, angle } => {
                let sp = to_screen*center;
                let dir = Vec2::new(angle.cos(),angle.sin());
                self.content.stroke(&dash,ID,ink,None,&Line::new(sp,sp+dir*(wl+hl)));
            }
            other => {
                if !self.settings.sg_anchor_path_labels { return; }
                if let Some(p) = other.point() {
                    let sp = to_screen*p;
                    self.content.stroke(&Stroke::new(1.0),ID,ink,None,&vello::kurbo::Circle::new(sp,3.0));
                    if let Some(label) = other.label() {
                        // Clamped to the canvas viewport, same idea as
                        // `draw_tooltip`'s own edge handling — an anchor
                        // near the canvas edge must not draw its label
                        // half off-screen into a docked panel.
                        const FS: f32 = 11.0;
                        const MARGIN: f64 = 4.0;
                        let tw = self.text.measure(label, FS);
                        let mut lx = sp.x + 8.0;
                        if lx + tw > viewport.x1 - MARGIN {
                            lx = sp.x - 8.0 - tw;
                        }
                        lx = lx.clamp(viewport.x0 + MARGIN, (viewport.x1 - MARGIN - tw).max(viewport.x0 + MARGIN));
                        let ly = (sp.y - 8.0).clamp(viewport.y0 + FS as f64 + MARGIN, viewport.y1 - MARGIN);
                        self.text.draw(&mut self.content, label, FS, ink, lx, ly);
                    }
                }
            }
        }
    }

    /// The Free Transform tool's on-canvas mode flyout: Constrain,
    /// Transform, Perspective, Free Distort — a row of 4 small buttons
    /// hanging just below the selection.
    pub(in crate::app) fn paint_free_transform_flyout(&mut self) {
        let Some(lay) = self.free_transform_flyout_layout() else { return };
        let pointer = self.pointer;
        let mode = self.free_transform_mode;
        let constrain = self.free_transform_constrain;
        let th = self.theme.clone();

        self.content.fill(Fill::NonZero, ID, th.panel_bg, None, &lay.panel.to_rounded_rect(ui_px(6.0)));
        self.content.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &lay.panel.to_rounded_rect(ui_px(6.0)));

        let buttons = [
            (lay.constrain, constrain && mode != free_transform::FreeTransformMode::Perspective),
            (lay.transform, mode == free_transform::FreeTransformMode::Transform),
            (lay.perspective, mode == free_transform::FreeTransformMode::Perspective),
            (lay.free_distort, mode == free_transform::FreeTransformMode::FreeDistort),
        ];
        for (i, (r, selected)) in buttons.into_iter().enumerate() {
            if selected {
                self.content.fill(Fill::NonZero, ID, th.accent, None, &r.to_rounded_rect(ui_px(4.0)));
            } else if r.contains(pointer) {
                self.content
                    .fill(Fill::NonZero, ID, th.accent.with_alpha(0.16), None, &r.to_rounded_rect(ui_px(4.0)));
            }
            self.content.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &r.to_rounded_rect(ui_px(4.0)));
            let color = if i == 0 && mode == free_transform::FreeTransformMode::Perspective { th.text_dim.with_alpha(0.35) } else if selected { th.on_accent } else { th.text_dim };
            let box_ = Rect::from_center_size(r.center(), (ui_px(18.0), ui_px(18.0)));
            match i {
                0 => paint_constrain_glyph(&mut self.content, box_, color),
                1 => icons::draw(&mut self.content, icons::Icon::FreeTransform, box_, color),
                2 => paint_perspective_glyph(&mut self.content, box_, color),
                _ => paint_free_distort_glyph(&mut self.content, box_, color),
            }
        }
    }

    /// Free Transform's live Perspective/Free Distort preview: the quad
    /// the drag has put the corners at so far, plus a warped stroke-only
    /// outline of each affected object's real geometry — computed fresh
    /// every frame from the object's own untouched geometry (the
    /// document isn't touched until release, same convention as Offset
    /// Path's own preview just below).
    pub(in crate::app) fn paint_warp_preview(&mut self) {
        let Drag::Warp { src_quad, dst_quad, objects, warping: true, .. } = &self.drag else { return };
        let (src_quad, dst_quad) = (*src_quad, *dst_quad);
        let objects = objects.clone();
        let to_screen = self.doc.view.to_screen();
        let accent = self.theme.accent;

        let mut quad_path = BezPath::new();
        let scr_quad = dst_quad.map(|p| to_screen * p);
        quad_path.move_to(scr_quad[0]);
        for p in &scr_quad[1..] {
            quad_path.line_to(*p);
        }
        quad_path.close_path();
        self.content.stroke(&Stroke::new(1.25), ID, accent, None, &quad_path);
        for p in scr_quad {
            let sq = Rect::from_center_size(p, (8.0, 8.0));
            self.content.fill(Fill::NonZero, ID, Color::from_rgb8(0xff, 0xff, 0xff), None, &sq);
            self.content.stroke(&Stroke::new(1.25), ID, accent, None, &sq);
        }

        if src_quad == dst_quad {
            return;
        }
        let h_doc = amalith_core::Homography::solve(
            src_quad.map(convert::point_to_core),
            dst_quad.map(convert::point_to_core),
        );
        let document = self.doc.editor.document();
        for id in objects {
            let Some(object) = document.object(id) else { continue };
            let Some(path_data) = object.kind.path_data() else { continue };
            let world = document.world_transform(id);
            // Warp the control points first, exactly like `Command::
            // WarpPaths` will on release, then flatten the *warped*
            // path — flattening the original curve and warping the
            // resulting sample points instead would show a different
            // curve than what actually commits, since a homography
            // doesn't distribute over Bezier interpolation.
            let local_h = h_doc.conjugate(world, world.inverse());
            let Some(warped) = local_h.warp_path(path_data) else { continue; };
            let path = convert::bez_path(&warped.geometry);
            self.content.stroke(&Stroke::new(1.25 / self.doc.view.zoom), to_screen * convert::affine(world), accent, None, &path);
        }
    }

    /// Offset Path's live Preview: for each target, computed fresh every
    /// frame from that object's own untouched geometry (never the
    /// document — the dialog doesn't touch it until OK). The offset
    /// *result* paints with the object's own real fill/stroke, opaque —
    /// standing in for what OK would actually create, the same way
    /// Illustrator's own preview does — while the *original*'s boundary
    /// draws as a thin accent contour on top of everything, so it stays
    /// visible as a reference regardless of which one the fill covers.
    pub(in crate::app) fn paint_offset_preview(&mut self) {
        let Some(dlg) = &self.offset_dialog else { return };
        if !dlg.preview {
            return;
        }
        let (offset, join, miter_limit) = (dlg.resolved_offset(), dlg.join, dlg.resolved_miter_limit());
        if offset.abs() < 1e-6 {
            return;
        }
        let doc = self.doc.editor.document();
        let vt = self.doc.view.to_screen();
        let accent = self.theme.accent;
        let zoom = self.doc.view.zoom.max(1e-6);
        for (id, data) in &dlg.originals {
            let Some(obj) = doc.object(*id) else {
                continue;
            };
            let world = doc.world_transform(*id) * data.geometry.clone();
            let Some(offset_pd) = amalith_commands::offset_path(&world, offset, join, miter_limit) else {
                continue;
            };
            let screen_offset = vt * convert::bez_path(&offset_pd.geometry);
            if let Some(c) = obj.appearance.fill.color().map(convert::color) {
                self.content.fill(Fill::NonZero, ID, c, None, &screen_offset);
            }
            if let Some(c) = obj.appearance.stroke.color().map(convert::color) {
                self.content.stroke(
                    &Stroke::new((obj.appearance.stroke_width * zoom).max(0.5)),
                    ID,
                    c,
                    None,
                    &screen_offset,
                );
            }
            let screen_original = vt * convert::bez_path(&world);
            self.content.stroke(&Stroke::new(1.0), ID, accent, None, &screen_original);
        }
    }

    pub(in crate::app) fn paint_ctx_menu(&mut self) {
        let Some(menu) = &self.ctx_menu else {
            return;
        };
        let fly = Self::ctx_menu_rect(menu.origin, &menu.items);
        let th = &self.theme;
        self.content
            .fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(ui_px(5.0)));
        self.content.stroke(
            &Stroke::new(ui_px(1.0)),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(ui_px(5.0)),
        );
        let mut y = fly.y0 + Self::metric_cm_pad();
        // `menu` is borrowed from `self`; collect what we need to draw so
        // the draw calls can borrow `self` mutably.
        let rows: Vec<(f64, Option<(String, bool)>)> = menu
            .items
            .iter()
            .map(|it| match it {
                CtxItem::Sep => (Self::metric_cm_sep(), None),
                CtxItem::Action { label, enabled, .. } => {
                    (Self::metric_cm_row(), Some((label.clone(), *enabled)))
                }
            })
            .collect();
        for (h, row) in rows {
            match row {
                None => {
                    let sy = y + Self::metric_cm_sep() * 0.5;
                    self.content.stroke(
                        &Stroke::new(ui_px(1.0)),
                        ID,
                        self.theme.border,
                        None,
                        &vello::kurbo::Line::new((fly.x0 + ui_px(8.0), sy), (fly.x1 - ui_px(8.0), sy)),
                    );
                }
                Some((label, enabled)) => {
                    let r = Rect::new(fly.x0, y, fly.x1, y + Self::metric_cm_row());
                    if enabled && r.contains(self.pointer) {
                        self.content
                            .fill(Fill::NonZero, ID, self.theme.strip_bg, None, &r);
                    }
                    let col = if enabled {
                        self.theme.text
                    } else {
                        self.theme.text_dim
                    };
                    self.text.draw(
                        &mut self.content,
                        &label,
                        12.5,
                        col,
                        r.x0 + ui_px(14.0),
                        r.center().y + ui_px(4.5),
                    );
                }
            }
            y += h;
        }
    }

    /// The Stroke Width Profile dropdown — a preview ribbon plus label
    /// per preset, same popover shape as `paint_align_to_menu`.
    pub(in crate::app) fn paint_width_profile_menu(&mut self) {
        let Some(anchor) = self.width_profile_menu else {
            return;
        };
        let fly = Self::width_profile_menu_rect(anchor);
        let th = &self.theme;
        self.content.fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(ui_px(4.0)));
        self.content.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &fly.to_rounded_rect(ui_px(4.0)));
        let mut y = fly.y0 + Self::metric_wp_pad();
        for preset in amalith_core::WidthProfilePreset::ALL {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::metric_wp_row());
            if row.contains(self.pointer) {
                self.content.fill(Fill::NonZero, ID, th.strip_bg, None, &row);
            }
            let uniform = preset == amalith_core::WidthProfilePreset::Uniform;
            let preview = Rect::new(
                row.x0 + ui_px(10.0), row.y0 + ui_px(5.0),
                row.x1 - ui_px(if uniform { 75.0 } else { 10.0 }), row.y1 - ui_px(5.0),
            );
            crate::context_bar::paint_width_profile_icon(&mut self.content, preview, preset, th.text);
            if uniform {
                self.text.draw(
                    &mut self.content,
                    preset.label(),
                    12.0,
                    th.text,
                    row.x1 - ui_px(65.0),
                    row.center().y + ui_px(4.0),
                );
            }
            y += Self::metric_wp_row();
        }
    }

    pub(in crate::app) fn paint_align_to_menu(&mut self) {
        let Some(anchor) = self.align_to_menu else {
            return;
        };
        let fly = Self::align_to_menu_rect(anchor);
        let th = &self.theme;
        self.content.fill(
            Fill::NonZero,
            ID,
            th.bg,
            None,
            &fly.to_rounded_rect(ui_px(4.0)),
        );
        self.content.stroke(
            &Stroke::new(ui_px(1.0)),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(ui_px(4.0)),
        );
        let mut y = fly.y0 + Self::metric_at_pad();
        for (to, label) in Self::align_to_items() {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::metric_at_row());
            if row.contains(self.pointer) {
                self.content
                    .fill(Fill::NonZero, ID, th.strip_bg, None, &row);
            }
            let on = self.align_to == to;
            if on {
                self.text.draw(
                    &mut self.content,
                    "✓",
                    12.0,
                    th.accent,
                    row.x0 + ui_px(10.0),
                    row.center().y + ui_px(4.0),
                );
            }
            self.text.draw(
                &mut self.content,
                label,
                12.5,
                if on { th.accent } else { th.text },
                row.x0 + ui_px(28.0),
                row.center().y + ui_px(4.5),
            );
            y += Self::metric_at_row();
        }
    }

    /// The options-bar "Area Type" dropdown — same popover shape as
    /// `paint_align_to_menu`.
    pub(in crate::app) fn paint_area_align_menu(&mut self) {
        let Some(anchor) = self.area_align_menu else {
            return;
        };
        let fly = Self::area_align_menu_rect(anchor);
        let th = &self.theme;
        self.content.fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(ui_px(4.0)));
        self.content.stroke(&Stroke::new(ui_px(1.0)), ID, th.border, None, &fly.to_rounded_rect(ui_px(4.0)));
        let mut y = fly.y0 + Self::metric_aa_pad();
        let cur = self.active_cross_align();
        for (align, label) in context_bar::area_type::OPTIONS {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::metric_aa_row());
            if row.contains(self.pointer) {
                self.content.fill(Fill::NonZero, ID, th.strip_bg, None, &row);
            }
            let on = cur == align;
            if on {
                self.text.draw(
                    &mut self.content,
                    "✓",
                    12.0,
                    th.accent,
                    row.x0 + ui_px(10.0),
                    row.center().y + ui_px(4.0),
                );
            }
            self.text.draw(
                &mut self.content,
                label,
                12.5,
                if on { th.accent } else { th.text },
                row.x0 + ui_px(28.0),
                row.center().y + ui_px(4.5),
            );
            y += Self::metric_aa_row();
        }
    }

    pub(in crate::app) fn paint_panel_menu(&mut self, wl: f64, hl: f64) {
        let Some(m) = self.panel_menu else {
            return;
        };
        let items = panels::menu(m.panel, &self.tip_ctx());
        let fly = Self::panel_menu_flyout(m.anchor, &items, wl, hl);
        let th = &self.theme;
        // Light the hamburger while its menu is open.
        self.content
            .fill(Fill::NonZero, ID, th.strip_active, None, &m.anchor);
        let c = m.anchor.center();
        let half = 5.5;
        let gap = ui_px(3.4);
        let stroke = Stroke::new(ui_px(1.4));
        for i in [-1, 0, 1] {
            let y = c.y + i as f64 * gap;
            self.content.stroke(
                &stroke,
                ID,
                th.text,
                None,
                &vello::kurbo::Line::new((c.x - half, y), (c.x + half, y)),
            );
        }
        self.content.fill(
            Fill::NonZero,
            ID,
            th.bg,
            None,
            &fly.to_rounded_rect(ui_px(6.0)),
        );
        self.content.stroke(
            &Stroke::new(ui_px(1.0)),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(ui_px(6.0)),
        );
        let mut y = fly.y0 + Self::metric_pm_pad();
        for e in &items {
            match e {
                panels::MenuEntry::Separator => {
                    let mid = y + Self::metric_pm_sep() * 0.5;
                    self.content.fill(
                        Fill::NonZero,
                        ID,
                        th.border,
                        None,
                        &Rect::new(fly.x0 + ui_px(10.0), mid, fly.x1 - ui_px(10.0), mid + 1.0),
                    );
                    y += Self::metric_pm_sep();
                }
                panels::MenuEntry::Item {
                    label, checked, ..
                } => {
                    let row = Rect::new(fly.x0, y, fly.x1, y + Self::metric_pm_row());
                    if row.contains(self.pointer) {
                        self.content
                            .fill(Fill::NonZero, ID, th.strip_bg, None, &row);
                    }
                    if *checked {
                        self.text.draw(
                            &mut self.content,
                            "✓",
                            12.0,
                            th.text,
                            row.x0 + ui_px(10.0),
                            row.center().y + ui_px(4.0),
                        );
                    }
                    self.text.draw(
                        &mut self.content,
                        label,
                        12.5,
                        th.text,
                        row.x0 + ui_px(28.0),
                        row.center().y + ui_px(4.5),
                    );
                    y += Self::metric_pm_row();
                }
            }
        }
    }
}

/// A small dark tooltip box near `anchor` (screen px), clamped inside the
/// `wl`×`hl` window.
pub(in crate::app) fn draw_tooltip(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    label: &str,
    anchor: Point,
    wl: f64,
    hl: f64,
) {
    let metrics = crate::metrics::with(Clone::clone);
    let fs = metrics.tooltip_font_size;
    let tw = text.measure(label, fs);
    let pad = metrics.tooltip_pad;
    let (bw, bh) = (tw + pad * 2.0, fs as f64 * metrics.ui_scale + pad * 1.6);
    let mut x = anchor.x + metrics.tooltip_offset_x;
    let mut y = anchor.y + metrics.tooltip_offset_y;
    if x + bw > wl - metrics.tooltip_margin {
        x = (anchor.x - bw - metrics.tooltip_flip_gap).max(metrics.tooltip_margin);
    }
    if y + bh > hl - metrics.tooltip_margin {
        y = (anchor.y - bh - metrics.tooltip_flip_gap).max(metrics.tooltip_margin);
    }
    let box_ = Rect::new(x, y, x + bw, y + bh);
    scene.fill(
        Fill::NonZero,
        ID,
        Color::from_rgb8(0x1a, 0x1a, 0x1c),
        None,
        &box_.to_rounded_rect(4.0),
    );
    scene.stroke(
        &Stroke::new(1.0),
        ID,
        theme.border,
        None,
        &box_.to_rounded_rect(4.0),
    );
    text.draw(
        scene,
        label,
        fs as f32,
        Color::from_rgb8(0xe8, 0xe8, 0xea),
        x + pad,
        y + bh - pad,
    );
}

/// A padlock — the Free Transform flyout's Constrain toggle.
fn paint_constrain_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let body = Rect::new(box_.x0 + w * 0.18, box_.y0 + h * 0.46, box_.x1 - w * 0.18, box_.y1 - h * 0.10);
    let sw = (w * 0.09).max(1.2);
    scene.stroke(&Stroke::new(sw), ID, color, None, &body.to_rounded_rect(ui_px(1.5)));
    let c = Point::new(box_.center().x, body.y0);
    let r = body.width() * 0.32;
    scene.stroke(
        &Stroke::new(sw),
        ID,
        color,
        None,
        &vello::kurbo::Arc::new(c, (r, r), std::f64::consts::PI, std::f64::consts::PI, 0.0),
    );
}

/// A trapezoid narrower at the top — the Free Transform flyout's
/// Perspective Distort mode.
fn paint_perspective_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let top_in = w * 0.24;
    let mut p = BezPath::new();
    p.move_to((box_.x0 + top_in, box_.y0 + h * 0.22));
    p.line_to((box_.x1 - top_in, box_.y0 + h * 0.22));
    p.line_to((box_.x1 - w * 0.10, box_.y1 - h * 0.22));
    p.line_to((box_.x0 + w * 0.10, box_.y1 - h * 0.22));
    p.close_path();
    scene.stroke(&Stroke::new((w * 0.09).max(1.2)), ID, color, None, &p);
}

/// An irregular quad — every corner offset by a different amount — the
/// Free Transform flyout's Free Distort mode.
fn paint_free_distort_glyph(scene: &mut Scene, box_: Rect, color: Color) {
    let w = box_.width();
    let h = box_.height();
    let mut p = BezPath::new();
    p.move_to((box_.x0 + w * 0.30, box_.y0 + h * 0.14));
    p.line_to((box_.x1 - w * 0.10, box_.y0 + h * 0.28));
    p.line_to((box_.x1 - w * 0.22, box_.y1 - h * 0.10));
    p.line_to((box_.x0 + w * 0.12, box_.y1 - h * 0.24));
    p.close_path();
    scene.stroke(&Stroke::new((w * 0.09).max(1.2)), ID, color, None, &p);
}
