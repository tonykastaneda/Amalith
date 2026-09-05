//! Master/Group/Panel geometry: turn a [`Master`] + a rect into concrete
//! rectangles, and turn a rect + cursor into a drop target.
//!
//! Pure math — no vello, no winit. Ported off the sizing/hit-testing
//! logic in the HTML/CSS/JS reference (`amalith-panelSys/app.js`); doc
//! comments name the JS function each Rust one replaces.

use vello::kurbo::{Point, Rect};

use crate::dock::{Master, MasterLayout, PanelId, Side, TAB_CONTENT_MAX_H, TAB_CONTENT_MIN_H};
use crate::theme::Theme;

/// Floating/docked Master width bounds (⇐ `MASTER_MIN_W`/`MASTER_MAX_W`).
pub const MASTER_MIN_W: f64 = 160.0;
pub const MASTER_MAX_W: f64 = 720.0;
/// A Tools master clamps narrower (⇐ `TOOLS_MIN_W`).
pub const TOOLS_MIN_W: f64 = 48.0;
/// Below this width a master's Stack-mode rows drop their labels (⇐
/// `COMPACT_BREAKPOINT`).
pub const COMPACT_BREAKPOINT: f64 = 240.0;
/// Edge band, from the viewport's left/right edge, that always docks
/// regardless of what's under the cursor (⇐ `DOCK_EDGE`).
pub const DOCK_EDGE: f64 = 36.0;
/// Half-width band around an already-docked master's own edge that also
/// docks (a "seam"), even away from the viewport edge (⇐ the `± 10`
/// checks in `resolveDockTarget`).
pub const DOCK_SEAM: f64 = 10.0;
/// Height of one Stack-mode panel row (icon + label).
pub const STACK_ROW_H: f64 = 34.0;
/// Height of a Master's own header (× and the chevron need real room).
pub const HEADER_H: f64 = 15.0;
/// Height of a Group's plain drag handle — just a thin grip strip, much
/// shorter than the Master's own header (it carries no controls of its
/// own, only the light-blue pill drawn inside it).
pub const GROUP_HANDLE_H: f64 = 10.0;

/// One panel row in Stack-mode display.
#[derive(Clone, Copy, Debug)]
pub struct PanelRow {
    pub panel: PanelId,
    pub rect: Rect,
}

/// One tab in Tabs-mode display.
#[derive(Clone, Copy, Debug)]
pub struct TabRect {
    pub panel: PanelId,
    pub rect: Rect,
}

/// One Group's laid-out geometry within its Master's body.
#[derive(Clone, Debug)]
pub struct GroupArea {
    pub index: usize,
    /// The plain light-blue drag/detach handle — no close/collapse of its
    /// own (⇐ the prototype's `.group-header`, just the grip pill).
    pub handle: Rect,
    /// Handle + whatever's below it.
    pub bounds: Rect,
    /// This group's active tab/row index (⇐ `Group::active`).
    pub active: usize,
    /// Stack mode: one row per panel. Empty in Tabs mode.
    pub rows: Vec<PanelRow>,
    /// Tabs mode: the tab strip. Zero-height in Stack mode.
    pub tab_strip: Rect,
    pub tabs: Vec<TabRect>,
    /// Tabs mode: the active tab's content pane. Zero-height in Stack
    /// mode.
    pub content: Rect,
    /// Tabs mode: the grip at the content pane's bottom edge, drag to
    /// resize (⇐ `.tab-content-resize`).
    pub resize_handle: Rect,
}

/// A Master's full laid-out geometry.
#[derive(Clone, Debug, Default)]
pub struct MasterFrame {
    pub bounds: Rect,
    pub header: Rect,
    pub close: Rect,
    /// Stack⇄Tabs toggle for a Normal master. Meaningless for Tools (its
    /// column count already reflows with width via `panels::tools`, no
    /// button needed).
    pub chevron: Rect,
    pub body: Rect,
    /// Where the last group's content actually ends (`body.y0` if there
    /// are none) — the shrink-wrapped extent a *floating* master's OS
    /// window height should track. Unlike `body`, this never exceeds what
    /// the groups actually need, regardless of how tall `bounds` was.
    pub content_bottom: f64,
    pub groups: Vec<GroupArea>,
    /// `true` when `bounds.width() < COMPACT_BREAKPOINT` — Stack-mode
    /// rows drop their labels, icon-only (⇐ `.compact`).
    pub compact: bool,
}

impl Default for GroupArea {
    fn default() -> Self {
        Self {
            index: 0,
            handle: Rect::ZERO,
            bounds: Rect::ZERO,
            active: 0,
            rows: Vec::new(),
            tab_strip: Rect::ZERO,
            tabs: Vec::new(),
            content: Rect::ZERO,
            resize_handle: Rect::ZERO,
        }
    }
}

/// Lays a Normal master's groups out top-down inside `bounds`, starting
/// right after its header. `tab_width` sizes each tab (mutates the font
/// cache, hence `&mut`). Natural, ungoverned height: nothing here clips
/// or scales groups to fit — that's `App`'s job (scroll a docked column,
/// or resize a floating window's OS-level height to match — see
/// `natural_height`).
pub fn layout_master(
    master: &Master,
    bounds: Rect,
    theme: &Theme,
    tab_width: &mut dyn FnMut(PanelId) -> f64,
) -> MasterFrame {
    let header = Rect::new(bounds.x0, bounds.y0, bounds.x1, (bounds.y0 + HEADER_H).min(bounds.y1));
    let close = Rect::new(header.x0, header.y0, header.x0 + 26.0, header.y1);
    let chevron = Rect::new(header.x1 - 26.0, header.y0, header.x1, header.y1);
    let body = Rect::new(bounds.x0, header.y1, bounds.x1, bounds.y1);
    let compact = bounds.width() < COMPACT_BREAKPOINT;

    let mut groups = Vec::with_capacity(master.groups.len());
    let mut y = body.y0;
    for (i, g) in master.groups.iter().enumerate() {
        let handle = Rect::new(body.x0, y, body.x1, (y + GROUP_HANDLE_H).min(body.y1));
        let mut area = GroupArea { index: i, handle, active: g.active, ..GroupArea::default() };
        match master.layout {
            MasterLayout::Stack => {
                let mut ry = handle.y1;
                for &panel in &g.panels {
                    let r = Rect::new(body.x0, ry, body.x1, ry + STACK_ROW_H);
                    area.rows.push(PanelRow { panel, rect: r });
                    ry += STACK_ROW_H;
                }
                area.bounds = Rect::new(body.x0, y, body.x1, ry);
                y = ry;
            }
            MasterLayout::Tabs => {
                let strip_y1 = handle.y1 + theme.tab_strip_h;
                area.tab_strip = Rect::new(body.x0, handle.y1, body.x1, strip_y1);
                let mut x = body.x0;
                for &panel in &g.panels {
                    let w = tab_width(panel).max(8.0);
                    area.tabs.push(TabRect { panel, rect: Rect::new(x, area.tab_strip.y0, x + w, strip_y1) });
                    x += w;
                }
                // Spawns at the active panel's own natural content height —
                // the user only pins an explicit height once they've
                // actually dragged the resize grip (`g.content_h`).
                let natural = g
                    .panels
                    .get(g.active)
                    .map_or(crate::dock::TAB_CONTENT_DEFAULT_H as f64, |&p| {
                        crate::panels::min_body_height(p, body.width())
                    });
                let content_h = g
                    .content_h
                    .map_or(natural, |h| h as f64)
                    .clamp(TAB_CONTENT_MIN_H as f64, TAB_CONTENT_MAX_H as f64);
                area.content = Rect::new(body.x0, strip_y1, body.x1, strip_y1 + content_h);
                area.resize_handle = Rect::new(body.x0, area.content.y1, body.x1, area.content.y1 + 6.0);
                area.bounds = Rect::new(body.x0, y, body.x1, area.resize_handle.y1);
                y = area.resize_handle.y1;
            }
        }
        groups.push(area);
    }

    MasterFrame {
        bounds,
        header,
        close,
        chevron,
        // The *full* area below the header, not shrunk to content — a
        // docked master's rect is the whole rail height regardless of how
        // little its groups actually need, and empty space below the
        // last group must still accept a "drop as new group" (⇐
        // `hit_test_panel_drop`'s fallback) the same as any other gap in
        // the body would. `content_bottom` below is the shrink-wrapped
        // extent `natural_height` needs instead.
        body,
        content_bottom: y.max(body.y0),
        groups,
        compact,
    }
}

/// A Master's natural (unclipped) content height at `width` — header plus
/// every group's own height — used to keep a *floating* master's OS
/// window in sync with its content (winit windows don't auto-size like a
/// `div`; see `App`'s callers). `tab_width`/`theme` as in
/// [`layout_master`].
pub fn natural_height(
    master: &Master,
    width: f64,
    theme: &Theme,
    tab_width: &mut dyn FnMut(PanelId) -> f64,
) -> f64 {
    let probe = Rect::new(0.0, 0.0, width, f64::MAX / 2.0);
    layout_master(master, probe, theme, tab_width).content_bottom
}

/// Where dragging a panel over an already-laid-out master's body would
/// land (⇐ `updateDropTarget`'s `drag.type === "panel"` branch).
#[derive(Clone, Debug, PartialEq)]
pub enum PanelDrop {
    /// Insert into an existing group's tab/row list at `at`.
    IntoGroup { group: usize, at: usize },
    /// Wrap it in a brand-new group inserted into the master's body at
    /// `at`.
    NewGroup { at: usize },
}

pub fn hit_test_panel_drop(frame: &MasterFrame, p: Point) -> Option<PanelDrop> {
    for g in &frame.groups {
        if g.tab_strip.contains(p) {
            let at = g.tabs.iter().position(|t| p.x < t.rect.center().x).unwrap_or(g.tabs.len());
            return Some(PanelDrop::IntoGroup { group: g.index, at });
        }
        if g.content.contains(p) || g.rows.iter().any(|r| r.rect.contains(p)) {
            let at = g.rows.iter().position(|r| p.y < r.rect.center().y).unwrap_or(g.rows.len());
            return Some(PanelDrop::IntoGroup { group: g.index, at });
        }
    }
    if frame.body.contains(p) {
        // Not over any group — lands as a new group at the position
        // implied by which existing group it's nearest (⇐ `placePlaceholder`
        // picking an insertion index by comparing midpoints).
        let at = frame
            .groups
            .iter()
            .position(|g| p.y < g.bounds.center().y)
            .unwrap_or(frame.groups.len());
        return Some(PanelDrop::NewGroup { at });
    }
    None
}

/// Where dragging a whole group over an already-laid-out master's body
/// would land (⇐ `updateDropTarget`'s `drag.type === "group"` branch).
#[derive(Clone, Debug, PartialEq)]
pub enum GroupDrop {
    /// Merge into the panel list of the group at `group`.
    MergeInto { group: usize },
    /// Become a new sibling group at `at`.
    NewSibling { at: usize },
}

pub fn hit_test_group_drop(frame: &MasterFrame, dragging: usize, p: Point) -> Option<GroupDrop> {
    for g in &frame.groups {
        if g.index == dragging {
            continue;
        }
        if g.tab_strip.contains(p) || g.content.contains(p) || g.rows.iter().any(|r| r.rect.contains(p)) {
            return Some(GroupDrop::MergeInto { group: g.index });
        }
    }
    if frame.body.contains(p) {
        let at = frame
            .groups
            .iter()
            .filter(|g| g.index != dragging)
            .position(|g| p.y < g.bounds.center().y)
            .unwrap_or(frame.groups.len());
        return Some(GroupDrop::NewSibling { at });
    }
    None
}

/// Where a dragged master's cursor position resolves to a dock spot (⇐
/// `resolveDockTarget`). `left`/`right` are the masters already docked to
/// each side, in order, by width — the caller excludes the one being
/// dragged. Checks the viewport edge bands first, then a ± [`DOCK_SEAM`]
/// window around any already-docked master's own edge.
pub fn resolve_dock_target(
    cursor_x: f64,
    viewport_w: f64,
    left: &[f64],
    right: &[f64],
) -> Option<(Side, usize, bool)> {
    if cursor_x <= DOCK_EDGE {
        return Some((Side::Left, insert_index_for(left, cursor_x), false));
    }
    if cursor_x >= viewport_w - DOCK_EDGE {
        return Some((Side::Right, insert_index_for_right(right, viewport_w, cursor_x), false));
    }

    // Left side seams: each docked master occupies [off, off+w) from x=0.
    let mut off = 0.0;
    for (i, &w) in left.iter().enumerate() {
        if (cursor_x - off).abs() <= DOCK_SEAM {
            return Some((Side::Left, i, true));
        }
        if (cursor_x - (off + w)).abs() <= DOCK_SEAM {
            return Some((Side::Left, i + 1, true));
        }
        off += w;
    }
    if !left.is_empty() && cursor_x > off && cursor_x < off + DOCK_EDGE {
        return Some((Side::Left, left.len(), true));
    }

    // Right side seams: each docked master occupies [viewport_w-off-w,
    // viewport_w-off) from the right edge inward.
    let mut off = 0.0;
    for (i, &w) in right.iter().enumerate() {
        let inner = viewport_w - off - w;
        let outer = viewport_w - off;
        if (cursor_x - outer).abs() <= DOCK_SEAM {
            return Some((Side::Right, i, true));
        }
        if (cursor_x - inner).abs() <= DOCK_SEAM {
            return Some((Side::Right, i + 1, true));
        }
        off += w;
    }
    if !right.is_empty() {
        let innermost = viewport_w - off;
        if cursor_x < innermost && cursor_x > innermost - DOCK_EDGE {
            return Some((Side::Right, right.len(), true));
        }
    }

    None
}

fn insert_index_for(widths: &[f64], cursor_x: f64) -> usize {
    let mut off = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        if cursor_x < off + w * 0.5 {
            return i;
        }
        off += w;
    }
    widths.len()
}

fn insert_index_for_right(widths: &[f64], viewport_w: f64, cursor_x: f64) -> usize {
    let mut off = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        let mid = viewport_w - off - w * 0.5;
        if cursor_x > mid {
            return i;
        }
        off += w;
    }
    widths.len()
}

/// `true` when `cursor_x` sits in either viewport edge band (⇐
/// `isPureDockEdge`) — used to prefer edge-docking over merging into
/// whatever master happens to be under the cursor there.
pub fn is_pure_dock_edge(cursor_x: f64, viewport_w: f64) -> bool {
    cursor_x <= DOCK_EDGE || cursor_x >= viewport_w - DOCK_EDGE
}

/// x-offset of the `index`-th docked master on `side`, given the widths
/// of every other master already docked there (⇐ the offset accumulation
/// in `layoutDocks`/`showDockPreview`).
pub fn dock_offset(widths: &[f64], index: usize) -> f64 {
    widths.iter().take(index).sum()
}

/// Width/height of a Stack-mode row's flyout preview (⇐ `openPanelFlyout`'s
/// `fw`/`fh`).
pub const FLYOUT_W: f64 = 280.0;
pub const FLYOUT_H: f64 = 220.0;

/// Where a Stack-mode row's flyout preview should sit: beside `row`,
/// flipping to the other side if it would run off `viewport`'s right
/// edge, and clamped fully inside `viewport` either way (⇐
/// `openPanelFlyout`'s positioning math).
pub fn flyout_rect(row: Rect, viewport: Rect) -> Rect {
    let mut left = row.x1 + 10.0;
    if left + FLYOUT_W > viewport.x1 - 12.0 {
        left = row.x0 - FLYOUT_W - 10.0;
    }
    left = left.max(viewport.x0 + 8.0);
    let mut top = row.y0;
    if top + FLYOUT_H > viewport.y1 - 12.0 {
        top = (viewport.y1 - FLYOUT_H - 12.0).max(viewport.y0 + 8.0);
    }
    top = top.max(viewport.y0 + 8.0);
    Rect::new(left, top, left + FLYOUT_W, top + FLYOUT_H)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::{Group, MasterKind};

    const A: PanelId = PanelId("a");
    const B: PanelId = PanelId("b");
    const C: PanelId = PanelId("c");

    fn theme() -> Theme {
        Theme::default()
    }

    fn w80(_: PanelId) -> f64 {
        80.0
    }

    fn master(layout: MasterLayout, groups: Vec<Vec<PanelId>>) -> Master {
        Master {
            id: 1,
            kind: MasterKind::Normal,
            layout,
            tools_density: crate::dock::ToolsDensity::Grid2x,
            groups: groups
                .into_iter()
                .enumerate()
                .map(|(i, panels)| Group::new(i as u64, panels))
                .collect(),
            dock: None,
            rect: [0.0, 0.0, 280.0, 400.0],
            scroll: 0.0,
        }
    }

    #[test]
    fn stack_mode_stacks_one_row_per_panel_across_every_group() {
        let m = master(MasterLayout::Stack, vec![vec![A, B], vec![C]]);
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
        assert_eq!(frame.groups.len(), 2);
        assert_eq!(frame.groups[0].rows.len(), 2);
        assert_eq!(frame.groups[1].rows.len(), 1);
        // Second group starts below the first group's handle + 2 rows.
        assert_eq!(frame.groups[1].handle.y0, frame.groups[0].bounds.y1);
        // Rows stack top-down with no gap.
        assert_eq!(frame.groups[0].rows[1].rect.y0, frame.groups[0].rows[0].rect.y1);
    }

    #[test]
    fn tabs_mode_reserves_a_strip_then_a_clamped_content_pane() {
        let mut m = master(MasterLayout::Tabs, vec![vec![A, B]]);
        m.groups[0].content_h = Some(999_999.0); // clamps down to MAX
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
        let g = &frame.groups[0];
        assert_eq!(g.tabs.len(), 2);
        assert_eq!(g.tabs[0].rect.x0, 0.0);
        assert_eq!(g.tabs[1].rect.x0, 80.0);
        assert!(
            (g.content.height() - TAB_CONTENT_MAX_H as f64).abs() < 1e-9,
            "got {}",
            g.content.height()
        );
        // Resize handle sits right under the content pane.
        assert_eq!(g.resize_handle.y0, g.content.y1);
    }

    #[test]
    fn natural_height_matches_the_body_bottom_a_giant_probe_rect_produces() {
        let m = master(MasterLayout::Stack, vec![vec![A]]);
        let h = natural_height(&m, 280.0, &theme(), &mut w80);
        assert_eq!(h, HEADER_H + GROUP_HANDLE_H + STACK_ROW_H);
    }

    #[test]
    fn compact_flips_once_width_drops_below_the_breakpoint() {
        let m = master(MasterLayout::Stack, vec![vec![A]]);
        let wide = layout_master(&m, Rect::new(0.0, 0.0, 300.0, 400.0), &theme(), &mut w80);
        let narrow = layout_master(&m, Rect::new(0.0, 0.0, 200.0, 400.0), &theme(), &mut w80);
        assert!(!wide.compact);
        assert!(narrow.compact);
    }

    #[test]
    fn hit_test_panel_drop_over_a_tab_strip_inserts_at_the_nearest_gap() {
        let m = master(MasterLayout::Tabs, vec![vec![A, B]]);
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
        let strip_y = frame.groups[0].tab_strip.center().y;
        assert_eq!(
            hit_test_panel_drop(&frame, Point::new(10.0, strip_y)),
            Some(PanelDrop::IntoGroup { group: 0, at: 0 })
        );
        assert_eq!(
            hit_test_panel_drop(&frame, Point::new(140.0, strip_y)),
            Some(PanelDrop::IntoGroup { group: 0, at: 2 })
        );
    }

    #[test]
    fn hit_test_panel_drop_outside_every_group_but_inside_the_body_makes_a_new_group() {
        let m = master(MasterLayout::Stack, vec![vec![A]]);
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
        let below_everything = Point::new(10.0, frame.body.y1 - 1.0);
        assert_eq!(
            hit_test_panel_drop(&frame, below_everything),
            Some(PanelDrop::NewGroup { at: 1 })
        );
    }

    #[test]
    fn hit_test_group_drop_skips_the_dragged_group_itself() {
        let m = master(MasterLayout::Stack, vec![vec![A], vec![B]]);
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
        let own_row = frame.groups[0].rows[0].rect.center();
        // Dragging group 0 itself over its own row must not "merge into
        // itself" — it should fall through to a sibling-position result.
        assert_ne!(
            hit_test_group_drop(&frame, 0, own_row),
            Some(GroupDrop::MergeInto { group: 0 })
        );
    }

    #[test]
    fn resolve_dock_target_claims_the_left_edge_band() {
        let t = resolve_dock_target(6.0, 1000.0, &[], &[]);
        assert_eq!(t, Some((Side::Left, 0, false)));
    }

    #[test]
    fn resolve_dock_target_claims_the_right_edge_band() {
        let t = resolve_dock_target(996.0, 1000.0, &[], &[]);
        assert_eq!(t, Some((Side::Right, 0, false)));
    }

    #[test]
    fn resolve_dock_target_finds_a_seam_between_two_docked_masters() {
        // Two masters docked left, 200px each — a seam sits at x=200.
        let t = resolve_dock_target(203.0, 1000.0, &[200.0, 200.0], &[]);
        assert_eq!(t, Some((Side::Left, 1, true)));
    }

    #[test]
    fn resolve_dock_target_is_none_in_open_canvas() {
        assert_eq!(resolve_dock_target(500.0, 1000.0, &[], &[]), None);
    }

    #[test]
    fn dock_offset_sums_the_widths_before_index() {
        assert_eq!(dock_offset(&[100.0, 150.0, 80.0], 2), 250.0);
        assert_eq!(dock_offset(&[100.0, 150.0], 0), 0.0);
    }

    #[test]
    fn flyout_rect_sits_beside_the_row_when_there_is_room() {
        let row = Rect::new(0.0, 100.0, 240.0, 134.0);
        let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let f = flyout_rect(row, viewport);
        assert_eq!(f.x0, row.x1 + 10.0);
        assert_eq!(f.y0, row.y0);
        assert_eq!(f.width(), FLYOUT_W);
        assert_eq!(f.height(), FLYOUT_H);
    }

    #[test]
    fn flyout_rect_flips_to_the_other_side_when_it_would_run_off_the_right_edge() {
        let row = Rect::new(760.0, 100.0, 1000.0, 134.0);
        let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let f = flyout_rect(row, viewport);
        assert_eq!(f.x0, row.x0 - FLYOUT_W - 10.0);
    }

    #[test]
    fn flyout_rect_stays_clamped_inside_the_viewport_near_the_bottom() {
        let row = Rect::new(0.0, 780.0, 240.0, 814.0);
        let viewport = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let f = flyout_rect(row, viewport);
        assert!(f.y1 <= viewport.y1 - 12.0 + 1e-9);
    }
}
