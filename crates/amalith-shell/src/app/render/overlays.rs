//! Overlay painters drawn on top of everything: the font dropdown and
//! the hover tooltip.

use super::super::*;

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
            &outer.to_rounded_rect(4.0),
        );
        self.content
            .stroke(&Stroke::new(1.0), ID, th.border, None, &outer.to_rounded_rect(4.0));
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
        let header = m.header_h(Self::FM_ROW);
        let items = m.matches();
        for (i, label) in items.iter().enumerate() {
            let y = outer.y0 + 3.0 + header + i as f64 * Self::FM_ROW - m.scroll;
            if y + Self::FM_ROW < outer.y0 || y > outer.y1 {
                continue;
            }
            let row = Rect::new(outer.x0, y, outer.x1, y + Self::FM_ROW);
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
                row.x0 + 10.0,
                row.center().y + 4.0,
            );
        }
        // The type-to-filter row, drawn last so scrolled entries can't
        // bleed over it.
        if header > 0.0 {
            let hrow = Rect::new(
                outer.x0,
                outer.y0 + 3.0,
                outer.x1,
                outer.y0 + 3.0 + Self::FM_ROW,
            );
            self.content.fill(Fill::NonZero, ID, th.strip_bg, None, &hrow);
            self.content.fill(
                Fill::NonZero,
                ID,
                th.border,
                None,
                &Rect::new(outer.x0, hrow.y1, outer.x1, hrow.y1 + 1.0),
            );
            let qx = hrow.x0 + 10.0;
            let qw = self.text.measure(&m.query, 12.0);
            self.text.draw(
                &mut self.content,
                &m.query,
                12.0,
                th.text,
                qx,
                hrow.center().y + 4.0,
            );
            self.content.fill(
                Fill::NonZero,
                ID,
                th.accent,
                None,
                &Rect::new(qx + qw + 1.0, hrow.y0 + 4.0, qx + qw + 2.4, hrow.y1 - 4.0),
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
            .fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(4.0));
        self.content.stroke(
            &Stroke::new(1.0),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(4.0),
        );
        let cur = self.doc.editor.document().settings.default_unit;
        let mut y = fly.y0 + Self::RM_PAD;
        for unit in amalith_core::Unit::ALL {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::RM_ROW);
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
                    row.x0 + 10.0,
                    row.center().y + 4.0,
                );
            }
            self.text.draw(
                &mut self.content,
                unit.label(),
                12.5,
                if on { th.accent } else { th.text },
                row.x0 + 28.0,
                row.center().y + 4.5,
            );
            y += Self::RM_ROW;
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
        let bar = Rect::new(region.x0 + inset, region.y0 + inset, region.x1, region.y0 + inset + 24.0);
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
        let arrow = Rect::new(bar.x0 + 4.0, bar.y0, bar.x0 + 22.0, bar.y1);
        {
            use vello::kurbo::BezPath;
            let cy = bar.center().y;
            let mut p = BezPath::new();
            p.move_to((arrow.x0 + 11.0, cy - 4.0));
            p.line_to((arrow.x0 + 6.0, cy));
            p.line_to((arrow.x0 + 11.0, cy + 4.0));
            self.content
                .stroke(&Stroke::new(1.5), ID, th.text, None, &p);
        }
        self.iso_bar.push((arrow, self.isolation.len() - 1));

        let mut x = arrow.x1 + 6.0;
        for (i, label) in crumbs.iter().enumerate() {
            if i > 0 {
                self.text.draw(&mut self.content, "›", 12.0, self.theme.text_dim, x, bar.center().y + 4.0);
                x += 12.0;
            }
            let w = self.text.measure(label, 12.5);
            let last = i == crumbs.len() - 1;
            let col = if last { self.theme.text } else { self.theme.text_dim };
            self.text.draw(&mut self.content, label, 12.5, col, x, bar.center().y + 4.5);
            // crumb 0 = owning layer; crumbs 1.. map to isolation depth i.
            if i >= 1 {
                self.iso_bar
                    .push((Rect::new(x - 3.0, bar.y0, x + w + 3.0, bar.y1), i));
            }
            x += w + 8.0;
        }
    }

    pub(in crate::app) fn paint_ctx_menu(&mut self) {
        let Some(menu) = &self.ctx_menu else {
            return;
        };
        let fly = Self::ctx_menu_rect(menu.origin, &menu.items);
        let th = &self.theme;
        self.content
            .fill(Fill::NonZero, ID, th.bg, None, &fly.to_rounded_rect(5.0));
        self.content.stroke(
            &Stroke::new(1.0),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(5.0),
        );
        let mut y = fly.y0 + Self::CM_PAD;
        // `menu` is borrowed from `self`; collect what we need to draw so
        // the draw calls can borrow `self` mutably.
        let rows: Vec<(f64, Option<(String, bool)>)> = menu
            .items
            .iter()
            .map(|it| match it {
                CtxItem::Sep => (Self::CM_SEP, None),
                CtxItem::Action { label, enabled, .. } => {
                    (Self::CM_ROW, Some((label.clone(), *enabled)))
                }
            })
            .collect();
        for (h, row) in rows {
            match row {
                None => {
                    let sy = y + Self::CM_SEP * 0.5;
                    self.content.stroke(
                        &Stroke::new(1.0),
                        ID,
                        self.theme.border,
                        None,
                        &vello::kurbo::Line::new((fly.x0 + 8.0, sy), (fly.x1 - 8.0, sy)),
                    );
                }
                Some((label, enabled)) => {
                    let r = Rect::new(fly.x0, y, fly.x1, y + Self::CM_ROW);
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
                        r.x0 + 14.0,
                        r.center().y + 4.5,
                    );
                }
            }
            y += h;
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
            &fly.to_rounded_rect(4.0),
        );
        self.content.stroke(
            &Stroke::new(1.0),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(4.0),
        );
        let mut y = fly.y0 + Self::AT_PAD;
        for (to, label) in Self::align_to_items() {
            let row = Rect::new(fly.x0, y, fly.x1, y + Self::AT_ROW);
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
                    row.x0 + 10.0,
                    row.center().y + 4.0,
                );
            }
            self.text.draw(
                &mut self.content,
                label,
                12.5,
                if on { th.accent } else { th.text },
                row.x0 + 28.0,
                row.center().y + 4.5,
            );
            y += Self::AT_ROW;
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
        let gap = 3.4;
        let stroke = Stroke::new(1.4);
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
            &fly.to_rounded_rect(6.0),
        );
        self.content.stroke(
            &Stroke::new(1.0),
            ID,
            th.border,
            None,
            &fly.to_rounded_rect(6.0),
        );
        let mut y = fly.y0 + Self::PM_PAD;
        for e in &items {
            match e {
                panels::MenuEntry::Separator => {
                    let mid = y + Self::PM_SEP * 0.5;
                    self.content.fill(
                        Fill::NonZero,
                        ID,
                        th.border,
                        None,
                        &Rect::new(fly.x0 + 10.0, mid, fly.x1 - 10.0, mid + 1.0),
                    );
                    y += Self::PM_SEP;
                }
                panels::MenuEntry::Item {
                    label, checked, ..
                } => {
                    let row = Rect::new(fly.x0, y, fly.x1, y + Self::PM_ROW);
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
                            row.x0 + 10.0,
                            row.center().y + 4.0,
                        );
                    }
                    self.text.draw(
                        &mut self.content,
                        label,
                        12.5,
                        th.text,
                        row.x0 + 28.0,
                        row.center().y + 4.5,
                    );
                    y += Self::PM_ROW;
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
    let fs = 11.5;
    let tw = text.measure(label, fs);
    let pad = 7.0;
    let (bw, bh) = (tw + pad * 2.0, fs as f64 + pad * 1.6);
    let mut x = anchor.x + 12.0;
    let mut y = anchor.y + 18.0;
    if x + bw > wl - 4.0 {
        x = (anchor.x - bw - 8.0).max(4.0);
    }
    if y + bh > hl - 4.0 {
        y = (anchor.y - bh - 8.0).max(4.0);
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
