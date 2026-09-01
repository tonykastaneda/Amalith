//! The context / control bar — the full-width strip under the tab bar.
//!
//! It is **one bar assembled from self-contained segments**. Each segment
//! (`status`, `fill_stroke`, `stroke`, `opacity`, `character`, …) is its
//! own module with four functions:
//!
//! - `applies(&Ctx) -> bool`   — is this segment relevant to the current focus?
//! - `measure(&Ctx) -> f64`    — how wide is it?
//! - `paint(scene, text, rect, &Ctx)`
//! - `hit(rect, local, &Ctx) -> Action`
//!
//! [`paint`] / [`hit`] walk [`SEGMENTS`] in priority order, give each its
//! width until the bar is full, and drop the rest. Selecting a text object
//! instead of a rectangle just changes which `applies` predicates fire —
//! there is no per-selection "toolbar", only the segment list.
//!
//! Segments are stateless. They read a [`Ctx`] and return a
//! [`crate::panels::Action`] (the shared UI-action vocabulary the shell's
//! `apply_panel_action` dispatches), exactly like a panel body.

use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::{Action, PaintSlot};
use crate::text::TextContext;
use crate::theme::Theme;

mod anchor;
mod character;
mod fill_stroke;
mod opacity;
mod status;
mod stroke;
mod xform;

const ID: vello::kurbo::Affine = vello::kurbo::Affine::IDENTITY;
/// Gap between adjacent segments; a hairline separator sits in the middle.
const GAP: f64 = 22.0;

/// The read-only slice of shell state a context-bar segment draws from.
/// Built once per paint / hit — the one construction site is the price of
/// not threading two dozen positional args through `paint_main`.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    /// Size of the object selection.
    pub selection_len: usize,
    /// True when text is the editing focus (caret in a text object, or the
    /// whole selection is text) — flips the `character` segment on and the
    /// paint / stroke segments off.
    pub text_context: bool,
    /// First selected object's appearance, for the fill / stroke / weight /
    /// opacity readouts.
    pub representative: Option<amalith_core::Appearance>,
    pub active_slot: PaintSlot,
    /// Stored stroke width / opacity, shown when nothing is selected.
    pub cur_weight: f64,
    pub cur_opacity: f32,
    /// Whether the Stroke flyout is open (the "Stroke" link reads active).
    pub stroke_open: bool,
    /// The type style the `character` segment shows.
    pub text_style: amalith_core::TextStyle,
    /// Number of individually-selected path anchors — flips the
    /// `anchor` (Convert) segment on.
    pub anchor_sel_len: usize,
    /// Transform readout for the first selected object.
    pub xform: Option<amalith_core::TransformValues>,
    pub xform_constrain: bool,
    pub xform_edit: Option<(crate::panels::transform::XformField, &'a str)>,
}

/// Identifies a segment so callers (e.g. the Stroke flyout anchor) can
/// find where a particular one landed.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegKind {
    Status,
    FillStroke,
    Stroke,
    Opacity,
    Character,
    Anchor,
    Xform,
}

struct Segment {
    kind: SegKind,
    applies: fn(&Ctx) -> bool,
    measure: fn(&Ctx) -> f64,
    paint: fn(&mut Scene, &mut TextContext, Rect, &Ctx),
    hit: fn(Rect, Point, &Ctx) -> Action,
}

/// Priority order, left to right. `status` first; then the context-specific
/// clusters. A segment whose `applies` is false is skipped, so the same
/// list serves every selection kind.
const SEGMENTS: &[Segment] = &[
    status::SEGMENT,
    xform::SEGMENT,
    anchor::SEGMENT,
    character::SEGMENT,
    fill_stroke::SEGMENT,
    stroke::SEGMENT,
    opacity::SEGMENT,
];

/// Lay the applicable segments out along `bar`, dropping any that would
/// overflow its right edge.
fn placed<'s>(bar: Rect, ctx: &Ctx) -> Vec<(&'s Segment, Rect)> {
    let mut out = Vec::new();
    let mut x = bar.x0 + 12.0;
    for seg in SEGMENTS {
        if !(seg.applies)(ctx) {
            continue;
        }
        let w = (seg.measure)(ctx);
        if x + w > bar.x1 - 18.0 {
            break;
        }
        out.push((seg, Rect::new(x, bar.y0, x + w, bar.y1)));
        x += w + GAP;
    }
    out
}

/// Draw the bar background and every visible segment.
pub fn paint(scene: &mut Scene, text: &mut TextContext, bar: Rect, ctx: &Ctx) {
    scene.fill(Fill::NonZero, ID, ctx.theme.strip_active, None, &bar);
    scene.fill(
        Fill::NonZero,
        ID,
        ctx.theme.border,
        None,
        &Rect::new(bar.x0, bar.y1 - 1.0, bar.x1, bar.y1),
    );
    for (i, (seg, r)) in placed(bar, ctx).into_iter().enumerate() {
        if i > 0 {
            scene.fill(
                Fill::NonZero,
                ID,
                ctx.theme.border,
                None,
                &Rect::new(r.x0 - GAP * 0.5, bar.y0 + 7.0, r.x0 - GAP * 0.5 + 1.0, bar.y1 - 7.0),
            );
        }
        (seg.paint)(scene, text, r, ctx);
    }
}

/// Resolve a press inside the bar to an [`Action`].
pub fn hit(bar: Rect, local: Point, ctx: &Ctx) -> Action {
    for (seg, r) in placed(bar, ctx) {
        if r.contains(local) {
            return (seg.hit)(r, local, ctx);
        }
    }
    Action::None
}

/// Where segment `kind` landed this frame, if it is visible — for anchoring
/// popovers (the Stroke flyout) to it.
/// Which Shape/Transform numeric field the pointer is over, if any.
pub fn xform_field_at(bar: Rect, ctx: &Ctx, p: Point) -> Option<crate::panels::transform::XformField> {
    let r = segment_rect(bar, ctx, SegKind::Xform)?;
    xform::field_at(r, p)
}

pub fn segment_rect(bar: Rect, ctx: &Ctx, kind: SegKind) -> Option<Rect> {
    placed(bar, ctx)
        .into_iter()
        .find(|(s, _)| s.kind == kind)
        .map(|(_, r)| r)
}

// ---- shared segment widgets ---------------------------------------------

/// Baseline y for 11.5px label text centred in `bar`.
fn baseline(bar: Rect) -> f64 {
    bar.y0 + bar.height() * 0.5 + 4.0
}

/// A boxed numeric readout plus an up / down stepper column. Returns the
/// (field, up, down) rects so `hit` can reuse the same geometry.
fn field(x: f64, cy: f64, w: f64) -> (Rect, Rect, Rect) {
    let field = Rect::new(x, cy - 10.0, x + w, cy + 10.0);
    let up = Rect::new(field.x1, cy - 10.0, field.x1 + 13.0, cy);
    let down = Rect::new(field.x1, cy, field.x1 + 13.0, cy + 10.0);
    (field, up, down)
}

fn draw_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    field: Rect,
    up: Rect,
    down: Rect,
    value: &str,
) {
    let border = theme.text_dim.with_alpha(0.5);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &field);
    text.draw(
        scene,
        value,
        11.5,
        theme.text,
        field.x0 + 6.0,
        field.y0 + field.height() * 0.5 + 4.0,
    );
    let col = Rect::new(up.x0, up.y0, up.x1, down.y1);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &col);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &col);
    let cx = col.x0 + col.width() * 0.5;
    tri(scene, cx, up.y1 - 3.0, up.y0 + 3.0, theme);
    tri(scene, cx, down.y0 + 3.0, down.y1 - 3.0, theme);
}

fn tri(scene: &mut Scene, cx: f64, base_y: f64, tip_y: f64, theme: &Theme) {
    let mut p = BezPath::new();
    p.move_to((cx - 3.0, base_y));
    p.line_to((cx + 3.0, base_y));
    p.line_to((cx, tip_y));
    p.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &p);
}

/// A bordered combo box: value text on the left, a ⌄ caret on the right.
fn draw_combo(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, value: &str) {
    let border = theme.text_dim.with_alpha(0.5);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    scene.stroke(&Stroke::new(1.0), ID, border, None, &r);
    text.draw(
        scene,
        value,
        11.5,
        theme.text,
        r.x0 + 7.0,
        r.y0 + r.height() * 0.5 + 4.0,
    );
    let cx = r.x1 - 10.0;
    let cy = r.center().y;
    let mut t = BezPath::new();
    t.move_to((cx - 3.0, cy - 2.0));
    t.line_to((cx + 3.0, cy - 2.0));
    t.line_to((cx, cy + 2.5));
    t.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &t);
}
