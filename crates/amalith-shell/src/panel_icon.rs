//! Small monochrome vector glyphs, one per dockable panel — what the icon
//! strip shows in "Collapse to Icons" mode (both the icon+label row and,
//! once the strip is dragged narrow enough, icon-only).
//!
//! Illustrator ships its own bitmap icon set for this; we have neither
//! that art nor a license to reproduce it, so these are original, simple
//! pictograms in the same spirit (a plain, recognizable mark for each
//! panel) drawn straight from vello primitives — no image asset, crisp at
//! any scale, and trivial to extend when a new panel is added.

use vello::kurbo::{Affine, BezPath, Circle, Line, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::dock::PanelId;

const ID: Affine = Affine::IDENTITY;

/// Draws `panel`'s glyph centered in `rect`, stroked/filled in `color`.
/// Unknown panel ids (there shouldn't be any, but new panels land here
/// before anyone remembers to add an icon) fall back to a plain dot
/// rather than drawing nothing, so a missing icon reads as "generic",
/// not "broken".
pub fn draw(scene: &mut Scene, panel: PanelId, rect: Rect, color: Color) {
    // Every glyph is designed on a 16x16 logical square; scale + center
    // it into whatever `rect` the caller actually has.
    let s = (rect.width().min(rect.height()) / 16.0).max(0.1);
    let cx = rect.x0 + rect.width() * 0.5;
    let cy = rect.y0 + rect.height() * 0.5;
    let stroke = Stroke::new((1.3 / s).max(1.0) * s);
    // Local helper: a point in the 16x16 design space, mapped into `rect`.
    let p = |x: f64, y: f64| (cx + (x - 8.0) * s, cy + (y - 8.0) * s);
    let line = |scene: &mut Scene, a: (f64, f64), b: (f64, f64)| {
        scene.stroke(&stroke, ID, color, None, &Line::new(p(a.0, a.1), p(b.0, b.1)));
    };
    let rect_at = |x0: f64, y0: f64, x1: f64, y1: f64| {
        let (a, _) = (p(x0, y0), ());
        let (b, _) = (p(x1, y1), ());
        Rect::new(a.0, a.1, b.0, b.1)
    };

    match panel.0 {
        "align" => {
            // A vertical guide with three bars of different lengths
            // snapped to it — "align to a common edge".
            line(scene, (8.0, 1.5), (8.0, 14.5));
            scene.stroke(&stroke, ID, color, None, &Line::new(p(8.0, 3.5), p(13.0, 3.5)));
            scene.stroke(&stroke, ID, color, None, &Line::new(p(8.0, 8.0), p(11.0, 8.0)));
            scene.stroke(&stroke, ID, color, None, &Line::new(p(8.0, 12.5), p(14.5, 12.5)));
        }
        "artboards" => {
            // A rectangle with crop-mark corners, like a page/artboard.
            scene.stroke(&stroke, ID, color, None, &rect_at(3.5, 3.5, 12.5, 12.5));
            for (cx0, cy0, dx, dy) in [
                (3.5, 3.5, -1.0, -1.0),
                (12.5, 3.5, 1.0, -1.0),
                (3.5, 12.5, -1.0, 1.0),
                (12.5, 12.5, 1.0, 1.0),
            ] {
                line(scene, (cx0, cy0), (cx0 + dx * 2.0, cy0));
                line(scene, (cx0, cy0), (cx0, cy0 + dy * 2.0));
            }
        }
        "character" | "paragraph" => {
            // A capital "A" for Character, a pilcrow-ish "¶" stand-in for
            // Paragraph — both built from strokes so they match the rest
            // of this set rather than depending on the text font's glyph
            // coverage at tiny sizes.
            if panel.0 == "character" {
                line(scene, (8.0, 2.5), (3.5, 13.5));
                line(scene, (8.0, 2.5), (12.5, 13.5));
                line(scene, (5.3, 9.5), (10.7, 9.5));
            } else {
                let mut path = BezPath::new();
                let a = p(10.0, 2.5);
                path.move_to(a);
                path.line_to(p(10.0, 13.5));
                path.line_to(p(8.4, 13.5));
                path.line_to(p(8.4, 8.6));
                path.curve_to(p(4.5, 8.6), p(4.5, 2.5), p(8.4, 2.5));
                path.close_path();
                scene.fill(Fill::NonZero, ID, color, None, &path);
                line(scene, (11.6, 2.5), (11.6, 13.5));
            }
        }
        "color" => {
            // Two overlapping swatch circles — one stroked, one filled —
            // echoing the fill-over-stroke color chip.
            let (c1, _) = (p(6.2, 8.5), ());
            let (c2, _) = (p(10.2, 8.5), ());
            scene.fill(
                Fill::NonZero,
                ID,
                color,
                None,
                &Circle::new(c2, 4.2 * s),
            );
            scene.stroke(&stroke, ID, color, None, &Circle::new(c1, 4.2 * s));
        }
        "gradient" => {
            // A small horizontal gradient bar: solid at one end, faded at
            // the other, banded to read clearly even at icon-only size.
            let bar = rect_at(2.5, 6.0, 13.5, 11.0);
            scene.stroke(&stroke, ID, color, None, &bar);
            let bands = 5;
            for i in 0..bands {
                let a = color.with_alpha((i + 1) as f32 / bands as f32);
                let x0 = 2.5 + (11.0 / bands as f64) * i as f64;
                let x1 = 2.5 + (11.0 / bands as f64) * (i + 1) as f64;
                scene.fill(Fill::NonZero, ID, a, None, &rect_at(x0, 6.0, x1, 11.0));
            }
        }
        "layers" => {
            // Three stacked diamonds — the classic "layers" mark.
            for dy in [3.2, 6.6, 10.0] {
                let mut path = BezPath::new();
                path.move_to(p(8.0, dy - 1.6));
                path.line_to(p(13.0, dy));
                path.line_to(p(8.0, dy + 1.6));
                path.line_to(p(3.0, dy));
                path.close_path();
                scene.stroke(&stroke, ID, color, None, &path);
            }
        }
        "pathfinder" => {
            // Two overlapping squares — Boolean-op iconography.
            scene.stroke(&stroke, ID, color, None, &rect_at(2.5, 5.5, 9.5, 12.5));
            scene.stroke(&stroke, ID, color, None, &rect_at(6.5, 2.5, 13.5, 9.5));
        }
        "swatches" => {
            // A 2x2 grid of small filled chips.
            for (x0, y0) in [(2.5, 2.5), (9.0, 2.5), (2.5, 9.0), (9.0, 9.0)] {
                scene.fill(Fill::NonZero, ID, color, None, &rect_at(x0, y0, x0 + 4.5, y0 + 4.5));
            }
        }
        "tools" => {
            // A selection-arrow cursor.
            let mut path = BezPath::new();
            path.move_to(p(3.5, 2.5));
            path.line_to(p(3.5, 13.0));
            path.line_to(p(6.7, 10.1));
            path.line_to(p(9.2, 13.2));
            path.line_to(p(10.6, 12.1));
            path.line_to(p(8.1, 9.0));
            path.line_to(p(12.0, 8.6));
            path.close_path();
            scene.fill(Fill::NonZero, ID, color, None, &path);
        }
        "transform" => {
            // A dashed bounding box with corner handles.
            let r = rect_at(3.0, 3.0, 13.0, 13.0);
            scene.stroke(
                &Stroke::new(stroke.width).with_dashes(0.0, [2.0 * s, 1.6 * s]),
                ID,
                color,
                None,
                &r,
            );
            for (x, y) in [(3.0, 3.0), (13.0, 3.0), (3.0, 13.0), (13.0, 13.0)] {
                scene.fill(Fill::NonZero, ID, color, None, &rect_at(x - 1.1, y - 1.1, x + 1.1, y + 1.1));
            }
        }
        _ => {
            scene.fill(Fill::NonZero, ID, color, None, &Circle::new(p(8.0, 8.0), 3.0 * s));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::Rect;

    /// Every real panel id draws without panicking, at both icon-only and
    /// icon+label sizes — a new panel that forgets to add a glyph here
    /// still falls through to the generic dot rather than crashing.
    #[test]
    fn every_dockable_panel_draws_a_glyph_without_panicking() {
        let ids = [
            "align", "artboards", "character", "color", "gradient", "layers", "paragraph",
            "pathfinder", "swatches", "tools", "transform", "some-future-panel",
        ];
        for id in ids {
            let mut scene = Scene::new();
            draw(&mut scene, PanelId(id), Rect::new(0.0, 0.0, 18.0, 18.0), Color::BLACK);
            draw(&mut scene, PanelId(id), Rect::new(0.0, 0.0, 30.0, 18.0), Color::BLACK);
        }
    }
}
