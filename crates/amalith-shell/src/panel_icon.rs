//! Theme-tinted panel glyphs for collapsed rows, with or without titles.

use vello::kurbo::{Affine, BezPath, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::dock::{PanelId, PanelKind};

/// Draw a panel-specific pictogram in a centered square, using the row's
/// active/dim foreground color. Coordinates share an 18-unit view box.
pub fn draw(scene: &mut Scene, panel: PanelId, rect: Rect, color: Color) {
    let size = rect.width().min(rect.height());
    if size <= 0.0 {
        return;
    }
    let center = rect.center();
    let transform = Affine::translate((center.x - size / 2.0, center.y - size / 2.0))
        * Affine::scale(size / 18.0);
    // Neutral panel window — the fallback glyph shared by unrecognized ids
    // and every float-only dialog panel, which never show a tab icon in
    // practice (they only ever paint into a lone floating window's body,
    // with no rail row to draw this icon into). Named so the choice reads
    // as deliberate, not an accident of a wildcard.
    const GENERIC: &str = "M2 3H16V15H2Z M2 7H16 M5 5H6";
    let artwork = match panel.0 {
        // Left alignment guide and two differently sized objects.
        PanelKind::Align => "M3 2V16 M6 4H15V7H6Z M6 11H12V14H6Z",
        // Offset sheets, matching the artboard metaphor.
        PanelKind::Artboards => "M3 11H2V2H11V3 M5 5H16V16H5Z",
        // A with a crossbar, legible without depending on a font.
        PanelKind::Character => "M3 15L9 3L15 15 M5 11H13 M2 15H6 M12 15H16",
        // Artist's palette with three paint wells and a thumb cutout.
        PanelKind::Color => "M15 10C18 4 11 1 6 3C0 5 1 13 6 15C10 17 12 15 10 13C9 11 12 10 15 10Z M5 6H6 M9 5H10 M4 10H5",
        PanelKind::Gradient => "M2 4H16V14H2Z",
        // Three stacked layers.
        PanelKind::Layers => "M2 6L9 2L16 6L9 10Z M2 10L9 14L16 10 M2 13L9 17L16 13",
        // Three overlapping chain links, in a diagonal run.
        PanelKind::Links => "M2 5H8V9H2Z M6 9H12V13H6Z M10 4H16V8H10Z",
        PanelKind::Paragraph => "M2 4H16 M2 7H12 M2 10H16 M2 13H12 M2 16H9",
        // Overlapping shapes with their shared region emphasized below.
        PanelKind::Pathfinder => "M2 2H11V11H2Z M7 7H16V16H7Z",
        PanelKind::Swatches => "M2 2H7V7H2Z M11 2H16V7H11Z M2 11H7V16H2Z M11 11H16V16H11Z",
        // Pen nib and its central slit.
        PanelKind::Tools => "M9 2L15 11L11 15H7L3 11Z M9 2V9 M7 15H11V17H7Z M8 10H10V12H8Z",
        // Bounding box with corner handles.
        PanelKind::Transform => "M5 3H13 M15 5V13 M13 15H5 M3 13V5 M2 2H5V5H2Z M13 2H16V5H13Z M2 13H5V16H2Z M13 13H16V16H13Z",
        PanelKind::Picker => "M11 3L15 7 M12 2L16 6L14 8L10 4Z M11 5L3 13L2 16L5 15L13 7",
        PanelKind::ExportScreens => "M7 3H2V16H15V11 M9 2H16V9 M16 2L7 11",
        // Float-only dialog panels never draw this icon in a real rail row
        // today — kept explicit rather than folded into `Unknown` so a
        // future panel that *should* get a bespoke glyph doesn't silently
        // inherit one of these by accident.
        PanelKind::ShapedlgRect
        | PanelKind::ShapedlgRound
        | PanelKind::ShapedlgEllipse
        | PanelKind::ShapedlgPolygon
        | PanelKind::ShapedlgStar
        | PanelKind::ShapedlgArc
        | PanelKind::ShapedlgSpiral
        | PanelKind::XformdlgReflect
        | PanelKind::XformdlgShear
        | PanelKind::Blenddlg
        | PanelKind::Offsetdlg
        | PanelKind::LayerOptionsDlg
        | PanelKind::AreaTypeDlg => GENERIC,
        // Unrecognized panel id (an old/future/typo'd string round-tripped
        // from a saved layout).
        PanelKind::Unknown(_) => GENERIC,
    };
    let path = BezPath::from_svg(artwork).expect("panel icon paths are valid SVG");
    scene.stroke(&Stroke::new(1.3), transform, color, None, &path);
    if panel.0 == PanelKind::Gradient {
        // Increasing ink coverage gives a monochrome ramp in either theme.
        for (x, width) in [(5.0, 0.6), (7.0, 0.9), (9.0, 1.2), (11.0, 1.5), (13.0, 2.3)] {
            scene.fill(
                Fill::NonZero,
                transform,
                color,
                None,
                &Rect::new(x, 4.0, x + width, 14.0),
            );
        }
    } else if panel.0 == PanelKind::Pathfinder {
        scene.fill(
            Fill::NonZero,
            transform,
            color,
            None,
            &Rect::new(7.0, 7.0, 11.0, 11.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::Rect;

    /// Every real panel kind draws without panicking, at both icon-only
    /// and icon+label sizes — plus an unrecognized id, exercising the
    /// `Unknown` fallback the same way an old saved layout's stray panel
    /// id would.
    #[test]
    fn every_dockable_panel_draws_without_panicking() {
        let ids: Vec<PanelKind> = PanelKind::ALL
            .into_iter()
            .chain(std::iter::once(PanelKind::Unknown("some-future-panel")))
            .collect();
        for kind in ids {
            let mut scene = Scene::new();
            draw(
                &mut scene,
                PanelId(kind),
                Rect::new(0.0, 0.0, 18.0, 18.0),
                Color::BLACK,
            );
            draw(
                &mut scene,
                PanelId(kind),
                Rect::new(0.0, 0.0, 30.0, 18.0),
                Color::BLACK,
            );
        }
    }
}
