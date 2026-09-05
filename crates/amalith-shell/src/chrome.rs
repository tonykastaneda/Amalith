//! Draws a [`MasterFrame`] into a vello [`Scene`]: the Master's own
//! header, every Group's plain handle, Stack-mode rows or Tabs-mode
//! strip/content, and drop indicators. Ported off
//! `amalith-panelSys/app.js`'s DOM structure — see `MasterFrame`'s own
//! doc comments for the JS-to-Rust mapping.

use vello::kurbo::{Affine, Line, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::dock::{Master, MasterLayout, PanelId};
use crate::layout::{GroupDrop, MasterFrame, PanelDrop};
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

/// The hamburger button on the right of a tab strip — a per-*tab* menu
/// (the active tab's own flyout).
pub fn panel_menu_rect(tab_strip: Rect, theme: &Theme) -> Rect {
    Rect::new(tab_strip.x1 - theme.panel_menu_w, tab_strip.y0, tab_strip.x1, tab_strip.y1)
}

/// Paint one Master: its own header (close × and a chevron — Stack⇄Tabs
/// for a Normal master, 2×/1× grid density for a Tools master), every
/// Group's plain handle, and each group's Stack-mode rows or Tabs-mode
/// strip. Doesn't paint any panel's actual *content* — that's
/// `panels::paint`'s job, into whatever rect this returns via `frame` (a
/// Tabs group's `GroupArea::content`; Stack mode has no inline content,
/// only the flyout does; a Tools master's grid is hosted directly in
/// `frame.body`, painted by the caller). `show_menu(panel)` gates the
/// hamburger per active tab; `open_flyout` marks the Stack-mode row whose
/// flyout is open with a pushed-in look.
/// A Master's header never carries a title — only its × and chevron.
/// `label`/`show_menu` are for its panels' own tab captions and
/// hamburgers, not the Master itself.
#[allow(clippy::too_many_arguments)]
pub fn paint_master(
    scene: &mut Scene,
    frame: &MasterFrame,
    master: &Master,
    theme: &Theme,
    text: &mut TextContext,
    label: &dyn Fn(PanelId) -> String,
    show_menu: &dyn Fn(PanelId) -> bool,
    open_flyout: Option<(usize, usize)>,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &frame.bounds);

    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &frame.header);
    paint_x(scene, frame.close, theme.text_dim, 3.5);
    let chevron_state = if master.is_tools() {
        master.tools_density == crate::dock::ToolsDensity::Grid1x
    } else {
        master.layout == MasterLayout::Tabs
    };
    paint_chevrons(scene, frame.chevron, theme.text_dim, chevron_state);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &frame.header);

    for g in &frame.groups {
        paint_group_handle(scene, g.handle, theme);

        match master.layout {
            MasterLayout::Stack => {
                for (i, row) in g.rows.iter().enumerate() {
                    // The row whose flyout is currently open reads as a
                    // pushed-in button — darker than the surrounding
                    // rows, no border, rather than a lifted/selected look.
                    let is_open = open_flyout == Some((g.index, i));
                    if is_open {
                        scene.fill(Fill::NonZero, ID, theme.bg, None, &row.rect);
                        let inner_shadow = Rect::new(row.rect.x0, row.rect.y0, row.rect.x1, row.rect.y0 + 2.0);
                        scene.fill(Fill::NonZero, ID, Color::from_rgba8(0, 0, 0, 60), None, &inner_shadow);
                    }
                    if i > 0 {
                        let sep = Rect::new(row.rect.x0, row.rect.y0 - 0.5, row.rect.x1, row.rect.y0 + 0.5);
                        scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
                    }
                    let color = if is_open { theme.text } else { theme.text_dim };
                    let icon_box = if frame.compact {
                        let c = row.rect.center();
                        Rect::new(c.x - 9.0, c.y - 9.0, c.x + 9.0, c.y + 9.0)
                    } else {
                        Rect::new(
                            row.rect.x0 + 10.0,
                            row.rect.y0 + (row.rect.height() - 18.0) * 0.5,
                            row.rect.x0 + 28.0,
                            row.rect.y0 + (row.rect.height() + 18.0) * 0.5,
                        )
                    };
                    crate::panel_icon::draw(scene, row.panel, icon_box, color);
                    if !frame.compact {
                        let baseline = row.rect.y0 + row.rect.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
                        text.draw(scene, &label(row.panel), TAB_TEXT_PX, color, icon_box.x1 + 8.0, baseline);
                    }
                }
            }
            MasterLayout::Tabs => {
                scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &g.tab_strip);
                for (i, tab) in g.tabs.iter().enumerate() {
                    let active = i == g.active;
                    if active {
                        scene.fill(Fill::NonZero, ID, theme.strip_active, None, &tab.rect);
                        let u = Rect::new(tab.rect.x0, tab.rect.y1 - 2.0, tab.rect.x1, tab.rect.y1);
                        scene.fill(Fill::NonZero, ID, theme.drop_line, None, &u);
                    }
                    if i > 0 {
                        let sep = Rect::new(tab.rect.x0 - 0.5, tab.rect.y0 + 4.0, tab.rect.x0 + 0.5, tab.rect.y1 - 4.0);
                        scene.fill(Fill::NonZero, ID, theme.border, None, &sep);
                    }
                    let color = if active { theme.text } else { theme.text_dim };
                    let baseline = tab.rect.y0 + tab.rect.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
                    text.draw(
                        scene,
                        &label(tab.panel),
                        TAB_TEXT_PX,
                        color,
                        tab.rect.x0 + theme.tab_pad_x * PANEL_TAB_PAD_MUL,
                        baseline,
                    );
                    paint_x(scene, panel_tab_close_rect(tab.rect), color, 3.5);
                }
                if g.tabs.get(g.active).is_some_and(|t| show_menu(t.panel)) {
                    let menu = panel_menu_rect(g.tab_strip, theme);
                    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &menu);
                    paint_hamburger(scene, menu, theme.text_dim);
                }
                scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &g.content);
                paint_resize_grip(scene, g.resize_handle, theme);
            }
        }
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &g.bounds);
    }
}

/// A Group's plain drag/detach handle — tinted light blue, no close or
/// collapse of its own (only a Master has those; see [`paint_master`]'s
/// header). Dragging it merges into another group, becomes a new sibling
/// group elsewhere, or detaches into its own new Master.
fn paint_group_handle(scene: &mut Scene, handle: Rect, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &handle);
    let c = handle.center();
    // As thin as the active-tab underline (2px) — this is a grip line,
    // not a button.
    let (w, h) = ((handle.width() * 0.36).clamp(24.0, 160.0), 2.5);
    let pill = Rect::new(c.x - w * 0.5, c.y - h * 0.5, c.x + w * 0.5, c.y + h * 0.5).to_rounded_rect(h * 0.5);
    scene.fill(Fill::NonZero, ID, theme.accent, None, &pill);
}

/// The grip line at a Tabs-mode content pane's bottom edge — drag to
/// resize (⇐ `.tab-content-resize`).
fn paint_resize_grip(scene: &mut Scene, r: Rect, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &r);
    let c = r.center();
    scene.stroke(&Stroke::new(1.5), ID, theme.text_dim.with_alpha(0.6), None, &Line::new((c.x - 12.0, c.y), (c.x + 12.0, c.y)));
}

/// Live drop cue while dragging a panel over an already-laid-out Master
/// (⇐ `updateDropTarget`'s panel branch: a tab-strip caret, a row-gap
/// insertion line, or a whole-body dashed outline for "new group").
pub fn paint_panel_drop(scene: &mut Scene, frame: &MasterFrame, drop: &PanelDrop, theme: &Theme) {
    match *drop {
        PanelDrop::IntoGroup { group, at } => {
            let Some(g) = frame.groups.get(group) else { return };
            if frame_layout(frame, group) == Some(MasterLayout::Tabs) {
                let x = tab_gap_x(g, at);
                let caret = Rect::new(x - 1.5, g.tab_strip.y0, x + 1.5, g.tab_strip.y1);
                scene.fill(Fill::NonZero, ID, theme.drop_line, None, &caret);
            } else {
                let y = row_gap_y(g, at);
                let line = Rect::new(g.bounds.x0, y - 1.5, g.bounds.x1, y + 1.5);
                scene.fill(Fill::NonZero, ID, theme.drop_line, None, &line);
            }
        }
        PanelDrop::NewGroup { at } => {
            let y = if at == 0 {
                frame.body.y0
            } else {
                frame.groups.get(at - 1).map_or(frame.body.y1, |g| g.bounds.y1)
            };
            let line = Rect::new(frame.body.x0, y - 1.5, frame.body.x1, y + 1.5);
            scene.fill(Fill::NonZero, ID, theme.drop_line, None, &line);
        }
    }
}

// `PanelDrop::IntoGroup` doesn't carry whether the target group is in
// Stack or Tabs mode; `paint_panel_drop` is always called against a frame
// built from one master, which has one `MasterLayout` for every group, so
// this just echoes that back cheaply rather than threading another
// parameter through `layout::hit_test_panel_drop`.
fn frame_layout(frame: &MasterFrame, group: usize) -> Option<MasterLayout> {
    let g = frame.groups.get(group)?;
    Some(if g.tab_strip.height() > 0.0 { MasterLayout::Tabs } else { MasterLayout::Stack })
}

fn tab_gap_x(g: &crate::layout::GroupArea, at: usize) -> f64 {
    if at == 0 {
        g.tab_strip.x0
    } else {
        g.tabs.get(at - 1).map_or(g.tab_strip.x0, |t| t.rect.x1)
    }
}

fn row_gap_y(g: &crate::layout::GroupArea, at: usize) -> f64 {
    if at == 0 {
        g.handle.y1
    } else {
        g.rows.get(at - 1).map_or(g.handle.y1, |r| r.rect.y1)
    }
}

/// Live drop cue while dragging a whole Group over another Master's body
/// (⇐ `updateDropTarget`'s group branch): a dashed outline around the
/// merge target, or an insertion line for a new sibling position.
pub fn paint_group_drop(scene: &mut Scene, frame: &MasterFrame, drop: &GroupDrop, theme: &Theme) {
    match *drop {
        GroupDrop::MergeInto { group } => {
            if let Some(g) = frame.groups.get(group) {
                scene.stroke(&Stroke::new(2.0), ID, theme.accent, None, &g.bounds.inset(-1.0));
            }
        }
        GroupDrop::NewSibling { at } => {
            let y = if at == 0 {
                frame.body.y0
            } else {
                frame.groups.get(at - 1).map_or(frame.body.y1, |g| g.bounds.y1)
            };
            let line = Rect::new(frame.body.x0, y - 1.5, frame.body.x1, y + 1.5);
            scene.fill(Fill::NonZero, ID, theme.drop_line, None, &line);
        }
    }
}

/// Outline shown around a whole Master while another Master is being
/// dragged over its body (⇐ `.master.drop-target`).
pub fn paint_master_merge_highlight(scene: &mut Scene, bounds: Rect, theme: &Theme) {
    scene.stroke(&Stroke::new(2.0), ID, theme.accent, None, &bounds.inset(-1.0));
}

/// The docking insertion line at the viewport edge/seam (⇐ `#dock-insert`).
pub fn paint_dock_insert(scene: &mut Scene, viewport_h: f64, x: f64, theme: &Theme) {
    let r = Rect::new(x - 2.0, 0.0, x + 2.0, viewport_h);
    scene.fill(Fill::NonZero, ID, theme.drop_line, None, &r);
}

/// Cheap drop shadow for the flyout / a torn-loose master mid-drag: a few
/// progressively larger, more transparent dark rects behind it. Vello has
/// no blur primitive; this is the standard fake, and at this scale the
/// banding isn't visible.
pub fn paint_shadow(scene: &mut Scene, bounds: Rect) {
    const LAYERS: [(f64, u8); 4] = [(2.0, 26), (5.0, 20), (9.0, 14), (14.0, 8)];
    for (spread, alpha) in LAYERS {
        let r = bounds.inflate(spread, spread);
        scene.fill(Fill::NonZero, ID, Color::from_rgba8(0, 0, 0, alpha), None, &r);
    }
}

/// The flyout preview beside a Stack-mode row (⇐ `#panel-flyout`): a small
/// floating card with a header (title + close) and a body rect the caller
/// paints the real panel content into.
pub fn paint_flyout_chrome(scene: &mut Scene, bounds: Rect, header: Rect, close: Rect, title: &str, theme: &Theme, text: &mut TextContext) {
    paint_shadow(scene, bounds);
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &bounds);
    scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &header);
    let baseline = header.y0 + header.height() * 0.5 + TAB_TEXT_PX as f64 * 0.34;
    text.draw(scene, title, TAB_TEXT_PX, theme.text, header.x0 + 10.0, baseline);
    paint_x(scene, close, theme.text_dim, 3.5);
    scene.stroke(&Stroke::new(1.5), ID, theme.text_dim, None, &bounds);
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
        scene.stroke(&stroke, ID, color, None, &Line::new((c.x - half, y), (c.x + half, y)));
    }
}

/// A double-chevron glyph — `«` (`pointing_left = true`) or `»`
/// (`pointing_left = false`). Toggles a Normal master between Tabs
/// (pointing left) and Stack (pointing right) display.
fn paint_chevrons(scene: &mut Scene, r: Rect, color: vello::peniko::Color, pointing_left: bool) {
    let c = r.center();
    let (dx, half) = (3.0_f64, 3.5_f64);
    let sign = if pointing_left { 1.0 } else { -1.0 };
    let stroke = Stroke::new(1.4);
    for i in [-1.0_f64, 1.0] {
        let cx = c.x + i * dx;
        scene.stroke(&stroke, ID, color, None, &Line::new((cx + sign * half * 0.6, c.y - half), (cx - sign * half * 0.6, c.y)));
        scene.stroke(&stroke, ID, color, None, &Line::new((cx - sign * half * 0.6, c.y), (cx + sign * half * 0.6, c.y + half)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::{Group, MasterKind};
    use crate::layout::layout_master;
    use vello::kurbo::Rect;

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
            groups: groups.into_iter().enumerate().map(|(i, p)| Group::new(i as u64, p)).collect(),
            dock: None,
            rect: [0.0, 0.0, 280.0, 400.0],
            scroll: 0.0,
        }
    }

    #[test]
    fn paint_master_stack_and_tabs_modes_dont_panic() {
        let mut text = TextContext::new();
        for layout in [MasterLayout::Stack, MasterLayout::Tabs] {
            let m = master(layout, vec![vec![PanelId("a"), PanelId("b")], vec![PanelId("c")]]);
            let frame = layout_master(&m, Rect::new(0.0, 0.0, 280.0, 400.0), &theme(), &mut w80);
            let mut scene = Scene::new();
            paint_master(
                &mut scene,
                &frame,
                &m,
                &theme(),
                &mut text,
                &|p| p.0.to_string(),
                &|_| false,
                Some((0, 0)),
            );
        }
    }

    #[test]
    fn paint_master_compact_mode_doesnt_panic() {
        let mut text = TextContext::new();
        let m = master(MasterLayout::Stack, vec![vec![PanelId("a")]]);
        let frame = layout_master(&m, Rect::new(0.0, 0.0, 100.0, 400.0), &theme(), &mut w80);
        assert!(frame.compact);
        let mut scene = Scene::new();
        paint_master(&mut scene, &frame, &m, &theme(), &mut text, &|p| p.0.to_string(), &|_| false, None);
    }

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
