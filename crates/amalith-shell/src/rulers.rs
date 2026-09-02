//! Canvas rulers (⌘R). Thin strips along the top and left edges of the
//! document view showing document-space coordinates in px, with tick
//! marks and labels that adapt to zoom.
//!
//! Purely an overlay: the ruler origin is the document origin `(0, 0)`.
//! When rulers are on, `App::canvas_viewport` insets the content region
//! by [`THICK`] on the top and left so artwork, hit-testing, culling and
//! fit-view all start past the strips.
//!
//! [`build`] draws the static ruler (strips, ticks, labels) — the shell
//! caches its output and only rebuilds it when the view changes.
//! [`marker`] draws the per-frame pointer lines.

use vello::kurbo::{Affine, BezPath, Line, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::canvas::CanvasView;
use crate::text::TextContext;
use crate::theme::Theme;

/// Strip thickness, logical px.
pub const THICK: f64 = 18.0;

const ID: Affine = Affine::IDENTITY;
/// Roughly this many screen px between labelled ticks.
const LABEL_PX: f64 = 110.0;
/// Label glyph size. Fixed and integer so every digit hits the GPU's
/// glyph atlas frame to frame instead of being re-rasterized.
const LABEL_SIZE: f32 = 10.0;

/// A "nice" step — 1, 2 or 5 × 10^k — covering at least `raw` doc units.
fn nice_step(raw: f64) -> f64 {
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let pow = 10f64.powf(raw.log10().floor());
    let f = raw / pow;
    let n = if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    n * pow
}

/// A ruler value without a trailing `.0` when it's integral.
fn label(v: f64) -> String {
    if v.fract().abs() < 1e-6 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

/// The two strip rects for `region`: (top, left).
fn strips(region: Rect) -> (Rect, Rect) {
    (
        Rect::new(region.x0, region.y0, region.x1, region.y0 + THICK),
        Rect::new(region.x0, region.y0, region.x0 + THICK, region.y1),
    )
}

/// Draw the static rulers (strips, ticks, labels) over `region` — the
/// full canvas rect between the rails, below the chrome. No pointer
/// marker; that changes every mouse move and is drawn by [`marker`].
pub fn build(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    region: Rect,
    view: &CanvasView,
) {
    let (top, left) = strips(region);
    let corner = Rect::new(region.x0, region.y0, region.x0 + THICK, region.y0 + THICK);
    let bg = theme.app_bar;

    scene.fill(Fill::NonZero, ID, bg, None, &top);
    scene.fill(Fill::NonZero, ID, bg, None, &left);
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(top.x0, top.y1 - 1.0, top.x1, top.y1),
    );
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(left.x1 - 1.0, left.y0, left.x1, left.y1),
    );

    let zoom = view.zoom.max(1e-9);
    let step = nice_step(LABEL_PX / zoom);
    let minor = step / 5.0;
    let major_ink = theme.text_dim;
    let minor_ink = theme.text_dim.with_alpha(0.5);
    let stroke = Stroke::new(1.0);

    // --- Top ruler (X). Ticks start past the corner square. All the
    // hairlines batch into two paths (minor / major) so Vello
    // tessellates them in one pass each, not one per tick. ---
    let x_lo = left.x1 + 1.0;
    let mut minors = BezPath::new();
    let mut majors = BezPath::new();
    let i0 = (((x_lo - view.pan.x) / zoom) / minor).ceil() as i64;
    let i1 = (((top.x1 - view.pan.x) / zoom) / minor).floor() as i64;
    for i in i0..=i1 {
        let v = i as f64 * minor;
        // Whole-pixel tick + label so hairlines stay crisp and glyphs
        // hit the atlas as the view pans.
        let sx = (view.pan.x + v * zoom).round();
        if i % 5 == 0 {
            majors.move_to((sx, top.y1 - THICK * 0.6));
            majors.line_to((sx, top.y1));
            text.draw(scene, &label(v), LABEL_SIZE, major_ink, sx + 3.0, top.y0 + 11.0);
        } else {
            minors.move_to((sx, top.y1 - THICK * 0.3));
            minors.line_to((sx, top.y1));
        }
    }
    scene.stroke(&stroke, ID, minor_ink, None, &minors);
    scene.stroke(&stroke, ID, major_ink, None, &majors);

    // --- Left ruler (Y). Labels as an upright digit column. ---
    let y_lo = left.y0 + THICK + 1.0;
    let mut minors = BezPath::new();
    let mut majors = BezPath::new();
    let i0 = (((y_lo - view.pan.y) / zoom) / minor).ceil() as i64;
    let i1 = (((left.y1 - view.pan.y) / zoom) / minor).floor() as i64;
    for i in i0..=i1 {
        let v = i as f64 * minor;
        let sy = (view.pan.y + v * zoom).round();
        if i % 5 == 0 {
            majors.move_to((left.x1 - THICK * 0.6, sy));
            majors.line_to((left.x1, sy));
            text.draw_column(
                scene,
                &label(v),
                LABEL_SIZE,
                major_ink,
                (left.x0 + THICK * 0.5).round(),
                sy + 3.0,
                8.0,
            );
        } else {
            minors.move_to((left.x1 - THICK * 0.3, sy));
            minors.line_to((left.x1, sy));
        }
    }
    scene.stroke(&stroke, ID, minor_ink, None, &minors);
    scene.stroke(&stroke, ID, major_ink, None, &majors);

    // Corner box, drawn last to mask any label near the join.
    scene.fill(Fill::NonZero, ID, bg, None, &corner);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &corner);
}

/// The pointer position lines on both rulers — cheap, drawn every frame.
pub fn marker(scene: &mut Scene, theme: &Theme, region: Rect, pointer: Point) {
    if !region.contains(pointer) {
        return;
    }
    let (top, left) = strips(region);
    let m = theme.accent;
    if pointer.x > left.x1 {
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            m,
            None,
            &Line::new((pointer.x, top.y0), (pointer.x, top.y1)),
        );
    }
    if pointer.y > top.y1 {
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            m,
            None,
            &Line::new((left.x0, pointer.y), (left.x1, pointer.y)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::nice_step;

    #[test]
    fn nice_step_snaps_to_1_2_5_decades() {
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.5), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert_eq!(nice_step(140.0), 200.0);
        assert_eq!(nice_step(0.03), 0.05);
    }
}
