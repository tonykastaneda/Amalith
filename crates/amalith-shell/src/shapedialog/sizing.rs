//! The **sizing** layer: a vertical stack of labelled numeric fields with
//! optional up/down steppers and a Width/Height constrain-link.
//!
//! It is the reusable middle of the shape dialog — it owns the rows, the
//! caret, keyboard editing, number parse/format, and the row layout /
//! hit-testing / painting. It knows nothing about shapes; a shape just
//! hands it a set of [`Field`]s and later reads the committed values back.

use crate::metrics::px as ui_px;

use vello::kurbo::{Affine, BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

pub(crate) fn metric_pad_x() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_pad_x) }
fn metric_top_pad() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_top_pad) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_field_h) }
fn metric_row_stride() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_row_stride) }
fn metric_label_w() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_label_w) }
/// Width reserved on the right of the W/H rows for the constrain-link icon.
fn metric_link_w() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_link_w) }
/// Width of the up/down stepper inside an integer field.
fn metric_step_w() -> f64 { crate::metrics::with(|m| m.shapedialog_sizing_step_w) }

/// Height a stack of `n` rows occupies from the panel-body top.
pub(crate) fn stack_height(n: usize) -> f64 {
    metric_top_pad() + n as f64 * metric_row_stride()
}

/// What a row's buffer means, for commit-time reformatting, and whether
/// it gets an up/down stepper.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Reformats with a `px` suffix on commit.
    Length,
    /// Integer, floored at 3, with an up/down stepper.
    Count,
    /// A plain signed decimal — no suffix, no stepper, no floor. For a row
    /// that isn't a length or a count (Arc's Slope, -100..100).
    Plain,
    /// Reformats with a `%` suffix on commit, no stepper (Spiral's Decay).
    Percent,
}

/// One editable row.
pub(crate) struct Field {
    label: &'static str,
    buf: String,
    kind: Kind,
}

impl Field {
    /// A length row — reformats to `"<n> px"` on commit.
    pub(crate) fn len(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: fmt_len(v),
            kind: Kind::Length,
        }
    }
    /// An integer-count row (min 3) with an up/down stepper.
    pub(crate) fn count(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: format!("{}", v.round().max(3.0) as i64),
            kind: Kind::Count,
        }
    }
    /// A plain signed-decimal row — see [`Kind::Plain`].
    pub(crate) fn plain(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: fmt_plain(v),
            kind: Kind::Plain,
        }
    }
    /// A percentage row — see [`Kind::Percent`].
    pub(crate) fn percent(label: &'static str, v: f64) -> Self {
        Self {
            label,
            buf: fmt_percent(v),
            kind: Kind::Percent,
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
        let y = body.y0 + metric_top_pad() + i as f64 * metric_row_stride();
        let right = body.x1 - metric_pad_x() - if self.has_link && i < 2 { metric_link_w() } else { 0.0 };
        Rect::new(body.x0 + metric_pad_x() + metric_label_w() + ui_px(8.0), y, right, y + metric_field_h())
    }

    fn link_rect(&self, body: Rect) -> Rect {
        let f0 = self.field_rect(body, 0);
        let f1 = self.field_rect(body, 1);
        let cx = body.x1 - metric_pad_x() - metric_link_w() * 0.5;
        Rect::new(cx - ui_px(8.0), f0.y0, cx + ui_px(8.0), f1.y1)
    }

    fn step_rects(&self, body: Rect, i: usize) -> (Rect, Rect) {
        let f = self.field_rect(body, i);
        let sx = Rect::new(f.x0 + 1.0, f.y0 + 1.0, f.x0 + metric_step_w(), f.y1 - 1.0);
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
            if self.fields[i].kind == Kind::Count {
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
        if crate::widgets::measurement_char(ch) {
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
        let Some(f) = self.fields.get_mut(i) else { return };
        let cur = parse_num(&f.buf, f.kind).unwrap_or(0.0);
        f.buf = match f.kind {
            Kind::Length => fmt_len((cur + delta).max(0.0)),
            Kind::Count => format!("{}", (cur + delta).max(3.0).round() as i64),
            // No clamp here — a signed row has no universal range; the
            // shape that reads it back (Arc's Slope, -100..100) clamps.
            Kind::Plain => fmt_plain(cur + delta),
            Kind::Percent => fmt_percent((cur + delta).max(0.0)),
        };
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
        let v = parse_num(&f.buf, f.kind);
        match f.kind {
            Kind::Length => {
                if let Some(v) = v {
                    f.buf = fmt_len(v.max(0.0));
                }
            }
            Kind::Count => {
                let n = v.unwrap_or(3.0).round().max(3.0);
                f.buf = format!("{}", n as i64);
            }
            Kind::Plain => {
                if let Some(v) = v {
                    f.buf = fmt_plain(v);
                }
            }
            Kind::Percent => {
                if let Some(v) = v {
                    f.buf = fmt_percent(v.max(0.0));
                }
            }
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
        let f = &self.fields[i];
        parse_num(&f.buf, f.kind).unwrap_or(0.0)
    }

    /// The committed row values, in row order.
    pub(crate) fn values(&self) -> Vec<f64> {
        self.fields
            .iter()
            .map(|f| parse_num(&f.buf, f.kind).unwrap_or(0.0))
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
                fr.x0 - ui_px(8.0) - lw,
                fr.y0 + metric_field_h() * 0.5 + ui_px(4.5),
            );
            let focused = i == self.focus;
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.bg,
                None,
                &fr.to_rounded_rect(ui_px(3.0)),
            );
            scene.stroke(
                &Stroke::new(if focused { 1.5 } else { 1.0 }),
                Affine::IDENTITY,
                if focused { theme.accent } else { theme.border },
                None,
                &fr.to_rounded_rect(ui_px(3.0)),
            );

            let mut tx = fr.x0 + ui_px(8.0);
            if f.kind == Kind::Count {
                let (up, down) = self.step_rects(body, i);
                tri(scene, up, true, theme.text_dim);
                tri(scene, down, false, theme.text_dim);
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    theme.border,
                    None,
                    &Rect::new(up.x1, fr.y0 + ui_px(3.0), up.x1 + 1.0, fr.y1 - ui_px(3.0)),
                );
                tx = up.x1 + ui_px(8.0);
            }
            text.draw(scene, &f.buf, 12.5, theme.text, tx, fr.y0 + metric_field_h() * 0.5 + ui_px(4.5));
            if focused && caret_on {
                let cx = tx + text.measure(&f.buf, 12.5) + 1.0;
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    theme.text,
                    None,
                    &Rect::new(cx, fr.y0 + ui_px(4.0), cx + 1.4, fr.y1 - ui_px(4.0)),
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
        p.move_to((cx - ui_px(4.0), r.y0 + ui_px(2.0)));
        p.line_to((cx + ui_px(2.0), r.y0 + ui_px(2.0)));
        p.line_to((cx + ui_px(2.0), r.y1 - ui_px(2.0)));
        p.line_to((cx - ui_px(4.0), r.y1 - ui_px(2.0)));
        scene.stroke(&Stroke::new(ui_px(1.4)), Affine::IDENTITY, col, None, &p);
        let mid = r.y0 + r.height() * 0.5;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            col,
            None,
            &Circle::new((cx + ui_px(2.0), mid), if self.linked { 2.6 } else { 1.8 }),
        );
    }
}

fn tri(scene: &mut Scene, cell: Rect, up: bool, color: Color) {
    let cx = cell.x0 + cell.width() * 0.5;
    let cy = cell.y0 + cell.height() * 0.5;
    let mut p = BezPath::new();
    if up {
        p.move_to((cx - ui_px(3.0), cy + 1.6));
        p.line_to((cx + ui_px(3.0), cy + 1.6));
        p.line_to((cx, cy - ui_px(2.4)));
    } else {
        p.move_to((cx - ui_px(3.0), cy - 1.6));
        p.line_to((cx + ui_px(3.0), cy - 1.6));
        p.line_to((cx, cy + ui_px(2.4)));
    }
    p.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &p);
}

/// The `MeasureKind` a row's own numbers are in — `Plain` (Arc's Slope)
/// is signed and unitless like `Count`, just without the integer floor.
fn measure_kind(kind: Kind) -> amalith_core::MeasureKind {
    match kind {
        Kind::Length => amalith_core::MeasureKind::Length(amalith_core::Unit::Px),
        Kind::Percent => amalith_core::MeasureKind::Percent,
        Kind::Count | Kind::Plain => amalith_core::MeasureKind::Count,
    }
}

fn parse_num(s: &str, kind: Kind) -> Option<f64> {
    amalith_core::parse_measurement(s, measure_kind(kind))
}

fn fmt_len(v: f64) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{} px", r.round() as i64)
    } else {
        format!("{} px", r)
    }
}

fn fmt_percent(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}%", r.round() as i64)
    } else {
        format!("{r}%")
    }
}

fn fmt_plain(v: f64) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}
