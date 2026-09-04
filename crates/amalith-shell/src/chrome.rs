//! Draws a [`Layout`] into a vello [`Scene`]: panel bodies, tab strips,
//! tab labels, splitters, and the drop indicator.

use vello::kurbo::{Affine, Circle, Line, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::dock::{DropTarget, NodePath, PanelId, Side};
use crate::layout::{IconRect, Layout, PanelArea};
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

/// The hamburger button on the right of a panel group's tab strip. Stays
/// on the tab strip (not the title bar) — it's a per-*tab* menu (the
/// active tab's own flyout), unlike close/collapse which act on the
/// whole group.
pub fn panel_menu_rect(tab_strip: Rect, theme: &Theme) -> Rect {
    Rect::new(
        tab_strip.x1 - theme.panel_menu_w,
        tab_strip.y0,
        tab_strip.x1,
        tab_strip.y1,
    )
}

/// The close (×) button at the left end of a group's title bar — closes
/// every tab the group holds (see `App::close_group`), matching
/// Illustrator's own title bar. Distinct from a single tab's own × (see
/// `panel_tab_close_rect`), which closes only that one tab.
pub fn group_close_rect(title_bar: Rect, theme: &Theme) -> Rect {
    Rect::new(title_bar.x0, title_bar.y0, title_bar.x0 + theme.group_close_w, title_bar.y1)
}

/// The collapse-to-icons («/») button at the right end of a group's title
/// bar — shown only on the group that owns the control (see
/// `is_column_top`), everything below it in the same column sharing that
/// one button's effect.
pub fn collapse_rect(title_bar: Rect, theme: &Theme) -> Rect {
    Rect::new(title_bar.x1 - theme.panel_collapse_w, title_bar.y0, title_bar.x1, title_bar.y1)
}

/// Is `area` the topmost group in its column — the one whose strip should
/// show the «/» button? Collapsing acts on a whole column at once (see
/// `dock::Rail::collapse`), so only the column's top group needs the
/// control at all; every group stacked below it shares that one button's
/// effect instead of repeating it. "Same column" here means "some other
/// area's horizontal span overlaps this one's and sits above it" — cheap
/// to check directly off the already-laid-out rects, no tree walk needed.
pub fn is_column_top(area: &PanelArea, all: &[PanelArea]) -> bool {
    !all.iter().any(|other| {
        other.bounds.y0 < area.bounds.y0 - 0.5
            && other.bounds.x0 < area.bounds.x1
            && other.bounds.x1 > area.bounds.x0
    })
}

/// Paint every group and splitter in `layout`. `label(panel)` supplies the
/// tab caption; `text` rasterizes it. `show_collapse`, together with
/// [`is_column_top`], gates the «/» button on each group's title bar —
/// callers pass `true` for both a rail (an attached group only shows it
/// at the top of its column) and a floating window (its one group is
/// trivially "the top of its own column", so it always qualifies).
pub fn paint(
    scene: &mut Scene,
    layout: &Layout,
    theme: &Theme,
    text: &mut TextContext,
    show_collapse: bool,
    label: &dyn Fn(PanelId) -> String,
) {
    for area in &layout.areas {
        scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &area.body);

        // Title bar: the whole-group handle, fully separate from the tab
        // strip below it — close (×) always, collapse («/») only on the
        // group that owns the control, both live here rather than woven
        // into per-tab chrome.
        scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &area.title_bar);
        let close = group_close_rect(area.title_bar, theme);
        paint_x(scene, close, theme.text_dim, 3.5);
        if show_collapse && (area.is_flyout || is_column_top(area, &layout.areas)) {
            let collapse = collapse_rect(area.title_bar, theme);
            paint_chevrons(scene, collapse, theme.text_dim, !area.is_flyout);
        }
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &area.title_bar);

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
            // Close (×): right-aligned — this tab only, unlike the title
            // bar's close which takes the whole group.
            paint_x(scene, panel_tab_close_rect(tab.rect), color, 3.5);
        }

        if area.show_menu {
            // Hamburger: three bars on the strip's right edge. Drawn last
            // so it sits above any tab that ran long.
            let menu = panel_menu_rect(area.tab_strip, theme);
            scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &menu);
            paint_hamburger(scene, menu, theme.text_dim);
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

/// A close ("×") glyph centered in `r`, arm half-length `a`.
fn paint_x(scene: &mut Scene, r: Rect, color: vello::peniko::Color, a: f64) {
    let c = r.center();
    let stroke = Stroke::new(1.4);
    scene.stroke(&stroke, ID, color, None, &Line::new((c.x - a, c.y - a), (c.x + a, c.y + a)));
    scene.stroke(&stroke, ID, color, None, &Line::new((c.x - a, c.y + a), (c.x + a, c.y - a)));
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

/// Paints a rail's icon strip: one row per *tab* nested in a collapsed
/// column (every tab in a group gets its own icon — see
/// `dock::IconColumn::icon_rows` — not just the group's active one), its
/// label (dropped once the strip is dragged narrower than
/// [`crate::layout::ICON_LABEL_THRESHOLD`] — Illustrator has no separate
/// on/off switch for this, just width), and a highlight both for the
/// group's actual active tab and for whichever row's flyout is open.
/// `label` is the same tab-caption function [`paint`] takes.
pub fn paint_icon_col(
    scene: &mut Scene,
    col: Rect,
    icon_rects: &[IconRect],
    open: Option<(usize, usize)>,
    theme: &Theme,
    text: &mut TextContext,
    label: &dyn Fn(PanelId) -> String,
) {
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &col);
    let labeled = col.width() >= crate::layout::ICON_LABEL_THRESHOLD;
    for (i, ir) in icon_rects.iter().enumerate() {
        // A new group's first tab (skip the very first row overall, which
        // has no group above it to separate from): a full-width divider
        // plus a small drag-handle, so a column of several collapsed
        // groups still reads as distinct, individually grabbable groups
        // rather than one flat list of icons.
        if i > 0 && ir.tab == 0 {
            paint_group_handle(scene, col, ir.rect.y0, theme);
        }
        let is_open = open == Some((ir.column, ir.group));
        if is_open && ir.active {
            scene.fill(Fill::NonZero, ID, theme.strip_active, None, &ir.rect);
        }
        // The group's actual active tab reads full-strength even when its
        // flyout isn't open; the rest of that group's tabs (and anything
        // in a closed group) stay dim — matching a docked tab strip's own
        // active/inactive contrast.
        let color = if ir.active { theme.text } else { theme.text_dim };
        let icon_box = Rect::new(
            ir.rect.x0 + 8.0,
            ir.rect.y0 + (ir.rect.height() - 18.0) * 0.5,
            ir.rect.x0 + 26.0,
            ir.rect.y0 + (ir.rect.height() + 18.0) * 0.5,
        );
        let icon_box = if labeled {
            icon_box
        } else {
            // Icon-only: center the glyph in the whole row instead of
            // hugging the left edge meant for the label to follow.
            let c = ir.rect.center();
            Rect::new(c.x - 9.0, c.y - 9.0, c.x + 9.0, c.y + 9.0)
        };
        crate::panel_icon::draw(scene, ir.panel, icon_box, color);
        if labeled {
            let baseline = ir.rect.y0 + ir.rect.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
            text.draw(scene, &label(ir.panel), TAB_TEXT_PX, color, icon_box.x1 + 6.0, baseline);
        }
    }
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &col);
}

/// The visual boundary between two collapsed groups in an icon strip: a
/// full-width divider line plus a small centered drag-handle (a row of
/// dots), sitting in the [`crate::layout::ICON_GROUP_GAP`] space above
/// `group_top_y` (the first row of the group that starts here). Purely
/// decorative — it doesn't change what's clickable, but without it nothing
/// visually distinguishes "several separate groups" from "one flat list",
/// especially once each is reduced to bare icons.
fn paint_group_handle(scene: &mut Scene, col: Rect, group_top_y: f64, theme: &Theme) {
    let gap_top = group_top_y - crate::layout::ICON_GROUP_GAP;
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(col.x0, gap_top, col.x1, gap_top + 1.0),
    );
    let cy = (gap_top + group_top_y) * 0.5 + 0.5;
    let dots = 5;
    let spacing = 5.0;
    let cx0 = col.x0 + col.width() * 0.5 - spacing * (dots - 1) as f64 * 0.5;
    for i in 0..dots {
        let c = (cx0 + spacing * i as f64, cy);
        scene.fill(Fill::NonZero, ID, theme.text_dim, None, &Circle::new(c, 1.3));
    }
}

/// Paints a collapsed *detached* panel: a persistent header (close × and
/// expand », always visible — a collapsed floating group must never be
/// left with no way back, unlike a rail's icon row which can lean on its
/// flyout) above one icon row per tab, every tab shown (not just the
/// active one, matching a docked column's own icon strip), the active
/// one highlighted. `header` and `rows` come from
/// [`crate::layout::floating_collapsed_rows`].
pub fn paint_floating_collapsed(
    scene: &mut Scene,
    header: Rect,
    rows: &[Rect],
    node: &crate::dock::Node,
    labeled: bool,
    theme: &Theme,
    text: &mut TextContext,
    label: &dyn Fn(PanelId) -> String,
) {
    let bottom = rows.last().map_or(header.y1, |r| r.y1);
    let bounds = Rect::new(header.x0, header.y0, header.x1, bottom);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &bounds);

    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &header);
    paint_x(scene, group_close_rect(header, theme), theme.text_dim, 3.5);
    paint_chevrons(scene, collapse_rect(header, theme), theme.text_dim, false);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &header);

    let crate::dock::Node::Tabs { panels, active } = node else {
        return;
    };
    for (i, (&r, &panel)) in rows.iter().zip(panels.iter()).enumerate() {
        let is_active = i == *active;
        if is_active {
            scene.fill(Fill::NonZero, ID, theme.strip_active, None, &r);
        }
        let color = if is_active { theme.text } else { theme.text_dim };
        let icon_box = if labeled {
            Rect::new(r.x0 + 8.0, r.y0 + (r.height() - 18.0) * 0.5, r.x0 + 26.0, r.y0 + (r.height() + 18.0) * 0.5)
        } else {
            let c = r.center();
            Rect::new(c.x - 9.0, c.y - 9.0, c.x + 9.0, c.y + 9.0)
        };
        crate::panel_icon::draw(scene, panel, icon_box, color);
        if labeled {
            let baseline = r.y0 + r.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
            text.draw(scene, &label(panel), TAB_TEXT_PX, color, icon_box.x1 + 6.0, baseline);
        }
        let sep = Rect::new(r.x0, r.y1 - 0.5, r.x1, r.y1 + 0.5);
        scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
    }
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &bounds);
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

    fn area_at(bounds: Rect) -> PanelArea {
        PanelArea {
            path: NodePath(Vec::new()),
            bounds,
            title_bar: bounds,
            tab_strip: bounds,
            body: bounds,
            tabs: Vec::new(),
            active: 0,
            show_menu: false,
            is_flyout: false,
        }
    }

    #[test]
    fn only_the_top_of_a_stacked_column_shows_the_collapse_button() {
        // Three groups stacked in one column (same x-range, increasing y) —
        // matching Illustrator's own chrome, only the first should count.
        let top = area_at(Rect::new(0.0, 0.0, 300.0, 100.0));
        let middle = area_at(Rect::new(0.0, 100.0, 300.0, 200.0));
        let bottom = area_at(Rect::new(0.0, 200.0, 300.0, 300.0));
        let all = [top.clone(), middle.clone(), bottom.clone()];
        assert!(is_column_top(&top, &all));
        assert!(!is_column_top(&middle, &all));
        assert!(!is_column_top(&bottom, &all));
    }

    #[test]
    fn side_by_side_columns_each_get_their_own_top() {
        // Two columns side by side (disjoint x-ranges) — each is its own
        // column and each counts as its own top, independent of the other.
        let left = area_at(Rect::new(0.0, 0.0, 150.0, 100.0));
        let right = area_at(Rect::new(150.0, 0.0, 300.0, 100.0));
        let all = [left.clone(), right.clone()];
        assert!(is_column_top(&left, &all));
        assert!(is_column_top(&right, &all));
    }

    #[test]
    fn paint_icon_col_draws_a_group_handle_between_groups_without_panicking() {
        // Two collapsed groups in one column — the handle/divider between
        // them (drawn at each group's first row, skipping the very first
        // row overall) shouldn't panic on a real IconRect list, narrow or
        // wide (icon-only vs. labeled).
        use crate::dock::{IconColumn, Node};
        use crate::layout::layout_icons;
        let icons = vec![
            IconColumn { node: Node::Tabs { panels: vec![PanelId("swatches")], active: 0 } },
            IconColumn { node: Node::Tabs { panels: vec![PanelId("gradient")], active: 0 } },
        ];
        let mut text = TextContext::new();
        for w in [40.0, 112.0] {
            let col = Rect::new(0.0, 0.0, w, 400.0);
            let rects = layout_icons(&icons, col);
            assert_eq!(rects.len(), 2, "one row per single-tab group");
            let mut scene = Scene::new();
            paint_icon_col(&mut scene, col, &rects, None, &Theme::default(), &mut text, &|p| p.0.to_string());
        }
    }
}
