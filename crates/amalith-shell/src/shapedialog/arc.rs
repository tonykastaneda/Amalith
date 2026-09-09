//! Arc: an open (or closed) curve spanning a Length X-Axis × Length
//! Y-Axis box, shaped by Slope (-100 concave .. 100 convex, a signed
//! `Field::plain` row — not a length or a count) and anchored to one of
//! the box's two diagonals by Base Along. Its own three controls (Type,
//! Base Along, Fill Arc) are mouse-only toggles/checkbox in the options
//! area below the rows — this app fills an open path by implicit closure
//! already (see `canvas::paint_object`'s `paint_path`), so "Fill Arc"
//! unchecked maps to [`Shape::suppress_fill`] rather than needing its own
//! fill-vs-stroke rendering distinction.

use crate::metrics::px as ui_px;

use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Arc {
    /// Type: `true` = Open, `false` = Closed.
    open: bool,
    /// Base Along: `true` = X Axis, `false` = Y Axis.
    base_x: bool,
    fill_arc: bool,
}

impl Default for Arc {
    fn default() -> Self {
        Self { open: true, base_x: true, fill_arc: false }
    }
}

fn metric_row_h() -> f64 { crate::metrics::with(|m| m.shapedialog_arc_row_h) }
fn metric_row_gap() -> f64 { crate::metrics::with(|m| m.shapedialog_arc_row_gap) }
fn metric_label_w() -> f64 { crate::metrics::with(|m| m.shapedialog_arc_label_w) }
fn metric_top_pad() -> f64 { crate::metrics::with(|m| m.shapedialog_arc_top_pad) }
/// Height the three rows (Type, Base Along, Fill Arc) occupy, matching
/// `type_row` / `base_row` / `fill_row`'s own offsets exactly.
fn metric_options_h() -> f64 { crate::metrics::with(|m| m.shapedialog_arc_options_h) }

impl Arc {
    fn type_row(&self, area: Rect) -> Rect {
        Rect::new(area.x0, area.y0 + metric_top_pad(), area.x1, area.y0 + metric_top_pad() + metric_row_h())
    }
    fn base_row(&self, area: Rect) -> Rect {
        let y = self.type_row(area).y1 + metric_row_gap();
        Rect::new(area.x0, y, area.x1, y + metric_row_h())
    }
    fn fill_row(&self, area: Rect) -> Rect {
        let y = self.base_row(area).y1 + metric_row_gap();
        Rect::new(area.x0, y, area.x1, y + metric_row_h())
    }
    fn toggle_rect(row: Rect) -> Rect {
        Rect::new(row.x0 + metric_label_w(), row.y0, row.x1, row.y1)
    }

    fn paint_toggle(
        scene: &mut Scene,
        text: &mut TextContext,
        theme: &Theme,
        row: Rect,
        label: &str,
        options: [&str; 2],
        left_selected: bool,
    ) {
        text.draw(scene, label, 12.5, theme.text_dim, row.x0, row.center().y + ui_px(4.5));
        let r = Self::toggle_rect(row);
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &r.to_rounded_rect(ui_px(4.0)));
        let left = Rect::new(r.x0, r.y0, r.center().x, r.y1);
        let right = Rect::new(r.center().x, r.y0, r.x1, r.y1);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &if left_selected { left } else { right });
        for (seg, opt, on) in [(left, options[0], left_selected), (right, options[1], !left_selected)] {
            let col = if on { theme.on_accent } else { theme.text_dim };
            let w = text.measure(opt, 11.5);
            text.draw(scene, opt, 11.5, col, seg.center().x - w * 0.5, seg.center().y + ui_px(4.0));
        }
    }
}

impl Shape for Arc {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Length X-Axis", p.arc.0),
            Field::len("Length Y-Axis", p.arc.1),
            Field::plain("Slope", p.arc.2),
        ]
    }

    fn seed(&mut self, p: &Params) {
        self.open = p.arc.3;
        self.base_x = p.arc.4;
        self.fill_arc = p.arc.5;
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let x_len = v[0].max(1.0);
        let y_len = v[1].max(1.0);
        let slope = v.get(2).copied().unwrap_or(50.0).clamp(-100.0, 100.0);

        // Base Along picks which diagonal of the box the curve spans:
        // X Axis is the classic bottom-left → top-right ramp; Y Axis
        // swaps it to top-left → bottom-right.
        let (p0, p1) = if self.base_x {
            (
                amalith_core::Point::new(a.x, a.y + y_len),
                amalith_core::Point::new(a.x + x_len, a.y),
            )
        } else {
            (
                amalith_core::Point::new(a.x, a.y),
                amalith_core::Point::new(a.x + x_len, a.y + y_len),
            )
        };

        // A single symmetric bulge off the chord's midpoint, scaled by
        // Slope — 0 is a straight line, ±100 the full half-chord bulge.
        // Elevated from a quadratic to the cubic `curve_to` needs.
        let mid = amalith_core::Point::new((p0.x + p1.x) * 0.5, (p0.y + p1.y) * 0.5);
        let (dx, dy) = (p1.x - p0.x, p1.y - p0.y);
        let chord = (dx * dx + dy * dy).sqrt().max(1e-6);
        let (nx, ny) = (-dy / chord, dx / chord);
        let bulge = (slope / 100.0) * chord * 0.5;
        let control = amalith_core::Point::new(mid.x + nx * bulge, mid.y + ny * bulge);
        let c1 = amalith_core::Point::new(
            p0.x + (control.x - p0.x) * 2.0 / 3.0,
            p0.y + (control.y - p0.y) * 2.0 / 3.0,
        );
        let c2 = amalith_core::Point::new(
            p1.x + (control.x - p1.x) * 2.0 / 3.0,
            p1.y + (control.y - p1.y) * 2.0 / 3.0,
        );

        let mut path = amalith_core::geom::BezPath::new();
        path.move_to(p0);
        path.curve_to(c1, c2, p1);
        if !self.open {
            path.close_path();
        }
        Geometry::Path(amalith_core::PathData::from_bezpath(path))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        let slope = v.get(2).copied().unwrap_or(50.0).clamp(-100.0, 100.0);
        p.arc = (v[0], v[1], slope, self.open, self.base_x, self.fill_arc);
    }

    fn has_link(&self) -> bool {
        true
    }

    fn suppress_fill(&self) -> bool {
        !self.fill_arc
    }

    fn options_height(&self) -> f64 {
        metric_options_h()
    }

    fn paint_options(&self, scene: &mut Scene, area: Rect, theme: &Theme, text: &mut TextContext) {
        Self::paint_toggle(scene, text, theme, self.type_row(area), "Type:", ["Open", "Closed"], self.open);
        Self::paint_toggle(scene, text, theme, self.base_row(area), "Base Along:", ["X Axis", "Y Axis"], self.base_x);

        let row = self.fill_row(area);
        let box_ = Rect::new(row.x0, row.center().y - ui_px(7.0), row.x0 + ui_px(14.0), row.center().y + ui_px(7.0));
        scene.stroke(&Stroke::new(ui_px(1.2)), Affine::IDENTITY, theme.text_dim, None, &box_.to_rounded_rect(ui_px(3.0)));
        if self.fill_arc {
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &box_.to_rounded_rect(ui_px(3.0)));
            let mut check = BezPath::new();
            check.move_to((box_.x0 + ui_px(3.0), box_.center().y));
            check.line_to((box_.x0 + ui_px(6.0), box_.y1 - ui_px(3.0)));
            check.line_to((box_.x1 - ui_px(2.5), box_.y0 + ui_px(3.0)));
            scene.stroke(&Stroke::new(ui_px(1.6)), Affine::IDENTITY, theme.on_accent, None, &check);
        }
        text.draw(scene, "Fill Arc", 12.5, theme.text, box_.x1 + ui_px(8.0), row.center().y + ui_px(4.5));
    }

    fn hit_options(&self, area: Rect, local: Point) -> Option<u32> {
        let type_toggle = Self::toggle_rect(self.type_row(area));
        if type_toggle.contains(local) {
            return Some(if local.x < type_toggle.center().x { 0 } else { 1 });
        }
        let base_toggle = Self::toggle_rect(self.base_row(area));
        if base_toggle.contains(local) {
            return Some(if local.x < base_toggle.center().x { 2 } else { 3 });
        }
        let row = self.fill_row(area);
        if row.contains(local) {
            return Some(4);
        }
        None
    }

    fn on_option(&mut self, tag: u32) -> bool {
        match tag {
            0 => self.open = true,
            1 => self.open = false,
            2 => self.base_x = true,
            3 => self.base_x = false,
            4 => self.fill_arc = !self.fill_arc,
            _ => return false,
        }
        true
    }
}
