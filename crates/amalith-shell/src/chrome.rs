//! Draws a [`Layout`] into a vello [`Scene`]: panel bodies, tab strips,
//! tab labels, splitters, and the drop indicator.

use vello::kurbo::{Affine, Line, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::dock::{DropTarget, IconColumn, NodePath, PanelId, Side};
use crate::layout::{IconRect, Layout};
use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;
const TAB_TEXT_PX: f32 = 12.6;

/// Panel tabs get 10% more side padding than the theme's base, and reserve
/// this much room on the right for the close (×) button.
pub const PANEL_TAB_PAD_MUL: f64 = 1.1;
pub const PANEL_TAB_CLOSE_W: f64 = 22.0;

/// The close-button hit / draw rect for a panel `tab`.
pub fn panel_tab_close_rect(tab: Rect) -> Rect {
    Rect::new(tab.x1 - PANEL_TAB_CLOSE_W, tab.y0, tab.x1, tab.y1)
}

/// The hamburger button on the right of a panel group's tab strip.
pub fn panel_menu_rect(tab_strip: Rect, theme: &Theme) -> Rect {
    Rect::new(
        tab_strip.x1 - theme.panel_menu_w,
        tab_strip.y0,
        tab_strip.x1,
        tab_strip.y1,
    )
}

/// The collapse-to-icons («) button, always at the far right of a panel
/// group's tab strip — just left of the hamburger when `has_menu` (that
/// group's active tab has one).
pub fn collapse_rect(tab_strip: Rect, has_menu: bool, theme: &Theme) -> Rect {
    let x1 = if has_menu {
        tab_strip.x1 - theme.panel_menu_w
    } else {
        tab_strip.x1
    };
    Rect::new(x1 - theme.panel_collapse_w, tab_strip.y0, x1, tab_strip.y1)
}

/// Paint every group and splitter in `layout`. `label(panel)` supplies the
/// tab caption; `text` rasterizes it.
pub fn paint(
    scene: &mut Scene,
    layout: &Layout,
    theme: &Theme,
    text: &mut TextContext,
    label: &dyn Fn(PanelId) -> String,
) {
    for area in &layout.areas {
        scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &area.body);
        scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &area.tab_strip);

        for (i, tab) in area.tabs.iter().enumerate() {
            let active = i == area.active;
            if active {
                scene.fill(Fill::NonZero, ID, theme.strip_active, None, &tab.rect);
                let u = Rect::new(tab.rect.x0, tab.rect.y1 - 2.0, tab.rect.x1, tab.rect.y1);
                scene.fill(Fill::NonZero, ID, theme.drop_line, None, &u);
            }
            if i > 0 {
                let sep = Rect::new(
                    tab.rect.x0 - 0.5,
                    tab.rect.y0 + 4.0,
                    tab.rect.x0 + 0.5,
                    tab.rect.y1 - 4.0,
                );
                scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
            }

            let color = if active { theme.text } else { theme.text_dim };
            let baseline = tab.rect.y0 + tab.rect.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
            // Label: left-aligned.
            text.draw(
                scene,
                &label(tab.panel),
                TAB_TEXT_PX,
                color,
                tab.rect.x0 + theme.tab_pad_x * PANEL_TAB_PAD_MUL,
                baseline,
            );
            // Close (×): right-aligned.
            let cr = panel_tab_close_rect(tab.rect);
            let c = cr.center();
            let a = 3.5;
            let xcol = if active { theme.text } else { theme.text_dim };
            scene.stroke(
                &Stroke::new(1.4),
                ID,
                xcol,
                None,
                &Line::new((c.x - a, c.y - a), (c.x + a, c.y + a)),
            );
            scene.stroke(
                &Stroke::new(1.4),
                ID,
                xcol,
                None,
                &Line::new((c.x - a, c.y + a), (c.x + a, c.y - a)),
            );
        }

        if area.show_menu {
            // Hamburger: three bars on the strip's right edge. Drawn last
            // so it sits above any tab that ran long.
            let menu = panel_menu_rect(area.tab_strip, theme);
            scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &menu);
            paint_hamburger(scene, menu, theme.text_dim);
        }

        // Collapse-to-icons («) on an ordinary group, or expand-from-icons
        // (») on a flyout's own strip — same drawn-last-so-it-wins spot.
        let collapse = collapse_rect(area.tab_strip, area.show_menu, theme);
        scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &collapse);
        paint_chevrons(scene, collapse, theme.text_dim, !area.is_flyout);

        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &area.bounds);
    }

    for sp in &layout.splitters {
        scene.fill(Fill::NonZero, ID, theme.splitter, None, &sp.rect);
    }
}

/// Overlay the Illustrator-style blue insertion indicator for `target`.
/// Call after [`paint`], while a drag is live.
pub fn paint_drop(
    scene: &mut Scene,
    target: &DropTarget,
    layout: &Layout,
    root: Rect,
    theme: &Theme,
) {
    match target {
        DropTarget::Float => {}
        DropTarget::Split { path, side } => {
            let Some(r) = rect_for_path(path, layout, root) else {
                return;
            };
            scene.fill(
                Fill::NonZero,
                ID,
                theme.drop_line,
                None,
                &edge_line(r, *side),
            );
        }
        DropTarget::Tab { path, index } => {
            let Some(area) = layout.areas.iter().find(|a| a.path == *path) else {
                return;
            };
            let x = if *index == 0 {
                area.tab_strip.x0
            } else if let Some(prev) = area.tabs.get(index - 1) {
                prev.rect.x1
            } else {
                area.tabs
                    .last()
                    .map(|t| t.rect.x1)
                    .unwrap_or(area.tab_strip.x0)
            };
            let caret = Rect::new(x - 1.5, area.tab_strip.y0, x + 1.5, area.tab_strip.y1);
            scene.fill(Fill::NonZero, ID, theme.drop_line, None, &caret);
        }
    }
}

fn rect_for_path(path: &NodePath, layout: &Layout, root: Rect) -> Option<Rect> {
    if path.0.is_empty() {
        return Some(root);
    }
    layout
        .areas
        .iter()
        .find(|a| a.path == *path)
        .map(|a| a.bounds)
}

fn paint_hamburger(scene: &mut Scene, r: Rect, color: vello::peniko::Color) {
    let c = r.center();
    let half = 5.5;
    let gap = 3.4;
    let stroke = Stroke::new(1.4);
    for i in [-1, 0, 1] {
        let y = c.y + i as f64 * gap;
        scene.stroke(
            &stroke,
            ID,
            color,
            None,
            &Line::new((c.x - half, y), (c.x + half, y)),
        );
    }
}

/// A double-chevron glyph — `«` (`pointing_left = true`, "Collapse to
/// Icons") or `»` (`pointing_left = false`, "Expand from Icons").
fn paint_chevrons(scene: &mut Scene, r: Rect, color: vello::peniko::Color, pointing_left: bool) {
    let c = r.center();
    let (dx, half) = (3.0_f64, 3.5_f64);
    let sign = if pointing_left { 1.0 } else { -1.0 };
    let stroke = Stroke::new(1.4);
    for i in [-1.0_f64, 1.0] {
        let cx = c.x + i * dx;
        scene.stroke(
            &stroke,
            ID,
            color,
            None,
            &Line::new((cx + sign * half * 0.6, c.y - half), (cx - sign * half * 0.6, c.y)),
        );
        scene.stroke(
            &stroke,
            ID,
            color,
            None,
            &Line::new((cx - sign * half * 0.6, c.y), (cx + sign * half * 0.6, c.y + half)),
        );
    }
}

/// Paints a rail's icon strip: one row per group nested in a collapsed
/// column, that group's active tab's label, and a highlight when that
/// row's flyout is open. `label` is the same tab-caption function
/// [`paint`] takes.
pub fn paint_icon_col(
    scene: &mut Scene,
    col: Rect,
    icons: &[IconColumn],
    icon_rects: &[IconRect],
    open: Option<(usize, usize)>,
    theme: &Theme,
    text: &mut TextContext,
    label: &dyn Fn(PanelId) -> String,
) {
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &col);
    for ir in icon_rects {
        let Some(rows) = icons.get(ir.column).map(IconColumn::rows) else {
            continue;
        };
        let Some((panels, active)) = rows.get(ir.row) else {
            continue;
        };
        let is_open = open == Some((ir.column, ir.row));
        if is_open {
            scene.fill(Fill::NonZero, ID, theme.strip_active, None, &ir.rect);
        }
        // Label by the group's *active* tab, not just its first — a
        // "Character/Paragraph" group with Paragraph active should say
        // "Paragraph" on the icon, matching what the flyout opens to.
        let shown = panels.get(*active).or_else(|| panels.first());
        if let Some(&shown) = shown {
            let color = if is_open { theme.text } else { theme.text_dim };
            let baseline = ir.rect.y0 + ir.rect.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
            text.draw(scene, &label(shown), TAB_TEXT_PX, color, ir.rect.x0 + 8.0, baseline);
        }
        let sep = Rect::new(ir.rect.x0, ir.rect.y1 - 0.5, ir.rect.x1, ir.rect.y1 + 0.5);
        scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
    }
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &col);
}

fn edge_line(r: Rect, side: Side) -> Rect {
    let t = 3.0;
    match side {
        Side::Left => Rect::new(r.x0, r.y0, r.x0 + t, r.y1),
        Side::Right => Rect::new(r.x1 - t, r.y0, r.x1, r.y1),
        Side::Top => Rect::new(r.x0, r.y0, r.x1, r.y0 + t),
        Side::Bottom => Rect::new(r.x0, r.y1 - t, r.x1, r.y1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vello::kurbo::Rect;

    #[test]
    fn hamburger_sits_on_the_right_of_the_tab_strip() {
        let theme = Theme::default();
        let strip = Rect::new(10.0, 20.0, 210.0, 47.3);
        let m = panel_menu_rect(strip, &theme);
        assert_eq!(m.x1, strip.x1);
        assert_eq!(m.y0, strip.y0);
        assert_eq!(m.y1, strip.y1);
        assert!((m.width() - theme.panel_menu_w).abs() < 1e-9);
    }
}
