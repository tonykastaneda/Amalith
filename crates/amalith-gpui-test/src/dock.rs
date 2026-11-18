//! The dock model, trimmed from `amalith-shell/src/dock.rs`: the same
//! Master → Group → Panel hierarchy and the same mutators, minus the
//! Tools master, Stack mode, serde and the real panel registry. Pure data,
//! no GPUI.

/// A panel is just its label here; no real panel bodies are ported.
pub type PanelId = &'static str;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub id: u64,
    pub panels: Vec<PanelId>,
    pub active: usize,
}

/// One on-screen unit: docked to a rail edge, or floating in its own OS
/// window. The same entity either way.
#[derive(Clone, Debug, PartialEq)]
pub struct Master {
    pub id: u64,
    pub groups: Vec<Group>,
    /// `Some((side, index))` while docked; index 0 is the outermost column.
    pub dock: Option<(Side, usize)>,
    /// `[x, y, w, h]`, screen-space logical px. While docked only `w` matters.
    pub rect: [f32; 4],
}

impl Master {
    pub fn panels(&self) -> Vec<PanelId> {
        self.groups.iter().flat_map(|g| g.panels.iter().copied()).collect()
    }

    pub fn locate(&self, panel: PanelId) -> Option<(usize, usize)> {
        self.groups.iter().enumerate().find_map(|(gi, g)| {
            g.panels.iter().position(|&p| p == panel).map(|pi| (gi, pi))
        })
    }

    pub fn title(&self) -> String {
        self.panels().join(" · ")
    }
}

#[derive(Clone, Debug, PartialEq)]
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
    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn new_group(&mut self, panels: Vec<PanelId>) -> Group {
        Group { id: self.alloc_id(), panels, active: 0 }
    }

    pub fn spawn_master(&mut self, panel_groups: Vec<Vec<PanelId>>, rect: [f32; 4]) -> u64 {
        let id = self.alloc_id();
        let groups = panel_groups.into_iter().map(|p| self.new_group(p)).collect();
        self.masters.push(Master { id, groups, dock: None, rect });
        id
    }

    pub fn master(&self, id: u64) -> Option<&Master> {
        self.masters.iter().find(|m| m.id == id)
    }

    pub fn master_mut(&mut self, id: u64) -> Option<&mut Master> {
        self.masters.iter_mut().find(|m| m.id == id)
    }

    pub fn contains(&self, panel: PanelId) -> bool {
        self.locate(panel).is_some()
    }

    pub fn locate(&self, panel: PanelId) -> Option<(u64, usize, usize)> {
        self.masters.iter().find_map(|m| m.locate(panel).map(|(g, p)| (m.id, g, p)))
    }

    /// Every master docked to `side`, outermost first.
    pub fn docked(&self, side: Side) -> Vec<u64> {
        let mut v: Vec<&Master> =
            self.masters.iter().filter(|m| m.dock.is_some_and(|(s, _)| s == side)).collect();
        v.sort_by_key(|m| m.dock.unwrap().1);
        v.into_iter().map(|m| m.id).collect()
    }

    fn reflow(&mut self, side: Side) {
        for (i, id) in self.docked(side).into_iter().enumerate() {
            if let Some(m) = self.master_mut(id) {
                m.dock = Some((side, i));
            }
        }
    }

    pub fn dock_master(&mut self, id: u64, side: Side, index: usize) {
        let prev = self.master(id).and_then(|m| m.dock).map(|(s, _)| s);
        let mut list: Vec<u64> = self.docked(side).into_iter().filter(|&x| x != id).collect();
        list.insert(index.min(list.len()), id);
        for (i, mid) in list.into_iter().enumerate() {
            if let Some(m) = self.master_mut(mid) {
                m.dock = Some((side, i));
            }
        }
        if let Some(prev) = prev.filter(|&p| p != side) {
            self.reflow(prev);
        }
    }

    pub fn undock(&mut self, id: u64) {
        let Some(side) = self.master(id).and_then(|m| m.dock).map(|(s, _)| s) else { return };
        if let Some(m) = self.master_mut(id) {
            m.dock = None;
        }
        self.reflow(side);
    }

    pub fn remove_master(&mut self, id: u64) {
        let side = self.master(id).and_then(|m| m.dock).map(|(s, _)| s);
        self.masters.retain(|m| m.id != id);
        if let Some(side) = side {
            self.reflow(side);
        }
    }

    /// No empty groups, no empty masters — docked or floating.
    pub fn prune_empty(&mut self, id: u64) {
        let Some(m) = self.master_mut(id) else { return };
        m.groups.retain(|g| !g.panels.is_empty());
        for g in &mut m.groups {
            g.active = g.active.min(g.panels.len() - 1);
        }
        if m.groups.is_empty() {
            self.remove_master(id);
        }
    }

    pub fn prune_all_empty(&mut self) {
        let ids: Vec<u64> = self.masters.iter().map(|m| m.id).collect();
        for id in ids {
            self.prune_empty(id);
        }
    }

    pub fn remove(&mut self, panel: PanelId) -> bool {
        let Some((mid, gi, _)) = self.locate(panel) else { return false };
        if let Some(g) = self.master_mut(mid).and_then(|m| m.groups.get_mut(gi)) {
            g.panels.retain(|&p| p != panel);
        }
        self.prune_empty(mid);
        true
    }

    /// Splices `source`'s whole group list into `target` at `at`.
    pub fn merge_masters(&mut self, source: u64, target: u64, at: usize) -> bool {
        if source == target || self.master(target).is_none() {
            return false;
        }
        let Some(pos) = self.masters.iter().position(|m| m.id == source) else { return false };
        let removed = self.masters.remove(pos);
        let side = removed.dock.map(|(s, _)| s);
        let t = self.master_mut(target).unwrap();
        let at = at.min(t.groups.len());
        t.groups.splice(at..at, removed.groups);
        if let Some(side) = side {
            self.reflow(side);
        }
        true
    }

    /// Appends `source` group's panels into `dest` group's tab list at `at`.
    pub fn merge_groups(&mut self, source: (u64, usize), dest: (u64, usize), at: usize) -> bool {
        if source == dest {
            return false;
        }
        let Some(panels) = self.master(source.0).and_then(|m| m.groups.get(source.1)).map(|g| g.panels.clone())
        else {
            return false;
        };
        let Some(g) = self.master_mut(dest.0).and_then(|m| m.groups.get_mut(dest.1)) else { return false };
        let at = at.min(g.panels.len());
        g.panels.splice(at..at, panels);
        g.active = at;
        if let Some(g) = self.master_mut(source.0).and_then(|m| m.groups.get_mut(source.1)) {
            g.panels.clear();
        }
        self.prune_empty(source.0);
        true
    }

    /// Moves a whole group into `dest_master` as a new sibling at `at`.
    pub fn move_group(&mut self, source: (u64, usize), dest_master: u64, at: usize) -> bool {
        if self.master(dest_master).is_none() {
            return false;
        }
        let Some(m) = self.master_mut(source.0) else { return false };
        if source.1 >= m.groups.len() {
            return false;
        }
        let g = m.groups.remove(source.1);
        let dest = self.master_mut(dest_master).unwrap();
        let at = at.min(dest.groups.len());
        dest.groups.insert(at, g);
        self.prune_empty(source.0);
        true
    }

    pub fn detach_group(&mut self, master: u64, at: usize, rect: [f32; 4]) -> Option<u64> {
        let m = self.master_mut(master)?;
        if at >= m.groups.len() {
            return None;
        }
        let g = m.groups.remove(at);
        self.prune_empty(master);
        let id = self.alloc_id();
        self.masters.push(Master { id, groups: vec![g], dock: None, rect });
        Some(id)
    }

    pub fn detach_panel(&mut self, panel: PanelId, rect: [f32; 4]) -> Option<u64> {
        if !self.remove(panel) {
            return None;
        }
        Some(self.spawn_master(vec![vec![panel]], rect))
    }

    pub fn move_panel_into_group(&mut self, panel: PanelId, dest: (u64, usize), at: usize) -> bool {
        let Some((old_m, old_g, old_i)) = self.locate(panel) else { return false };
        // Resolve the destination by stable id: removing the panel may prune
        // its old group and shift every later index.
        let Some(dest_gid) = self.master(dest.0).and_then(|m| m.groups.get(dest.1)).map(|g| g.id) else {
            return false;
        };
        let at = if (old_m, old_g) == dest && old_i < at { at - 1 } else { at };
        self.remove(panel);
        let Some(g) = self.master_mut(dest.0).and_then(|m| m.groups.iter_mut().find(|g| g.id == dest_gid))
        else {
            return false;
        };
        let at = at.min(g.panels.len());
        g.panels.insert(at, panel);
        g.active = at;
        true
    }

    pub fn move_panel_new_group(&mut self, panel: PanelId, dest_master: u64, at: usize) -> bool {
        if !self.contains(panel) || self.master(dest_master).is_none() {
            return false;
        }
        // Count the dest's groups before removal shifts them.
        let at = match self.locate(panel) {
            Some((m, g, _)) if m == dest_master && g < at && self.master(m).unwrap().groups[g].panels.len() == 1 => {
                at - 1
            }
            _ => at,
        };
        self.remove(panel);
        let g = self.new_group(vec![panel]);
        let Some(m) = self.master_mut(dest_master) else { return false };
        let at = at.min(m.groups.len());
        m.groups.insert(at, g);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: [f32; 4] = [0.0, 0.0, 200.0, 100.0];

    #[test]
    fn docking_orders_and_undock_reflows() {
        let mut d = DockModel::default();
        let a = d.spawn_master(vec![vec!["a"]], R);
        let b = d.spawn_master(vec![vec!["b"]], R);
        d.dock_master(a, Side::Left, 0);
        d.dock_master(b, Side::Left, 0);
        assert_eq!(d.docked(Side::Left), vec![b, a]);
        d.undock(b);
        assert_eq!(d.master(a).unwrap().dock, Some((Side::Left, 0)));
    }

    #[test]
    fn moving_a_lone_panel_out_removes_its_master() {
        let mut d = DockModel::default();
        let src = d.spawn_master(vec![vec!["a"]], R);
        let dst = d.spawn_master(vec![vec!["b"]], R);
        assert!(d.move_panel_into_group("a", (dst, 0), 1));
        assert!(d.master(src).is_none());
        assert_eq!(d.master(dst).unwrap().groups[0].panels, vec!["b", "a"]);
    }

    #[test]
    fn reorder_within_a_group_accounts_for_its_own_slot() {
        let mut d = DockModel::default();
        let m = d.spawn_master(vec![vec!["a", "b", "c"]], R);
        // Dropped before "c" (slot 2) — lands between b and c.
        assert!(d.move_panel_into_group("a", (m, 0), 2));
        assert_eq!(d.master(m).unwrap().groups[0].panels, vec!["b", "a", "c"]);
    }

    #[test]
    fn new_group_below_its_own_lone_group_lands_in_the_right_slot() {
        let mut d = DockModel::default();
        let m = d.spawn_master(vec![vec!["a"], vec!["b"], vec!["c"]], R);
        assert!(d.move_panel_new_group("a", m, 2));
        let groups: Vec<_> = d.master(m).unwrap().groups.iter().map(|g| g.panels.clone()).collect();
        assert_eq!(groups, vec![vec!["b"], vec!["a"], vec!["c"]]);
    }

    #[test]
    fn detach_group_and_merge_masters_round_trip() {
        let mut d = DockModel::default();
        let m = d.spawn_master(vec![vec!["a"], vec!["b"]], R);
        let n = d.detach_group(m, 1, R).unwrap();
        assert_eq!(d.master(n).unwrap().panels(), vec!["b"]);
        assert!(d.merge_masters(n, m, 0));
        assert_eq!(d.master(m).unwrap().panels(), vec!["b", "a"]);
    }
}
