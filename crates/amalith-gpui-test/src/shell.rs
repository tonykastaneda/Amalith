//! Shared app state: the dock model, the OS windows backing floating
//! masters, and every drop resolved into a dock-model mutation.
//!
//! GPUI drag-and-drop only works inside one window, so drops that cross
//! windows are resolved here from screen coordinates: a drag released
//! outside its own window (see `ui::release_outside_listener`), and a
//! floating window moved by its native title bar (see `floating_moved`).

use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, AppContext, Bounds, Context, Entity, Pixels, Point, TitlebarOptions, Window, WindowBounds,
    WindowHandle, WindowKind, WindowOptions, point, px, size,
};

use crate::dock::{DockModel, PanelId, Side};
use crate::native::{self, NativeWindow};
use crate::ui::FloatingView;

/// Edge band of the main window that always docks (⇐ `layout_dock_edge`).
pub const DOCK_EDGE: f32 = 36.0;
/// Band around an already-docked column's edge that also docks (⇐ `layout_dock_seam`).
pub const DOCK_SEAM: f32 = 10.0;
pub const FLOAT_W: f32 = 280.0;
pub const FLOAT_H: f32 = 340.0;
/// Cursor offset inside a torn-off window: over its title bar.
pub const GRAB: (f32, f32) = (60.0, 14.0);

pub const ALL_PANELS: [PanelId; 13] = [
    "Appearance", "Character", "Paragraph", "Color", "Swatches", "Layers", "Artboards",
    "Transform", "Align", "Pathfinder", "Links", "Gradient", "Symbols",
];

/// What a GPUI drag is carrying.
#[derive(Clone, Debug)]
pub enum Dragged {
    Panel(PanelId),
    Group { master: u64, group: usize },
    Master(u64),
}

/// Where a drag lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// Into a group's tab strip before tab `at`.
    IntoGroup { master: u64, group: usize, at: usize },
    /// As a new group in `master`'s body before group `at`.
    NewGroup { master: u64, at: usize },
    /// Docked at `index` among the masters on `side`, counting every
    /// master docked there now (the dragged one included).
    Dock { side: Side, index: usize },
    /// Its own floating window, title bar under this screen point.
    Float { x: f32, y: f32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WinKey {
    Main,
    Floating(u64),
}

/// Each window's content area in screen coordinates, refreshed on every
/// render. A plain `RefCell` rather than `Shell` state, so views can write
/// it while rendering without updating (and re-notifying) the entity.
pub type Frames = Rc<RefCell<HashMap<WinKey, Bounds<Pixels>>>>;

/// The window's content area (below its native title bar) in screen space.
pub fn content_bounds(window: &Window) -> Bounds<Pixels> {
    let b = window.bounds();
    let vp = window.viewport_size();
    Bounds::new(point(b.origin.x, b.origin.y + (b.size.height - vp.height)), vp)
}

pub struct Shell {
    pub dock: DockModel,
    pub windows: HashMap<u64, WindowHandle<FloatingView>>,
    pub frames: Frames,
    /// The in-window GPUI drag in flight, if any, so a release outside the
    /// window can still be resolved.
    pub drag: Option<Dragged>,
    /// Where the floating window currently being moved by its title bar
    /// would land if released now.
    pub os_move_preview: Option<(u64, Target)>,
    os_move_last: Option<Instant>,
    /// The floating master torn off mid-drag that follows the cursor until
    /// release (⇐ `Drag::DraggingPanel`/`DraggingGroup`/`MovingMaster`).
    pub live: Option<u64>,
    /// `live` as of the mouse-up being dispatched right now, so in-window
    /// drop handlers can still translate their payload.
    pub released_live: Option<u64>,
    pub native: HashMap<u64, NativeWindow>,
}

pub fn default_layout() -> DockModel {
    let mut d = DockModel::default();
    let left = d.spawn_master(vec![vec!["Appearance"], vec!["Character", "Paragraph"]], [0.0, 0.0, 220.0, 0.0]);
    d.dock_master(left, Side::Left, 0);
    let right = d.spawn_master(
        vec![vec!["Color", "Swatches"], vec!["Layers", "Artboards"], vec!["Transform", "Align", "Pathfinder"]],
        [0.0, 0.0, 280.0, 0.0],
    );
    d.dock_master(right, Side::Right, 0);
    let links = d.spawn_master(vec![vec!["Links"]], [0.0, 0.0, 200.0, 0.0]);
    d.dock_master(links, Side::Right, 1);
    d
}

impl Shell {
    pub fn new() -> Self {
        Self {
            dock: default_layout(),
            windows: HashMap::new(),
            frames: Rc::default(),
            drag: None,
            os_move_preview: None,
            os_move_last: None,
            live: None,
            released_live: None,
            native: HashMap::new(),
        }
    }

    fn docked_widths(&self, side: Side) -> Vec<(u64, f32)> {
        self.dock
            .docked(side)
            .into_iter()
            .filter_map(|id| self.dock.master(id).map(|m| (id, m.rect[2])))
            .collect()
    }

    /// ⇐ `layout::resolve_dock_target`: `x` is main-window-local.
    pub fn dock_target_at(&self, x: f32, w: f32) -> Option<(Side, usize)> {
        let left: Vec<f32> = self.docked_widths(Side::Left).into_iter().map(|(_, w)| w).collect();
        let right: Vec<f32> = self.docked_widths(Side::Right).into_iter().map(|(_, w)| w).collect();
        if x <= DOCK_EDGE {
            let mut off = 0.0;
            let i = left.iter().position(|&lw| { let hit = x < off + lw * 0.5; off += lw; hit });
            return Some((Side::Left, i.unwrap_or(left.len())));
        }
        if x >= w - DOCK_EDGE {
            let mut off = 0.0;
            let i = right.iter().position(|&rw| { let hit = x > w - off - rw * 0.5; off += rw; hit });
            return Some((Side::Right, i.unwrap_or(right.len())));
        }
        let mut off = 0.0;
        for (i, &lw) in left.iter().enumerate() {
            if (x - off).abs() <= DOCK_SEAM {
                return Some((Side::Left, i));
            }
            if (x - (off + lw)).abs() <= DOCK_SEAM {
                return Some((Side::Left, i + 1));
            }
            off += lw;
        }
        let mut off = 0.0;
        for (i, &rw) in right.iter().enumerate() {
            if (x - (w - off)).abs() <= DOCK_SEAM {
                return Some((Side::Right, i));
            }
            if (x - (w - off - rw)).abs() <= DOCK_SEAM {
                return Some((Side::Right, i + 1));
            }
            off += rw;
        }
        None
    }

    /// The master whose on-screen area contains screen point `p`, ignoring
    /// `exclude`. Floating windows are checked first: they sit on top.
    fn master_at_screen(&self, p: Point<Pixels>, exclude: Option<u64>) -> Option<u64> {
        let frames = self.frames.borrow();
        for (key, b) in frames.iter() {
            if let WinKey::Floating(id) = *key {
                if Some(id) != exclude && b.contains(&p) && self.dock.master(id).is_some() {
                    return Some(id);
                }
            }
        }
        let main = frames.get(&WinKey::Main)?;
        if !main.contains(&p) {
            return None;
        }
        let x = f32::from(p.x - main.origin.x);
        let w = f32::from(main.size.width);
        let mut off = 0.0;
        for (id, lw) in self.docked_widths(Side::Left) {
            if x >= off && x < off + lw {
                return (Some(id) != exclude).then_some(id);
            }
            off += lw;
        }
        let mut off = 0.0;
        for (id, rw) in self.docked_widths(Side::Right) {
            if x <= w - off && x > w - off - rw {
                return (Some(id) != exclude).then_some(id);
            }
            off += rw;
        }
        None
    }

    /// Resolves a drag released outside the window it started in, ignoring
    /// `exclude` (the source window, or the live window under the cursor).
    pub fn resolve_screen_point(&self, p: Point<Pixels>, exclude: Option<u64>) -> Target {
        let float = Target::Float { x: f32::from(p.x), y: f32::from(p.y) };
        let floating_hit = self.frames.borrow().iter().any(|(k, b)| match k {
            WinKey::Floating(id) => Some(*id) != exclude && b.contains(&p),
            WinKey::Main => false,
        });
        if !floating_hit {
            if let Some(main) = self.frames.borrow().get(&WinKey::Main).copied() {
                if main.contains(&p) {
                    let x = f32::from(p.x - main.origin.x);
                    if let Some((side, index)) = self.dock_target_at(x, f32::from(main.size.width)) {
                        return Target::Dock { side, index };
                    }
                }
            }
        }
        match self.master_at_screen(p, exclude) {
            Some(master) => Target::NewGroup { master, at: usize::MAX },
            None => float,
        }
    }

    /// Where floating window `id` would land if its title-bar move ended
    /// with its content area at `content` (screen space).
    fn resolve_window_drop(&self, id: u64, content: Bounds<Pixels>) -> Option<Target> {
        let main = self.frames.borrow().get(&WinKey::Main).copied()?;
        let overlaps_vertically = content.origin.y < main.bottom() && content.bottom() > main.origin.y;
        if overlaps_vertically {
            let w = f32::from(main.size.width);
            let lx = f32::from(content.origin.x - main.origin.x);
            let rx = f32::from(content.right() - main.origin.x);
            if let Some((Side::Left, index)) = self.dock_target_at(lx, w) {
                return Some(Target::Dock { side: Side::Left, index });
            }
            if let Some((Side::Right, index)) = self.dock_target_at(rx, w) {
                return Some(Target::Dock { side: Side::Right, index });
            }
        }
        // The title bar's centre stands in for the cursor, which GPUI
        // can't see during a native window move.
        let grip = point(content.center().x, content.origin.y - px(14.0));
        self.master_at_screen(grip, Some(id)).map(|master| Target::NewGroup { master, at: usize::MAX })
    }

    /// The master holding only `panel`, if there is one.
    fn lone_master(&self, panel: PanelId) -> Option<u64> {
        let (mid, ..) = self.dock.locate(panel)?;
        (self.dock.master(mid)?.panels() == [panel]).then_some(mid)
    }

    fn single_group(&self, master: u64) -> bool {
        self.dock.master(master).is_some_and(|m| m.groups.len() == 1)
    }

    fn is_floating(&self, master: u64) -> bool {
        self.dock.master(master).is_some_and(|m| m.dock.is_none())
    }

    /// Docks `master`, translating `index` from "slot among everything
    /// docked now" to `dock_master`'s "slot once `master` is pulled out".
    fn dock_whole(&mut self, master: u64, side: Side, index: usize) {
        let index = match self.dock.master(master).and_then(|m| m.dock) {
            Some((s, i)) if s == side && i < index => index - 1,
            _ => index,
        };
        self.dock.dock_master(master, side, index);
    }

    /// The single mutation path for every drop, in-window or cross-window
    /// (⇐ the `Drag::MovingMaster` / `DraggingGroup` / `DraggingPanel`
    /// release arms in `amalith-shell`).
    pub fn apply_drop(&mut self, dragged: Dragged, target: Target, cx: &mut Context<Self>) {
        let dragged = self.live_payload(dragged);
        self.live = None;
        let float_rect = |x: f32, y: f32| [x - GRAB.0, y - GRAB.1, FLOAT_W, FLOAT_H];
        let d = &mut self.dock;
        match (dragged, target) {
            (Dragged::Panel(p), Target::IntoGroup { master, group, at }) => {
                d.move_panel_into_group(p, (master, group), at);
            }
            (Dragged::Panel(p), Target::NewGroup { master, at }) => {
                if self.lone_master(p) != Some(master) {
                    self.dock.move_panel_new_group(p, master, at);
                }
            }
            (Dragged::Panel(p), Target::Dock { side, index }) => match self.lone_master(p) {
                Some(m) => self.dock_whole(m, side, index),
                None => {
                    if let Some(id) = self.dock.detach_panel(p, [0.0, 0.0, FLOAT_W, FLOAT_H]) {
                        self.dock.dock_master(id, side, index);
                    }
                }
            },
            (Dragged::Panel(p), Target::Float { x, y }) => {
                // Already alone in its own window: nothing to tear off.
                if !self.lone_master(p).is_some_and(|m| self.is_floating(m)) {
                    self.dock.detach_panel(p, float_rect(x, y));
                }
            }
            (Dragged::Group { master: m, group: g }, Target::IntoGroup { master, group, at }) => {
                d.merge_groups((m, g), (master, group), at);
            }
            (Dragged::Group { master: m, group: g }, Target::NewGroup { master, at }) => {
                let at = if master == m && g < at { at - 1 } else { at };
                d.move_group((m, g), master, at);
            }
            (Dragged::Group { master: m, group: g }, Target::Dock { side, index }) => {
                if self.single_group(m) {
                    self.dock_whole(m, side, index);
                } else if let Some(id) = self.dock.detach_group(m, g, [0.0, 0.0, FLOAT_W, FLOAT_H]) {
                    self.dock.dock_master(id, side, index);
                }
            }
            (Dragged::Group { master: m, group: g }, Target::Float { x, y }) => {
                if !(self.single_group(m) && self.is_floating(m)) {
                    self.dock.detach_group(m, g, float_rect(x, y));
                }
            }
            (Dragged::Master(m), Target::IntoGroup { master: t, group, at }) if t != m => {
                if self.single_group(m) {
                    self.dock.merge_groups((m, 0), (t, group), at);
                } else {
                    self.dock.merge_masters(m, t, group);
                }
            }
            (Dragged::Master(m), Target::NewGroup { master: t, at }) if t != m => {
                if self.single_group(m) {
                    self.dock.move_group((m, 0), t, at);
                } else {
                    self.dock.merge_masters(m, t, at);
                }
            }
            (Dragged::Master(m), Target::Dock { side, index }) => self.dock_whole(m, side, index),
            (Dragged::Master(m), Target::Float { x, y }) => {
                if !self.is_floating(m) {
                    let w = self.dock.master(m).map_or(FLOAT_W, |mm| mm.rect[2]);
                    self.dock.undock(m);
                    if let Some(mm) = self.dock.master_mut(m) {
                        mm.rect = [x - GRAB.0, y - GRAB.1, w, FLOAT_H];
                    }
                }
            }
            (Dragged::Master(_), _) => {}
        }
        self.dock.prune_all_empty();
        self.changed(cx);
    }

    /// A GPUI drag payload still names where the dragged thing *was*; once
    /// it's been torn off live, it's the live master's only group.
    fn live_payload(&self, dragged: Dragged) -> Dragged {
        match (self.live.or(self.released_live), dragged) {
            (Some(live), Dragged::Group { .. }) => Dragged::Group { master: live, group: 0 },
            (Some(live), Dragged::Master(_)) => Dragged::Master(live),
            (_, d) => d,
        }
    }

    /// Tears the dragged thing off into its own floating window under the
    /// cursor at screen point `p`, mid-drag; the window then follows the
    /// cursor (see `follow_cursor`) until release.
    pub fn go_live(&mut self, dragged: &Dragged, p: Point<Pixels>, cx: &mut Context<Self>) {
        if self.live.is_some() {
            return;
        }
        // Something that survives the move, to find its master afterwards.
        let probe = match dragged {
            Dragged::Panel(p) => Some(*p),
            Dragged::Group { master, group } => {
                self.dock.master(*master).and_then(|m| m.groups.get(*group)).and_then(|g| g.panels.first().copied())
            }
            Dragged::Master(_) => None,
        };
        self.apply_drop(dragged.clone(), Target::Float { x: f32::from(p.x), y: f32::from(p.y) }, cx);
        let master = match (probe, dragged) {
            (Some(panel), _) => self.dock.locate(panel).map(|(m, ..)| m),
            (None, Dragged::Master(m)) => Some(*m),
            _ => None,
        };
        self.live = master.filter(|&m| self.is_floating(m));
    }

    /// Keeps the live window's title bar under the cursor.
    pub fn follow_cursor(&mut self, p: Point<Pixels>) {
        let Some(live) = self.live else { return };
        let (x, y) = (f32::from(p.x) - GRAB.0, f32::from(p.y) - GRAB.1);
        if let Some(m) = self.dock.master_mut(live) {
            m.rect[0] = x;
            m.rect[1] = y;
        }
        if let Some(w) = self.native.get(&live) {
            native::move_window(w, x, y);
        }
    }

    /// Notify views and reconcile OS windows with the model once the
    /// current update unwinds (opening a window renders it, which reads
    /// `Shell`, so it can't happen mid-update).
    pub fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        let this = cx.entity();
        cx.defer(move |cx| sync_windows(this, cx));
    }

    pub fn close_master(&mut self, id: u64, cx: &mut Context<Self>) {
        self.dock.remove_master(id);
        self.changed(cx);
    }

    /// Re-opens a hidden panel, docked into the outermost right column.
    pub fn show_panel(&mut self, panel: PanelId, cx: &mut Context<Self>) {
        if self.dock.contains(panel) {
            return;
        }
        let id = self.dock.spawn_master(vec![vec![panel]], [0.0, 0.0, FLOAT_W, FLOAT_H]);
        match self.dock.docked(Side::Right).first() {
            Some(&right) => {
                self.dock.merge_masters(id, right, usize::MAX);
            }
            None => self.dock.dock_master(id, Side::Right, 0),
        }
        self.changed(cx);
    }

    pub fn reset_layout(&mut self, cx: &mut Context<Self>) {
        self.dock = default_layout();
        self.changed(cx);
    }

    /// A floating window closed by its own close button folds into the
    /// outermost right column, so nothing is lost (⇐ `CloseRequested`).
    pub fn floating_closed(&mut self, id: u64, cx: &mut Context<Self>) {
        self.windows.remove(&id);
        self.native.remove(&id);
        self.frames.borrow_mut().remove(&WinKey::Floating(id));
        if self.dock.master(id).is_none() {
            return;
        }
        match self.dock.docked(Side::Right).first() {
            Some(&right) => {
                self.dock.merge_masters(id, right, usize::MAX);
            }
            None => self.dock.dock_master(id, Side::Right, 0),
        }
        self.changed(cx);
    }

    /// Floating window `id` was moved or resized (by the OS). A move
    /// previews its drop target live and commits it once the mouse is up.
    pub fn floating_moved(&mut self, id: u64, outer: Bounds<Pixels>, content: Bounds<Pixels>, cx: &mut Context<Self>) {
        let Some(m) = self.dock.master_mut(id) else { return };
        let rect = [
            f32::from(outer.origin.x),
            f32::from(outer.origin.y),
            f32::from(outer.size.width),
            f32::from(outer.size.height),
        ];
        let moved = m.rect[0] != rect[0] || m.rect[1] != rect[1];
        m.rect = rect;
        // A live tear-off is being moved by `follow_cursor`, not the user's
        // title-bar drag; its drop resolves on mouse-up instead.
        if !moved || self.live == Some(id) {
            return;
        }
        self.os_move_preview = self.resolve_window_drop(id, content).map(|t| (id, t));
        cx.notify();
        let watching = self.os_move_last.is_some();
        self.os_move_last = Some(Instant::now());
        if watching {
            return;
        }
        cx.spawn(async move |this, cx| {
            let settle = if cfg!(target_os = "macos") { Duration::from_millis(60) } else { Duration::from_millis(400) };
            loop {
                cx.background_executor().timer(Duration::from_millis(50)).await;
                let done = this
                    .update(cx, |s, cx| {
                        let idle = s.os_move_last.is_none_or(|t| t.elapsed() >= settle);
                        if !idle || mouse_buttons_down() {
                            return false;
                        }
                        s.os_move_last = None;
                        if let Some((id, target)) = s.os_move_preview.take() {
                            s.apply_drop(Dragged::Master(id), target, cx);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(target_os = "macos")]
fn mouse_buttons_down() -> bool {
    objc2_app_kit::NSEvent::pressedMouseButtons() != 0
}

#[cfg(not(target_os = "macos"))]
fn mouse_buttons_down() -> bool {
    false
}

/// Opens a window for every floating master that lacks one, closes every
/// window whose master is gone or docked, and keeps titles current.
pub fn sync_windows(shell: Entity<Shell>, cx: &mut App) {
    let (wanted, stale) = {
        let s = shell.read(cx);
        let wanted: Vec<(u64, [f32; 4], String)> = s
            .dock
            .masters
            .iter()
            .filter(|m| m.dock.is_none())
            .map(|m| (m.id, m.rect, m.title()))
            .collect();
        let stale: Vec<(u64, WindowHandle<FloatingView>)> = s
            .windows
            .iter()
            .filter(|(id, _)| !wanted.iter().any(|(w, ..)| w == *id))
            .map(|(id, h)| (*id, *h))
            .collect();
        (wanted, stale)
    };
    for (id, handle) in stale {
        shell.update(cx, |s, _| {
            s.windows.remove(&id);
            s.native.remove(&id);
            s.frames.borrow_mut().remove(&WinKey::Floating(id));
        });
        handle.update(cx, |_, window, _| window.remove_window()).ok();
    }
    for (id, rect, title) in wanted {
        if let Some(handle) = shell.read(cx).windows.get(&id).copied() {
            handle.update(cx, |_, window, _| window.set_window_title(&title)).ok();
            continue;
        }
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                point(px(rect[0]), px(rect[1])),
                size(px(rect[2].max(160.0)), px(rect[3].max(120.0))),
            ))),
            titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
            kind: WindowKind::Floating,
            // Don't steal key status mid-drag from the window that owns the mouse.
            focus: false,
            window_min_size: Some(size(px(160.0), px(120.0))),
            ..Default::default()
        };
        let view_shell = shell.clone();
        let opened = cx.open_window(options, move |window, cx| cx.new(|cx| FloatingView::new(view_shell, id, window, cx)));
        if let Ok(handle) = opened {
            shell.update(cx, |s, _| {
                s.windows.insert(id, handle);
            });
        }
    }
}
