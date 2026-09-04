//! Placeholder glyphs for the icon strip ("Collapse to Icons" mode, both
//! icon+title and icon-only). Every panel just draws a plain green square
//! for now — a deliberate, obvious placeholder (not a real pictogram)
//! marking "this panel still needs a real icon", rather than a hand-drawn
//! approximation that might pass for finished art. Swap [`draw`]'s body
//! out per panel as real icons land; the call sites (the icon strip, the
//! collapsed floating-panel row) don't need to change.

use vello::kurbo::Rect;
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::dock::PanelId;

/// A flat green square placeholder for `panel`'s icon, centered in `rect`
/// with a small margin. `color` (the strip's active/dim text color) is
/// intentionally unused — the placeholder stays the same bright green
/// either way, so it reads as "no icon yet" rather than blending in.
pub fn draw(scene: &mut Scene, _panel: PanelId, rect: Rect, _color: Color) {
    const PLACEHOLDER: Color = Color::from_rgb8(0x2e, 0xcc, 0x40);
    let margin = (rect.width().min(rect.height()) * 0.12).max(1.0);
    let square = rect.inset(-margin);
    scene.fill(Fill::NonZero, vello::kurbo::Affine::IDENTITY, PLACEHOLDER, None, &square);
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::Rect;

    /// Every real panel id draws without panicking, at both icon-only and
    /// icon+label sizes.
    #[test]
    fn every_dockable_panel_draws_a_placeholder_without_panicking() {
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
