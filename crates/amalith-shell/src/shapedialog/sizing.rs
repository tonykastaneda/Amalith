//! The **sizing** layer: a vertical stack of labelled numeric fields with
//! optional up/down steppers and a Width/Height constrain-link.
//!
//! It is the reusable middle of the shape dialog — it owns the rows, the
//! caret, keyboard editing, number parse/format, and the row layout /
//! hit-testing / painting. It knows nothing about shapes; a shape just
//! hands it a set of [`Field`]s and later reads the committed values back.

use vello::kurbo::{Affine, BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

pub(crate) const PAD_X: f64 = 20.0;
const TOP_PAD: f64 = 18.0;
const FIELD_H: f64 = 24.0;
const ROW_STRIDE: f64 = 42.0;
const LABEL_W: f64 = 66.0;
/// Width reserved on the right of the W/H rows for the constrain-link icon.
const LINK_W: f64 = 26.0;
/// Width of the up/down stepper inside an integer field.
const STEP_W: f64 = 15.0;

/// Height a stack of `n` rows occupies from the panel-body top.
pub(crate) fn stack_height(n: usize) -> f64 {
    TOP_PAD + n as f64 * ROW_STRIDE
}

/// One editable row.
pub(crate) struct Field {
    label: &'static str,
    buf: String,
    /// Reformat with a `px` suffix on commit; integer + stepper otherwise.
    length: bool,
}

impl Field {
    /// A length row — reformats to `"<n> px"` on commit.
    pub(crate) fn len(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: fmt_len(v),
            length: true,
        }
    }
    /// An integer-count row (min 3) with an up/down stepper.
    pub(crate) fn count(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: format!("{}", v.round().max(3.0) as i64),
            length: false,
        }
    }
}

/// Where a pointer landed inside the field stack.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Hit {
    None,
    Field(usize),
    Step(usize, i32),
    Link,
}

pub(crate) struct Sizing {
    fields: Vec<Field>,
    focus: usize,
    has_link: bool,
    linked: bool,
    link_ratio: f64,
}

impl Sizing {
    pub(crate) fn new(fields: Vec<Field>, has_link: bool) -> Self {
        // Length rows (Width/Height) take focus first; a count row last,
        // matching Illustrator.
        let focus = if has_link {
            0
        } else {
            fields.len().saturating_sub(1)
        };
        Self {
            fields,
            focus,
            has_link,
            linked: false,
            link_ratio: 1.0,
        }
    }

    /// Height this stack occupies from the panel-body top.
    pub(crate) fn height(&self) -> f64 {
        stack_height(self.fields.len())
    }

    // --- layout ----------------------------------------------------

    fn field_rect(&self, body: Rect, i: usize) -> Rect {
        let y = body.y0 + TOP_PAD + i as f64 * ROW_STRIDE;
        let right = body.x1 - PAD_X - if self.has_link && i < 2 { LINK_W } else { 0.0 };
        Rect::new(body.x0 + PAD_X + LABEL_W + 8.0, y, right, y + FIELD_H)
    }

    fn link_rect(&self, body: Rect) -> Rect {
        let f0 = self.field_rect(body, 0);
        let f1 = self.field_rect(body, 1);
        let cx = body.x1 - PAD_X - LINK_W * 0.5;
        Rect::new(cx - 8.0, f0.y0, cx + 8.0, f1.y1)
    }

    fn step_rects(&self, body: Rect, i: usize) -> (Rect, Rect) {
        let f = self.field_rect(body, i);
        let sx = Rect::new(f.x0 + 1.0, f.y0 + 1.0, f.x0 + STEP_W, f.y1 - 1.0);
        let mid = sx.y0 + sx.height() * 0.5;
        (
            Rect::new(sx.x0, sx.y0, sx.x1, mid),
            Rect::new(sx.x0, mid, sx.x1, sx.y1),
        )
    }

    pub(crate) fn hit(&self, body: Rect, local: Point) -> Hit {
        if self.has_link && self.link_rect(body).contains(local) {
            return Hit::Link;
        }
        for i in 0..self.fields.len() {
            if !self.fields[i].length {
                let (up, down) = self.step_rects(body, i);
                if up.contains(local) {
                    return Hit::Step(i, 1);
                }
                if down.contains(local) {
                    return Hit::Step(i, -1);
                }
            }
            if self.field_rect(body, i).contains(local) {
                return Hit::Field(i);
            }
        }
        Hit::None
    }

    // --- editing -------------------------------------------------

    pub(crate) fn focus_field(&mut self, i: usize) {
        if i < self.fields.len() && i != self.focus {
            self.commit_focus();
            self.focus = i;
        }
    }

    pub(crate) fn push_char(&mut self, ch: char) {
        if ch.is_ascii_digit() || ch == '.' || ch == '-' {
            self.fields[self.focus].buf.push(ch);
        }
    }

    pub(crate) fn backspace(&mut self) {
        self.fields[self.focus].buf.pop();
    }

    pub(crate) fn focus_next(&mut self) {
        self.commit_focus();
        self.focus = (self.focus + 1) % self.fields.len();
    }

    pub(crate) fn focus_prev(&mut self) {
        self.commit_focus();
        self.focus = (self.focus + self.fields.len() - 1) % self.fields.len();
    }

    pub(crate) fn step(&mut self, i: usize, delta: f64) {
        if i >= self.fields.len() {
            return;
        }
        let v = (parse_num(&self.fields[i].buf).unwrap_or(3.0) + delta).max(3.0);
        self.fields[i].buf = format!("{}", v.round() as i64);
        self.focus = i;
    }

    pub(crate) fn toggle_link(&mut self) {
        if !self.has_link {
            return;
        }
        self.linked = !self.linked;
        if self.linked {
            let (w, h) = (self.value(0), self.value(1));
            self.link_ratio = if w > 0.0 { h / w } else { 1.0 };
        }
    }

    /// Reformat the focused buffer, and mirror W↔H while linked.
    pub(crate) fn commit_focus(&mut self) {
        let f = &mut self.fields[self.focus];
        let v = parse_num(&f.buf);
        if f.length {
            if let Some(v) = v {
                f.buf = fmt_len(v.max(0.0));
            }
        } else {
            let n = v.unwrap_or(3.0).round().max(3.0);
            f.buf = format!("{}", n as i64);
        }
        if self.linked && self.has_link && self.focus < 2 {
            let other = 1 - self.focus;
            let base = self.value(self.focus);
            let mirrored = if self.focus == 0 {
                base * self.link_ratio
            } else if self.link_ratio.abs() > f64::EPSILON {
                base / self.link_ratio
            } else {
                base
            };
            self.fields[other].buf = fmt_len(mirrored.max(0.0));
        }
    }

    pub(crate) fn commit_all(&mut self) {
        let here = self.focus;
        for i in 0..self.fields.len() {
            self.focus = i;
            self.commit_focus();
        }
        self.focus = here;
    }

    fn value(&self, i: usize) -> f64 {
        parse_num(&self.fields[i].buf).unwrap_or(0.0)
    }

    /// The committed row values, in row order.
    pub(crate) fn values(&self) -> Vec<f64> {
        self.fields
            .iter()
            .map(|f| parse_num(&f.buf).unwrap_or(0.0))
            .collect()
    }

    // --- painting ------------------------------------------------

    pub(crate) fn paint(
        &self,
        scene: &mut Scene,
        body: Rect,
        theme: &Theme,
        text: &mut TextContext,
        caret_on: bool,
    ) {
        for (i, f) in self.fields.iter().enumerate() {
            let fr = self.field_rect(body, i);
            let lw = text.measure(f.label, 12.5);
            text.draw(
                scene,
                f.label,
                12.5,
                theme.text_dim,
                fr.x0 - 8.0 - lw,
                fr.y0 + FIELD_H * 0.5 + 4.5,
            );
            let focused = i == self.focus;
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.bg,
                None,
                &fr.to_rounded_rect(3.0),
            );
            scene.stroke(
                &Stroke::new(if focused { 1.5 } else { 1.0 }),
                Affine::IDENTITY,
                if focused { theme.accent } else { theme.border },
                None,
                &fr.to_rounded_rect(3.0),
            );

            let mut tx = fr.x0 + 8.0;
            if !f.length {
                let (up, down) = self.step_rects(body, i);
                tri(scene, up, true, theme.text_dim);
                tri(scene, down, false, theme.text_dim);
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    theme.border,
                    None,
                    &Rect::new(up.x1, fr.y0 + 3.0, up.x1 + 1.0, fr.y1 - 3.0),
                );
                tx = up.x1 + 8.0;
            }
            text.draw(scene, &f.buf, 12.5, theme.text, tx, fr.y0 + FIELD_H * 0.5 + 4.5);
            if focused && caret_on {
                let cx = tx + text.measure(&f.buf, 12.5) + 1.0;
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    theme.text,
                    None,
                    &Rect::new(cx, fr.y0 + 4.0, cx + 1.4, fr.y1 - 4.0),
                );
            }
        }

        if self.has_link {
            self.draw_link(scene, self.link_rect(body), theme);
        }
    }

    fn draw_link(&self, scene: &mut Scene, r: Rect, theme: &Theme) {
        let col = if self.linked {
            theme.accent
        } else {
            theme.text_dim
        };
        let cx = r.x0 + r.width() * 0.5;
        let mut p = BezPath::new();
        p.move_to((cx - 4.0, r.y0 + 2.0));
        p.line_to((cx + 2.0, r.y0 + 2.0));
        p.line_to((cx + 2.0, r.y1 - 2.0));
        p.line_to((cx - 4.0, r.y1 - 2.0));
        scene.stroke(&Stroke::new(1.4), Affine::IDENTITY, col, None, &p);
        let mid = r.y0 + r.height() * 0.5;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            col,
            None,
            &Circle::new((cx + 2.0, mid), if self.linked { 2.6 } else { 1.8 }),
        );
    }
}

fn tri(scene: &mut Scene, cell: Rect, up: bool, color: Color) {
    let cx = cell.x0 + cell.width() * 0.5;
    let cy = cell.y0 + cell.height() * 0.5;
    let mut p = BezPath::new();
    if up {
        p.move_to((cx - 3.0, cy + 1.6));
        p.line_to((cx + 3.0, cy + 1.6));
        p.line_to((cx, cy - 2.4));
    } else {
        p.move_to((cx - 3.0, cy - 1.6));
        p.line_to((cx + 3.0, cy - 1.6));
        p.line_to((cx, cy + 2.4));
    }
    p.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &p);
}

fn parse_num(s: &str) -> Option<f64> {
    let t: String = s
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    t.parse().ok()
}

fn fmt_len(v: f64) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{} px", r.round() as i64)
    } else {
        format!("{} px", r)
    }
}
