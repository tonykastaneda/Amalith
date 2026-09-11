//! The Area Type Options dialog — Type ▸ Area Type Options…, for a single
//! selected Area Type frame (horizontal or vertical). Layout / hit-testing
//! / painting live here; `app/area_type_dialog.rs` is the App-side glue
//! (spawning the floating window, closing it, keyboard), mirroring
//! `offsetdlg.rs` / `app/offset_dialog.rs`.
//!
//! Matches Illustrator's own dialog shell field-for-field, but only
//! Width / Height, the Align dropdown, Auto Size, and Preview are real —
//! Rows/Columns (a genuine multi-frame text-splitting feature, unrelated
//! to vertical text specifically) and Offset are drawn greyed, the same
//! "present but inert" convention the Paragraph panel's own bullet/
//! numbered-list buttons already use.

use crate::metrics::px as ui_px;

use amalith_core::{ObjectId, TextAlign};
use vello::kurbo::{Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

pub fn metric_w() -> f64 { crate::metrics::with(|m| m.areatypedlg_w) }
fn metric_pad() -> f64 { crate::metrics::with(|m| m.areatypedlg_pad) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.areatypedlg_field_h) }
fn metric_row_gap() -> f64 { crate::metrics::with(|m| m.areatypedlg_row_gap) }
fn metric_label_w() -> f64 { crate::metrics::with(|m| m.areatypedlg_label_w) }
fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.areatypedlg_btn_h) }

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Width,
    Height,
}

pub struct AreaTypeDialog {
    pub target: ObjectId,
    /// The target's own orientation — never edited here (that's the Type
    /// tool's job); only flips the Align dropdown's label and Auto Size's
    /// fixed-vs-auto axis is otherwise identical either way (see the
    /// module doc comment on `TextKind::Area`'s `height` field already
    /// doing double duty as the wrap axis for both orientations).
    pub vertical: bool,
    pub width: String,
    pub height: String,
    pub align: TextAlign,
    /// `true` -> the wrap axis (`height`) grows to fit instead of
    /// clipping at a fixed value — `TextKind::Area`'s own existing
    /// `height: None` behavior, just exposed as a checkbox.
    pub auto_size: bool,
    pub focus: Field,
    pub preview: bool,
}

impl AreaTypeDialog {
    /// `height` is the box's *fixed* height when it has one, or (for an
    /// auto-size box) the caller's best current measurement of the
    /// content — never derived from `width`, so unchecking Auto Size
    /// doesn't hand the user an arbitrary square frame.
    pub fn open(target: ObjectId, vertical: bool, width: f64, height: f64, auto_size: bool, align: TextAlign) -> Self {
        Self {
            target,
            vertical,
            width: trim_num(width),
            height: trim_num(height),
            align,
            auto_size,
            focus: Field::Width,
            preview: true,
        }
    }

    pub fn resolved_width(&self) -> f64 {
        amalith_core::parse_measurement(self.width.trim(), amalith_core::MeasureKind::Length(amalith_core::Unit::Px))
            .unwrap_or(1.0)
            .max(1.0)
    }

    pub fn resolved_height(&self) -> f64 {
        amalith_core::parse_measurement(self.height.trim(), amalith_core::MeasureKind::Length(amalith_core::Unit::Px))
            .unwrap_or(1.0)
            .max(1.0)
    }

    pub fn push_char(&mut self, ch: char) {
        if !crate::widgets::measurement_char(ch) {
            return;
        }
        let buf = match self.focus {
            Field::Width => &mut self.width,
            Field::Height => &mut self.height,
        };
        if buf.len() < 10 {
            buf.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        match self.focus {
            Field::Width => self.width.pop(),
            Field::Height => self.height.pop(),
        };
    }

    pub fn nudge(&mut self, dir: f64) {
        match self.focus {
            Field::Width => self.width = trim_num((self.resolved_width() + dir).max(1.0)),
            Field::Height => self.height = trim_num((self.resolved_height() + dir).max(1.0)),
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
/// regardless of state, like `offsetdlg::body_height`.
pub fn body_height() -> f64 {
    metric_pad()
        + 2.0 * (metric_field_h() + metric_row_gap()) // Width / Height
        + ui_px(10.0) // Rows/Columns placeholder block
        + ui_px(10.0) // Offset placeholder block
        + metric_field_h() + metric_row_gap() // Align dropdown
        + metric_field_h() + metric_row_gap() // Text Flow placeholder row
        + ui_px(6.0) + metric_btn_h() // Auto Size + Preview row
        + ui_px(6.0) + metric_btn_h() // OK/Cancel row
        + metric_pad()
}

struct Layout {
    width_field: Rect,
    height_field: Rect,
    align_seg: [Rect; 4],
    auto_size: Rect,
    preview: Rect,
    ok: Rect,
    cancel: Rect,
}

fn layout(body: Rect) -> Layout {
    let x0 = body.x0 + metric_pad();
    let x1 = body.x1 - metric_pad();
    let mut y = body.y0 + metric_pad();
    let width_field = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    y += metric_field_h() + metric_row_gap();
    let height_field = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    y += metric_field_h() + metric_row_gap();
    // Rows/Columns + Offset placeholder blocks (inert — see module doc).
    y += ui_px(10.0) + ui_px(10.0);
    let align_row = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    let seg_w = align_row.width() / 4.0;
    let align_seg = std::array::from_fn(|i| {
        Rect::new(
            align_row.x0 + seg_w * i as f64,
            align_row.y0,
            align_row.x0 + seg_w * (i + 1) as f64,
            align_row.y1,
        )
    });
    y += metric_field_h() + metric_row_gap();
    // Text Flow placeholder row (inert).
    y += metric_field_h() + metric_row_gap();
    y += ui_px(6.0);
    let auto_size = Rect::new(x0, y + (metric_btn_h() - ui_px(16.0)) * 0.5, x0 + ui_px(16.0), y + (metric_btn_h() + ui_px(16.0)) * 0.5);
    y += metric_btn_h() + ui_px(6.0);
    let preview = Rect::new(x0, y + (metric_btn_h() - ui_px(16.0)) * 0.5, x0 + ui_px(16.0), y + (metric_btn_h() + ui_px(16.0)) * 0.5);
    let btn_w = ui_px(68.0);
    let ok = Rect::new(x1 - btn_w, y, x1, y + metric_btn_h());
    let cancel = Rect::new(ok.x0 - ui_px(8.0) - btn_w, y, ok.x0 - ui_px(8.0), y + metric_btn_h());
    Layout { width_field, height_field, align_seg, auto_size, preview, ok, cancel }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Width,
    Height,
    /// Index into the Right / Center / Left / Justify segmented toggle.
    Align(usize),
    AutoSize,
    Preview,
    Ok,
    Cancel,
    None,
}

pub fn hit(dlg: &AreaTypeDialog, body: Rect, p: Point) -> Hit {
    let lay = layout(body);
    if lay.width_field.contains(p) {
        return Hit::Width;
    }
    if !dlg.auto_size && lay.height_field.contains(p) {
        return Hit::Height;
    }
    // `cross_align` (what this segment edits) only means anything for
    // vertical text — see `TextData::cross_align`'s doc comment — so a
    // horizontal box's Align row is drawn greyed and must not eat clicks,
    // the same "disabled field" rule the Height field follows above.
    if dlg.vertical {
        for (i, r) in lay.align_seg.iter().enumerate() {
            if r.contains(p) {
                return Hit::Align(i);
            }
        }
    }
    if lay.auto_size.inflate(ui_px(6.0), ui_px(6.0)).contains(p) {
        return Hit::AutoSize;
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

const ID: vello::kurbo::Affine = vello::kurbo::Affine::IDENTITY;
// `Start` hugs the box's right edge, so it's labeled "Right" (not
// "Left") — see `context_bar::area_type::OPTIONS`'s doc comment. Keep
// this array, that one, and `app/action.rs`'s `AreaTypeHit::Align`
// handler in sync.
const ALIGN_LABELS: [&str; 4] = ["Right", "Center", "Left", "Justify"];
const ALIGNS: [TextAlign; 4] = [TextAlign::Start, TextAlign::Center, TextAlign::End, TextAlign::JustifyAll];

pub fn paint(
    scene: &mut Scene,
    dlg: &AreaTypeDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(body);

    let field = |scene: &mut Scene, text: &mut TextContext, r: Rect, label: &str, value: &str, focused: bool, enabled: bool| {
        let lc = if enabled { theme.text_dim } else { theme.text_dim.with_alpha(0.4) };
        text.draw(scene, label, 12.5, lc, body.x0 + metric_pad(), r.center().y + ui_px(4.5));
        if !enabled {
            return;
        }
        scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
        scene.stroke(
            &Stroke::new(if focused { 1.5 } else { 1.0 }),
            ID,
            if focused { theme.accent } else { theme.text_dim.with_alpha(0.5) },
            None,
            &r,
        );
        let shown = if focused && caret_on { format!("{value}|") } else { value.to_string() };
        text.draw(scene, &shown, 13.0, theme.text, r.x0 + ui_px(10.0), r.center().y + ui_px(4.5));
        let px_w = text.measure("px", 11.5);
        text.draw(scene, "px", 11.5, theme.text_dim, r.x1 - px_w - ui_px(8.0), r.center().y + ui_px(4.5));
    };

    field(scene, text, lay.width_field, "Width:", &dlg.width, dlg.focus == Field::Width, true);
    field(scene, text, lay.height_field, "Height:", &dlg.height, dlg.focus == Field::Height, !dlg.auto_size);

    text.draw(scene, "Rows/Columns:", 11.5, theme.text_dim.with_alpha(0.4), body.x0 + metric_pad(), lay.height_field.y1 + ui_px(20.0));
    text.draw(scene, "Offset:", 11.5, theme.text_dim.with_alpha(0.4), body.x0 + metric_pad(), lay.height_field.y1 + ui_px(34.0));

    // Meaningless for a horizontal box — `cross_align` only positions a
    // *vertical* text block's columns within the frame's width; drawn
    // greyed and inert, the same convention Rows/Columns/Offset already
    // use, rather than mislabeled (Right/Center/Left/Justify) *and*
    // silently doing nothing.
    let align_label = if dlg.vertical { "Align Vertical:" } else { "Align:" };
    let label_ink = if dlg.vertical { theme.text_dim } else { theme.text_dim.with_alpha(0.4) };
    text.draw(scene, align_label, 12.5, label_ink, body.x0 + metric_pad(), lay.align_seg[0].center().y + ui_px(4.5));
    for (i, r) in lay.align_seg.iter().enumerate() {
        if !dlg.vertical {
            scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.25), None, r);
            let w = text.measure(ALIGN_LABELS[i], 11.0);
            text.draw(scene, ALIGN_LABELS[i], 11.0, theme.text_dim.with_alpha(0.4), r.center().x - w * 0.5, r.center().y + ui_px(4.0));
            continue;
        }
        let on = ALIGNS[i] == dlg.align;
        scene.fill(Fill::NonZero, ID, if on { theme.accent } else { theme.bg }, None, r);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.5), None, r);
        let col = if on { theme.on_accent } else { theme.text_dim };
        let w = text.measure(ALIGN_LABELS[i], 11.0);
        text.draw(scene, ALIGN_LABELS[i], 11.0, col, r.center().x - w * 0.5, r.center().y + ui_px(4.0));
    }

    text.draw(scene, "Text Flow:", 11.5, theme.text_dim.with_alpha(0.4), body.x0 + metric_pad(), lay.align_seg[0].y1 + ui_px(20.0));

    scene.stroke(&Stroke::new(ui_px(1.2)), ID, theme.text_dim, None, &lay.auto_size);
    if dlg.auto_size {
        let inset = lay.auto_size.inflate(ui_px(-3.0), ui_px(-3.0));
        scene.fill(Fill::NonZero, ID, theme.accent, None, &inset);
    }
    text.draw(scene, "Auto Size", 12.0, theme.text, lay.auto_size.x1 + ui_px(8.0), lay.auto_size.center().y + ui_px(4.5));

    scene.stroke(&Stroke::new(ui_px(1.2)), ID, theme.text_dim, None, &lay.preview);
    if dlg.preview {
        let inset = lay.preview.inflate(ui_px(-3.0), ui_px(-3.0));
        scene.fill(Fill::NonZero, ID, theme.accent, None, &inset);
    }
    text.draw(scene, "Preview", 12.0, theme.text, lay.preview.x1 + ui_px(8.0), lay.preview.center().y + ui_px(4.5));

    crate::widgets::button(scene, text, theme, lay.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, lay.ok, "OK", true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body() -> Rect {
        Rect::new(0.0, 0.0, metric_w(), body_height())
    }

    #[test]
    fn open_seeds_auto_size_from_the_caller_supplied_flag() {
        let dlg = AreaTypeDialog::open(ObjectId::new(), false, 200.0, 130.0, true, TextAlign::Start);
        assert!(dlg.auto_size);
        assert_eq!(dlg.width, "200");
        assert_eq!(dlg.height, "130", "auto-size still seeds the field from the caller's real measurement, not from width");
    }

    #[test]
    fn open_seeds_a_fixed_height_field() {
        let dlg = AreaTypeDialog::open(ObjectId::new(), true, 120.0, 80.0, false, TextAlign::End);
        assert!(!dlg.auto_size);
        assert_eq!(dlg.height, "80");
        assert_eq!(dlg.align, TextAlign::End);
    }

    #[test]
    fn clicking_each_align_segment_reports_its_own_index() {
        let dlg = AreaTypeDialog::open(ObjectId::new(), true, 100.0, 50.0, false, TextAlign::Start);
        let b = body();
        let lay = layout(b);
        for (i, seg) in lay.align_seg.iter().enumerate() {
            assert_eq!(hit(&dlg, b, seg.center()), Hit::Align(i));
        }
    }

    #[test]
    fn the_align_row_is_unclickable_on_a_horizontal_box() {
        let dlg = AreaTypeDialog::open(ObjectId::new(), false, 100.0, 50.0, false, TextAlign::Start);
        let b = body();
        let lay = layout(b);
        for seg in &lay.align_seg {
            assert_eq!(hit(&dlg, b, seg.center()), Hit::None, "cross_align does nothing for horizontal text, so the row must not eat clicks");
        }
    }

    #[test]
    fn the_height_field_is_unclickable_while_auto_size_is_on() {
        let mut dlg = AreaTypeDialog::open(ObjectId::new(), false, 100.0, 50.0, false, TextAlign::Start);
        let b = body();
        let height_center = layout(b).height_field.center();
        assert_eq!(hit(&dlg, b, height_center), Hit::Height);
        dlg.auto_size = true;
        assert_eq!(hit(&dlg, b, height_center), Hit::None, "a disabled field must not eat clicks");
    }

    #[test]
    fn ok_and_cancel_and_preview_hit_their_own_buttons() {
        let dlg = AreaTypeDialog::open(ObjectId::new(), false, 100.0, 50.0, false, TextAlign::Start);
        let b = body();
        let lay = layout(b);
        assert_eq!(hit(&dlg, b, lay.ok.center()), Hit::Ok);
        assert_eq!(hit(&dlg, b, lay.cancel.center()), Hit::Cancel);
        assert_eq!(hit(&dlg, b, lay.preview.center()), Hit::Preview);
        assert_eq!(hit(&dlg, b, lay.auto_size.center()), Hit::AutoSize);
    }

    #[test]
    fn nudge_and_push_char_edit_whichever_field_has_focus() {
        let mut dlg = AreaTypeDialog::open(ObjectId::new(), false, 100.0, 50.0, false, TextAlign::Start);
        dlg.focus = Field::Width;
        dlg.nudge(5.0);
        assert_eq!(dlg.resolved_width(), 105.0);
        dlg.focus = Field::Height;
        dlg.nudge(-5.0);
        assert_eq!(dlg.resolved_height(), 45.0);
        dlg.push_char('9');
        assert_eq!(dlg.height, "459");
    }
}
