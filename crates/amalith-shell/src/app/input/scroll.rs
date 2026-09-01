//! Wheel and pinch: canvas zoom / pan, plus scroll-to-nudge over the
//! New Document modal, the font dropdown, the Stroke flyout, and the
//! context bar.

use vello::kurbo::Vec2;
use winit::event::MouseScrollDelta;

use crate::context_bar;
use crate::stroke_panel;

use super::super::{opt_bar_rect, App, APP_BAR_H, OPT_BAR_H};

impl App {
    pub(in crate::app) fn on_pinch(&mut self, delta: f64) {
        self.doc.view.zoom_at(1.0 + delta, self.pointer);
    }

    pub(in crate::app) fn on_wheel(&mut self, delta: MouseScrollDelta) {
        let (dx, dy) = match delta {
            // Line-based (mouse wheel): each notch ≈ 30 logical px.
            MouseScrollDelta::LineDelta(x, y) => (x as f64 * 30.0, y as f64 * 30.0),
            // Pixel-based (trackpad): physical px → logical.
            MouseScrollDelta::PixelDelta(p) => (p.x / self.scale, p.y / self.scale),
        };
        // The New Document modal scrolls its content.
        if let Some(form) = &mut self.newdoc {
            form.scroll = (form.scroll - dy).max(0.0);
            self.request_main_redraw();
            return;
        }
        // The Home screen's recent-document grid.
        if let Some(hm) = &mut self.home {
            hm.on_scroll(dy);
            self.request_main_redraw();
            return;
        }
        // An open font dropdown scrolls its (filtered) list.
        if let Some(m) = &mut self.font_menu {
            let shown = m.matches().len();
            let max = ((shown.saturating_sub(Self::FM_ROWS)) as f64 * Self::FM_ROW).max(0.0);
            m.scroll = (m.scroll - dy).clamp(0.0, max);
            self.request_main_redraw();
            return;
        }
        // Scrolling over a Stroke-flyout field nudges it.
        if self.stroke_popover && dy.abs() > 0.5 {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let lay = self.stroke_flyout_layout(w);
            let repr = self.stroke_style_repr();
            if let Some(hit) = stroke_panel::scroll_hit(&lay, &repr, self.pointer) {
                let dir = if dy > 0.0 { 1 } else { -1 };
                self.apply_stroke_flyout(hit, dir);
                return;
            }
        }
        // Scrolling over a context-bar Stroke / Opacity / Character
        // segment nudges that segment's value.
        if self.picker.is_none()
            && self.pointer.y >= APP_BAR_H
            && self.pointer.y < APP_BAR_H + OPT_BAR_H
            && dy.abs() > 0.5
        {
            let w = self.main_logical_size().map_or(1280.0, |(w, _)| w);
            let bar = opt_bar_rect(w);
            let p = self.pointer;
            let cx = self.context_bar_ctx();
            let over = |k| context_bar::segment_rect(bar, &cx, k).is_some_and(|r| r.contains(p));
            let (sw, so, sc) = (
                over(context_bar::SegKind::Stroke),
                over(context_bar::SegKind::Opacity),
                over(context_bar::SegKind::Character),
            );
            drop(cx);
            let dir = if dy > 0.0 { 1 } else { -1 };
            if sw {
                self.step_weight(dir);
                return;
            }
            if so {
                self.step_opacity(dir);
                return;
            }
            if sc {
                self.step_font_size(dir as f64);
                return;
            }
        }
        if self.cmd_down {
            // ⌘ + scroll → zoom at the cursor.
            let factor = 2f64.powf(dy / 180.0);
            self.doc.view.zoom_at(factor, self.pointer);
        } else {
            // Plain scroll → pan.
            self.doc.view.pan += Vec2::new(dx, dy);
        }
    }
}
