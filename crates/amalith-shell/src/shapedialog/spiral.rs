//! Spiral: Radius, Decay (% the radius shrinks by every quarter turn) and
//! Segments (quarter-turns — Illustrator counts a full winding as 4, so
//! `turns = segments / 4`), centred on the click point like Polygon and
//! Star. Style is a mouse-only pair of icon buttons picking the winding
//! direction, in the options area below the rows.

use std::f64::consts::{FRAC_PI_2, TAU};

use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

use super::{Field, Geometry, Params, Shape};

pub(crate) struct Spiral {
    clockwise: bool,
}

impl Default for Spiral {
    fn default() -> Self {
        Self { clockwise: true }
    }
}

const BTN: f64 = 40.0;
const BTN_GAP: f64 = 10.0;
const LABEL_W: f64 = 96.0;
const TOP_PAD: f64 = 10.0;
const BOT_PAD: f64 = 10.0;
const OPTIONS_H: f64 = TOP_PAD + BTN + BOT_PAD;

impl Spiral {
    fn style_row(&self, area: Rect) -> Rect {
        Rect::new(area.x0, area.y0 + TOP_PAD, area.x1, area.y0 + TOP_PAD + BTN)
    }
    fn style_button(row: Rect, left: bool) -> Rect {
        let x0 = row.x0 + LABEL_W + if left { 0.0 } else { BTN + BTN_GAP };
        Rect::new(x0, row.y0, x0 + BTN, row.y0 + BTN)
    }

    /// A small stroked spiral glyph inside `r`, mirrored (winding the
    /// other way) when `!clockwise`.
    fn draw_glyph(scene: &mut Scene, r: Rect, color: vello::peniko::Color, clockwise: bool) {
        let c = r.center();
        let radius = r.width() * 0.36;
        let decay = 0.72_f64;
        let turns = 2.2_f64;
        let steps_per_turn = 40i64;
        let dir = if clockwise { 1.0 } else { -1.0 };
        let total = (turns * steps_per_turn as f64) as i64;
        let mut p = BezPath::new();
        for i in 0..=total {
            let t = i as f64 / steps_per_turn as f64 * TAU * dir;
            let k = decay.powf(t.abs() / FRAC_PI_2);
            let pt = (c.x + radius * k * t.cos(), c.y + radius * k * t.sin());
            if i == 0 {
                p.move_to(pt);
            } else {
                p.line_to(pt);
            }
        }
        scene.stroke(&Stroke::new(1.4), Affine::IDENTITY, color, None, &p);
    }
}

impl Shape for Spiral {
    fn rows(&self, p: &Params) -> Vec<Field> {
        vec![
            Field::len("Radius", p.spiral.0),
            Field::percent("Decay", p.spiral.1),
            Field::count("Segments", p.spiral.2),
        ]
    }

    fn seed(&mut self, p: &Params) {
        self.clockwise = p.spiral.3;
    }

    fn geometry(&self, a: Point, v: &[f64]) -> Geometry {
        let radius = v[0].max(1.0);
        let decay = (v[1] / 100.0).clamp(0.001, 1.0);
        let segments = v[2].round().max(4.0);
        let turns = segments / 4.0;
        let dir = if self.clockwise { 1.0 } else { -1.0 };
        let steps_per_turn = 48i64;
        let total_steps = (turns * steps_per_turn as f64).round() as i64;
        let pts: Vec<amalith_core::Point> = (0..=total_steps)
            .map(|i| {
                let t = i as f64 / steps_per_turn as f64 * TAU * dir;
                let k = decay.powf(t.abs() / FRAC_PI_2);
                amalith_core::Point::new(a.x + radius * k * t.cos(), a.y + radius * k * t.sin())
            })
            .collect();
        Geometry::Path(amalith_core::PathData::polyline(&pts))
    }

    fn write_params(&self, v: &[f64], p: &mut Params) {
        p.spiral = (v[0], v[1].max(0.0), v[2].round().max(4.0), self.clockwise);
    }

    fn options_height(&self) -> f64 {
        OPTIONS_H
    }

    fn paint_options(&self, scene: &mut Scene, area: Rect, theme: &Theme, text: &mut TextContext) {
        let row = self.style_row(area);
        text.draw(scene, "Style:", 12.5, theme.text_dim, row.x0, row.center().y + 4.5);
        for left in [true, false] {
            let r = Self::style_button(row, left);
            let selected = left == self.clockwise;
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                if selected { theme.strip_active } else { theme.bg },
                None,
                &r.to_rounded_rect(4.0),
            );
            scene.stroke(
                &Stroke::new(if selected { 1.5 } else { 1.0 }),
                Affine::IDENTITY,
                if selected { theme.accent } else { theme.border },
                None,
                &r.to_rounded_rect(4.0),
            );
            Self::draw_glyph(scene, r, theme.text, left);
        }
    }

    fn hit_options(&self, area: Rect, local: Point) -> Option<u32> {
        let row = self.style_row(area);
        if Self::style_button(row, true).contains(local) {
            return Some(0);
        }
        if Self::style_button(row, false).contains(local) {
            return Some(1);
        }
        None
    }

    fn on_option(&mut self, tag: u32) -> bool {
        match tag {
            0 => self.clockwise = true,
            1 => self.clockwise = false,
            _ => return false,
        }
        true
    }
}
