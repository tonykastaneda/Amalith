//! The dock model: a pure layout tree, no rendering, no windowing.
//!
//! This is the replicable core of the panel system. It knows nothing about
//! Artboards or Layers — only opaque [`PanelId`]s. The shell renders it and
//! spawns OS windows for its [`DockModel::floating`] groups; this module
//! just answers "where does everything sit" and "if I drop here, what
//! happens".
//!
//! Layout is one tree of [`Node`]s per surface: a [`Rail`] on each side of
//! the document window, plus one tree per detached [`Floating`] group.
//! Splits carry child weights; tab groups carry an active index. Every
//! mutation is a small, testable operation.

use std::collections::VecDeque;

/// Opaque, stable identifier for a panel kind. The app maps these to real
/// panels via its registry; the dock never dereferences one.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PanelId(pub &'static str);

/// A node in a dock tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// A row (`Horizontal`) or column (`Vertical`) of children, each with a
    /// weight; weights are normalized on read, so any positive values work.
    Split { axis: Axis, children: Vec<Child> },
    /// A tab group: one or more panels, one shown at a time.
    Tabs { panels: Vec<PanelId>, active: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Child {
    pub node: Node,
    /// Relative size along the parent split's axis. Only ratios matter.
    pub weight: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Axis {
    Horizontal,
    Vertical,
}

/// Where in a surface's tree a drag would land.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DropTarget {
    /// Add the dragged panel as a tab of the group at `path`.
    Tab { path: NodePath, index: usize },
    /// Split the node at `path`, placing the dragged panel on `side`.
    Split { path: NodePath, side: Side },
    /// Nothing under the cursor — the panel floats.
    Float,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn axis(self) -> Axis {
        match self {
            Side::Left | Side::Right => Axis::Horizontal,
            Side::Top | Side::Bottom => Axis::Vertical,
        }
    }
    fn is_leading(self) -> bool {
        matches!(self, Side::Left | Side::Top)
    }
}

/// A path from a tree root to a node: the child index at each level.
/// Empty path == the root itself.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct NodePath(pub Vec<usize>);

/// A detached group living in its own OS window. The shell keeps a
/// `id -> winit WindowId` map alongside; the model itself never touches
/// windowing.
#[derive(Clone, Debug, PartialEq)]
pub struct Floating {
    /// Stable within a `DockModel` for the life of the group.
    pub id: u64,
    pub node: Node,
    /// Top-left + size of the OS window, in virtual-desktop logical points:
    /// `[x, y, w, h]`.
    pub rect: [f32; 4],
}

/// Which edge of the document window a rail sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailSide {
    Left,
    Right,
}

/// Default rail width, logical points.
pub const RAIL_DEFAULT_W: f32 = 320.0;

/// One docked column: a single [`Node`] tree (or empty) plus how wide the
/// whole rail is. All the tree mechanics live here so every rail — left,
/// right, and any future one — behaves identically.
#[derive(Clone, Debug, PartialEq)]
pub struct Rail {
    pub tree: Option<Node>,
    /// Rail width in logical points; the user drags the rail's inner edge
    /// to change it.
    pub width: f32,
}

impl Default for Rail {
    fn default() -> Self {
        Self {
            tree: None,
            width: RAIL_DEFAULT_W,
        }
    }
}

impl Rail {
    pub fn with(node: Node) -> Self {
        Self {
            tree: Some(node),
            width: RAIL_DEFAULT_W,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_none()
    }

    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = Vec::new();
        if let Some(t) = &self.tree {
            collect(t, &mut out);
        }
        out
    }

    /// Removes `panel`, pruning empty tab groups and collapsing single-child
    /// splits; empties the rail if that was the last panel.
    pub fn remove(&mut self, panel: PanelId) -> bool {
        let Some(t) = &mut self.tree else {
            return false;
        };
        let hit = remove_in(t, panel);
        if node_is_empty(t) {
            self.tree = None;
        }
        hit
    }

    /// Applies a resolved drop of `panel` onto this rail. `Float` is a
    /// no-op here (the caller detaches instead).
    pub fn dock(&mut self, panel: PanelId, target: DropTarget) {
        self.remove(panel);
        match target {
            DropTarget::Float => {}
            DropTarget::Tab { path, index } => {
                let Some(t) = &mut self.tree else {
                    self.tree = Some(Node::Tabs {
                        panels: vec![panel],
                        active: 0,
                    });
                    return;
                };
                if let Some(Node::Tabs { panels, active }) = node_at_mut(t, &path) {
                    let i = index.min(panels.len());
                    panels.insert(i, panel);
                    *active = i;
                }
            }
            DropTarget::Split { path, side } => {
                let Some(t) = self.tree.take() else {
                    self.tree = Some(Node::Tabs {
                        panels: vec![panel],
                        active: 0,
                    });
                    return;
                };
                self.tree = Some(split_at(t, &path, side, panel));
            }
        }
    }

    /// Make tab `index` active in the tab group at `path`. `true` if `path`
    /// pointed at a tab group.
    pub fn activate_tab(&mut self, path: &NodePath, index: usize) -> bool {
        let Some(t) = &mut self.tree else {
            return false;
        };
        if let Some(Node::Tabs { panels, active }) = node_at_mut(t, path) {
            if !panels.is_empty() {
                *active = index.min(panels.len() - 1);
            }
            true
        } else {
            false
        }
    }

    /// Move the boundary after child `gap` of the split at `path` so child
    /// `gap` takes `frac` (clamped 5%–95%) of that pair's combined span;
    /// their weight sum is preserved so other children hold their size.
    pub fn set_boundary(&mut self, path: &NodePath, gap: usize, frac: f32) -> bool {
        let Some(t) = &mut self.tree else {
            return false;
        };
        let Some(Node::Split { children, .. }) = node_at_mut(t, path) else {
            return false;
        };
        if gap + 1 >= children.len() {
            return false;
        }
        let frac = frac.clamp(0.05, 0.95);
        let pair = children[gap].weight.max(0.0) + children[gap + 1].weight.max(0.0);
        let pair = if pair <= 0.0 { 2.0 } else { pair };
        children[gap].weight = pair * frac;
        children[gap + 1].weight = pair * (1.0 - frac);
        true
    }

    /// Set the rail's width, feeding the whole change to one edge column so
    /// the others keep their pixel size.
    ///
    /// If the rail's top node is a horizontal split of two-or-more columns,
    /// every column but the one on the resized edge is pinned to its
    /// current width and the edge column absorbs the delta. `edge_is_last`
    /// says which column touches the moving edge (the right rail resizes
    /// from its left edge → first column; the left rail from its right edge
    /// → last column). `gap` is the splitter thickness. With a single
    /// column or a vertical stack this is just a width change.
    pub fn set_width_absorbing(&mut self, new_w: f32, gap: f32, edge_is_last: bool) {
        let old_w = self.width;
        self.width = new_w.max(1.0);

        let Some(Node::Split {
            axis: Axis::Horizontal,
            children,
        }) = &mut self.tree
        else {
            return;
        };
        let n = children.len();
        if n < 2 {
            return;
        }
        let wsum: f32 = children.iter().map(|c| c.weight.max(0.0)).sum();
        if wsum <= 0.0 {
            return;
        }
        let strut = gap * (n as f32 - 1.0);
        let old_avail = (old_w - strut).max(1.0);
        let new_avail = (new_w - strut).max(1.0);

        let px: Vec<f32> = children
            .iter()
            .map(|c| old_avail * c.weight.max(0.0) / wsum)
            .collect();
        let absorb = if edge_is_last { n - 1 } else { 0 };
        let pinned: f32 = px
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != absorb)
            .map(|(_, v)| *v)
            .sum();
        let absorb_px = (new_avail - pinned).max(48.0);

        // Weights become target pixel widths; layout then reproduces them.
        for (i, c) in children.iter_mut().enumerate() {
            c.weight = if i == absorb { absorb_px } else { px[i] };
        }
    }

    /// First tab group a breadth-first walk finds — a fallback drop spot.
    pub fn any_tab_path(&self) -> Option<NodePath> {
        let t = self.tree.as_ref()?;
        walk(t)
            .into_iter()
            .find_map(|(path, node)| matches!(node, Node::Tabs { .. }).then_some(path))
    }

    /// Path to the tab group currently holding `panel`.
    pub fn path_of(&self, panel: PanelId) -> Option<NodePath> {
        let t = self.tree.as_ref()?;
        walk(t).into_iter().find_map(|(path, node)| match node {
            Node::Tabs { panels, .. } if panels.contains(&panel) => Some(path),
            _ => None,
        })
    }

    fn tab_len(&self, path: &NodePath) -> Option<usize> {
        let t = self.tree.as_ref()?;
        walk(t).into_iter().find_map(|(p, node)| {
            (&p == path).then_some(node).and_then(|n| match n {
                Node::Tabs { panels, .. } => Some(panels.len()),
                _ => None,
            })
        })
    }
}

/// The whole panel layout: a rail on each side of the canvas, plus any
/// detached groups.
#[derive(Clone, Debug, PartialEq)]
pub struct DockModel {
    pub left: Rail,
    pub right: Rail,
    pub floating: Vec<Floating>,
    next_id: u64,
}

impl DockModel {
    /// New model with `right` populating the right rail and an empty left.
    pub fn new(right: Node) -> Self {
        Self {
            left: Rail::default(),
            right: Rail::with(right),
            floating: Vec::new(),
            next_id: 1,
        }
    }

    pub fn rail(&self, side: RailSide) -> &Rail {
        match side {
            RailSide::Left => &self.left,
            RailSide::Right => &self.right,
        }
    }

    pub fn rail_mut(&mut self, side: RailSide) -> &mut Rail {
        match side {
            RailSide::Left => &mut self.left,
            RailSide::Right => &mut self.right,
        }
    }

    /// Every panel placed anywhere, in an arbitrary but stable order.
    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = self.left.panels();
        out.extend(self.right.panels());
        for f in &self.floating {
            collect(&f.node, &mut out);
        }
        out
    }

    pub fn contains(&self, panel: PanelId) -> bool {
        self.panels().contains(&panel)
    }

    /// Removes `panel` from wherever it sits — either rail or a floating
    /// group (empty floating groups are dropped).
    pub fn remove(&mut self, panel: PanelId) -> bool {
        let mut removed = self.left.remove(panel);
        removed |= self.right.remove(panel);
        self.floating.retain_mut(|f| {
            let hit = remove_in(&mut f.node, panel);
            removed |= hit;
            !node_is_empty(&f.node)
        });
        removed
    }

    /// Detaches `panel` into a new single-tab floating group at `rect`.
    /// Returns the new group's id, or `None` if the panel wasn't placed.
    pub fn detach(&mut self, panel: PanelId, rect: [f32; 4]) -> Option<u64> {
        if !self.contains(panel) {
            return None;
        }
        self.remove(panel);
        let id = self.next_id;
        self.next_id += 1;
        self.floating.push(Floating {
            id,
            node: Node::Tabs {
                panels: vec![panel],
                active: 0,
            },
            rect,
        });
        Some(id)
    }

    pub fn floating(&self, id: u64) -> Option<&Floating> {
        self.floating.iter().find(|f| f.id == id)
    }

    pub fn floating_mut(&mut self, id: u64) -> Option<&mut Floating> {
        self.floating.iter_mut().find(|f| f.id == id)
    }

    /// Removes and returns the floating group `id`.
    pub fn remove_floating(&mut self, id: u64) -> Option<Floating> {
        let i = self.floating.iter().position(|f| f.id == id)?;
        Some(self.floating.remove(i))
    }

    /// Docks floating group `id` into the `side` rail: its active panel
    /// lands at `target`, siblings tab in beside it. Returns the re-docked
    /// panels; empty if `id` is unknown or `target` is `Float`.
    pub fn redock(&mut self, id: u64, side: RailSide, target: DropTarget) -> Vec<PanelId> {
        if matches!(target, DropTarget::Float) {
            return Vec::new();
        }
        let Some(f) = self.remove_floating(id) else {
            return Vec::new();
        };
        // Width the group had while floating. It's used only when this
        // dock ADDS a column — stacking a panel onto an existing column
        // just adopts that column's width for visual consistency.
        let float_w = f.rect[2].max(1.0);
        let adds_column = matches!(target, DropTarget::Split { .. });
        const GAP: f32 = 6.0;
        let mut panels = Vec::new();
        collect(&f.node, &mut panels);
        if let Node::Tabs { active, .. } = &f.node {
            if *active < panels.len() {
                panels.swap(0, *active);
            }
        }
        let rail = self.rail_mut(side);

        // Pixel widths of the columns already in the rail, before docking.
        let existing_px: Vec<f32> = match &rail.tree {
            Some(Node::Split {
                axis: Axis::Horizontal,
                children,
            }) if !children.is_empty() => {
                let wsum: f32 = children
                    .iter()
                    .map(|c| c.weight.max(0.0))
                    .sum::<f32>()
                    .max(1e-3);
                let avail = (rail.width - GAP * (children.len() as f32 - 1.0)).max(1.0);
                children
                    .iter()
                    .map(|c| (avail * c.weight.max(0.0) / wsum).max(48.0))
                    .collect()
            }
            Some(_) => vec![rail.width.max(48.0)],
            None => Vec::new(),
        };

        let mut it = panels.iter().copied();
        if let Some(first) = it.next() {
            rail.dock(first, target);
        }
        for p in it {
            if let Some(path) = rail.path_of(panels[0]) {
                let len = rail.tab_len(&path).unwrap_or(0);
                rail.dock(p, DropTarget::Tab { path, index: len });
            }
        }

        if existing_px.is_empty() {
            // First panel in a previously-empty rail — take its own width.
            rail.width = float_w;
        } else if adds_column {
            // New column: it gets `float_w`, every existing column keeps
            // its pixel width, and the rail grows to fit.
            if let Some(Node::Split {
                axis: Axis::Horizontal,
                children,
            }) = &mut rail.tree
            {
                if children.len() == existing_px.len() + 1 {
                    let n = children.len();
                    let new_idx = children
                        .iter()
                        .position(|c| {
                            let mut v = Vec::new();
                            collect(&c.node, &mut v);
                            v.contains(&panels[0])
                        })
                        .unwrap_or(n - 1);
                    let mut old = existing_px.iter().copied();
                    for (i, c) in children.iter_mut().enumerate() {
                        c.weight = if i == new_idx {
                            float_w
                        } else {
                            old.next().unwrap_or(48.0)
                        };
                    }
                    rail.width =
                        float_w + existing_px.iter().sum::<f32>() + GAP * (n as f32 - 1.0);
                }
            }
        }
        // Stacking onto an existing column: nothing to do — the panel
        // adopts that column's width and the rail width is unchanged.

        panels
    }

    /// Round-trip for workspace persistence lives in the app for now; the
    /// tree derives `serde` when that crate is added.
    #[doc(hidden)]
    pub fn _assert_shape(&self) {}
}

fn collect(node: &Node, out: &mut Vec<PanelId>) {
    match node {
        Node::Tabs { panels, .. } => out.extend(panels.iter().copied()),
        Node::Split { children, .. } => {
            for c in children {
                collect(&c.node, out);
            }
        }
    }
}

fn node_is_empty(node: &Node) -> bool {
    match node {
        Node::Tabs { panels, .. } => panels.is_empty(),
        Node::Split { children, .. } => children.is_empty(),
    }
}

/// Removes `panel` from `node`'s subtree and normalizes: drop empty tab
/// groups, and replace a split that has one child left with that child.
fn remove_in(node: &mut Node, panel: PanelId) -> bool {
    match node {
        Node::Tabs { panels, active } => {
            if let Some(pos) = panels.iter().position(|p| *p == panel) {
                panels.remove(pos);
                *active = (*active).min(panels.len().saturating_sub(1));
                true
            } else {
                false
            }
        }
        Node::Split { children, .. } => {
            let mut hit = false;
            for c in children.iter_mut() {
                hit |= remove_in(&mut c.node, panel);
            }
            children.retain(|c| !node_is_empty(&c.node));
            if children.len() == 1 {
                let only = children.remove(0).node;
                *node = only;
            }
            hit
        }
    }
}

fn node_at_mut<'a>(node: &'a mut Node, path: &NodePath) -> Option<&'a mut Node> {
    let mut cur = node;
    for &i in &path.0 {
        match cur {
            Node::Split { children, .. } => cur = &mut children.get_mut(i)?.node,
            Node::Tabs { .. } => return None,
        }
    }
    Some(cur)
}

/// Splits the node reached by `path` (from `root`), inserting `panel` on
/// `side`. If the target's parent split already runs on the needed axis,
/// the new tab group is spliced in as a sibling instead of nesting.
fn split_at(mut root: Node, path: &NodePath, side: Side, panel: PanelId) -> Node {
    let new_tab = Node::Tabs {
        panels: vec![panel],
        active: 0,
    };
    if path.0.is_empty() {
        return wrap_split(root, new_tab, side);
    }
    // Walk to the parent of the target.
    let (parent_path, last) = path.0.split_at(path.0.len() - 1);
    let last = last[0];
    if let Some(Node::Split { axis, children }) =
        node_at_mut(&mut root, &NodePath(parent_path.to_vec()))
    {
        if *axis == side.axis() {
            let at = if side.is_leading() { last } else { last + 1 };
            let at = at.min(children.len());
            children.insert(
                at,
                Child {
                    node: new_tab,
                    weight: 1.0,
                },
            );
            return root;
        }
    }
    // Otherwise nest a fresh split around the target node.
    if let Some(target) = node_at_mut(&mut root, path) {
        let taken = std::mem::replace(
            target,
            Node::Tabs {
                panels: vec![],
                active: 0,
            },
        );
        *target = wrap_split(taken, new_tab, side);
    }
    root
}

fn wrap_split(existing: Node, incoming: Node, side: Side) -> Node {
    let (a, b) = if side.is_leading() {
        (incoming, existing)
    } else {
        (existing, incoming)
    };
    Node::Split {
        axis: side.axis(),
        children: vec![
            Child {
                node: a,
                weight: 1.0,
            },
            Child {
                node: b,
                weight: 1.0,
            },
        ],
    }
}

/// Breadth-first walk yielding `(path, &node)` for every node in a tree.
pub fn walk(root: &Node) -> Vec<(NodePath, &Node)> {
    let mut out = Vec::new();
    let mut q: VecDeque<(NodePath, &Node)> = VecDeque::new();
    q.push_back((NodePath::default(), root));
    while let Some((path, node)) = q.pop_front() {
        out.push((path.clone(), node));
        if let Node::Split { children, .. } = node {
            for (i, c) in children.iter().enumerate() {
                let mut p = path.clone();
                p.0.push(i);
                q.push_back((p, &c.node));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: PanelId = PanelId("a");
    const B: PanelId = PanelId("b");
    const C: PanelId = PanelId("c");

    fn tabs(ids: &[PanelId]) -> Node {
        Node::Tabs {
            panels: ids.to_vec(),
            active: 0,
        }
    }

    #[test]
    fn detach_then_dock_round_trips() {
        let mut m = DockModel::new(tabs(&[A, B]));
        m.detach(B, [10.0, 10.0, 210.0, 310.0]);
        assert_eq!(m.right.tree, Some(tabs(&[A])));
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating[0].node, tabs(&[B]));

        // Drag B back as a tab of the right rail's group; it becomes active
        // and the emptied floating group is torn down.
        let id = m.floating[0].id;
        m.redock(
            id,
            RailSide::Right,
            DropTarget::Tab {
                path: NodePath::default(),
                index: 1,
            },
        );
        assert_eq!(
            m.right.tree,
            Some(Node::Tabs {
                panels: vec![A, B],
                active: 1,
            })
        );
        assert!(
            m.floating.is_empty(),
            "floating group torn down when emptied"
        );
    }

    #[test]
    fn split_on_a_bare_root_makes_a_two_child_split() {
        let mut m = DockModel::new(tabs(&[A]));
        m.right.dock(
            B,
            DropTarget::Split {
                path: NodePath::default(),
                side: Side::Right,
            },
        );
        match m.right.tree.unwrap() {
            Node::Split { axis, children } => {
                assert_eq!(axis, Axis::Horizontal);
                assert_eq!(children.len(), 2);
                assert_eq!(children[0].node, tabs(&[A]));
                assert_eq!(children[1].node, tabs(&[B]));
            }
            _ => panic!("expected a split"),
        }
    }

    #[test]
    fn split_along_the_existing_axis_splices_a_sibling_instead_of_nesting() {
        // root = [A | B]  (horizontal). Dropping C to the right of B should
        // give [A | B | C], not [A | [B | C]].
        let mut m = DockModel::new(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B]),
                    weight: 1.0,
                },
            ],
        });
        m.right.dock(
            C,
            DropTarget::Split {
                path: NodePath(vec![1]),
                side: Side::Right,
            },
        );
        match m.right.tree.unwrap() {
            Node::Split { axis, children } => {
                assert_eq!(axis, Axis::Horizontal);
                assert_eq!(children.len(), 3);
                assert_eq!(children[2].node, tabs(&[C]));
            }
            _ => panic!("expected a flat 3-way split"),
        }
    }

    #[test]
    fn removing_the_last_panel_prunes_up_to_an_empty_model() {
        let mut m = DockModel::new(Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B]),
                    weight: 1.0,
                },
            ],
        });
        assert!(m.remove(A));
        // Split collapsed to its one remaining child.
        assert_eq!(m.right.tree, Some(tabs(&[B])));
        assert!(m.remove(B));
        assert_eq!(m.right.tree, None);
        assert!(!m.remove(B), "second remove is a no-op");
    }

    #[test]
    fn detach_hands_back_an_id_and_redock_puts_the_panel_where_asked() {
        let mut m = DockModel::new(tabs(&[A, B, C]));
        let id = m.detach(B, [40.0, 40.0, 220.0, 300.0]).expect("detached");
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating(id).unwrap().node, tabs(&[B]));

        // Redock B by splitting the root to its left.
        let moved = m.redock(
            id,
            RailSide::Right,
            DropTarget::Split {
                path: NodePath(vec![]),
                side: Side::Left,
            },
        );
        assert_eq!(moved, vec![B]);
        assert!(m.floating.is_empty());
        match m.right.tree.unwrap() {
            Node::Split { axis, children } => {
                assert_eq!(axis, Axis::Horizontal);
                assert_eq!(children[0].node, tabs(&[B]));
                assert_eq!(children[1].node, tabs(&[A, C]));
            }
            _ => panic!("expected a split with B on the left"),
        }
    }

    #[test]
    fn redock_with_a_float_target_is_a_no_op() {
        let mut m = DockModel::new(tabs(&[A, B]));
        let id = m.detach(B, [0.0; 4]).unwrap();
        assert!(m.redock(id, RailSide::Right, DropTarget::Float).is_empty());
        assert!(m.floating(id).is_some(), "still floating");
    }

    #[test]
    fn activate_tab_clamps_and_reports_hits() {
        let mut m = DockModel::new(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B, C]),
                    weight: 1.0,
                },
            ],
        });
        assert!(m.right.activate_tab(&NodePath(vec![1]), 1));
        // Out-of-range index clamps to the last tab.
        assert!(m.right.activate_tab(&NodePath(vec![1]), 9));
        match &m.right.tree {
            Some(Node::Split { children, .. }) => match &children[1].node {
                Node::Tabs { active, .. } => assert_eq!(*active, 1),
                _ => panic!(),
            },
            _ => panic!(),
        }
        // Path lands on a split, not a tab group.
        assert!(!m.right.activate_tab(&NodePath(vec![]), 0));
    }

    #[test]
    fn set_boundary_reweights_one_pair_and_leaves_the_rest() {
        let mut m = DockModel::new(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[C]),
                    weight: 1.0,
                },
            ],
        });
        // Drag the first boundary to the 25% mark: pair sum 2.0 -> 0.5 / 1.5.
        assert!(m.right.set_boundary(&NodePath(vec![]), 0, 0.25));
        match m.right.tree.unwrap() {
            Node::Split { children, .. } => {
                assert!((children[0].weight - 0.5).abs() < 1e-6);
                assert!((children[1].weight - 1.5).abs() < 1e-6);
                assert!((children[2].weight - 1.0).abs() < 1e-6);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn set_width_absorbing_pins_the_other_columns() {
        let mut rail = Rail::with(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[C]),
                    weight: 1.0,
                },
            ],
        });
        rail.width = 300.0; // three 100px columns, no splitter gap
                            // Widen from the left edge to 400: the first column absorbs the
                            // whole +100; the other two stay at 100.
        rail.set_width_absorbing(400.0, 0.0, false);
        assert_eq!(rail.width, 400.0);
        match rail.tree.unwrap() {
            Node::Split { children, .. } => {
                assert!((children[0].weight - 200.0).abs() < 1e-3);
                assert!((children[1].weight - 100.0).abs() < 1e-3);
                assert!((children[2].weight - 100.0).abs() < 1e-3);
            }
            _ => panic!("expected a horizontal split"),
        }
    }

    #[test]
    fn set_boundary_rejects_a_nonexistent_gap() {
        let mut m = DockModel::new(tabs(&[A]));
        assert!(!m.right.set_boundary(&NodePath(vec![]), 0, 0.5));
    }

    #[test]
    fn walk_yields_root_first_then_children() {
        let root = Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child {
                    node: tabs(&[A]),
                    weight: 1.0,
                },
                Child {
                    node: tabs(&[B, C]),
                    weight: 2.0,
                },
            ],
        };
        let paths: Vec<_> = walk(&root).into_iter().map(|(p, _)| p.0).collect();
        assert_eq!(paths, vec![vec![], vec![0], vec![1]]);
    }
}
