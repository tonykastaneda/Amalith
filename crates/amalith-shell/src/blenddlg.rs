//! The Blend Options dialog: spacing mode (Smooth Color / Specified Steps
//! / Specified Distance) and, for the latter two, the value — Object ▸
//! Blend ▸ Blend Options. Layout / hit-testing / painting live here;
//! `app/blend_dialog.rs` is the App-side glue (spawning the floating
//! window, closing it, keyboard), mirroring `xformdlg.rs` /
//! `app/xform_dialog.rs`. The numeric field is hand-rolled (a plain
//! string buffer), not the reusable `TextField` widget — `TextField::
//! paint` needs `&mut self`, which doesn't fit the read-only `Ctx` the
//! generic panel-paint pipeline hands every dialog (`xformdlg` and
//! `shapedialog` hand-roll their own fields for the same reason).

use amalith_core::{BlendSpacing, ObjectId};
use vello::kurbo::{Circle, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

pub const W: f64 = 240.0;
const PAD: f64 = 14.0;
const ROW_H: f64 = 26.0;
const ROW_GAP: f64 = 4.0;
const FIELD_H: f64 = 26.0;
const BTN_H: f64 = 30.0;

/// Which row is selected — the dialog's own choice, decoupled from
/// [`BlendSpacing`] so switching rows before OK doesn't need a value yet.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    SmoothColor,
    Steps,
    Distance,
}

pub struct BlendDialog {
    pub group: ObjectId,
    /// Preserved as-is and written back unchanged on OK — this dialog
    /// only ever edits spacing, never the spine.
    pub spine: Option<ObjectId>,
    /// The spacing this group had when the dialog opened — restored if
    /// Preview is on and the dialog is cancelled.
    pub original_spacing: BlendSpacing,
    pub mode: Mode,
    /// The Steps/Distance value, shared across both modes for simplicity
    /// (switching modes keeps whatever number is already typed).
    pub value: String,
    pub focused: bool,
    /// Off by default — Illustrator's own Blend Options starts unchecked
    /// too. While on, every edit re-applies the chosen spacing live; off,
    /// nothing changes until OK.
    pub preview: bool,
}

impl BlendDialog {
    pub fn open(group: ObjectId, spacing: BlendSpacing, spine: Option<ObjectId>) -> Self {
        let (mode, value) = match spacing {
            BlendSpacing::SmoothColor => (Mode::SmoothColor, "20".to_string()),
            BlendSpacing::SpecifiedSteps(n) => (Mode::Steps, n.to_string()),
            BlendSpacing::SpecifiedDistance(d) => (Mode::Distance, format!("{d:.0}")),
        };
        Self {
            group,
            spine,
            original_spacing: spacing,
            mode,
            value,
            focused: mode != Mode::SmoothColor,
            preview: false,
        }
    }

    /// The `BlendSpacing` OK should commit.
    pub fn resolved_spacing(&self) -> BlendSpacing {
        match self.mode {
            Mode::SmoothColor => BlendSpacing::SmoothColor,
            Mode::Steps => {
                BlendSpacing::SpecifiedSteps(self.value.trim().parse::<u32>().unwrap_or(1).max(1))
            }
            Mode::Distance => BlendSpacing::SpecifiedDistance(
                self.value.trim().parse::<f64>().unwrap_or(10.0).max(0.1),
            ),
        }
    }

    pub fn push_char(&mut self, ch: char) {
        let allowed = ch.is_ascii_digit() || (self.mode == Mode::Distance && ch == '.' && !self.value.contains('.'));
        if allowed && self.value.len() < 9 {
            self.value.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        self.value.pop();
    }

    pub fn nudge(&mut self, dir: f64) {
        if self.mode == Mode::SmoothColor {
            return;
        }
        let step = if self.mode == Mode::Steps { 1.0 } else { 10.0 };
        let cur: f64 = self.value.trim().parse().unwrap_or(0.0);
        let next = (cur + dir * step).max(0.0);
        self.value = format!("{next:.0}");
    }
}

/// Body height (window-local, excluding the tab strip) — fixed
/// regardless of `mode` so switching rows never resizes the window.
pub fn body_height() -> f64 {
    PAD + 3.0 * (ROW_H + ROW_GAP) + 8.0 + FIELD_H + 12.0 + BTN_H + PAD
}

struct Layout {
    rows: [Rect; 3],
    field: Rect,
    preview: Rect,
    ok: Rect,
    cancel: Rect,
}

fn layout(body: Rect) -> Layout {
    let x0 = body.x0 + PAD;
    let x1 = body.x1 - PAD;
    let mut y = body.y0 + PAD;
    let rows = std::array::from_fn(|_| {
        let r = Rect::new(x0, y, x1, y + ROW_H);
        y += ROW_H + ROW_GAP;
        r
    });
    y += 8.0;
    let field = Rect::new(x0 + 90.0, y, x1, y + FIELD_H);
    y += FIELD_H + 12.0;
    // Preview checkbox (+ label) on the left, Cancel/OK on the right —
    // all one row, matching Illustrator's own Blend Options layout.
    let preview = Rect::new(x0, y + (BTN_H - 16.0) / 2.0, x0 + 16.0, y + (BTN_H + 16.0) / 2.0);
    let btn_w = 68.0;
    let ok = Rect::new(x1 - btn_w, y, x1, y + BTN_H);
    let cancel = Rect::new(ok.x0 - 8.0 - btn_w, y, ok.x0 - 8.0, y + BTN_H);
    Layout { rows, field, preview, ok, cancel }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Row(usize),
    Field,
    Preview,
    Ok,
    Cancel,
    None,
}

pub fn hit(body: Rect, p: Point) -> Hit {
    let lay = layout(body);
    for (i, r) in lay.rows.iter().enumerate() {
        if r.contains(p) {
            return Hit::Row(i);
        }
    }
    if lay.field.contains(p) {
        return Hit::Field;
    }
    // A little slop around the small checkbox glyph — easier to hit.
    if lay.preview.inflate(6.0, 6.0).contains(p) {
        return Hit::Preview;
    }
    if lay.ok.contains(p) {
        return Hit::Ok;
    }
    if lay.cancel.contains(p) {
        return Hit::Cancel;
    }
    Hit::None
}

const ID: vello::kurbo::Affine = vello::kurbo::Affine::IDENTITY;

pub fn paint(
    scene: &mut Scene,
    dlg: &BlendDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(body);
    let labels = ["Smooth Color", "Specified Steps", "Specified Distance"];
    let modes = [Mode::SmoothColor, Mode::Steps, Mode::Distance];
    for (i, r) in lay.rows.iter().enumerate() {
        let on = modes[i] == dlg.mode;
        let bullet_c = Point::new(r.x0 + 6.0, r.center().y);
        if on {
            scene.fill(Fill::NonZero, ID, theme.accent, None, &Circle::new(bullet_c, 4.0));
        }
        scene.stroke(&Stroke::new(1.2), ID, theme.text_dim, None, &Circle::new(bullet_c, 5.5));
        let col = if on { theme.text } else { theme.text_dim };
        text.draw(scene, labels[i], 12.5, col, r.x0 + 18.0, r.center().y + 4.5);
    }
    let enabled = dlg.mode != Mode::SmoothColor;
    let label = match dlg.mode {
        Mode::Steps => "Steps:",
        _ => "Distance:",
    };
    let label_col = if enabled { theme.text_dim } else { theme.text_dim.with_alpha(0.4) };
    text.draw(scene, label, 12.0, label_col, body.x0 + PAD, lay.field.center().y + 4.5);
    if enabled {
        scene.fill(Fill::NonZero, ID, theme.bg, None, &lay.field);
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            if dlg.focused { theme.accent } else { theme.text_dim.with_alpha(0.5) },
            None,
            &lay.field,
        );
        let shown = if dlg.focused && caret_on {
            format!("{}|", dlg.value)
        } else {
            dlg.value.clone()
        };
        text.draw(scene, &shown, 13.0, theme.text, lay.field.x0 + 10.0, lay.field.center().y + 4.5);
    }
    scene.stroke(&Stroke::new(1.2), ID, theme.text_dim, None, &lay.preview);
    if dlg.preview {
        let inset = Rect::new(
            lay.preview.x0 + 3.0,
            lay.preview.y0 + 3.0,
            lay.preview.x1 - 3.0,
            lay.preview.y1 - 3.0,
        );
        scene.fill(Fill::NonZero, ID, theme.accent, None, &inset);
    }
    text.draw(
        scene,
        "Preview",
        12.0,
        theme.text,
        lay.preview.x1 + 8.0,
        lay.preview.center().y + 4.5,
    );
    for (r, label, primary) in [(lay.cancel, "Cancel", false), (lay.ok, "OK", true)] {
        let bg = if primary { theme.accent } else { theme.strip_bg };
        scene.fill(Fill::NonZero, ID, bg, None, &r);
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &r);
        let col = if primary { theme.on_accent } else { theme.text };
        let tw = text.measure(label, 12.5);
        text.draw(scene, label, 12.5, col, r.center().x - tw / 2.0, r.center().y + 4.5);
    }
}
