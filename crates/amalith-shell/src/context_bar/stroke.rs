//! The stroke Weight field, labelled "Stroke" — the label opens the
//! Stroke flyout. Shown alongside `fill_stroke`.

use crate::metrics::px as ui_px;

use vello::kurbo::{BezPath, Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::panels::Action;
use crate::text::TextContext;

use super::{baseline, draw_field, field, Ctx, SegKind, Segment, ID};

fn metric_profile_w() -> f64 { crate::metrics::with(|m| m.context_bar_stroke_profile_w) }

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Stroke,
    applies: |ctx| !ctx.text_context,
    measure: |_| ui_px(136.0) + metric_profile_w() + ui_px(8.0),
    paint,
    hit,
};

/// (link, weight field, up, down, profile combo) rects.
fn parts(r: Rect) -> (Rect, Rect, Rect, Rect, Rect) {
    let cy = r.center().y;
    let link = Rect::new(r.x0 - ui_px(4.0), cy - ui_px(10.5), r.x0 + ui_px(48.0), cy + ui_px(10.5));
    let (f, up, down) = field(r.x0 + ui_px(53.0), cy, ui_px(64.0));
    let px0 = down.x1 + ui_px(8.0);
    let profile = Rect::new(px0, cy - ui_px(11.0), px0 + metric_profile_w(), cy + ui_px(11.0));
    (link, f, up, down, profile)
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let (_, f, up, down, profile) = parts(r);
    let base = baseline(r);
    let link_color = if ctx.stroke_open {
        theme.accent
    } else {
        theme.text
    };
    text.draw(scene, "Stroke", 13.0, link_color, r.x0, base);
    let uw = text.measure("Stroke", 13.0);
    scene.stroke(
        &Stroke::new(ui_px(1.0)),
        ID,
        link_color.with_alpha(if ctx.stroke_open { 1.0 } else { 0.5 }),
        None,
        &Line::new((r.x0, base + ui_px(2.0)), (r.x0 + uw, base + ui_px(2.0))),
    );

    let editing = ctx.stroke_weight_edit.is_some();
    let shown = match ctx.stroke_weight_edit {
        Some(buf) => buf.to_string(),
        None => {
            let w = ctx
                .representative
                .map(|a| a.stroke_width)
                .unwrap_or(ctx.cur_weight);
            format!("{w:.1} px")
        }
    };
    draw_field(scene, text, theme, f, up, down, &shown, editing);

    // Profile: closed, this just shows a plain straight-line "Uniform"
    // icon + label — it's an action trigger for the preset picker below,
    // not a live readout of which named preset (if any) produced the
    // selection's current width points.
    let border = if ctx.width_profile_menu { theme.accent } else { theme.text_dim.with_alpha(0.5) };
    scene.fill(Fill::NonZero, ID, theme.bg, None, &profile);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, border, None, &profile);
    let icon = Rect::new(profile.x0 + ui_px(6.0), profile.y0 + ui_px(4.0), profile.x0 + ui_px(30.0), profile.y1 - ui_px(4.0));
    let iy = icon.center().y;
    scene.stroke(&Stroke::new(1.5), ID, theme.text, None, &Line::new((icon.x0, iy), (icon.x1, iy)));
    text.draw(scene, "Uniform", 12.5, theme.text, icon.x1 + ui_px(6.0), profile.center().y + ui_px(4.0));
    let cx = profile.x1 - ui_px(10.0);
    let cy = profile.center().y;
    let mut caret = BezPath::new();
    caret.move_to((cx - ui_px(3.0), cy - ui_px(2.0)));
    caret.line_to((cx + ui_px(3.0), cy - ui_px(2.0)));
    caret.line_to((cx, cy + ui_px(2.5)));
    caret.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &caret);
}

fn hit(r: Rect, local: Point, _ctx: &Ctx) -> Action {
    let (link, f, up, down, profile) = parts(r);
    if link.contains(local) {
        Action::ToggleStrokeFlyout
    } else if up.contains(local) {
        Action::StepWeight(1)
    } else if down.contains(local) {
        Action::StepWeight(-1)
    } else if f.contains(local) {
        Action::BeginStrokeWeightEdit
    } else if profile.contains(local) {
        Action::OpenWidthProfileMenu(profile)
    } else {
        Action::None
    }
}

/// Whether the pointer is over the Weight field — for scroll-to-nudge and
/// commit-on-click-away, mirroring `xform::field_at`.
pub fn weight_field_at(r: Rect, p: Point) -> bool {
    let (_, f, _, _, _) = parts(r);
    f.contains(p)
}
