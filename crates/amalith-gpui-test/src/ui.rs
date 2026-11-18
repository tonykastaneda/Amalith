//! Views: the main window (left rail, canvas, right rail) and one window
//! per floating master. Both render masters through `render_master`, the
//! same way `amalith-shell` paints a master identically docked or floating.
//! A neutral light/dark look of its own, following the OS appearance — not
//! Amalith's theme.

use gpui::{
    AnyElement, App, AppContext, Context, DispatchPhase, DragMoveEvent, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window, WindowAppearance, canvas, div, prelude::FluentBuilder, px, rgb, rgba,
};

use crate::dock::{Group, Side};
use crate::native;
use crate::shell::{ALL_PANELS, Dragged, Shell, Target, WinKey, content_bounds};

#[derive(Clone, Copy)]
struct Theme {
    app_bg: Hsla,
    rail_bg: Hsla,
    card_bg: Hsla,
    strip_bg: Hsla,
    header_bg: Hsla,
    border: Hsla,
    text: Hsla,
    muted: Hsla,
    faint: Hsla,
    hover: Hsla,
    accent: Hsla,
    accent_soft: Hsla,
    canvas_bg: Hsla,
    artboard: Hsla,
}

impl Theme {
    fn for_window(window: &Window) -> Self {
        match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
        }
    }

    fn dark() -> Self {
        Self {
            app_bg: rgb(0x16171a).into(),
            rail_bg: rgb(0x1d1e22).into(),
            card_bg: rgb(0x26282d).into(),
            strip_bg: rgb(0x202125).into(),
            header_bg: rgb(0x1d1e22).into(),
            border: rgb(0x33353b).into(),
            text: rgb(0xe8e9ec).into(),
            muted: rgb(0x9b9ea6).into(),
            faint: rgb(0x3a3d44).into(),
            hover: rgb(0x2f3137).into(),
            accent: rgb(0x5b8cff).into(),
            accent_soft: rgba(0x5b8cff2e).into(),
            canvas_bg: rgb(0x121315).into(),
            artboard: rgb(0xfafafa).into(),
        }
    }

    fn light() -> Self {
        Self {
            app_bg: rgb(0xe9eaee).into(),
            rail_bg: rgb(0xf3f4f6).into(),
            card_bg: rgb(0xffffff).into(),
            strip_bg: rgb(0xf6f7f9).into(),
            header_bg: rgb(0xf3f4f6).into(),
            border: rgb(0xdcdee3).into(),
            text: rgb(0x1c1d21).into(),
            muted: rgb(0x6b6f78).into(),
            faint: rgb(0xe3e5ea).into(),
            hover: rgb(0xeceef1).into(),
            accent: rgb(0x3b74f5).into(),
            accent_soft: rgba(0x3b74f526).into(),
            canvas_bg: rgb(0xdfe1e6).into(),
            artboard: rgb(0xffffff).into(),
        }
    }
}

/// The pill that follows the cursor during a drag — until a live tear-off
/// puts the real window under the cursor instead.
pub struct Ghost {
    label: SharedString,
    theme: Theme,
    shell: Entity<Shell>,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.theme;
        if self.shell.read(cx).live.is_some() {
            return div();
        }
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1p5()
            .bg(t.card_bg)
            .border_1()
            .border_color(t.accent)
            .rounded_lg()
            .shadow_lg()
            .text_xs()
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.text)
            .child(div().size_1p5().rounded_full().bg(t.accent))
            .child(self.label.clone())
    }
}

/// Drag-start: records the payload on `Shell` (for drops outside the
/// window) and builds the ghost.
fn ghost(shell: &Entity<Shell>, theme: Theme) -> impl Fn(&Dragged, Point<Pixels>, &mut Window, &mut App) -> Entity<Ghost> + 'static {
    let shell = shell.clone();
    move |dragged, _, _, cx| {
        let label = match dragged {
            Dragged::Panel(p) => p.to_string(),
            Dragged::Group { master, group } => shell
                .read(cx)
                .dock
                .master(*master)
                .and_then(|m| m.groups.get(*group))
                .map_or("Group".into(), |g| g.panels.join(" · ")),
            Dragged::Master(m) => shell.read(cx).dock.master(*m).map_or("Panels".into(), |m| m.title()),
        };
        shell.update(cx, |s, _| s.drag = Some(dragged.clone()));
        let shell = shell.clone();
        cx.new(|cx| {
            cx.observe(&shell, |_, _, cx| cx.notify()).detach();
            Ghost { label: label.into(), theme, shell }
        })
    }
}

fn drop_to(shell: &Entity<Shell>, target: Target) -> impl Fn(&Dragged, &mut Window, &mut App) + 'static {
    let shell = shell.clone();
    move |dragged, _, cx| {
        shell.update(cx, |s, cx| {
            s.drag = None;
            s.apply_drop(dragged.clone(), target, cx);
        })
    }
}

/// Window-wide drag plumbing GPUI's per-window drag-and-drop lacks: keeps
/// a live tear-off under the cursor (the mouse stays captured by the
/// window the drag began in, even past its edge), and resolves a drag
/// released outside this window in screen space.
fn window_listeners(shell: &Entity<Shell>, key: WinKey) -> impl IntoElement {
    let shell = shell.clone();
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let follow = shell.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && follow.read(cx).live.is_some() {
                    let screen = content_bounds(window).origin + e.position;
                    follow.update(cx, |s, _| s.follow_cursor(screen));
                }
            });
            window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let (dragged, live) = shell.update(cx, |s, _| {
                    let live = s.live.take();
                    s.released_live = live;
                    (s.drag.take(), live)
                });
                let clear = shell.clone();
                cx.defer(move |cx| clear.update(cx, |s, _| s.released_live = None));
                let Some(dragged) = dragged else { return };
                let vp = window.viewport_size();
                let p = e.position;
                if p.x >= px(0.0) && p.y >= px(0.0) && p.x < vp.width && p.y < vp.height {
                    return; // An in-window drop target handles it (or nothing does).
                }
                let screen = content_bounds(window).origin + p;
                let exclude = match key {
                    WinKey::Floating(id) => Some(id),
                    WinKey::Main => live,
                };
                shell.update(cx, |s, cx| {
                    let target = s.resolve_screen_point(screen, exclude);
                    s.apply_drop(dragged, target, cx);
                });
            });
        },
    )
    .absolute()
    .size_full()
}

/// Placeholder panel body: a few label + control rows, so a panel reads as
/// a panel without porting any real one.
fn skeleton(t: Theme, panel: &str) -> impl IntoElement {
    let rows: [(&str, f32); 3] = match panel.len() % 3 {
        0 => [("Opacity", 0.7), ("Blend", 0.5), ("Stroke", 0.85)],
        1 => [("Width", 0.6), ("Height", 0.6), ("Angle", 0.4)],
        _ => [("Font", 0.9), ("Size", 0.45), ("Leading", 0.55)],
    };
    div().flex().flex_col().gap_2().p_3().children(rows.into_iter().map(move |(label, frac)| {
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(div().w(px(52.0)).flex_none().text_xs().text_color(t.muted).child(label))
            .child(
                div()
                    .flex_1()
                    .h(px(20.0))
                    .rounded_md()
                    .bg(t.strip_bg)
                    .border_1()
                    .border_color(t.border)
                    .child(div().h_full().w(gpui::relative(frac)).rounded_md().bg(t.faint)),
            )
    }))
}

fn render_group(shell: &Entity<Shell>, t: Theme, mid: u64, gi: usize, g: &Group) -> impl IntoElement {
    let active = g.panels.get(g.active).copied().unwrap_or_default();
    let tabs = g.panels.iter().enumerate().map(|(pi, &p)| {
        let is_active = pi == g.active;
        let activate = {
            let shell = shell.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
                shell.update(cx, |s, cx| {
                    if let Some(g) = s.dock.master_mut(mid).and_then(|m| m.groups.get_mut(gi)) {
                        g.active = pi;
                    }
                    cx.notify();
                })
            }
        };
        div()
            .id(("tab", g.id << 8 | pi as u64))
            .relative()
            .flex()
            .items_center()
            .px_3()
            .h_full()
            .text_xs()
            .border_l_2()
            .border_color(gpui::transparent_black())
            .map(|d| {
                if is_active {
                    d.bg(t.card_bg).text_color(t.text).font_weight(FontWeight::MEDIUM)
                } else {
                    d.text_color(t.muted).hover(|s| s.text_color(t.text).bg(t.hover))
                }
            })
            .when(is_active, |d| {
                d.child(div().absolute().top_0().left_0().right_0().h(px(2.0)).bg(t.accent))
            })
            .drag_over::<Dragged>(move |s, _, _, _| s.border_color(t.accent))
            .on_drop(drop_to(shell, Target::IntoGroup { master: mid, group: gi, at: pi }))
            .on_click(activate)
            .on_drag(Dragged::Panel(p), ghost(shell, t))
            .child(p)
    });
    let end = Target::IntoGroup { master: mid, group: gi, at: g.panels.len() };
    div()
        .flex()
        .flex_col()
        .mx_2()
        .rounded_lg()
        .overflow_hidden()
        .bg(t.card_bg)
        .border_1()
        .border_color(t.border)
        .shadow_sm()
        .child(
            div()
                .id(("group-handle", g.id))
                .group("handle")
                .flex()
                .justify_center()
                .items_center()
                .h(px(10.0))
                .bg(t.strip_bg)
                .hover(|s| s.bg(t.hover))
                .on_drag(Dragged::Group { master: mid, group: gi }, ghost(shell, t))
                .child(
                    div()
                        .w(px(28.0))
                        .h(px(3.0))
                        .rounded_full()
                        .bg(t.faint)
                        .group_hover("handle", move |s| s.bg(t.muted)),
                ),
        )
        .child(
            div()
                .flex()
                .h(px(30.0))
                .bg(t.strip_bg)
                .border_b_1()
                .border_color(t.border)
                .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent_soft))
                .on_drop(drop_to(shell, end))
                .children(tabs),
        )
        .child(
            div()
                .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent_soft))
                .on_drop(drop_to(shell, end))
                .child(skeleton(t, active)),
        )
}

/// One master, docked or floating — header, then each group with a
/// "new group" drop gap above it and one more below the last.
fn render_master(shell: &Entity<Shell>, s: &Shell, t: Theme, mid: u64, dragging: bool) -> AnyElement {
    let Some(m) = s.dock.master(mid) else { return div().into_any_element() };
    let previewed = matches!(s.os_move_preview, Some((_, Target::NewGroup { master, .. })) if master == mid);
    let gap = |at: usize| {
        div()
            .flex_none()
            .h(px(if dragging { 14.0 } else { 8.0 }))
            .mx_2()
            .flex()
            .items_center()
            .child(div().w_full().h(px(2.0)).rounded_full())
            .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent_soft).rounded_md())
            .on_drop(drop_to(shell, Target::NewGroup { master: mid, at }))
    };
    let close = {
        let shell = shell.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| shell.update(cx, |s, cx| s.close_master(mid, cx))
    };
    let mut body = div().id(("master-body", mid)).flex_1().flex().flex_col().overflow_y_scroll();
    for (gi, g) in m.groups.iter().enumerate() {
        body = body.child(gap(gi)).child(render_group(shell, t, mid, gi, g));
    }
    body = body.child(
        div()
            .flex_1()
            .min_h(px(32.0))
            .mx_2()
            .mt_2()
            .mb_2()
            .rounded_lg()
            .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent_soft))
            .on_drop(drop_to(shell, Target::NewGroup { master: mid, at: m.groups.len() })),
    );
    let docked = m.dock.is_some();
    div()
        .size_full()
        .flex()
        .flex_col()
        .bg(t.rail_bg)
        .when(previewed, |d| d.border_2().border_color(t.accent).bg(t.accent_soft))
        .child(
            div()
                .id(("master-header", mid))
                .group("master-header")
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .h(px(28.0))
                .pl_2()
                .pr_1()
                .bg(t.header_bg)
                .border_b_1()
                .border_color(t.border)
                .text_xs()
                .text_color(t.muted)
                .hover(|s| s.text_color(t.text))
                .on_drag(Dragged::Master(mid), ghost(shell, t))
                .child(div().flex_none().text_color(t.faint).group_hover("master-header", move |s| s.text_color(t.muted)).child("⠿"))
                .child(div().flex_1().min_w_0().truncate().child(if docked { m.title() } else { "Drag to dock or merge".into() }))
                .child(
                    div()
                        .id(("master-close", mid))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(20.0))
                        .rounded_md()
                        .text_color(t.muted)
                        .hover(|s| s.bg(t.hover).text_color(t.text))
                        .on_click(close)
                        .child("✕"),
                ),
        )
        .child(body)
        .into_any_element()
}

pub struct MainView {
    shell: Entity<Shell>,
    _subs: Vec<Subscription>,
}

impl MainView {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            true
        });
        let subs = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
        ];
        Self { shell, _subs: subs }
    }
}

impl Render for MainView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_window(window);
        let shell = &self.shell;
        let s = shell.read(cx);
        s.frames.borrow_mut().insert(WinKey::Main, content_bounds(window));
        let in_window_drag = cx.has_active_drag();
        let dragging = in_window_drag || s.os_move_preview.is_some();
        let preview = s.os_move_preview.map(|(_, target)| target);

        let seam = |side: Side, index: usize| {
            let lit = preview == Some(Target::Dock { side, index });
            div()
                .h_full()
                .flex_none()
                .w(px(if dragging { 16.0 } else { 1.0 }))
                .bg(if lit { t.accent } else if dragging { t.accent_soft } else { t.border })
                .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent))
                .on_drop(drop_to(shell, Target::Dock { side, index }))
        };
        let column = |id: u64| {
            let w = s.dock.master(id).map_or(240.0, |m| m.rect[2]);
            div().h_full().flex_none().w(px(w)).child(render_master(shell, s, t, id, dragging))
        };

        let left = s.dock.docked(Side::Left);
        let mut left_rail = div().flex().flex_none().h_full();
        for (i, &id) in left.iter().enumerate() {
            left_rail = left_rail.child(seam(Side::Left, i)).child(column(id));
        }
        left_rail = left_rail.child(seam(Side::Left, left.len()));

        // Index 0 is the outermost (rightmost) column.
        let right = s.dock.docked(Side::Right);
        let mut right_rail = div().flex().flex_none().h_full().child(seam(Side::Right, right.len()));
        for (i, &id) in right.iter().enumerate().rev() {
            right_rail = right_rail.child(column(id)).child(seam(Side::Right, i));
        }

        let float_here = {
            let shell = shell.clone();
            move |dragged: &Dragged, window: &mut Window, cx: &mut App| {
                let p = content_bounds(window).origin + window.mouse_position();
                shell.update(cx, |s, cx| {
                    s.drag = None;
                    s.apply_drop(dragged.clone(), Target::Float { x: f32::from(p.x), y: f32::from(p.y) }, cx);
                })
            }
        };
        // Crossing onto the canvas tears the dragged thing off live.
        let tear_off = {
            let shell = shell.clone();
            move |e: &DragMoveEvent<Dragged>, window: &mut Window, cx: &mut App| {
                if !e.bounds.contains(&e.event.position) || shell.read(cx).live.is_some() {
                    return;
                }
                let dragged = e.drag(cx).clone();
                let p = content_bounds(window).origin + e.event.position;
                shell.update(cx, |s, cx| s.go_live(&dragged, p, cx));
            }
        };
        let live = s.live.is_some();
        let hint = |text: &'static str| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().size_1().rounded_full().bg(t.muted))
                .child(text)
        };
        // `min_w_0` + clipping: the canvas gives up width to the rails, never
        // the other way round, however many columns get docked.
        let canvas_area = div()
            .relative()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_6()
            .bg(t.canvas_bg)
            .drag_over::<Dragged>(move |s, _, _, _| s.bg(t.accent_soft))
            .on_drop(float_here)
            .on_drag_move(tear_off)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(t.muted).child("Artboard 1"))
                    .child(div().w(px(360.0)).h(px(240.0)).bg(t.artboard).rounded_sm().shadow_lg()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .px_4()
                    .py_3()
                    .rounded_lg()
                    .bg(t.rail_bg)
                    .border_1()
                    .border_color(t.border)
                    .text_xs()
                    .text_color(t.muted)
                    .child(hint("Drag a tab, group grip or column header onto the canvas to tear it off"))
                    .child(hint("Drop on a rail edge or seam to dock, on a tab strip or gap to merge"))
                    .child(hint("Move a floating window by its title bar onto an edge to dock it")),
            )
            .when(in_window_drag && live, |d| {
                d.child(
                    div()
                        .absolute()
                        .top_4()
                        .px_3()
                        .py_1()
                        .rounded_full()
                        .bg(t.accent)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(gpui::white())
                        .child("Release to leave it floating · or drop on an edge to dock"),
                )
            });

        let hidden: Vec<_> = ALL_PANELS.iter().copied().filter(|p| !s.dock.contains(p)).collect();
        let reset = {
            let shell = shell.clone();
            button(t, "Reset layout".into(), false, move |cx| shell.update(cx, |s, cx| s.reset_layout(cx)))
        };
        let toolbar = div()
            .flex()
            .flex_none()
            .flex_wrap()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .bg(t.rail_bg)
            .border_b_1()
            .border_color(t.border)
            .text_xs()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .mr_2()
                    .child(div().size_2().rounded_full().bg(t.accent))
                    .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).text_color(t.text).child("Dock test"))
                    .child(div().text_color(t.muted).child("GPUI")),
            )
            .child(reset)
            .when(!hidden.is_empty(), |d| {
                d.child(div().w(px(1.0)).h(px(16.0)).mx_1().bg(t.border))
                    .child(div().text_color(t.muted).child("Closed:"))
                    .children(hidden.into_iter().map(|p| {
                        let shell = shell.clone();
                        button(t, SharedString::from(format!("+ {p}")), true, move |cx| {
                            shell.update(cx, |s, cx| s.show_panel(p, cx))
                        })
                    }))
            });

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.app_bg)
            .text_color(t.text)
            .child(toolbar)
            .child(div().flex_1().min_h_0().flex().child(left_rail).child(canvas_area).child(right_rail))
            .child(window_listeners(shell, WinKey::Main))
    }
}

fn button(t: Theme, label: SharedString, chip: bool, on_click: impl Fn(&mut App) + 'static) -> impl IntoElement {
    div()
        .id(label.clone())
        .px_2p5()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(t.border)
        .text_color(if chip { t.muted } else { t.text })
        .bg(if chip { t.strip_bg } else { t.card_bg })
        .hover(|s| s.bg(t.hover).text_color(t.text))
        .on_click(move |_, _, cx| on_click(cx))
        .child(label)
}

pub struct FloatingView {
    shell: Entity<Shell>,
    master: u64,
    _subs: Vec<Subscription>,
}

impl FloatingView {
    pub fn new(shell: Entity<Shell>, master: u64, window: &mut Window, cx: &mut Context<Self>) -> Self {
        if let Some(native) = native::native_window(window) {
            shell.update(cx, |s, _| s.native.insert(master, native));
        }
        let on_close = shell.clone();
        window.on_window_should_close(cx, move |_, cx| {
            on_close.update(cx, |s, cx| s.floating_closed(master, cx));
            true
        });
        let subs = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.observe_window_appearance(window, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                let (outer, content) = (window.bounds(), content_bounds(window));
                let id = this.master;
                this.shell.update(cx, |s, cx| s.floating_moved(id, outer, content, cx));
            }),
        ];
        Self { shell, master, _subs: subs }
    }
}

impl Render for FloatingView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::for_window(window);
        let s = self.shell.read(cx);
        s.frames.borrow_mut().insert(WinKey::Floating(self.master), content_bounds(window));
        let dragging = cx.has_active_drag();
        div()
            .relative()
            .size_full()
            .bg(t.rail_bg)
            .text_color(t.text)
            .child(render_master(&self.shell, s, t, self.master, dragging))
            .child(window_listeners(&self.shell, WinKey::Floating(self.master)))
    }
}
