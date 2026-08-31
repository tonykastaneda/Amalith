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
