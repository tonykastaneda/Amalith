//! Draws a [`Layout`] into a vello [`Scene`]: panel bodies, tab strips,
//! tab labels, splitters, and the drop indicator.

use vello::kurbo::{Affine, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::dock::{DropTarget, NodePath, PanelId, Side};
use crate::layout::Layout;
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
                &vello::kurbo::Line::new((c.x - a, c.y - a), (c.x + a, c.y + a)),
            );
            scene.stroke(
                &Stroke::new(1.4),
                ID,
                xcol,
                None,
                &vello::kurbo::Line::new((c.x - a, c.y + a), (c.x + a, c.y - a)),
            );
        }

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

fn edge_line(r: Rect, side: Side) -> Rect {
    let t = 3.0;
    match side {
        Side::Left => Rect::new(r.x0, r.y0, r.x0 + t, r.y1),
        Side::Right => Rect::new(r.x1 - t, r.y0, r.x1, r.y1),
        Side::Top => Rect::new(r.x0, r.y0, r.x1, r.y0 + t),
        Side::Bottom => Rect::new(r.x0, r.y1 - t, r.x1, r.y1),
    }
}
