//! The Reflect / Shear dialogs — Illustrator's `Object ▸ Transform ▸
//! Reflect…`/`Shear…`, reachable here from the canvas right-click menu.
//!
//! Bespoke floating panels, exactly like the exact-size shape dialogs and
//! the colour picker (see `crate::shapedialog`): draggable by their tab
//! strip, never dockable, never in the Window menu. Both dialogs apply the
//! *whole selection* as one rigid group around a single shared pivot — the
//! selection's bounding-box centre for the right-click entry points here.
//! (A future Reflect/Shear *tool*, where the user clicks to place that
//! pivot instead, would reuse the same `amalith_core::xform::reflect_about`
//! / `shear_about` math — only where the pivot comes from differs.)

use crate::metrics::px as ui_px;

use amalith_core::xform::{reflect_about, shear_about};
use amalith_core::ObjectId;
use vello::kurbo::{Affine, Circle, Line, Point, Rect, Shape, Stroke, Vec2};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;

pub fn metric_w() -> f64 { crate::metrics::with(|m| m.xformdlg_w) }

fn metric_pad() -> f64 { crate::metrics::with(|m| m.xformdlg_pad) }
fn metric_row_h() -> f64 { crate::metrics::with(|m| m.xformdlg_row_h) }
fn metric_radio_r() -> f64 { crate::metrics::with(|m| m.xformdlg_radio_r) }
fn metric_dial_r() -> f64 { crate::metrics::with(|m| m.xformdlg_dial_r) }
fn metric_field_w() -> f64 { crate::metrics::with(|m| m.xformdlg_field_w) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.xformdlg_field_h) }
/// Left column each row's label sits in, before the dial — wide enough
/// for "Horizontal" (the widest Axis-row label). The standalone Shear
/// Angle row uses its own, wider column ([`SHEAR_LABEL_W`]).
/// Left column for the Shear Angle row's own (longer, unindented) label.
fn metric_shear_label_w() -> f64 { crate::metrics::with(|m| m.xformdlg_shear_label_w) }
fn metric_box_top_gap() -> f64 { crate::metrics::with(|m| m.xformdlg_box_top_gap) }
fn metric_box_bottom_pad() -> f64 { crate::metrics::with(|m| m.xformdlg_box_bottom_pad) }
fn metric_section_gap() -> f64 { crate::metrics::with(|m| m.xformdlg_section_gap) }
fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.xformdlg_btn_h) }
fn metric_btn_w() -> f64 { crate::metrics::with(|m| m.xformdlg_btn_w) }
fn metric_btn_gap() -> f64 { crate::metrics::with(|m| m.xformdlg_btn_gap) }
fn metric_bot_pad() -> f64 { crate::metrics::with(|m| m.xformdlg_bot_pad) }
fn metric_check_s() -> f64 { crate::metrics::with(|m| m.xformdlg_check_s) }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
    Angle,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Reflect,
    Shear,
}

/// Which text field (if any) currently has the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    None,
    /// Shear only.
    ShearAngle,
    /// The Axis box's own "Angle" field.
    AxisAngle,
}

/// Which of the dialog's angle dials a drag is turning.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialField {
    Shear,
    Axis,
}

/// Where a press landed. A dial press carries the angle already resolved
/// from that press point, plus the dial's own screen centre — so the App
/// glue can both apply this press's value and arm a drag continuation
/// (`angle_at(center, pointer)` on every further move) without re-deriving
/// the layout.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    None,
    SelectAxis(Axis),
    FocusField(Focus),
    Dial(DialField, f64, Point),
    TogglePreview,
    Copy,
    Cancel,
    Ok,
}

/// What a resolved [`Hit`] asks the App glue to do next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    None,
    /// A value changed — re-run the live preview if it's on.
    Changed,
    Copy,
    Cancel,
    Ok,
}

pub struct TransformDialog {
    pub kind: Kind,
    pub axis: Axis,
    pub axis_buf: String,
    pub shear_buf: String,
    pub focus: Focus,
    /// The focused field's whole value reads as selected (drawn
    /// highlighted) until the next keystroke, which replaces it outright
    /// instead of appending — set on every click into a field, matching
    /// every other numeric field in the app (`xform_edit` et al.'s
    /// `fresh`).
    pub fresh: bool,
    pub preview: bool,
    /// Selection bbox centre (document space) at the moment this dialog
    /// opened — the shared pivot every selected object reflects/shears
    /// around, so a multi-object selection moves as one rigid group.
    pub pivot: amalith_core::Point,
    /// Each selected object's own local transform, captured when the
    /// dialog opened. Preview always recomputes from these — never
    /// cumulatively from the live (already-previewed) document — so
    /// repeated edits can't drift, and Cancel can restore them exactly.
    pub originals: Vec<(ObjectId, amalith_core::Affine)>,
}

impl TransformDialog {
    /// `axis` is the dialog's initial Axis selection — Reflect opens on
    /// Vertical, Shear on Horizontal, matching Illustrator's own dialogs.
    pub fn open(
        kind: Kind,
        axis: Axis,
        pivot: amalith_core::Point,
        originals: Vec<(ObjectId, amalith_core::Affine)>,
    ) -> Self {
        Self {
            kind,
            axis,
            axis_buf: "90".into(),
            shear_buf: "0".into(),
            focus: Focus::None,
            fresh: false,
            preview: true,
            pivot,
            originals,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            Kind::Reflect => "Reflect",
            Kind::Shear => "Shear",
        }
    }

    /// The resolved axis angle, in degrees to the horizontal.
    pub fn axis_deg(&self) -> f64 {
        match self.axis {
            Axis::Horizontal => 0.0,
            Axis::Vertical => 90.0,
            Axis::Angle => parse_deg(&self.axis_buf).unwrap_or(0.0),
        }
    }

    /// Shear-only: the slant angle, in degrees.
    pub fn shear_deg(&self) -> f64 {
        parse_deg(&self.shear_buf).unwrap_or(0.0).clamp(-89.0, 89.0)
    }

    /// This dialog's new local transform for one originally-captured
    /// object, given its *current* parent world transform (fetched fresh
    /// each time — objects don't reparent while this dialog is open, but
    /// nothing here assumes the parent transform itself never changes).
    pub fn resolve(&self, local: amalith_core::Affine, parent: amalith_core::Affine) -> amalith_core::Affine {
        match self.kind {
            Kind::Reflect => reflect_about(local, parent, self.pivot, self.axis_deg()),
            Kind::Shear => shear_about(local, parent, self.pivot, self.shear_deg(), self.axis_deg()),
        }
    }

    /// Resolve a press's [`Hit`] into what the App glue should do.
    pub fn apply(&mut self, hit: Hit) -> Outcome {
        match hit {
            Hit::None => Outcome::None,
            Hit::SelectAxis(axis) => {
                self.axis = axis;
                self.focus = if axis == Axis::Angle { Focus::AxisAngle } else { Focus::None };
                Outcome::Changed
            }
            Hit::FocusField(f) => {
                self.focus = f;
                self.fresh = true;
                Outcome::None
            }
            Hit::Dial(field, deg, _center) => {
                self.set_dial_angle(field, deg);
                Outcome::Changed
            }
            Hit::TogglePreview => {
                self.preview = !self.preview;
                Outcome::Changed
            }
            Hit::Copy => Outcome::Copy,
            Hit::Cancel => Outcome::Cancel,
            Hit::Ok => Outcome::Ok,
        }
    }

    // --- keyboard (mirrors `shapedialog::ShapeDialog`'s API shape) ------

    pub fn push_char(&mut self, ch: char) {
        if !crate::widgets::measurement_char(ch) {
            return;
        }
        let fresh = std::mem::take(&mut self.fresh);
        if let Some(buf) = self.focused_buf() {
            if fresh {
                buf.clear();
            }
            buf.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        self.fresh = false;
        if let Some(buf) = self.focused_buf() {
            buf.pop();
        }
    }

    /// Tab: cycle to the next field that's actually live right now (the
    /// Axis-box angle field only when `Axis::Angle` is selected).
    pub fn focus_next(&mut self) {
        let live = self.live_fields();
        if live.is_empty() {
            self.focus = Focus::None;
            return;
        }
        let i = live.iter().position(|&f| f == self.focus).map_or(0, |i| (i + 1) % live.len());
        self.focus = live[i];
    }

    pub fn focus_prev(&mut self) {
        let live = self.live_fields();
        if live.is_empty() {
            self.focus = Focus::None;
            return;
        }
        let i = live.iter().position(|&f| f == self.focus).map_or(0, |i| (i + live.len() - 1) % live.len());
        self.focus = live[i];
    }

    fn live_fields(&self) -> Vec<Focus> {
        let mut v = Vec::new();
        if self.kind == Kind::Shear {
            v.push(Focus::ShearAngle);
        }
        if self.axis == Axis::Angle {
            v.push(Focus::AxisAngle);
        }
        v
    }

    fn focused_buf(&mut self) -> Option<&mut String> {
        match self.focus {
            Focus::ShearAngle => Some(&mut self.shear_buf),
            Focus::AxisAngle => Some(&mut self.axis_buf),
            Focus::None => None,
        }
    }

    /// Set an angle from a dial drag, in degrees. The Axis dial always
    /// selects `Axis::Angle` (a line, so normalized to `[0, 180)`); the
    /// Shear dial just sets the slant (clamped, not normalized — it isn't
    /// a line).
    pub fn set_dial_angle(&mut self, field: DialField, deg: f64) {
        self.fresh = false;
        match field {
            DialField::Shear => self.shear_buf = trim_deg(deg.clamp(-89.0, 89.0)),
            DialField::Axis => {
                self.axis = Axis::Angle;
                let norm = ((deg % 180.0) + 180.0) % 180.0;
                self.axis_buf = trim_deg(norm);
            }
        }
    }

    /// Nudge field `which`'s value by `delta` degrees — shared by an
    /// arrow-key nudge of the focused field and a scroll-wheel nudge of
    /// whichever field the pointer merely hovers (no click needed, and no
    /// focus change beyond the Axis-selection side effect below).
    pub fn nudge(&mut self, which: Focus, delta: f64) {
        self.fresh = false;
        match which {
            Focus::ShearAngle => {
                let v = parse_deg(&self.shear_buf).unwrap_or(0.0) + delta;
                self.shear_buf = trim_deg(v.clamp(-89.0, 89.0));
            }
            Focus::AxisAngle => {
                // Matches the dial: turning this value always means "use
                // the custom angle," even if Horizontal/Vertical was still
                // selected when the scroll/nudge landed.
                self.axis = Axis::Angle;
                let v = parse_deg(&self.axis_buf).unwrap_or(0.0) + delta;
                let norm = ((v % 180.0) + 180.0) % 180.0;
                self.axis_buf = trim_deg(norm);
            }
            Focus::None => {}
        }
    }

    /// Arrow-key nudge of whichever field currently has the caret.
    pub fn nudge_focused(&mut self, delta: f64) {
        self.nudge(self.focus, delta);
    }
}

fn parse_deg(s: &str) -> Option<f64> {
    amalith_core::parse_measurement(s, amalith_core::MeasureKind::Angle)
}

fn trim_deg(v: f64) -> String {
    let r = (v * 100.0).round() / 100.0;
    if (r - r.round()).abs() < 1e-6 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}

/// Angle (degrees to the horizontal, standard math convention) of `p`
/// relative to `center`, for turning a dial drag into a value.
pub fn angle_at(center: Point, p: Point) -> f64 {
    let v: Vec2 = p - center;
    (-v.y).atan2(v.x).to_degrees()
}

// --- layout ---------------------------------------------------------------
//
// Every rect below is in the SAME (real, on-screen) coordinate space as
// `body` — offsets are added onto `body.x0`/`body.y0`, never a fresh
// (0, 0) origin — matching every other panel's `paint`/`hit` convention
// (see e.g. `panels::artboards::row_rect`).

struct Layout {
    shear_row: Option<Rect>,
    axis_box: Rect,
    horizontal_row: Rect,
    vertical_row: Rect,
    angle_row: Rect,
    preview_row: Rect,
    ok: Rect,
    cancel: Rect,
    copy: Rect,
}

fn layout(kind: Kind, body: Rect) -> Layout {
    let mut y = body.y0 + metric_pad();
    let x0 = body.x0;
    let x1 = body.x1;
    let shear_row = if kind == Kind::Shear {
        let r = Rect::new(x0 + metric_pad(), y, x1 - metric_pad(), y + metric_row_h());
        y += metric_row_h() + metric_section_gap();
        Some(r)
    } else {
        None
    };
    let box_top = y;
    y += metric_box_top_gap();
    let horizontal_row = Rect::new(x0 + metric_pad() * 1.4, y, x1 - metric_pad(), y + metric_row_h());
    y += metric_row_h();
    let vertical_row = Rect::new(x0 + metric_pad() * 1.4, y, x1 - metric_pad(), y + metric_row_h());
    y += metric_row_h();
    let angle_row = Rect::new(x0 + metric_pad() * 1.4, y, x1 - metric_pad(), y + metric_row_h());
    y += metric_row_h() + metric_box_bottom_pad();
    let axis_box = Rect::new(x0 + metric_pad() * 0.6, box_top, x1 - metric_pad() * 0.6, y);
    y += metric_section_gap() * 0.5;
    let preview_row = Rect::new(x0 + metric_pad(), y, x1 - metric_pad(), y + metric_check_s().max(ui_px(16.0)));
    y += preview_row.height() + metric_section_gap();

    let ok = Rect::new(x1 - metric_pad() - metric_btn_w(), y, x1 - metric_pad(), y + metric_btn_h());
    let cancel = Rect::new(ok.x0 - metric_btn_gap() - metric_btn_w(), ok.y0, ok.x0 - metric_btn_gap(), ok.y1);
    let copy = Rect::new(cancel.x0 - metric_btn_gap() - metric_btn_w(), cancel.y0, cancel.x0 - metric_btn_gap(), cancel.y1);

    Layout {
        shear_row,
        axis_box,
        horizontal_row,
        vertical_row,
        angle_row,
        preview_row,
        ok,
        cancel,
        copy,
    }
}

/// Full body height this dialog needs (its width is fixed at [`W`]).
pub fn body_height(kind: Kind) -> f64 {
    let l = layout(kind, Rect::new(0.0, 0.0, metric_w(), 0.0));
    l.ok.y1 + metric_bot_pad()
}

/// The Axis rows' own label column: past the radio + its gap to the label.
fn metric_axis_label_col() -> f64 { crate::metrics::with(|m| m.xformdlg_axis_label_col) }

/// Dial centre, `label_col` past `row.x0` — the Shear Angle row (no radio)
/// passes [`SHEAR_LABEL_W`]; the Axis box's Angle row passes
/// [`AXIS_LABEL_COL`] (past its radio too). Keeping this a parameter
/// (rather than a fixed offset) is what stops the dial from landing on
/// top of whichever label happens to be longest.
fn dial_center_at(row: Rect, label_col: f64) -> Point {
    Point::new(row.x0 + label_col + metric_dial_r(), row.center().y)
}
fn field_rect_at(row: Rect, label_col: f64) -> Rect {
    let x0 = row.x0 + label_col + metric_dial_r() * 2.0 + ui_px(12.0);
    Rect::new(x0, row.center().y - metric_field_h() * 0.5, x0 + metric_field_w(), row.center().y + metric_field_h() * 0.5)
}
fn radio_center(row: Rect) -> Point {
    Point::new(row.x0 + metric_radio_r(), row.center().y)
}
fn preview_check_rect(row: Rect) -> Rect {
    Rect::new(row.x0, row.center().y - metric_check_s() * 0.5, row.x0 + metric_check_s(), row.center().y + metric_check_s() * 0.5)
}

/// Resolve a press at `local` (same coordinate space as `body`) into a
/// [`Hit`].
pub fn hit(dlg: &TransformDialog, body: Rect, local: Point) -> Hit {
    let l = layout(dlg.kind, body);
    if l.ok.contains(local) {
        return Hit::Ok;
    }
    if l.cancel.contains(local) {
        return Hit::Cancel;
    }
    if l.copy.contains(local) {
        return Hit::Copy;
    }
    if preview_check_rect(l.preview_row).contains(local) {
        return Hit::TogglePreview;
    }
    if let Some(row) = l.shear_row {
        let c = dial_center_at(row, metric_shear_label_w());
        if Circle::new(c, metric_dial_r() + 3.0).contains(local) {
            return Hit::Dial(DialField::Shear, angle_at(c, local), c);
        }
        if field_rect_at(row, metric_shear_label_w()).contains(local) {
            return Hit::FocusField(Focus::ShearAngle);
        }
    }
    for (row, axis) in [
        (l.horizontal_row, Axis::Horizontal),
        (l.vertical_row, Axis::Vertical),
        (l.angle_row, Axis::Angle),
    ] {
        if Circle::new(radio_center(row), metric_radio_r() + 4.0).contains(local) {
            return Hit::SelectAxis(axis);
        }
    }
    let ac = dial_center_at(l.angle_row, metric_axis_label_col());
    if Circle::new(ac, metric_dial_r() + 3.0).contains(local) {
        return Hit::Dial(DialField::Axis, angle_at(ac, local), ac);
    }
    if field_rect_at(l.angle_row, metric_axis_label_col()).contains(local) {
        return Hit::FocusField(Focus::AxisAngle);
    }
    Hit::None
}

// --- painting -------------------------------------------------------------

pub fn paint(scene: &mut Scene, dlg: &TransformDialog, body: Rect, theme: &Theme, text: &mut TextContext, caret_on: bool) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let l = layout(dlg.kind, body);

    if let Some(row) = l.shear_row {
        text.draw(scene, "Shear Angle:", 12.5, theme.text, row.x0, row.center().y + ui_px(4.5));
        draw_dial(scene, dial_center_at(row, metric_shear_label_w()), theme, dlg.shear_deg(), false);
        draw_field(
            scene,
            text,
            theme,
            field_rect_at(row, metric_shear_label_w()),
            &dlg.shear_buf,
            dlg.focus == Focus::ShearAngle,
            dlg.fresh,
            caret_on,
        );
    }

    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &l.axis_box.to_rounded_rect(ui_px(4.0)));
    let label_w = text.measure("Axis", 11.0) + ui_px(8.0);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.panel_bg,
        None,
        &Rect::new(l.axis_box.x0 + ui_px(8.0), l.axis_box.y0 - ui_px(6.0), l.axis_box.x0 + ui_px(8.0) + label_w, l.axis_box.y0 + ui_px(6.0)),
    );
    text.draw(scene, "Axis", 11.0, theme.text_dim, l.axis_box.x0 + ui_px(12.0), l.axis_box.y0 + ui_px(4.0));

    for (row, axis, glyph) in [
        (l.horizontal_row, Axis::Horizontal, dlg.kind == Kind::Reflect),
        (l.vertical_row, Axis::Vertical, dlg.kind == Kind::Reflect),
        (l.angle_row, Axis::Angle, false),
    ] {
        draw_radio(scene, radio_center(row), theme, dlg.axis == axis);
        let label = match axis {
            Axis::Horizontal => "Horizontal",
            Axis::Vertical => "Vertical",
            Axis::Angle => "Angle:",
        };
        text.draw(scene, label, 12.5, theme.text, row.x0 + metric_radio_r() * 2.0 + ui_px(10.0), row.center().y + ui_px(4.5));
        if axis == Axis::Angle {
            draw_dial(scene, dial_center_at(row, metric_axis_label_col()), theme, dlg.axis_deg(), dlg.axis == Axis::Angle);
            draw_field(
                scene,
                text,
                theme,
                field_rect_at(row, metric_axis_label_col()),
                &dlg.axis_buf,
                dlg.focus == Focus::AxisAngle,
                dlg.fresh,
                caret_on,
            );
        } else if glyph {
            draw_reflect_glyph(scene, dial_center_at(row, metric_axis_label_col()), theme, axis == Axis::Vertical);
        }
    }

    let cb = preview_check_rect(l.preview_row);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.6), None, &cb);
    if dlg.preview {
        scene.fill(Fill::NonZero, ID, theme.accent, None, &cb.inset(ui_px(-ui_px(2.0))));
    }
    text.draw(scene, "Preview", 12.5, theme.text, cb.x1 + ui_px(8.0), cb.y0 + cb.height() * 0.5 + ui_px(4.5));

    crate::widgets::button(scene, text, theme, l.copy, "Copy", false);
    crate::widgets::button(scene, text, theme, l.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, l.ok, "OK", true);
}

fn draw_radio(scene: &mut Scene, c: Point, theme: &Theme, selected: bool) {
    let ring = Circle::new(c, metric_radio_r());
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text_dim, None, &ring);
    if selected {
        scene.fill(Fill::NonZero, ID, theme.accent, None, &Circle::new(c, metric_radio_r() * 0.5));
    }
}

fn draw_dial(scene: &mut Scene, c: Point, theme: &Theme, angle_deg: f64, active: bool) {
    let ring = Circle::new(c, metric_dial_r());
    let color = if active { theme.accent } else { theme.text_dim };
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, color, None, &ring);
    let r = angle_deg.to_radians();
    let d = Vec2::new(r.cos(), -r.sin()) * (metric_dial_r() - ui_px(2.0));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, color, None, &Line::new(c - d, c + d));
}

/// The small mirrored-triangles glyph beside Reflect's Horizontal/Vertical
/// rows (Shear's own Axis rows carry no icon).
fn draw_reflect_glyph(scene: &mut Scene, c: Point, theme: &Theme, vertical: bool) {
    use vello::kurbo::BezPath;
    let mut p = BezPath::new();
    if vertical {
        p.move_to(c + Vec2::new(ui_px(-ui_px(9.0)), ui_px(-ui_px(6.0))));
        p.line_to(c + Vec2::new(ui_px(-ui_px(3.0)), 0.0));
        p.line_to(c + Vec2::new(ui_px(-ui_px(9.0)), ui_px(6.0)));
        p.close_path();
        p.move_to(c + Vec2::new(ui_px(9.0), ui_px(-ui_px(6.0))));
        p.line_to(c + Vec2::new(ui_px(3.0), 0.0));
        p.line_to(c + Vec2::new(ui_px(9.0), ui_px(6.0)));
        p.close_path();
    } else {
        p.move_to(c + Vec2::new(ui_px(-ui_px(6.0)), ui_px(-ui_px(9.0))));
        p.line_to(c + Vec2::new(0.0, ui_px(-ui_px(3.0))));
        p.line_to(c + Vec2::new(ui_px(6.0), ui_px(-ui_px(9.0))));
        p.close_path();
        p.move_to(c + Vec2::new(ui_px(-ui_px(6.0)), ui_px(9.0)));
        p.line_to(c + Vec2::new(0.0, ui_px(3.0)));
        p.line_to(c + Vec2::new(ui_px(6.0), ui_px(9.0)));
        p.close_path();
    }
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &p);
}

#[allow(clippy::too_many_arguments)]
fn draw_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    value: &str,
    focused: bool,
    selected: bool,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    let border = if focused { theme.accent } else { theme.text_dim.with_alpha(0.5) };
    scene.stroke(&Stroke::new(if focused { 1.25 } else { 1.0 }), ID, border, None, &r);
    let label = format!("{value}°");
    let label_w = text.measure(&label, 12.5);
    // A double-click (or any fresh focus) reads as the whole value
    // selected — a solid highlight behind the text, exactly like a real
    // text field's selection, since the next keystroke replaces it
    // outright instead of appending.
    if focused && selected {
        scene.fill(
            Fill::NonZero,
            ID,
            theme.accent,
            None,
            &Rect::new(r.x0 + ui_px(5.0), r.y0 + ui_px(3.0), r.x0 + ui_px(7.0) + label_w + ui_px(2.0), r.y1 - ui_px(3.0)),
        );
    }
    let ink = if focused && selected { theme.on_accent } else { theme.text };
    text.draw(scene, &label, 12.5, ink, r.x0 + ui_px(7.0), r.y0 + r.height() * 0.5 + ui_px(4.5));
    if focused && !selected && caret_on {
        let cx = r.x0 + ui_px(7.0) + label_w + 1.0;
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text, None, &Line::new((cx, r.y0 + ui_px(4.0)), (cx, r.y1 - ui_px(4.0))));
    }
}

