//! The dock model: a pure layout tree, no rendering, no windowing.
//!
//! This is the replicable core of the panel system. It knows nothing about
//! Artboards or Layers — only opaque [`PanelId`]s. The shell renders it and
//! spawns OS windows for its [`DockModel::floating`] groups; this module
//! just answers "where does everything sit" and "if I drop here, what
//! happens".
//!
//! Layout is one tree of [`Node`]s per surface: the main window's
//! [`DockModel::root`], plus one tree per detached [`Floating`] group.
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

/// A detached group living in its own OS window.
#[derive(Clone, Debug, PartialEq)]
pub struct Floating {
    pub node: Node,
    /// Global (virtual-desktop) screen rect, in logical points.
    pub rect: [f32; 4],
    /// Set by the shell once an OS window backs this group.
    pub window: Option<u64>,
}

/// The whole panel layout: the document window's tree plus any detached
/// groups.
#[derive(Clone, Debug, PartialEq)]
pub struct DockModel {
    pub root: Option<Node>,
    pub floating: Vec<Floating>,
}

impl DockModel {
    pub fn new(root: Node) -> Self {
        Self {
            root: Some(root),
            floating: Vec::new(),
        }
    }

    /// Every panel currently placed anywhere (main tree + floating), in an
    /// arbitrary but stable order. Used by the app to know what to build.
    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            collect(root, &mut out);
        }
        for f in &self.floating {
            collect(&f.node, &mut out);
        }
        out
    }

    /// Is `panel` placed somewhere?
    pub fn contains(&self, panel: PanelId) -> bool {
        self.panels().contains(&panel)
    }

    /// Removes `panel` wherever it is, pruning empty tab groups and
    /// collapsing single-child splits. Returns `true` if it was present.
    pub fn remove(&mut self, panel: PanelId) -> bool {
        let mut removed = false;
        if let Some(root) = &mut self.root {
            removed |= remove_in(root, panel);
        }
        if self.root.as_ref().is_some_and(node_is_empty) {
            self.root = None;
        }
        self.floating.retain_mut(|f| {
            let hit = remove_in(&mut f.node, panel);
            removed |= hit;
            !node_is_empty(&f.node)
        });
        removed
    }

    /// Detaches `panel` into a new floating group at `rect`. No-op if the
    /// panel isn't placed.
    pub fn detach(&mut self, panel: PanelId, rect: [f32; 4]) {
        if !self.contains(panel) {
            return;
        }
        self.remove(panel);
        self.floating.push(Floating {
            node: Node::Tabs {
                panels: vec![panel],
                active: 0,
            },
            rect,
            window: None,
        });
    }

    /// Applies a resolved drop of `panel` onto the main tree. `Float`
    /// targets are handled by [`Self::detach`] instead.
    pub fn dock(&mut self, panel: PanelId, target: DropTarget) {
        self.remove(panel);
        match target {
            DropTarget::Float => {}
            DropTarget::Tab { path, index } => {
                let Some(root) = &mut self.root else {
                    self.root = Some(Node::Tabs {
                        panels: vec![panel],
                        active: 0,
                    });
                    return;
                };
                if let Some(Node::Tabs { panels, active }) = node_at_mut(root, &path) {
                    let i = index.min(panels.len());
                    panels.insert(i, panel);
                    *active = i;
                }
            }
            DropTarget::Split { path, side } => {
                let Some(root) = self.root.take() else {
                    self.root = Some(Node::Tabs {
                        panels: vec![panel],
                        active: 0,
                    });
                    return;
                };
                self.root = Some(split_at(root, &path, side, panel));
            }
        }
    }

    /// Make tab `index` the active one in the tab group at `path`.
    /// Returns `true` if `path` pointed at a tab group.
    pub fn activate_tab(&mut self, path: &NodePath, index: usize) -> bool {
        let Some(root) = &mut self.root else {
            return false;
        };
        if let Some(Node::Tabs { panels, active }) = node_at_mut(root, path) {
            if !panels.is_empty() {
                *active = index.min(panels.len() - 1);
            }
            true
        } else {
            false
        }
    }

    /// Move the boundary after child `gap` of the split at `path` so that
    /// child `gap` takes `frac` (clamped to 5%–95%) of the combined span of
    /// children `gap` and `gap + 1`. Their weight sum is preserved, so every
    /// other child keeps its size. Returns `true` on success.
    pub fn set_boundary(&mut self, path: &NodePath, gap: usize, frac: f32) -> bool {
        let Some(root) = &mut self.root else {
            return false;
        };
        let Some(Node::Split { children, .. }) = node_at_mut(root, path) else {
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
        assert_eq!(m.root, Some(tabs(&[A])));
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating[0].node, tabs(&[B]));

        // Drag B back as a tab of the root group; the dropped tab becomes active.
        m.dock(
            B,
            DropTarget::Tab {
                path: NodePath::default(),
                index: 1,
            },
        );
        assert_eq!(
            m.root,
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
        m.dock(
            B,
            DropTarget::Split {
                path: NodePath::default(),
                side: Side::Right,
            },
        );
        match m.root.unwrap() {
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
        m.dock(
            C,
            DropTarget::Split {
                path: NodePath(vec![1]),
                side: Side::Right,
            },
        );
        match m.root.unwrap() {
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
        assert_eq!(m.root, Some(tabs(&[B])));
        assert!(m.remove(B));
        assert_eq!(m.root, None);
        assert!(!m.remove(B), "second remove is a no-op");
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
        assert!(m.activate_tab(&NodePath(vec![1]), 1));
        // Out-of-range index clamps to the last tab.
        assert!(m.activate_tab(&NodePath(vec![1]), 9));
        match &m.root {
            Some(Node::Split { children, .. }) => match &children[1].node {
                Node::Tabs { active, .. } => assert_eq!(*active, 1),
                _ => panic!(),
            },
            _ => panic!(),
        }
        // Path lands on a split, not a tab group.
        assert!(!m.activate_tab(&NodePath(vec![]), 0));
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
        assert!(m.set_boundary(&NodePath(vec![]), 0, 0.25));
        match m.root.unwrap() {
            Node::Split { children, .. } => {
                assert!((children[0].weight - 0.5).abs() < 1e-6);
                assert!((children[1].weight - 1.5).abs() < 1e-6);
                assert!((children[2].weight - 1.0).abs() < 1e-6);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn set_boundary_rejects_a_nonexistent_gap() {
        let mut m = DockModel::new(tabs(&[A]));
        assert!(!m.set_boundary(&NodePath(vec![]), 0, 0.5));
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
