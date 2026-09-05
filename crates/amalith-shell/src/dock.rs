//! The dock model: a pure data model, no rendering, no windowing.
//!
//! Ported 1:1 off a working HTML/CSS/JS reference the user built
//! (`amalith-panelSys/{index.html,styles.css,app.js}`) after every attempt
//! to build this from written descriptions or mockups got corrected — see
//! that file's `app.js` for the source of truth this mirrors. Doc comments
//! below name the JS function each Rust one replaces, so the mapping stays
//! traceable.
//!
//! The hierarchy is flat at every level, unlike the tree this replaced:
//! **Master → Group → Panel**. A [`Master`] is one on-screen unit (docked
//! to a rail edge or floating as its own OS window — the same thing
//! either way, just an `Option<(Side, index)>`); it holds an ordered list
//! of [`Group`]s. A `Group` holds an ordered list of opaque [`PanelId`]s
//! with one active tab. There is no recursive splitting *inside* a
//! master's body — the only side-by-side arrangement is multiple Masters
//! docked at the same edge (see [`DockModel::dock_master`]).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Opaque, stable identifier for a panel kind. The app maps these to real
/// panels via its registry; the dock never dereferences one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PanelId(pub &'static str);

// `PanelId` wraps a `&'static str`, so serde can't fill one in directly on
// load. Equality / hashing compare the string *contents* (derived), so a
// leaked copy of the name is interchangeable with the original static —
// and the panel-name set is tiny and fixed, so the leak is bounded.
impl Serialize for PanelId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0)
    }
}
impl<'de> Deserialize<'de> for PanelId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(PanelId(Box::leak(s.into_boxed_str())))
    }
}

/// Which rail edge a [`Master`] is docked to. The prototype never docks
/// top/bottom, only left/right.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Side {
    Left,
    Right,
}

/// A Master's display mode — the header's chevron toggles this (⇐
/// `toggleMasterLayout`/`setMasterLayout`). Meaningless for
/// [`MasterKind::Tools`], which has its own [`ToolsDensity`] instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MasterLayout {
    /// Every panel in every group shown as a flat clickable row (icon +
    /// label); a click opens a flyout preview, a press-and-hold drags it.
    Stack,
    /// Each group shows its own tab strip and one active panel's body —
    /// closest to what a docked panel looked like before this rewrite.
    Tabs,
}

/// Three panels keep their own bespoke behavior outside this model
/// entirely (Color Picker, Shape Dialogs — see `App::picker` /
/// `App::shape_dialog`, neither lives in `DockModel`). Tools is the one
/// bespoke *Master* kind: a plain icon grid, no groups, never merges with
/// another master (⇐ `createToolsMaster`, the `dataset.kind === "tools"`
/// checks throughout `app.js`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MasterKind {
    Normal,
    Tools,
}

/// A Tools master's own grid density toggle (⇐ `setToolsLayout`'s
/// `"grid-2x15"` / `"grid-1x30"`). Meaningless for [`MasterKind::Normal`].
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum ToolsDensity {
    Grid2x,
    Grid1x,
}

/// One tab group: an ordered list of panels sharing one tab strip, one
/// active at a time. Groups never nest and never split — a Master's
/// `groups` list is the only structure above this.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u64,
    pub panels: Vec<PanelId>,
    pub active: usize,
    /// Tabs-mode content pane height, logical px — `None` means "not
    /// pinned yet", so it spawns at the active panel's own natural content
    /// height instead of a fixed default. Drag-resizable (⇐ the
    /// prototype's `.tab-content-resize` handle, `setupTabContentResize`);
    /// once dragged, the user's explicit height sticks regardless of which
    /// tab is active. Ignored entirely in [`MasterLayout::Stack`].
    #[serde(default)]
    pub content_h: Option<f32>,
}

impl Group {
    pub fn new(id: u64, panels: Vec<PanelId>) -> Self {
        Self { id, panels, active: 0, content_h: None }
    }

    pub fn is_empty(&self) -> bool {
        self.panels.is_empty()
    }
}

/// Tabs-mode content pane bounds, logical px (⇐ `TAB_CONTENT_MIN_H` /
/// `TAB_CONTENT_MAX_H` / `TAB_CONTENT_DEFAULT_H`).
pub const TAB_CONTENT_MIN_H: f32 = 80.0;
pub const TAB_CONTENT_MAX_H: f32 = 480.0;
pub const TAB_CONTENT_DEFAULT_H: f32 = 160.0;

/// One Master Group: an on-screen unit that is either docked to a rail
/// edge or floating as its own OS window — the *same* entity either way
/// (⇐ every `.master` div in the prototype, toggled by `dataset.dock`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Master {
    pub id: u64,
    pub kind: MasterKind,
    pub layout: MasterLayout,
    #[serde(default)]
    pub tools_density: ToolsDensity,
    pub groups: Vec<Group>,
    /// `Some((side, index))` while docked; `index` is this master's
    /// position among every other master docked to the same `side` (⇐
    /// `dataset.dock` + `dataset.dockIndex`, kept in step by
    /// [`DockModel::dock_master`]/[`DockModel::undock`]).
    pub dock: Option<(Side, usize)>,
    /// `[x, y, w, h]` in logical points. While docked, only `w` is
    /// meaningful (height is however tall the rail's edge is; `x`/`y` are
    /// computed fresh from the docked order every layout, not stored) —
    /// `x`/`y` still hold the last floating position so un-docking drops
    /// it back where it last floated free, matching `undock`'s behavior
    /// of leaving `style.left`/`style.top` alone until the next drag.
    pub rect: [f32; 4],
    /// Scroll offset for a *docked* master whose groups overflow the
    /// available height (⇐ CSS `overflow-y: auto` on `.master.docked
    /// .master-body` — there are no inter-group splitters in this model,
    /// a docked column just scrolls instead of everything shrinking to
    /// fit).
    #[serde(default)]
    pub scroll: f32,
}

impl Default for ToolsDensity {
    fn default() -> Self {
        ToolsDensity::Grid2x
    }
}

impl Master {
    fn new(id: u64, kind: MasterKind, groups: Vec<Group>, rect: [f32; 4]) -> Self {
        Self {
            id,
            kind,
            layout: MasterLayout::Tabs,
            tools_density: ToolsDensity::Grid2x,
            groups,
            dock: None,
            rect,
            scroll: 0.0,
        }
    }

    pub fn is_tools(&self) -> bool {
        self.kind == MasterKind::Tools
    }

    pub fn is_empty(&self) -> bool {
        !self.is_tools() && self.groups.iter().all(Group::is_empty)
    }

    pub fn panels(&self) -> Vec<PanelId> {
        self.groups.iter().flat_map(|g| g.panels.iter().copied()).collect()
    }

    pub fn group(&self, idx: usize) -> Option<&Group> {
        self.groups.get(idx)
    }

    pub fn group_mut(&mut self, idx: usize) -> Option<&mut Group> {
        self.groups.get_mut(idx)
    }

    /// Index of the group holding `panel`, and `panel`'s index within it.
    pub fn locate(&self, panel: PanelId) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(gi, g)| {
            g.panels.iter().position(|&p| p == panel).map(|pi| (gi, pi))
        })
    }
}

/// The whole panel layout: every [`Master`], in z-order (later = drawn on
/// top / brought-to-front last, ⇐ `bringToFront`'s implicit DOM order).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockModel {
    pub masters: Vec<Master>,
    next_id: u64,
}

impl Default for DockModel {
    fn default() -> Self {
        Self { masters: Vec::new(), next_id: 1 }
    }
}

impl DockModel {
    pub fn new() -> Self {
        Self::default()
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Bumps the id allocator past every id already in use (masters and
    /// their groups) — call after replacing `masters` wholesale from a
    /// saved layout, or new ones spawned afterward could collide with
    /// ones that were just loaded.
    pub fn ensure_next_id(&mut self) {
        let max = self
            .masters
            .iter()
            .flat_map(|m| std::iter::once(m.id).chain(m.groups.iter().map(|g| g.id)))
            .max()
            .unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
    }

    /// Spawns a new floating Normal master holding `groups` at `rect`.
    /// Every group is given a fresh id. Returns the new master's id.
    pub fn spawn_master(&mut self, panel_groups: Vec<Vec<PanelId>>, rect: [f32; 4]) -> u64 {
        let mid = self.alloc_id();
        let groups = panel_groups
            .into_iter()
            .map(|panels| {
                let gid = self.alloc_id();
                Group::new(gid, panels)
            })
            .collect();
        self.masters.push(Master::new(mid, MasterKind::Normal, groups, rect));
        mid
    }

    /// Places `panel` alone in its own floating Normal master at `rect` —
    /// used for the handful of panels that always live in their own
    /// single-panel window (the colour picker, a shape dialog, Export for
    /// Screens; see `App::is_float_only`). If it already floats alone,
    /// that master is reused and repositioned; if it's placed elsewhere,
    /// it's pulled out first; otherwise a fresh master is spawned.
    pub fn float_alone(&mut self, panel: PanelId, rect: [f32; 4]) -> u64 {
        if let Some(m) = self
            .masters
            .iter_mut()
            .find(|m| m.dock.is_none() && m.panels().as_slice() == [panel])
        {
            m.rect = rect;
            return m.id;
        }
        if self.contains(panel) {
            self.remove(panel);
        }
        self.spawn_master(vec![vec![panel]], rect)
    }

    /// The master currently holding `panel`, if any (⇐ the old
    /// `floating_id_of` — these single-purpose panels never dock, so
    /// there's no docked/floating distinction left to make here).
    pub fn floating_id_of(&self, panel: PanelId) -> Option<u64> {
        self.locate(panel).map(|(m, ..)| m)
    }

    /// Spawns a new floating Tools master (⇐ `createToolsMaster`) — no
    /// groups; the panel content lives entirely in `panels::tools`,
    /// rendered straight from the master's kind.
    pub fn spawn_tools_master(&mut self, rect: [f32; 4]) -> u64 {
        let mid = self.alloc_id();
        self.masters.push(Master::new(mid, MasterKind::Tools, Vec::new(), rect));
        mid
    }

    pub fn master(&self, id: u64) -> Option<&Master> {
        self.masters.iter().find(|m| m.id == id)
    }

    pub fn master_mut(&mut self, id: u64) -> Option<&mut Master> {
        self.masters.iter_mut().find(|m| m.id == id)
    }

    /// Every panel placed anywhere, in an arbitrary but stable order.
    pub fn panels(&self) -> Vec<PanelId> {
        self.masters.iter().flat_map(Master::panels).collect()
    }

    pub fn contains(&self, panel: PanelId) -> bool {
        self.masters.iter().any(|m| m.panels().contains(&panel))
    }

    /// The master (and, within it, group/panel index) currently holding
    /// `panel`, if any.
    pub fn locate(&self, panel: PanelId) -> Option<(u64, usize, usize)> {
        self.masters.iter().find_map(|m| m.locate(panel).map(|(g, p)| (m.id, g, p)))
    }

    /// Removes `panel` from wherever it sits, pruning empty groups/masters
    /// per [`Self::prune_empty`]. `true` if it was actually found.
    pub fn remove(&mut self, panel: PanelId) -> bool {
        let Some((mid, gi, _)) = self.locate(panel) else {
            return false;
        };
        if let Some(m) = self.master_mut(mid) {
            if let Some(g) = m.group_mut(gi) {
                g.panels.retain(|&p| p != panel);
                g.active = g.active.min(g.panels.len().saturating_sub(1));
            }
        }
        self.prune_empty(mid);
        true
    }

    /// Drops empty groups from `master`, then the master itself if it has
    /// none left — except a *docked* master, which always keeps one empty
    /// group so the column shell stays put for panels to be dropped back
    /// into (⇐ `pruneEmpty`'s "Keep at least one empty group inside a
    /// docked master" / "Docked masters always remain as masters" rules).
    /// A Tools master is never pruned this way — it has no groups to
    /// begin with.
    pub fn prune_empty(&mut self, master: u64) {
        let Some(m) = self.master_mut(master) else { return };
        if m.is_tools() {
            return;
        }
        m.groups.retain(|g| !g.is_empty());
        if m.groups.is_empty() {
            // No exception for a docked master — there is no such thing
            // as an empty Master, docked or floating. Removing it
            // re-flows whatever else shares its side.
            self.remove_master(master);
        }
    }

    /// Runs [`Self::prune_empty`] over every master — none of them may
    /// ever sit empty, docked or floating; call this as a blanket safety
    /// net after any drag-commit, so the invariant holds even from a
    /// mutation path that doesn't already prune the specific master(s)
    /// it touched.
    pub fn prune_all_empty(&mut self) {
        let ids: Vec<u64> = self.masters.iter().map(|m| m.id).collect();
        for id in ids {
            self.prune_empty(id);
        }
    }

    /// Removes a master outright (its window should close) and re-flows
    /// whatever else was docked to its side, if it was docked.
    pub fn remove_master(&mut self, id: u64) {
        let side = self.master(id).and_then(|m| m.dock).map(|(s, _)| s);
        self.masters.retain(|m| m.id != id);
        if let Some(side) = side {
            self.reflow_dock_indices(side);
        }
    }

    /// Every master docked to `side`, in dock order (⇐ `getDocked`).
    pub fn docked(&self, side: Side) -> Vec<u64> {
        let mut v: Vec<&Master> = self
            .masters
            .iter()
            .filter(|m| m.dock.is_some_and(|(s, _)| s == side))
            .collect();
        v.sort_by_key(|m| m.dock.unwrap().1);
        v.into_iter().map(|m| m.id).collect()
    }

    fn reflow_dock_indices(&mut self, side: Side) {
        let ids = self.docked(side);
        for (i, id) in ids.into_iter().enumerate() {
            if let Some(m) = self.master_mut(id) {
                m.dock = Some((side, i));
            }
        }
    }

    /// Docks `id` to `side` at position `index` among the masters already
    /// there, shifting them up (⇐ `dockMaster`). If `id` was docked
    /// elsewhere, that side is re-flowed too.
    pub fn dock_master(&mut self, id: u64, side: Side, index: usize) {
        let prev_side = self.master(id).and_then(|m| m.dock).map(|(s, _)| s);

        let mut list: Vec<u64> = self.docked(side).into_iter().filter(|&x| x != id).collect();
        let at = index.min(list.len());
        list.insert(at, id);
        for (i, mid) in list.into_iter().enumerate() {
            if let Some(m) = self.master_mut(mid) {
                m.dock = Some((side, i));
            }
        }

        if let Some(prev) = prev_side {
            if prev != side {
                self.reflow_dock_indices(prev);
            }
        }
    }

    /// Un-docks `id`, leaving it floating at its current `rect` (⇐
    /// `undock`), and re-flows whatever else was on that side.
    pub fn undock(&mut self, id: u64) {
        let Some(side) = self.master(id).and_then(|m| m.dock).map(|(s, _)| s) else {
            return;
        };
        if let Some(m) = self.master_mut(id) {
            m.dock = None;
        }
        self.reflow_dock_indices(side);
    }

    /// Merges `source`'s entire group list into `target`'s as one
    /// contiguous block inserted at `at` (clamped to `target`'s group
    /// count — pass `usize::MAX`, or any index at least that count, to
    /// always append at the end), then drops `source` (⇐ `mergeMasters`
    /// with a live placeholder position — the reference doesn't
    /// special-case a multi-group source, so neither does this: the
    /// whole block lands together, docked or floating target either
    /// way). Refuses if either is a Tools master, they're the same
    /// master, or either id doesn't resolve.
    pub fn merge_masters(&mut self, source: u64, target: u64, at: usize) -> bool {
        if source == target {
            return false;
        }
        let (Some(s), Some(_)) = (self.master(source), self.master(target)) else {
            return false;
        };
        if s.is_tools() || self.master(target).is_some_and(Master::is_tools) {
            return false;
        }
        let Some(pos) = self.masters.iter().position(|m| m.id == source) else {
            return false;
        };
        let removed = self.masters.remove(pos);
        let side = removed.dock.map(|(s, _)| s);
        if let Some(t) = self.master_mut(target) {
            let at = at.min(t.groups.len());
            t.groups.splice(at..at, removed.groups);
        } else {
            // Target vanished between the checks above and here — put
            // the groups back rather than lose them.
            self.masters.push(removed);
            return false;
        }
        if let Some(side) = side {
            self.reflow_dock_indices(side);
        }
        true
    }

    /// Merges `source_group`'s panels into `dest_group`'s panel list at
    /// `at` (clamped to the list length), then drops the now-empty source
    /// group (⇐ `mergeGroups`, which always respects the drop
    /// placeholder's exact position rather than just appending).
    pub fn merge_groups(
        &mut self,
        source: (u64, usize),
        dest: (u64, usize),
        at: usize,
    ) -> bool {
        if source.0 == dest.0 && source.1 == dest.1 {
            return false;
        }
        let Some(mut panels) = self
            .master(source.0)
            .and_then(|m| m.group(source.1))
            .map(|g| g.panels.clone())
        else {
            return false;
        };
        if panels.is_empty() {
            return false;
        }
        let Some(dest_m) = self.master_mut(dest.0) else { return false };
        let Some(dest_g) = dest_m.group_mut(dest.1) else { return false };
        let at = at.min(dest_g.panels.len());
        for (offset, p) in panels.drain(..).enumerate() {
            dest_g.panels.insert(at + offset, p);
        }

        if let Some(m) = self.master_mut(source.0) {
            if let Some(g) = m.group_mut(source.1) {
                g.panels.clear();
            }
        }
        self.prune_empty(source.0);
        true
    }

    /// Moves `source_group` out of its master and into `dest_master`'s
    /// group list at `at`, as a new sibling group (⇐ the group drag's
    /// `"into-master"` mode). Prunes the source master if that leaves it
    /// empty.
    pub fn move_group(&mut self, source: (u64, usize), dest_master: u64, at: usize) -> bool {
        if source.0 == dest_master {
            // Reorder within the same master.
            let Some(m) = self.master_mut(source.0) else { return false };
            if source.1 >= m.groups.len() {
                return false;
            }
            let g = m.groups.remove(source.1);
            let at = at.min(m.groups.len());
            m.groups.insert(at, g);
            return true;
        }
        let Some(m) = self.master_mut(source.0) else { return false };
        if source.1 >= m.groups.len() {
            return false;
        }
        let g = m.groups.remove(source.1);
        let Some(dest) = self.master_mut(dest_master) else {
            // Destination vanished — put it back.
            if let Some(m) = self.master_mut(source.0) {
                m.groups.insert(source.1.min(m.groups.len()), g);
            }
            return false;
        };
        let at = at.min(dest.groups.len());
        dest.groups.insert(at, g);
        self.prune_empty(source.0);
        true
    }

    /// Pulls group `at` out of `master` into its own brand-new floating
    /// Normal master at `rect` (⇐ `detachGroup`). `None` if `master`/`at`
    /// don't resolve. Prunes `master` if that was its last group.
    pub fn detach_group(&mut self, master: u64, at: usize, rect: [f32; 4]) -> Option<u64> {
        let m = self.master_mut(master)?;
        if at >= m.groups.len() {
            return None;
        }
        // The new Master keeps whichever display mode the group was
        // pulled out of — detaching from a Tabs-mode master lands as a
        // Tabs-mode master, from Stack as Stack.
        let layout = m.layout;
        let g = m.groups.remove(at);
        self.prune_empty(master);
        let nid = self.alloc_id();
        let mut new_master = Master::new(nid, MasterKind::Normal, vec![g], rect);
        new_master.layout = layout;
        self.masters.push(new_master);
        Some(nid)
    }

    /// Pulls `panel` out from wherever it sits and wraps it in a fresh
    /// group inside a brand-new floating Normal master at `rect` (⇐
    /// `detachPanel`). `false` if `panel` wasn't placed. The new Master
    /// keeps the layout mode `panel` was detached from (a tab stays a
    /// tab, a Stack row stays a Stack row).
    pub fn detach_panel(&mut self, panel: PanelId, rect: [f32; 4]) -> Option<u64> {
        let Some((old_mid, ..)) = self.locate(panel) else {
            return None;
        };
        let layout = self.master(old_mid).map(|m| m.layout).unwrap_or(MasterLayout::Stack);
        self.remove(panel);
        let gid = self.alloc_id();
        let nid = self.alloc_id();
        let mut new_master = Master::new(nid, MasterKind::Normal, vec![Group::new(gid, vec![panel])], rect);
        new_master.layout = layout;
        self.masters.push(new_master);
        Some(nid)
    }

    /// Moves `panel` into `dest`'s group at `dest_group`, inserted at
    /// panel index `at` (⇐ the panel drag's `"into-group"` mode). Prunes
    /// the panel's old spot if it leaves a group/master empty.
    pub fn move_panel_into_group(&mut self, panel: PanelId, dest: (u64, usize), at: usize) -> bool {
        let Some((old_mid, old_gi, _)) = self.locate(panel) else { return false };
        if old_mid == dest.0 && old_gi == dest.1 {
            // Reorder within the same group.
            let Some(m) = self.master_mut(old_mid) else { return false };
            let Some(g) = m.group_mut(old_gi) else { return false };
            let Some(from) = g.panels.iter().position(|&p| p == panel) else { return false };
            g.panels.remove(from);
            let at = at.min(g.panels.len());
            g.panels.insert(at, panel);
            return true;
        }
        // `remove` prunes the panel's old (now possibly empty) group,
        // which can shift every later group in that same master down by
        // one — resolve `dest`'s *stable* group id before removing, then
        // re-find wherever it landed afterward, rather than trusting the
        // index to still mean the same thing.
        let Some(dest_group_id) = self.master(dest.0).and_then(|m| m.group(dest.1)).map(|g| g.id) else {
            return false;
        };
        self.remove(panel);
        let Some(m) = self.master_mut(dest.0) else { return false };
        let Some(gi) = m.groups.iter().position(|g| g.id == dest_group_id) else { return false };
        let g = &mut m.groups[gi];
        let at = at.min(g.panels.len());
        g.panels.insert(at, panel);
        g.active = at;
        true
    }

    /// Moves `panel` out of wherever it sits and into `dest_master`'s
    /// body as a brand-new group at position `at` (⇐ the panel drag's
    /// `"new-group"` mode).
    pub fn move_panel_new_group(&mut self, panel: PanelId, dest_master: u64, at: usize) -> bool {
        if !self.contains(panel) {
            return false;
        }
        self.remove(panel);
        let Some(len) = self.master(dest_master).map(|m| m.groups.len()) else {
            return false;
        };
        let at = at.min(len);
        let gid = self.alloc_id();
        let Some(m) = self.master_mut(dest_master) else { return false };
        m.groups.insert(at, Group::new(gid, vec![panel]));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PanelId = PanelId("a");
    const B: PanelId = PanelId("b");
    const C: PanelId = PanelId("c");
    const D: PanelId = PanelId("d");

    fn rect() -> [f32; 4] {
        [0.0, 0.0, 200.0, 100.0]
    }

    #[test]
    fn spawn_master_gives_every_group_and_the_master_itself_a_fresh_id() {
        let mut d = DockModel::new();
        let m1 = d.spawn_master(vec![vec![A]], rect());
        let m2 = d.spawn_master(vec![vec![B]], rect());
        assert_ne!(m1, m2);
        let g1 = d.master(m1).unwrap().groups[0].id;
        let g2 = d.master(m2).unwrap().groups[0].id;
        assert_ne!(g1, g2);
    }

    #[test]
    fn dock_master_orders_by_index_and_shifts_existing_ones_up() {
        let mut d = DockModel::new();
        let m1 = d.spawn_master(vec![vec![A]], rect());
        let m2 = d.spawn_master(vec![vec![B]], rect());
        d.dock_master(m1, Side::Left, 0);
        d.dock_master(m2, Side::Left, 0); // inserts before m1
        assert_eq!(d.docked(Side::Left), vec![m2, m1]);
        assert_eq!(d.master(m1).unwrap().dock, Some((Side::Left, 1)));
        assert_eq!(d.master(m2).unwrap().dock, Some((Side::Left, 0)));
    }

    #[test]
    fn undock_reflows_the_side_it_leaves() {
        let mut d = DockModel::new();
        let m1 = d.spawn_master(vec![vec![A]], rect());
        let m2 = d.spawn_master(vec![vec![B]], rect());
        d.dock_master(m1, Side::Left, 0);
        d.dock_master(m2, Side::Left, 1);
        d.undock(m1);
        assert_eq!(d.master(m1).unwrap().dock, None);
        assert_eq!(d.docked(Side::Left), vec![m2]);
        assert_eq!(d.master(m2).unwrap().dock, Some((Side::Left, 0)));
    }

    #[test]
    fn merge_masters_appends_groups_and_drops_the_source() {
        let mut d = DockModel::new();
        let target = d.spawn_master(vec![vec![A]], rect());
        let source = d.spawn_master(vec![vec![B], vec![C]], rect());
        assert!(d.merge_masters(source, target, usize::MAX));
        assert!(d.master(source).is_none());
        let groups: Vec<_> = d.master(target).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec![A], vec![B], vec![C]]);
    }

    #[test]
    fn merge_masters_inserts_the_whole_source_group_list_at_a_position() {
        let mut d = DockModel::new();
        let target = d.spawn_master(vec![vec![A], vec![D]], rect());
        let source = d.spawn_master(vec![vec![B], vec![C]], rect());
        assert!(d.merge_masters(source, target, 1));
        assert!(d.master(source).is_none());
        let groups: Vec<_> = d.master(target).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec![A], vec![B], vec![C], vec![D]]);
    }

    #[test]
    fn merge_masters_refuses_a_tools_master_on_either_side() {
        let mut d = DockModel::new();
        let normal = d.spawn_master(vec![vec![A]], rect());
        let tools = d.spawn_tools_master(rect());
        assert!(!d.merge_masters(tools, normal, 0));
        assert!(!d.merge_masters(normal, tools, 0));
        assert!(d.master(tools).is_some());
        assert!(d.master(normal).is_some());
    }

    #[test]
    fn merge_groups_inserts_at_the_given_position_and_prunes_the_source() {
        let mut d = DockModel::new();
        let target = d.spawn_master(vec![vec![A, D]], rect());
        let source = d.spawn_master(vec![vec![B, C]], rect());
        assert!(d.merge_groups((source, 0), (target, 0), 1));
        assert_eq!(d.master(target).unwrap().groups[0].panels, vec![A, B, C, D]);
        // The source master had exactly one group, now empty — it's gone.
        assert!(d.master(source).is_none());
    }

    #[test]
    fn detach_group_pulls_just_one_group_into_its_own_new_master() {
        let mut d = DockModel::new();
        let orig = d.spawn_master(vec![vec![A], vec![B]], rect());
        let nid = d.detach_group(orig, 1, rect()).unwrap();
        assert_eq!(d.master(orig).unwrap().groups.len(), 1);
        assert_eq!(d.master(nid).unwrap().groups[0].panels, vec![B]);
    }

    #[test]
    fn detach_group_removes_the_origin_master_when_it_was_its_last_group() {
        let mut d = DockModel::new();
        let orig = d.spawn_master(vec![vec![A]], rect());
        d.detach_group(orig, 0, rect()).unwrap();
        assert!(d.master(orig).is_none());
    }

    #[test]
    fn a_docked_master_disappears_too_once_its_last_panel_is_removed() {
        // There is no such thing as an empty Master — docked or
        // floating, losing its last panel removes it outright.
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A]], rect());
        d.dock_master(m, Side::Right, 0);
        d.remove(A);
        assert!(d.master(m).is_none());
    }

    #[test]
    fn removing_a_docked_masters_last_panel_reflows_its_side() {
        let mut d = DockModel::new();
        let m1 = d.spawn_master(vec![vec![A]], rect());
        let m2 = d.spawn_master(vec![vec![B]], rect());
        d.dock_master(m1, Side::Right, 0);
        d.dock_master(m2, Side::Right, 1);
        d.remove(A);
        assert!(d.master(m1).is_none());
        assert_eq!(d.docked(Side::Right), vec![m2]);
        assert_eq!(d.master(m2).unwrap().dock, Some((Side::Right, 0)));
    }

    #[test]
    fn a_floating_master_disappears_once_its_last_panel_is_removed() {
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A]], rect());
        d.remove(A);
        assert!(d.master(m).is_none());
    }

    #[test]
    fn detach_panel_wraps_it_in_a_fresh_group_and_master() {
        let mut d = DockModel::new();
        let orig = d.spawn_master(vec![vec![A, B]], rect());
        let nid = d.detach_panel(B, rect()).unwrap();
        assert_eq!(d.master(orig).unwrap().groups[0].panels, vec![A]);
        assert_eq!(d.master(nid).unwrap().groups[0].panels, vec![B]);
    }

    #[test]
    fn detach_panel_keeps_the_source_masters_display_mode() {
        let mut d = DockModel::new();
        let orig = d.spawn_master(vec![vec![A, B]], rect());
        d.master_mut(orig).unwrap().layout = MasterLayout::Tabs;
        let nid = d.detach_panel(B, rect()).unwrap();
        assert_eq!(d.master(nid).unwrap().layout, MasterLayout::Tabs);

        // And the reverse: a Stack-mode source stays Stack.
        let orig2 = d.spawn_master(vec![vec![C, D]], rect());
        d.master_mut(orig2).unwrap().layout = MasterLayout::Stack;
        let nid2 = d.detach_panel(D, rect()).unwrap();
        assert_eq!(d.master(orig2).unwrap().layout, MasterLayout::Stack);
        assert_eq!(d.master(nid2).unwrap().layout, MasterLayout::Stack);
    }

    #[test]
    fn detach_group_keeps_the_source_masters_display_mode() {
        let mut d = DockModel::new();
        let orig = d.spawn_master(vec![vec![A], vec![B]], rect());
        d.master_mut(orig).unwrap().layout = MasterLayout::Tabs;
        let nid = d.detach_group(orig, 1, rect()).unwrap();
        assert_eq!(d.master(nid).unwrap().layout, MasterLayout::Tabs);
    }

    #[test]
    fn moving_a_lone_floating_masters_only_panel_into_another_group_removes_it_entirely() {
        // The exact "single panel into a group" repro: a floating master
        // that holds nothing but this one panel must not survive as an
        // empty shell once it's dragged elsewhere — there is no such
        // thing as an empty Master.
        let mut d = DockModel::new();
        let source = d.spawn_master(vec![vec![A]], rect());
        let dest = d.spawn_master(vec![vec![B]], rect());
        assert!(d.move_panel_into_group(A, (dest, 0), 1));
        assert!(d.master(source).is_none(), "the now-empty source master must be gone");
        assert_eq!(d.master(dest).unwrap().groups[0].panels, vec![B, A]);
    }

    #[test]
    fn prune_all_empty_sweeps_every_master_not_just_one() {
        let mut d = DockModel::new();
        let a = d.spawn_master(vec![vec![A]], rect());
        let b = d.spawn_master(vec![vec![B]], rect());
        // Force both into the "somehow ended up with an empty group"
        // state directly, bypassing the normal mutators, to prove the
        // sweep — not any one mutator's own bookkeeping — is what fixes
        // it here.
        d.master_mut(a).unwrap().groups[0].panels.clear();
        d.master_mut(b).unwrap().groups[0].panels.clear();
        d.prune_all_empty();
        assert!(d.master(a).is_none());
        assert!(d.master(b).is_none());
    }

    #[test]
    fn move_panel_into_group_reorders_within_the_same_group() {
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A, B, C]], rect());
        assert!(d.move_panel_into_group(C, (m, 0), 0));
        assert_eq!(d.master(m).unwrap().groups[0].panels, vec![C, A, B]);
    }

    #[test]
    fn move_panel_into_group_moves_across_groups_and_prunes_the_old_one() {
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A], vec![B]], rect());
        assert!(d.move_panel_into_group(A, (m, 1), 0));
        assert_eq!(d.master(m).unwrap().groups.len(), 1);
        assert_eq!(d.master(m).unwrap().groups[0].panels, vec![A, B]);
    }

    #[test]
    fn move_panel_new_group_wraps_it_as_a_sibling_group_in_the_target_master() {
        let mut d = DockModel::new();
        let src = d.spawn_master(vec![vec![A, B]], rect());
        let dest = d.spawn_master(vec![vec![C]], rect());
        assert!(d.move_panel_new_group(B, dest, 1));
        assert_eq!(d.master(src).unwrap().groups[0].panels, vec![A]);
        let groups: Vec<_> = d.master(dest).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec![C], vec![B]]);
    }

    #[test]
    fn move_group_moves_a_whole_group_to_another_master_at_a_position() {
        let mut d = DockModel::new();
        let src = d.spawn_master(vec![vec![A], vec![B]], rect());
        let dest = d.spawn_master(vec![vec![C]], rect());
        assert!(d.move_group((src, 1), dest, 0));
        assert_eq!(d.master(src).unwrap().groups.len(), 1);
        let groups: Vec<_> = d.master(dest).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec![B], vec![C]]);
    }

    #[test]
    fn move_group_reorders_within_the_same_master() {
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A], vec![B], vec![C]], rect());
        assert!(d.move_group((m, 2), m, 0));
        let groups: Vec<_> = d.master(m).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec![C], vec![A], vec![B]]);
    }

    #[test]
    fn locate_finds_a_panel_by_master_and_group_and_slot() {
        let mut d = DockModel::new();
        let m = d.spawn_master(vec![vec![A], vec![B, C]], rect());
        assert_eq!(d.locate(C), Some((m, 1, 1)));
        assert_eq!(d.locate(D), None);
    }

    #[test]
    fn spawn_tools_master_has_no_groups_and_reports_as_tools() {
        let mut d = DockModel::new();
        let m = d.spawn_tools_master(rect());
        assert!(d.master(m).unwrap().is_tools());
        assert!(d.master(m).unwrap().groups.is_empty());
        // A Tools master's emptiness check never fires (it never has
        // groups to begin with) — `remove`/`prune_empty` must leave it be.
        d.prune_empty(m);
        assert!(d.master(m).is_some());
    }
}
