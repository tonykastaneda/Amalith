//! The Offset Path dialog: offset distance, corner join (Miter/Round/
//! Bevel), and a miter limit (shown only for Miter) — Object ▸ Path ▸
//! Offset Path. Layout / hit-testing / painting live here; `app/
//! offset_dialog.rs` is the App-side glue (spawning the floating window,
//! closing it, keyboard), mirroring `blenddlg.rs` / `app/blend_dialog.rs`.

use crate::metrics::px as ui_px;

use amalith_core::{LineJoin, ObjectId, OffsetEffect, PathData};
use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

/// What OK commits to. `Objects` is Object ▸ Path ▸ Offset Path —
/// destructive, inserts new sibling objects (`App::close_offset_dialog`).
/// `AppearanceItem` is the same dialog retargeted at one entry in an
/// Appearance-panel item's own effect stack — OK pushes or replaces that
/// one `Effect::Offset` entry, no new object. `effect_index` is `None`
/// while adding a new effect (the footer's fx menu; OK appends) or
/// `Some(i)` while editing an existing one (a nested row; OK replaces
/// entry `i`). Reusing one dialog for both keeps Illustrator's own
/// "Offset Path" dialog muscle memory intact even though the two
/// commands underneath it behave quite differently.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Target {
    Objects,
    AppearanceItem {
        object: ObjectId,
        item_index: usize,
        effect_index: Option<usize>,
    },
}

pub fn metric_w() -> f64 { crate::metrics::with(|m| m.offsetdlg_w) }
fn metric_pad() -> f64 { crate::metrics::with(|m| m.offsetdlg_pad) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.offsetdlg_field_h) }
fn metric_row_gap() -> f64 { crate::metrics::with(|m| m.offsetdlg_row_gap) }
fn metric_label_w() -> f64 { crate::metrics::with(|m| m.offsetdlg_label_w) }
fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.offsetdlg_btn_h) }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Offset,
    MiterLimit,
}

pub struct OffsetDialog {
    /// Each target's geometry when the dialog opened — Preview and
    /// Cancel both always start from this, never the (possibly already
    /// offset) live geometry, so repeated edits can't compound.
    pub originals: Vec<(ObjectId, PathData)>,
    pub offset: String,
    pub join: LineJoin,
    pub miter_limit: String,
    pub focus: Field,
    /// On by default — Illustrator's own Offset Path starts with Preview
    /// checked (unlike Blend Options, which starts unchecked).
    pub preview: bool,
    pub target: Target,
}

impl OffsetDialog {
    pub fn open(originals: Vec<(ObjectId, PathData)>) -> Self {
        Self {
            originals,
            offset: "10".to_string(),
            join: LineJoin::Miter,
            miter_limit: "4".to_string(),
            focus: Field::Offset,
            preview: true,
            target: Target::Objects,
        }
    }

    /// Retargets the same dialog at one entry in an Appearance-panel
    /// item's effect stack instead of the destructive object-path
    /// command — seeded from `current` when editing an effect that
    /// already exists (`effect_index: Some(i)`), or the same defaults as
    /// `open` when adding a new one (`effect_index: None`, `current`
    /// should be `None` too).
    pub fn open_for_item(object: ObjectId, item_index: usize, effect_index: Option<usize>, current: Option<OffsetEffect>) -> Self {
        let (offset, join, miter_limit) = match current {
            Some(fx) => (trim_num(fx.amount), fx.join, trim_num(fx.miter_limit)),
            None => ("10".to_string(), LineJoin::Miter, "4".to_string()),
        };
        Self {
            originals: Vec::new(),
            offset,
            join,
            miter_limit,
            focus: Field::Offset,
            preview: true,
            target: Target::AppearanceItem { object, item_index, effect_index },
        }
    }

    /// The effect OK would commit, for the `AppearanceItem` target.
    pub fn resolved_effect(&self) -> OffsetEffect {
        OffsetEffect {
            amount: self.resolved_offset(),
            join: self.join,
            miter_limit: self.resolved_miter_limit(),
        }
    }

    /// The offset distance, in document px — typing e.g. `5in` converts.
    pub fn resolved_offset(&self) -> f64 {
        amalith_core::parse_measurement(self.offset.trim(), amalith_core::MeasureKind::Length(amalith_core::Unit::Px))
            .unwrap_or(0.0)
    }

    pub fn resolved_miter_limit(&self) -> f64 {
        amalith_core::parse_measurement(self.miter_limit.trim(), amalith_core::MeasureKind::Count)
            .unwrap_or(4.0)
            .max(1.0)
    }

    pub fn objects(&self) -> Vec<ObjectId> {
        self.originals.iter().map(|&(id, _)| id).collect()
    }

    pub fn push_char(&mut self, ch: char) {
        let allowed = crate::widgets::measurement_char(ch);
        if !allowed {
            return;
        }
        let buf = match self.focus {
            Field::Offset => &mut self.offset,
            Field::MiterLimit => &mut self.miter_limit,
        };
        if buf.len() < 10 {
            buf.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        match self.focus {
            Field::Offset => self.offset.pop(),
            Field::MiterLimit => self.miter_limit.pop(),
        };
    }

    pub fn nudge(&mut self, dir: f64) {
        match self.focus {
            Field::Offset => self.offset = trim_num(self.resolved_offset() + dir),
            Field::MiterLimit => self.miter_limit = trim_num((self.resolved_miter_limit() + dir).max(1.0)),
        }
    }
}

fn trim_num(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}

/// Body height (window-local, excluding the tab strip) — fixed
/// regardless of `join` so switching it never resizes the window.
pub fn body_height() -> f64 {
    metric_pad() + 3.0 * (metric_field_h() + metric_row_gap()) + ui_px(6.0) + metric_btn_h() + metric_pad()
}

struct Layout {
    offset_field: Rect,
    join_seg: [Rect; 3],
    miter_field: Rect,
    preview: Rect,
    ok: Rect,
    cancel: Rect,
}

fn layout(body: Rect) -> Layout {
    let x0 = body.x0 + metric_pad();
    let x1 = body.x1 - metric_pad();
    let mut y = body.y0 + metric_pad();
    let offset_field = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    y += metric_field_h() + metric_row_gap();
    let join_row = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    let seg_w = join_row.width() / 3.0;
    let join_seg = std::array::from_fn(|i| {
        Rect::new(
            join_row.x0 + seg_w * i as f64,
            join_row.y0,
            join_row.x0 + seg_w * (i + 1) as f64,
            join_row.y1,
        )
    });
    y += metric_field_h() + metric_row_gap();
    let miter_field = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    y += metric_field_h() + metric_row_gap() + ui_px(6.0);
    let preview = Rect::new(x0, y + (metric_btn_h() - ui_px(16.0)) * 0.5, x0 + ui_px(16.0), y + (metric_btn_h() + ui_px(16.0)) * 0.5);
    let btn_w = ui_px(68.0);
    let ok = Rect::new(x1 - btn_w, y, x1, y + metric_btn_h());
    let cancel = Rect::new(ok.x0 - ui_px(8.0) - btn_w, y, ok.x0 - ui_px(8.0), y + metric_btn_h());
    Layout { offset_field, join_seg, miter_field, preview, ok, cancel }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Offset,
    /// Index into the Miter / Round / Bevel segmented toggle.
    Join(usize),
    MiterLimit,
    Preview,
    Ok,
    Cancel,
    None,
}

pub fn hit(dlg: &OffsetDialog, body: Rect, p: Point) -> Hit {
    let lay = layout(body);
    if lay.offset_field.contains(p) {
        return Hit::Offset;
    }
    for (i, r) in lay.join_seg.iter().enumerate() {
        if r.contains(p) {
            return Hit::Join(i);
        }
    }
    if dlg.join == LineJoin::Miter && lay.miter_field.contains(p) {
        return Hit::MiterLimit;
    }
    if lay.preview.inflate(ui_px(6.0), ui_px(6.0)).contains(p) {
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

const ID: Affine = Affine::IDENTITY;
const JOIN_LABELS: [&str; 3] = ["Miter", "Round", "Bevel"];
const JOINS: [LineJoin; 3] = [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel];

pub fn paint(
    scene: &mut Scene,
    dlg: &OffsetDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(body);

    text.draw(scene, "Offset:", 12.5, theme.text_dim, body.x0 + metric_pad(), lay.offset_field.center().y + ui_px(4.5));
    scene.fill(Fill::NonZero, ID, theme.bg, None, &lay.offset_field);
    scene.stroke(
        &Stroke::new(if dlg.focus == Field::Offset { 1.5 } else { 1.0 }),
        ID,
        if dlg.focus == Field::Offset { theme.accent } else { theme.text_dim.with_alpha(0.5) },
        None,
        &lay.offset_field,
    );
    let offset_shown = if dlg.focus == Field::Offset && caret_on {
        format!("{}|", dlg.offset)
    } else {
        dlg.offset.clone()
    };
    text.draw(scene, &offset_shown, 13.0, theme.text, lay.offset_field.x0 + ui_px(10.0), lay.offset_field.center().y + ui_px(4.5));
    let px_w = text.measure("px", 11.5);
    text.draw(scene, "px", 11.5, theme.text_dim, lay.offset_field.x1 - px_w - ui_px(8.0), lay.offset_field.center().y + ui_px(4.5));

    text.draw(scene, "Joins:", 12.5, theme.text_dim, body.x0 + metric_pad(), lay.join_seg[0].center().y + ui_px(4.5));
    for (i, r) in lay.join_seg.iter().enumerate() {
        let on = JOINS[i] == dlg.join;
        scene.fill(Fill::NonZero, ID, if on { theme.accent } else { theme.bg }, None, r);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.5), None, r);
        let col = if on { theme.on_accent } else { theme.text_dim };
        let w = text.measure(JOIN_LABELS[i], 11.5);
        text.draw(scene, JOIN_LABELS[i], 11.5, col, r.center().x - w * 0.5, r.center().y + ui_px(4.0));
    }

    let miter_enabled = dlg.join == LineJoin::Miter;
    let miter_label_col = if miter_enabled { theme.text_dim } else { theme.text_dim.with_alpha(0.4) };
    text.draw(scene, "Miter limit:", 12.5, miter_label_col, body.x0 + metric_pad(), lay.miter_field.center().y + ui_px(4.5));
    if miter_enabled {
        scene.fill(Fill::NonZero, ID, theme.bg, None, &lay.miter_field);
        scene.stroke(
            &Stroke::new(if dlg.focus == Field::MiterLimit { 1.5 } else { 1.0 }),
            ID,
            if dlg.focus == Field::MiterLimit { theme.accent } else { theme.text_dim.with_alpha(0.5) },
            None,
            &lay.miter_field,
        );
        let shown = if dlg.focus == Field::MiterLimit && caret_on {
            format!("{}|", dlg.miter_limit)
        } else {
            dlg.miter_limit.clone()
        };
        text.draw(scene, &shown, 13.0, theme.text, lay.miter_field.x0 + ui_px(10.0), lay.miter_field.center().y + ui_px(4.5));
    }

    scene.stroke(&Stroke::new(ui_px(1.2)), ID, theme.text_dim, None, &lay.preview);
    if dlg.preview {
        let inset = lay.preview.inflate(ui_px(-ui_px(3.0)), ui_px(-ui_px(3.0)));
        scene.fill(Fill::NonZero, ID, theme.accent, None, &inset);
    }
    text.draw(scene, "Preview", 12.0, theme.text, lay.preview.x1 + ui_px(8.0), lay.preview.center().y + ui_px(4.5));

    crate::widgets::button(scene, text, theme, lay.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, lay.ok, "OK", true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_for_item_with_no_current_effect_seeds_the_same_defaults_as_open() {
        let object = ObjectId::new();
        let plain = OffsetDialog::open(Vec::new());
        let fresh = OffsetDialog::open_for_item(object, 0, None, None);
        assert_eq!(fresh.offset, plain.offset);
        assert_eq!(fresh.join, plain.join);
        assert_eq!(fresh.miter_limit, plain.miter_limit);
        assert_eq!(fresh.target, Target::AppearanceItem { object, item_index: 0, effect_index: None });
        assert!(fresh.originals.is_empty(), "no source object to preview via the destructive-path overlay");
    }

    #[test]
    fn open_for_item_with_a_current_effect_seeds_its_values_and_resolved_effect_round_trips() {
        let object = ObjectId::new();
        let fx = OffsetEffect { amount: -6.5, join: LineJoin::Round, miter_limit: 7.0 };
        let dlg = OffsetDialog::open_for_item(object, 2, Some(0), Some(fx));
        assert_eq!(dlg.resolved_offset(), -6.5);
        assert_eq!(dlg.join, LineJoin::Round);
        assert_eq!(dlg.resolved_miter_limit(), 7.0);
        assert_eq!(dlg.resolved_effect(), fx);
        assert_eq!(dlg.target, Target::AppearanceItem { object, item_index: 2, effect_index: Some(0) });
    }
}
