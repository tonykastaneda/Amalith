//! The generic Distort & Transform effect dialog — one shared numeric-
//! fields-plus-checkboxes dialog for Zig Zag, Pucker & Bloat, Roughen,
//! Transform, and Tweak, and Twist, instead of six near-identical copies
//! of `offsetdlg.rs`. Every one of these effects only ever lives in an
//! Appearance-panel item's own effect stack (unlike Offset Path, which
//! also has a destructive Object ▸ Path equivalent) — so unlike
//! `offsetdlg::OffsetDialog`, this dialog has no `Target` split, it
//! always edits one `(object, item_index, effect_index)` slot. Layout /
//! hit-testing / painting live here; `app/effect_dialog.rs` is the
//! App-side glue (spawning the floating window, closing it, keyboard),
//! mirroring `offsetdlg.rs` / `app/offset_dialog.rs`.

use crate::metrics::px as ui_px;

use amalith_core::{
    Effect, MeasureKind, ObjectId, PuckerBloatEffect, RoughenEffect, TransformEffect, TweakEffect,
    TwistEffect, Unit, ZigZagEffect,
};
use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;
use crate::widgets::{self, NumEdit};

const ID: Affine = Affine::IDENTITY;

/// Which Distort & Transform effect this dialog is editing — decides its
/// field/checkbox layout and how OK's values become an [`Effect`]. Free
/// Distort isn't here (a canvas corner-drag, not a numeric dialog — see
/// [`Effect`]'s own doc comment).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EffectKind {
    ZigZag,
    PuckerBloat,
    Roughen,
    Transform,
    Tweak,
    Twist,
}

impl EffectKind {
    pub const ALL: [EffectKind; 6] = [
        EffectKind::ZigZag,
        EffectKind::PuckerBloat,
        EffectKind::Roughen,
        EffectKind::Transform,
        EffectKind::Tweak,
        EffectKind::Twist,
    ];

    /// The footer fx menu's own entry label for this kind.
    pub fn menu_label(self) -> &'static str {
        match self {
            EffectKind::ZigZag => "Zig Zag…",
            EffectKind::PuckerBloat => "Pucker & Bloat…",
            EffectKind::Roughen => "Roughen…",
            EffectKind::Transform => "Transform…",
            EffectKind::Tweak => "Tweak…",
            EffectKind::Twist => "Twist…",
        }
    }

    fn title(self) -> &'static str {
        match self {
            EffectKind::ZigZag => "Zig Zag",
            EffectKind::PuckerBloat => "Pucker & Bloat",
            EffectKind::Roughen => "Roughen",
            EffectKind::Transform => "Transform",
            EffectKind::Tweak => "Tweak",
            EffectKind::Twist => "Twist",
        }
    }

    fn fields(self) -> &'static [(&'static str, MeasureKind)] {
        use MeasureKind::{Angle, Count, Length, Percent};
        match self {
            EffectKind::ZigZag => &[("Size:", Length(Unit::Px)), ("Ridges/Seg:", Count)],
            EffectKind::PuckerBloat => &[("Amount:", Percent)],
            EffectKind::Roughen => &[("Size:", Length(Unit::Px)), ("Detail:", Count)],
            EffectKind::Transform => &[
                ("Move H:", Length(Unit::Px)),
                ("Move V:", Length(Unit::Px)),
                ("Scale H:", Percent),
                ("Scale V:", Percent),
                ("Rotate:", Angle),
            ],
            EffectKind::Tweak => &[("Horizontal:", Percent), ("Vertical:", Percent)],
            EffectKind::Twist => &[("Angle:", Angle)],
        }
    }

    fn checks(self) -> &'static [&'static str] {
        match self {
            EffectKind::ZigZag | EffectKind::Roughen => &["Smooth"],
            EffectKind::Transform => &["Reflect H", "Reflect V"],
            EffectKind::Tweak => &["Anchor Points", "In Handles", "Out Handles"],
            EffectKind::PuckerBloat | EffectKind::Twist => &[],
        }
    }

    /// Seeded field/check values from `current` (editing an existing
    /// effect of this same kind) or this kind's own defaults (adding a
    /// new one). The returned `u64` is the jittered effects' own random
    /// seed — read back from `current` so re-editing size/detail doesn't
    /// reroll the jitter pattern; freshly generated for a new one.
    fn seed_from(self, current: Option<&Effect>) -> (Vec<f64>, Vec<bool>, u64) {
        match (self, current) {
            (EffectKind::ZigZag, Some(Effect::ZigZag(fx))) => (vec![fx.size, fx.ridges_per_segment], vec![fx.smooth], 0),
            (EffectKind::ZigZag, _) => (vec![10.0, 4.0], vec![false], 0),
            (EffectKind::PuckerBloat, Some(Effect::PuckerBloat(fx))) => (vec![fx.amount], vec![], 0),
            (EffectKind::PuckerBloat, _) => (vec![20.0], vec![], 0),
            (EffectKind::Roughen, Some(Effect::Roughen(fx))) => (vec![fx.size, fx.detail], vec![fx.smooth], fx.seed),
            (EffectKind::Roughen, _) => (vec![5.0, 8.0], vec![false], fresh_seed()),
            (EffectKind::Transform, Some(Effect::Transform(fx))) => {
                (vec![fx.move_x, fx.move_y, fx.scale_x, fx.scale_y, fx.rotate], vec![fx.reflect_x, fx.reflect_y], 0)
            }
            (EffectKind::Transform, _) => (vec![0.0, 0.0, 100.0, 100.0, 0.0], vec![false, false], 0),
            (EffectKind::Tweak, Some(Effect::Tweak(fx))) => {
                (vec![fx.horizontal, fx.vertical], vec![fx.modify_anchors, fx.modify_in, fx.modify_out], fx.seed)
            }
            (EffectKind::Tweak, _) => (vec![10.0, 10.0], vec![true, true, true], fresh_seed()),
            (EffectKind::Twist, Some(Effect::Twist(fx))) => (vec![fx.angle], vec![], 0),
            (EffectKind::Twist, _) => (vec![45.0], vec![], 0),
        }
    }

    fn build(self, fields: &[f64], checks: &[bool], seed: u64) -> Effect {
        match self {
            EffectKind::ZigZag => Effect::ZigZag(ZigZagEffect {
                size: fields[0],
                ridges_per_segment: fields[1].max(0.1),
                smooth: checks[0],
            }),
            EffectKind::PuckerBloat => Effect::PuckerBloat(PuckerBloatEffect { amount: fields[0].clamp(-100.0, 100.0) }),
            EffectKind::Roughen => Effect::Roughen(RoughenEffect {
                size: fields[0],
                detail: fields[1].max(0.1),
                smooth: checks[0],
                seed,
            }),
            EffectKind::Transform => Effect::Transform(TransformEffect {
                move_x: fields[0],
                move_y: fields[1],
                scale_x: fields[2],
                scale_y: fields[3],
                rotate: fields[4],
                reflect_x: checks[0],
                reflect_y: checks[1],
            }),
            EffectKind::Tweak => Effect::Tweak(TweakEffect {
                horizontal: fields[0],
                vertical: fields[1],
                modify_anchors: checks[0],
                modify_in: checks[1],
                modify_out: checks[2],
                seed,
            }),
            EffectKind::Twist => Effect::Twist(TwistEffect { angle: fields[0] }),
        }
    }
}

impl EffectKind {
    /// Which kind a *stored* effect is, for editing an existing nested
    /// row — `None` for `Effect::Offset`, which routes to `offsetdlg`
    /// instead of this dialog.
    pub fn of(effect: &Effect) -> Option<Self> {
        match effect {
            Effect::Offset(_) => None,
            Effect::ZigZag(_) => Some(EffectKind::ZigZag),
            Effect::PuckerBloat(_) => Some(EffectKind::PuckerBloat),
            Effect::Roughen(_) => Some(EffectKind::Roughen),
            Effect::Transform(_) => Some(EffectKind::Transform),
            Effect::Tweak(_) => Some(EffectKind::Tweak),
            Effect::Twist(_) => Some(EffectKind::Twist),
        }
    }
}

fn fresh_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn trim_num(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 1e-9 {
        format!("{}", r.round() as i64)
    } else {
        format!("{r}")
    }
}

pub struct EffectDialog {
    pub object: ObjectId,
    pub item_index: usize,
    /// `None` while adding a new effect (OK appends); `Some(i)` while
    /// editing entry `i` already in the item's `effects` Vec (OK
    /// replaces it).
    pub effect_index: Option<usize>,
    pub kind: EffectKind,
    pub fields: Vec<NumEdit>,
    pub checks: Vec<bool>,
    seed: u64,
    pub focus: usize,
    /// On by default, matching `offsetdlg::OffsetDialog`'s own Illustrator-
    /// derived convention.
    pub preview: bool,
}

impl EffectDialog {
    pub fn open_for_item(
        object: ObjectId,
        item_index: usize,
        effect_index: Option<usize>,
        kind: EffectKind,
        current: Option<&Effect>,
    ) -> Self {
        let (values, checks, seed) = kind.seed_from(current);
        let fields = values
            .iter()
            .zip(kind.fields())
            .map(|(v, (_, mk))| NumEdit::seeded(trim_num(*v), *mk))
            .collect();
        Self { object, item_index, effect_index, kind, fields, checks, seed, focus: 0, preview: true }
    }

    pub fn resolved_values(&self) -> Vec<f64> {
        self.fields
            .iter()
            .zip(self.kind.fields())
            .map(|(f, (_, mk))| amalith_core::parse_measurement(&f.buf, *mk).unwrap_or(0.0))
            .collect()
    }

    pub fn resolved_effect(&self) -> Effect {
        self.kind.build(&self.resolved_values(), &self.checks, self.seed)
    }

    pub fn push_char(&mut self, ch: char) {
        if !widgets::measurement_char(ch) {
            return;
        }
        if let Some(f) = self.fields.get_mut(self.focus) {
            if f.fresh {
                f.buf.clear();
                f.fresh = false;
            }
            if f.buf.len() < 12 {
                f.buf.push(ch);
            }
        }
    }

    pub fn backspace(&mut self) {
        if let Some(f) = self.fields.get_mut(self.focus) {
            f.fresh = false;
            f.buf.pop();
        }
    }

    pub fn nudge(&mut self, dir: f64) {
        if let Some(f) = self.fields.get_mut(self.focus) {
            let cur = amalith_core::parse_measurement(&f.buf, f.kind).unwrap_or(0.0);
            f.buf = trim_num(cur + dir);
            f.fresh = false;
        }
    }

    pub fn toggle_check(&mut self, i: usize) {
        if let Some(c) = self.checks.get_mut(i) {
            *c = !*c;
        }
    }
}

struct Layout {
    fields: Vec<Rect>,
    checks: Vec<Rect>,
    preview: Rect,
    ok: Rect,
    cancel: Rect,
}

fn layout(n_fields: usize, n_checks: usize, body: Rect) -> Layout {
    let pad = ui_px(14.0);
    let field_h = ui_px(26.0);
    let row_gap = ui_px(8.0);
    let label_w = ui_px(88.0);
    let check_h = ui_px(16.0);
    let btn_h = ui_px(28.0);
    let x0 = body.x0 + pad;
    let x1 = body.x1 - pad;
    let mut y = body.y0 + pad;
    let mut fields = Vec::with_capacity(n_fields);
    for _ in 0..n_fields {
        fields.push(Rect::new(x0 + label_w, y, x1, y + field_h));
        y += field_h + row_gap;
    }
    let mut checks = Vec::with_capacity(n_checks);
    for _ in 0..n_checks {
        checks.push(Rect::new(x0, y, x0 + check_h, y + check_h));
        y += check_h + row_gap;
    }
    y += ui_px(4.0);
    let preview = Rect::new(x0, y + (btn_h - ui_px(16.0)) * 0.5, x0 + ui_px(16.0), y + (btn_h + ui_px(16.0)) * 0.5);
    let btn_w = ui_px(68.0);
    let ok = Rect::new(x1 - btn_w, y, x1, y + btn_h);
    let cancel = Rect::new(ok.x0 - ui_px(8.0) - btn_w, y, ok.x0 - ui_px(8.0), y + btn_h);
    Layout { fields, checks, preview, ok, cancel }
}

/// Body height (window-local, excluding the tab strip) for a dialog
/// showing `kind`'s own field/check count — unlike `offsetdlg`'s fixed
/// height (every Offset Path field is always shown), this genuinely
/// varies per effect kind.
pub fn body_height(kind: EffectKind) -> f64 {
    let pad = ui_px(14.0);
    let field_h = ui_px(26.0);
    let row_gap = ui_px(8.0);
    let check_h = ui_px(16.0);
    let btn_h = ui_px(28.0);
    let n_fields = kind.fields().len() as f64;
    let n_checks = kind.checks().len() as f64;
    pad + n_fields * (field_h + row_gap) + n_checks * (check_h + row_gap) + ui_px(4.0) + btn_h + pad
}

pub fn metric_w() -> f64 {
    ui_px(300.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Field(usize),
    Check(usize),
    Preview,
    Ok,
    Cancel,
    None,
}

pub fn hit(dlg: &EffectDialog, body: Rect, p: Point) -> Hit {
    let lay = layout(dlg.fields.len(), dlg.checks.len(), body);
    for (i, r) in lay.fields.iter().enumerate() {
        if r.contains(p) {
            return Hit::Field(i);
        }
    }
    for (i, r) in lay.checks.iter().enumerate() {
        if r.inflate(ui_px(6.0), ui_px(6.0)).contains(p) {
            return Hit::Check(i);
        }
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

pub fn paint(scene: &mut Scene, dlg: &EffectDialog, body: Rect, theme: &Theme, text: &mut TextContext, caret_on: bool) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(dlg.fields.len(), dlg.checks.len(), body);

    for (i, ((r, (label, _)), edit)) in lay.fields.iter().zip(dlg.kind.fields()).zip(&dlg.fields).enumerate() {
        text.draw(scene, label, 12.5, theme.text_dim, body.x0 + ui_px(14.0), r.center().y + ui_px(4.5));
        scene.fill(Fill::NonZero, ID, theme.bg, None, r);
        let focused = dlg.focus == i;
        scene.stroke(
            &Stroke::new(if focused { 1.5 } else { 1.0 }),
            ID,
            if focused { theme.accent } else { theme.text_dim.with_alpha(0.5) },
            None,
            r,
        );
        let shown = if focused && caret_on { format!("{}|", edit.buf) } else { edit.buf.clone() };
        text.draw(scene, &shown, 13.0, theme.text, r.x0 + ui_px(10.0), r.center().y + ui_px(4.5));
    }

    for (i, (r, label)) in lay.checks.iter().zip(dlg.kind.checks()).enumerate() {
        scene.stroke(&Stroke::new(ui_px(1.2)), ID, theme.text_dim, None, r);
        if dlg.checks[i] {
            scene.fill(Fill::NonZero, ID, theme.accent, None, &r.inflate(-ui_px(3.0), -ui_px(3.0)));
        }
        text.draw(scene, label, 12.0, theme.text, r.x1 + ui_px(8.0), r.center().y + ui_px(4.0));
    }

    scene.stroke(&Stroke::new(ui_px(1.2)), ID, theme.text_dim, None, &lay.preview);
    if dlg.preview {
        let inset = lay.preview.inflate(-ui_px(3.0), -ui_px(3.0));
        scene.fill(Fill::NonZero, ID, theme.accent, None, &inset);
    }
    text.draw(scene, "Preview", 12.0, theme.text, lay.preview.x1 + ui_px(8.0), lay.preview.center().y + ui_px(4.5));

    crate::widgets::button(scene, text, theme, lay.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, lay.ok, "OK", true);
}

/// The dialog window's tab-strip title.
pub fn window_title(kind: EffectKind) -> &'static str {
    kind.title()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_for_item_with_no_current_effect_seeds_kind_defaults() {
        let object = ObjectId::new();
        let dlg = EffectDialog::open_for_item(object, 0, None, EffectKind::ZigZag, None);
        assert_eq!(dlg.fields.len(), 2);
        assert_eq!(dlg.fields[0].buf, "10");
        assert_eq!(dlg.fields[1].buf, "4");
        assert_eq!(dlg.checks, vec![false]);
        assert_eq!(dlg.effect_index, None);
    }

    #[test]
    fn open_for_item_with_a_current_effect_seeds_its_values_and_round_trips() {
        let object = ObjectId::new();
        let fx = ZigZagEffect { size: 12.5, ridges_per_segment: 6.0, smooth: true };
        let dlg = EffectDialog::open_for_item(object, 3, Some(1), EffectKind::ZigZag, Some(&Effect::ZigZag(fx)));
        assert_eq!(dlg.resolved_effect(), Effect::ZigZag(fx));
        assert_eq!(dlg.effect_index, Some(1));
        assert_eq!(dlg.item_index, 3);
    }

    #[test]
    fn roughen_keeps_its_existing_seed_when_re_edited() {
        let object = ObjectId::new();
        let fx = RoughenEffect { size: 4.0, detail: 8.0, smooth: false, seed: 12345 };
        let dlg = EffectDialog::open_for_item(object, 0, Some(0), EffectKind::Roughen, Some(&Effect::Roughen(fx)));
        let Effect::Roughen(rebuilt) = dlg.resolved_effect() else { panic!() };
        assert_eq!(rebuilt.seed, 12345, "editing Roughen must not reroll its jitter pattern");
    }

    #[test]
    fn toggle_check_flips_only_the_targeted_check() {
        let object = ObjectId::new();
        let mut dlg = EffectDialog::open_for_item(object, 0, None, EffectKind::Transform, None);
        assert_eq!(dlg.checks, vec![false, false]);
        dlg.toggle_check(1);
        assert_eq!(dlg.checks, vec![false, true]);
    }

    #[test]
    fn every_kind_builds_a_matching_effect_variant_from_its_own_fields() {
        for kind in EffectKind::ALL {
            let object = ObjectId::new();
            let dlg = EffectDialog::open_for_item(object, 0, None, kind, None);
            let effect = dlg.resolved_effect();
            let matches = matches!(
                (kind, &effect),
                (EffectKind::ZigZag, Effect::ZigZag(_))
                    | (EffectKind::PuckerBloat, Effect::PuckerBloat(_))
                    | (EffectKind::Roughen, Effect::Roughen(_))
                    | (EffectKind::Transform, Effect::Transform(_))
                    | (EffectKind::Tweak, Effect::Tweak(_))
                    | (EffectKind::Twist, Effect::Twist(_))
            );
            assert!(matches, "{kind:?} built a mismatched effect variant: {effect:?}");
        }
    }
}
