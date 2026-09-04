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

/// A node in a dock tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// A row (`Horizontal`) or column (`Vertical`) of children, each with a
    /// weight; weights are normalized on read, so any positive values work.
    Split { axis: Axis, children: Vec<Child> },
    /// A tab group: one or more panels, one shown at a time.
    Tabs { panels: Vec<PanelId>, active: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Child {
    pub node: Node,
    /// Relative size along the parent split's axis. Only ratios matter.
    pub weight: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
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
    /// Set while this floating panel is collapsed to an icon (Illustrator
    /// states "Collapsed Icon + Title [Detached]" / "Collapse Icon Only
    /// [Detached]") — the width the OS window has actually been shrunk
    /// to, icon+title at or above `layout::ICON_LABEL_THRESHOLD`, icon
    /// only below. A floating group here is always a single panel (this
    /// app has no floating-group stacking yet), so unlike a rail's icon
    /// strip there's only ever one row — no `IconColumn`/rows needed.
    pub icon_w: Option<f32>,
    /// The OS window size to restore on expand — captured the moment the
    /// collapse chevron shrinks it, since after that `rect`'s own size
    /// tracks the shrunk icon window instead.
    pub expanded_size: Option<[f32; 2]>,
}

impl Floating {
    /// Shrinks this floating panel to its icon-strip size (Illustrator's
    /// "Collapse to Icons" for a detached panel), remembering the size to
    /// restore on [`Self::expand`]. Every tab gets its own icon row (not
    /// just the active one — matching a docked column's icon strip), plus
    /// a persistent header row above them with its own close/expand
    /// controls, so a fully collapsed group is never left with no visible
    /// way back to full size. Returns the `[w, h]` the caller should
    /// resize the actual OS window to, or `None` if already collapsed.
    pub fn collapse(&mut self) -> Option<[f32; 2]> {
        if self.icon_w.is_some() {
            return None;
        }
        self.expanded_size = Some([self.rect[2], self.rect[3]]);
        let rows = match &self.node {
            Node::Tabs { panels, .. } => panels.len().max(1),
            Node::Split { .. } => 1,
        } as f32;
        // Matches layout::ICON_COL_W / ICON_ROW_H and theme::group_title_h
        // — bare literals here rather than imports, to avoid a dock <->
        // layout <-> theme cycle; all three describe the same
        // "header + one row per tab" geometry the icon strip itself uses.
        const HEADER_H: f32 = 20.0;
        const ROW_H: f32 = 30.0;
        let size = [112.0_f32, HEADER_H + ROW_H * rows];
        self.icon_w = Some(size[0]);
        self.rect[2] = size[0];
        self.rect[3] = size[1];
        Some(size)
    }

    /// Restores this floating panel to the size it had before collapsing.
    /// Returns that `[w, h]`, or `None` if it wasn't collapsed.
    pub fn expand(&mut self) -> Option<[f32; 2]> {
        let size = self.expanded_size.take()?;
        self.icon_w = None;
        self.rect[2] = size[0];
        self.rect[3] = size[1];
        Some(size)
    }
}

/// Which edge of the document window a rail sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RailSide {
    Left,
    Right,
}

/// Default rail width, logical points.
pub const RAIL_DEFAULT_W: f32 = 320.0;

/// Assumed splitter thickness when the model reasons about column pixel
/// widths (the renderer's exact value comes from the theme).
const SPLIT_GAP: f32 = 6.0;

/// One tab group collapsed to an icon-strip entry (Illustrator's
/// "Collapse to Icons") — collapsing applies to a whole dock *column* at
/// once, matching Illustrator (a "column" being either the rail's entire
/// tree, or — when the rail holds several docked side by side — one
/// top-level horizontal-split child of it). The column's original
/// sub-tree is kept verbatim, weights and internal groups included, so
/// expanding restores it exactly; it's just not part of `tree` while
/// collapsed, so it never competes with the rest of the rail's split
/// weights for space — the icon strip is a fixed-width column of its own.
/// Each `Tabs` group nested inside still gets its own icon row (see
/// [`Self::rows`]) even though the whole column collapses/expands as one
/// unit — clicking one row only ever *previews* that group in a flyout,
/// it doesn't pull it back into the tree by itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IconColumn {
    pub node: Node,
}

/// One icon in a collapsed column's strip: `panel`'s own icon row, tagged
/// with which of the column's original `Tabs` groups it came from
/// (`group`, indexing [`IconColumn::groups`] — a single `Rail::expand`
/// call restores the whole group's group's column together) and its
/// index within that group's own tab list (`tab`). Illustrator gives
/// every *tab* in a collapsed group its own icon, not just the group's
/// active one — a 3-tab "Pathfinder / Transform / Align" group collapses
/// to three icons, not one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconPanelRow {
    pub group: usize,
    pub tab: usize,
    pub panel: PanelId,
    /// Whether `panel` is currently `group`'s active tab — the strip
    /// highlights this one, matching whichever tab a click elsewhere on
    /// the group would currently show.
    pub active: bool,
    /// True for a group's first tab — tells the renderer to leave extra
    /// space above this row, so a column's original groups still read as
    /// separate clusters once collapsed, not one flat list.
    pub group_start: bool,
}

impl IconColumn {
    /// Every `Tabs` group nested in this column, top-to-bottom, as
    /// `(panels, active)` — what a flyout needs to reconstruct one
    /// group's full tab strip. Use [`Self::icon_rows`] for what the icon
    /// strip itself draws (one row per *tab*, not per group).
    pub fn groups(&self) -> Vec<(Vec<PanelId>, usize)> {
        let mut out = Vec::new();
        fn walk(n: &Node, out: &mut Vec<(Vec<PanelId>, usize)>) {
            match n {
                Node::Tabs { panels, active } => out.push((panels.clone(), *active)),
                Node::Split { children, .. } => {
                    for c in children {
                        walk(&c.node, out);
                    }
                }
            }
        }
        walk(&self.node, &mut out);
        out
    }

    /// Every panel in this column, top-to-bottom, one icon row each — see
    /// [`IconPanelRow`].
    pub fn icon_rows(&self) -> Vec<IconPanelRow> {
        let mut out = Vec::new();
        for (gi, (panels, active)) in self.groups().into_iter().enumerate() {
            for (ti, panel) in panels.into_iter().enumerate() {
                out.push(IconPanelRow {
                    group: gi,
                    tab: ti,
                    panel,
                    active: ti == active,
                    group_start: ti == 0,
                });
            }
        }
        out
    }

    /// Sets group `group`'s active tab to `tab` — walked in the same
    /// order as [`Self::groups`]/[`Self::icon_rows`]. Called when a click
    /// lands on a specific tab's icon row, so the flyout that opens (and
    /// the strip's own highlighted icon) reflects the one actually
    /// clicked, not whichever was active when the column collapsed.
    /// `false` if `group` is out of range.
    pub fn set_active(&mut self, group: usize, tab: usize) -> bool {
        fn walk(n: &mut Node, remaining: &mut usize, tab: usize) -> bool {
            match n {
                Node::Tabs { active, .. } => {
                    if *remaining == 0 {
                        *active = tab;
                        return true;
                    }
                    *remaining -= 1;
                    false
                }
                Node::Split { children, .. } => {
                    for c in children {
                        if walk(&mut c.node, remaining, tab) {
                            return true;
                        }
                    }
                    false
                }
            }
        }
        let mut remaining = group;
        walk(&mut self.node, &mut remaining, tab)
    }
}

/// One docked column: a single [`Node`] tree (or empty) plus how wide the
/// whole rail is. All the tree mechanics live here so every rail — left,
/// right, and any future one — behaves identically.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rail {
    pub tree: Option<Node>,
    /// Rail width in logical points; the user drags the rail's inner edge
    /// to change it.
    pub width: f32,
    /// Groups collapsed to icons, in the order they stack in the icon
    /// strip. Old saved layouts have no such field, hence the default.
    #[serde(default)]
    pub icons: Vec<IconColumn>,
    /// Width of the icon strip itself, when `icons` is non-empty — the
    /// user drags its own inner edge to change it, independent of `width`.
    /// Matches Illustrator: an icon column has no separate on/off toggle
    /// for showing labels, dragging it below a threshold just hides them
    /// (see `layout::ICON_LABEL_THRESHOLD`). Old saved layouts have no
    /// such field, hence the default (kept in sync with
    /// `layout::ICON_COL_W`, the labeled width every icon strip starts
    /// at).
    #[serde(default = "default_icon_col_w")]
    pub icon_col_w: f32,
}

fn default_icon_col_w() -> f32 {
    112.0
}

impl Default for Rail {
    fn default() -> Self {
        Self {
            tree: None,
            width: RAIL_DEFAULT_W,
            icons: Vec::new(),
            icon_col_w: default_icon_col_w(),
        }
    }
}

impl Rail {
    pub fn with(node: Node) -> Self {
        Self {
            tree: Some(node),
            width: RAIL_DEFAULT_W,
            icon_col_w: default_icon_col_w(),
            icons: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.tree.is_none() && self.icons.is_empty()
    }

    /// The node reached by `path` from this rail's root, if any.
    pub fn node_at(&self, path: &NodePath) -> Option<&Node> {
        self.tree.as_ref().and_then(|t| node_at(t, path))
    }

    pub fn panels(&self) -> Vec<PanelId> {
        let mut out = Vec::new();
        if let Some(t) = &self.tree {
            collect(t, &mut out);
        }
        for c in &self.icons {
            collect(&c.node, &mut out);
        }
        out
    }

    /// Removes `panel`, pruning empty tab groups and collapsing single-child
    /// splits; empties the rail if that was the last panel. If a whole
    /// column disappears, the rail shrinks by that column's width so the
    /// survivors keep their size. Also checks the icon strip — a panel
    /// collapsed to an icon is just as much "in this rail" as one in the
    /// tree, and re-docking it (the Window menu's toggle logic assumes
    /// `panels()`/`remove` agree on that) would otherwise leave a stale
    /// duplicate behind in `icons`.
    pub fn remove(&mut self, panel: PanelId) -> bool {
        if let Some(ci) = self.icons.iter().position(|c| {
            let mut v = Vec::new();
            collect(&c.node, &mut v);
            v.contains(&panel)
        }) {
            let hit = remove_in(&mut self.icons[ci].node, panel);
            if node_is_empty(&self.icons[ci].node) {
                self.icons.remove(ci);
            }
            return hit;
        }
        let Some(t) = &mut self.tree else {
            return false;
        };

        // Column pixel widths + which one holds `panel`, before removal.
        let cols_before: Option<(Vec<f32>, usize)> = match &*t {
            Node::Split {
                axis: Axis::Horizontal,
                children,
            } if children.len() >= 2 => {
                let wsum: f32 = children
                    .iter()
                    .map(|c| c.weight.max(0.0))
                    .sum::<f32>()
                    .max(1e-3);
                let avail =
                    (self.width - SPLIT_GAP * (children.len() as f32 - 1.0)).max(1.0);
                let px: Vec<f32> = children
                    .iter()
                    .map(|c| avail * c.weight.max(0.0) / wsum)
                    .collect();
                children
                    .iter()
                    .position(|c| {
                        let mut v = Vec::new();
                        collect(&c.node, &mut v);
                        v.contains(&panel)
                    })
                    .map(|i| (px, i))
            }
            _ => None,
        };

        let hit = remove_in(t, panel);
        if node_is_empty(t) {
            self.tree = None;
        }

        if hit {
            if let Some((px, removed_i)) = cols_before {
                let n_surv = px.len() - 1;
                let surviving: f32 = px
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != removed_i)
                    .map(|(_, v)| *v)
                    .sum();
                match &self.tree {
                    Some(Node::Split {
                        axis: Axis::Horizontal,
                        children,
                    }) if children.len() == n_surv && n_surv >= 2 => {
                        self.width = surviving + SPLIT_GAP * (n_surv as f32 - 1.0);
                    }
                    Some(_) if n_surv == 1 => self.width = surviving,
                    _ => {}
                }
            }
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

    /// Collapses a whole dock *column* to an icon strip — Illustrator's
    /// granularity: `path` addresses any node within the column (usually
    /// the tab group whose own « was clicked), and this walks up to that
    /// column's top-level boundary before collapsing. A "column" is one
    /// top-level child of the tree when the tree is a horizontal split of
    /// several docked side by side (`path`'s first index picks which);
    /// otherwise (a single column, or one already-vertical stack of
    /// groups) it's the whole tree. The column's sub-tree is pulled out
    /// verbatim (pruning empty splits behind it, the same bookkeeping
    /// `remove` already does for each panel) and appended to `icons`, so
    /// expanding can restore its internal groups and weights exactly.
    /// `false` if `path` doesn't resolve to anything, or the column turns
    /// out to hold no panels at all.
    pub fn collapse(&mut self, path: &NodePath) -> bool {
        let Some(root) = &self.tree else {
            return false;
        };
        let node = match root {
            Node::Split {
                axis: Axis::Horizontal,
                children,
            } => match path.0.first().and_then(|&i| children.get(i)) {
                Some(child) => child.node.clone(),
                None => return false,
            },
            other => other.clone(),
        };
        let mut panels = Vec::new();
        collect(&node, &mut panels);
        if panels.is_empty() {
            return false;
        }
        for p in panels {
            self.remove(p);
        }
        self.icons.push(IconColumn { node });
        true
    }

    /// Expands icon-strip column `index` back into the tree, as a new
    /// bottom split of the rail — its own column, with its original
    /// internal groups and vertical-split weights intact, not merged into
    /// an existing tab group. `false` if `index` is out of range.
    pub fn expand(&mut self, index: usize) -> bool {
        if index >= self.icons.len() {
            return false;
        }
        let column = self.icons.remove(index);
        let mut panels = Vec::new();
        collect(&column.node, &mut panels);
        let Some(&first) = panels.first() else {
            return true; // an empty column: nothing to put back.
        };
        // Dock `first` alone to get a fresh bottom split (reusing `dock`'s
        // existing split/graft logic), then graft the column's real,
        // possibly multi-group sub-tree in over that single-tab
        // placeholder once we know exactly where it landed.
        self.dock(
            first,
            DropTarget::Split {
                path: NodePath(Vec::new()),
                side: Side::Bottom,
            },
        );
        if let Some(path) = self.path_of(first) {
            if let Some(t) = &mut self.tree {
                if let Some(slot) = node_at_mut(t, &path) {
                    *slot = column.node;
                }
            }
        }
        true
    }

    /// Pulls just *one* group out of a collapsed column's icon strip —
    /// Illustrator lets you tear a group straight off its icon, not only
    /// after expanding the whole column back to the dock. Returns that
    /// group's own `Node::Tabs` (for `DockModel::float_node`), pruning it
    /// out of the column (and dropping the column entirely if that was
    /// its last group) while leaving every other collapsed group in this
    /// or any other column untouched. `None` if `column`/`group` don't
    /// resolve to anything.
    pub fn detach_icon_group(&mut self, column: usize, group: usize) -> Option<Node> {
        let col = self.icons.get_mut(column)?;
        let (panels, active) = col.groups().into_iter().nth(group)?;
        for &p in &panels {
            remove_in(&mut col.node, p);
        }
        if node_is_empty(&col.node) {
            self.icons.remove(column);
        }
        Some(Node::Tabs { panels, active })
    }

    /// Pulls a whole *docked* group out at once — Illustrator's own
    /// title-bar drag: grabbing a group (any group, not just the top of
    /// its column) and dragging it detaches every tab it holds together,
    /// not one at a time. Returns that group's `Node::Tabs` (for
    /// `DockModel::float_node`); `None` if `path` doesn't resolve to a
    /// `Tabs` node. Reuses `remove` per panel rather than splicing the
    /// tree directly, so the same column-width-absorption bookkeeping a
    /// panel-by-panel removal already gets (see `remove`) applies here
    /// too.
    pub fn detach_group(&mut self, path: &NodePath) -> Option<Node> {
        let Node::Tabs { panels, active } = self.node_at(path)?.clone() else {
            return None;
        };
        for &p in &panels {
            self.remove(p);
        }
        Some(Node::Tabs { panels, active })
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
        Some(self.push_floating(panel, rect))
    }

    /// Place `panel` in its own floating group at `rect`. If it already
    /// floats alone, that group is reused and moved. If it is docked (or
    /// tabbed with others), it is torn out. If it isn't placed yet, a new
    /// group is spawned.
    pub fn float_alone(&mut self, panel: PanelId, rect: [f32; 4]) -> u64 {
        if let Some(f) = self.floating.iter_mut().find(|f| match &f.node {
            Node::Tabs { panels, .. } => panels.as_slice() == [panel],
            _ => false,
        }) {
            f.rect = rect;
            return f.id;
        }
        if self.contains(panel) {
            return self.detach(panel, rect).expect("panel was contained");
        }
        self.push_floating(panel, rect)
    }

    /// The floating group currently holding `panel`, if any.
    pub fn floating_id_of(&self, panel: PanelId) -> Option<u64> {
        self.floating.iter().find_map(|f| {
            let mut ids = Vec::new();
            collect(&f.node, &mut ids);
            ids.contains(&panel).then_some(f.id)
        })
    }

    fn push_floating(&mut self, panel: PanelId, rect: [f32; 4]) -> u64 {
        self.float_node(
            Node::Tabs {
                panels: vec![panel],
                active: 0,
            },
            rect,
        )
    }

    /// Places an already-built `node` — usually a whole multi-tab group
    /// pulled out of a collapsed column's icon strip via
    /// `Rail::detach_icon_group` — into its own new floating window at
    /// `rect`. Unlike `push_floating`/`detach`/`float_alone`, which only
    /// ever start a single-panel group, this keeps the group's tabs (and
    /// which one was active) together, matching what tearing a *group*
    /// off a dock in Illustrator does.
    pub fn float_node(&mut self, node: Node, rect: [f32; 4]) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.floating.push(Floating {
            id,
            node,
            rect,
            icon_w: None,
            expanded_size: None,
        });
        id
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
                let avail = (rail.width - SPLIT_GAP * (children.len() as f32 - 1.0)).max(1.0);
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
                        float_w + existing_px.iter().sum::<f32>() + SPLIT_GAP * (n as f32 - 1.0);
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

fn node_at<'a>(node: &'a Node, path: &NodePath) -> Option<&'a Node> {
    let mut cur = node;
    for &i in &path.0 {
        match cur {
            Node::Split { children, .. } => cur = &children.get(i)?.node,
            Node::Tabs { .. } => return None,
        }
    }
    Some(cur)
}

impl Node {
    /// Shortest height (logical px) this subtree can take before a panel's
    /// content is clipped. `panel_min(id, width)` is a panel's natural body
    /// height; `strip_h` the tab strip; `gap` the splitter thickness;
    /// `width` the space available across the subtree.
    pub fn min_height(
        &self,
        width: f64,
        strip_h: f64,
        gap: f64,
        panel_min: &dyn Fn(PanelId, f64) -> f64,
    ) -> f64 {
        match self {
            Node::Tabs { panels, active } => {
                let body = panels
                    .get(*active)
                    .map_or(0.0, |p| panel_min(*p, (width - 2.0).max(0.0)));
                strip_h + body
            }
            Node::Split { axis, children } => match axis {
                Axis::Vertical => {
                    let inner = children
                        .iter()
                        .map(|c| c.node.min_height(width, strip_h, gap, panel_min))
                        .sum::<f64>();
                    inner + gap * children.len().saturating_sub(1) as f64
                }
                Axis::Horizontal => children
                    .iter()
                    .map(|c| c.node.min_height(width, strip_h, gap, panel_min))
                    .fold(0.0, f64::max),
            },
        }
    }
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
    fn float_alone_spawns_or_reuses_a_single_tab_group() {
        let mut m = DockModel::new(tabs(&[A]));
        let id = m.float_alone(C, [8.0, 9.0, 200.0, 300.0]);
        assert_eq!(m.floating_id_of(C), Some(id));
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating[0].rect, [8.0, 9.0, 200.0, 300.0]);

        let again = m.float_alone(C, [40.0, 50.0, 200.0, 300.0]);
        assert_eq!(again, id);
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating[0].rect, [40.0, 50.0, 200.0, 300.0]);

        let torn = m.float_alone(A, [0.0, 0.0, 100.0, 100.0]);
        assert_ne!(torn, id);
        assert!(m.right.is_empty());
        assert_eq!(m.floating.len(), 2);
    }

    #[test]
    fn collapsing_a_floating_panel_shrinks_it_and_remembers_the_old_size() {
        let mut m = DockModel::new(tabs(&[A]));
        let id = m.float_alone(C, [8.0, 9.0, 240.0, 400.0]);
        let f = m.floating_mut(id).unwrap();
        assert_eq!(f.icon_w, None);

        let size = f.collapse().expect("was open, should collapse");
        assert_eq!(f.icon_w, Some(size[0]));
        assert_eq!(f.expanded_size, Some([240.0, 400.0]));
        assert_eq!([f.rect[2], f.rect[3]], size, "rect tracks the shrunk size");
        // Collapsing an already-collapsed panel is a no-op, not a second
        // stash that would clobber the real pre-collapse size.
        assert_eq!(f.collapse(), None);
        assert_eq!(f.expanded_size, Some([240.0, 400.0]));
    }

    #[test]
    fn collapsing_a_multi_tab_floating_group_leaves_room_for_every_tab() {
        // A group torn off with several tabs together (float_node) must
        // collapse tall enough for one icon row per tab, not squash them
        // all into a single-row height the way an early version did.
        let mut three = Floating {
            id: 0,
            node: Node::Tabs { panels: vec![A, B, C], active: 0 },
            rect: [0.0, 0.0, 240.0, 400.0],
            icon_w: None,
            expanded_size: None,
        };
        let mut one = Floating {
            id: 1,
            node: Node::Tabs { panels: vec![A], active: 0 },
            rect: [0.0, 0.0, 240.0, 400.0],
            icon_w: None,
            expanded_size: None,
        };
        let three_h = three.collapse().expect("was open")[1];
        let one_h = one.collapse().expect("was open")[1];
        assert!(
            three_h > one_h,
            "three tabs must collapse taller than a lone one ({three_h} vs {one_h})"
        );
    }

    #[test]
    fn expanding_a_floating_panel_restores_its_pre_collapse_size() {
        let mut m = DockModel::new(tabs(&[A]));
        let id = m.float_alone(C, [8.0, 9.0, 240.0, 400.0]);
        let f = m.floating_mut(id).unwrap();
        f.collapse();

        let size = f.expand().expect("was collapsed, should expand");
        assert_eq!(size, [240.0, 400.0]);
        assert_eq!(f.icon_w, None);
        assert_eq!(f.expanded_size, None);
        assert_eq!([f.rect[2], f.rect[3]], [240.0, 400.0]);
        // Expanding an already-open panel is a no-op.
        assert_eq!(f.expand(), None);
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

    #[test]
    fn collapse_takes_the_whole_column_not_just_one_stacked_group() {
        // A vertical stack of two groups is *one* column (nothing splits
        // it horizontally into side-by-side columns) — collapsing from
        // either group's path takes both, matching Illustrator's "collapse
        // or expand all panel icons in a column" at once.
        let mut r = Rail::with(Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B, C]), weight: 2.0 },
            ],
        });
        let whole_column = r.tree.clone().unwrap();
        assert!(r.collapse(&NodePath(vec![1])));
        assert_eq!(r.tree, None, "the column was the rail's entire tree");
        assert_eq!(r.icons, vec![IconColumn { node: whole_column }]);
        // Collapsing is not just closing — every panel still counts as
        // "in this rail" (the Window menu's toggle logic depends on this).
        assert!(r.panels().contains(&A));
        assert!(r.panels().contains(&B));
        assert!(r.panels().contains(&C));
        // Each originally-stacked group still gets its own entry.
        assert_eq!(r.icons[0].groups(), vec![(vec![A], 0), (vec![B, C], 0)]);
        // ...and, in the icon strip itself, every *tab* in a group gets
        // its own icon row (three tabs total across the two groups here).
        let icon_rows = r.icons[0].icon_rows();
        assert_eq!(icon_rows.len(), 3);
        assert_eq!(
            icon_rows.iter().map(|r| (r.group, r.tab, r.panel)).collect::<Vec<_>>(),
            vec![(0, 0, A), (1, 0, B), (1, 1, C)]
        );
    }

    #[test]
    fn collapse_takes_only_the_clicked_column_when_columns_sit_side_by_side() {
        // A horizontal split *is* multiple side-by-side columns — only the
        // one `path` falls under collapses; the other stays fully docked.
        let mut r = Rail::with(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B, C]), weight: 1.0 },
            ],
        });
        assert!(r.collapse(&NodePath(vec![1])));
        // The split collapsed to its one remaining column, same as `remove`.
        assert_eq!(r.tree, Some(tabs(&[A])));
        assert_eq!(r.icons, vec![IconColumn { node: tabs(&[B, C]) }]);
    }

    #[test]
    fn collapse_of_an_only_column_empties_the_tree_but_not_the_rail() {
        let mut r = Rail::with(tabs(&[A]));
        assert!(r.collapse(&NodePath(vec![])));
        assert_eq!(r.tree, None);
        assert_eq!(r.icons.len(), 1);
        // The rail itself is not empty — it still holds a collapsed
        // column, and must keep claiming its share of the window (an
        // `is_empty` that only looked at `tree` would let the canvas
        // paint over the icon strip).
        assert!(!r.is_empty());
    }

    #[test]
    fn collapse_rejects_an_empty_rail() {
        let mut r = Rail::default();
        assert!(!r.collapse(&NodePath(vec![])));
        assert!(r.icons.is_empty());
    }

    #[test]
    fn expand_restores_a_collapsed_column_as_its_own_bottom_split() {
        let mut r = Rail::with(tabs(&[A]));
        assert!(r.collapse(&NodePath(vec![])));
        assert!(r.expand(0));
        assert!(r.icons.is_empty());
        assert_eq!(r.tree, Some(tabs(&[A])));
    }

    #[test]
    fn expand_restores_a_multi_group_column_with_its_internal_structure_intact() {
        let mut r = Rail::with(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child {
                    node: Node::Split {
                        axis: Axis::Vertical,
                        children: vec![
                            Child { node: tabs(&[B]), weight: 1.0 },
                            Child {
                                node: Node::Tabs { panels: vec![C], active: 0 },
                                weight: 3.0,
                            },
                        ],
                    },
                    weight: 1.0,
                },
            ],
        });
        assert!(r.collapse(&NodePath(vec![1])));
        assert!(r.expand(0));
        // B and C are back as two separate stacked groups (not merged into
        // one tab group), with their original 1:3 weight intact.
        let path_b = r.path_of(B).expect("B is back in the tree");
        let path_c = r.path_of(C).expect("C is back in the tree");
        assert_ne!(path_b, path_c, "B and C stay separate groups");
        let Some(Node::Split { children, .. }) = r.node_at(&NodePath(vec![path_b.0[0]])) else {
            panic!("expected the restored column to still be a vertical split");
        };
        assert_eq!(children[0].weight, 1.0);
        assert_eq!(children[1].weight, 3.0);
    }

    #[test]
    fn expand_rejects_an_out_of_range_index() {
        let mut r = Rail::with(tabs(&[A]));
        assert!(!r.expand(0));
    }

    #[test]
    fn detach_icon_group_pulls_just_one_group_out_of_a_multi_group_column() {
        // A stacked column of two groups, both collapsed to one icon
        // column (matches collapse_takes_the_whole_column_...): detaching
        // the second group by itself must leave the first fully intact.
        let mut r = Rail::with(Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B, C]), weight: 2.0 },
            ],
        });
        r.collapse(&NodePath(vec![1]));
        assert_eq!(r.icons.len(), 1);

        let out = r.detach_icon_group(0, 1).expect("group 1 exists");
        assert_eq!(out, Node::Tabs { panels: vec![B, C], active: 0 });
        // Group 0 (A) is still there, group 1 is gone — not the whole
        // column, just the one group.
        assert_eq!(r.icons, vec![IconColumn { node: tabs(&[A]) }]);
        assert!(r.panels().contains(&A));
        assert!(!r.panels().contains(&B));
        assert!(!r.panels().contains(&C));
    }

    #[test]
    fn detach_icon_group_drops_the_whole_column_once_its_last_group_leaves() {
        let mut r = Rail::with(tabs(&[A]));
        r.collapse(&NodePath(vec![]));
        assert_eq!(r.icons.len(), 1);

        let out = r.detach_icon_group(0, 0).expect("the only group");
        assert_eq!(out, Node::Tabs { panels: vec![A], active: 0 });
        assert!(r.icons.is_empty(), "an emptied column is dropped, not left as a husk");
        assert!(r.is_empty());
    }

    #[test]
    fn detach_icon_group_rejects_an_out_of_range_column_or_group() {
        let mut r = Rail::with(tabs(&[A]));
        r.collapse(&NodePath(vec![]));
        assert!(r.detach_icon_group(1, 0).is_none());
        assert!(r.detach_icon_group(0, 5).is_none());
    }

    #[test]
    fn float_node_keeps_a_multi_tab_group_together_in_one_window() {
        let mut m = DockModel::new(tabs(&[A]));
        let node = Node::Tabs { panels: vec![B, C], active: 1 };
        let id = m.float_node(node.clone(), [1.0, 2.0, 300.0, 400.0]);
        assert_eq!(m.floating.len(), 1);
        assert_eq!(m.floating[0].id, id);
        assert_eq!(m.floating[0].node, node);
        assert_eq!(m.floating[0].rect, [1.0, 2.0, 300.0, 400.0]);
        assert_eq!(m.floating[0].icon_w, None);
    }

    #[test]
    fn detach_group_pulls_a_middle_group_out_of_a_stack_intact() {
        // A three-group stack, fully docked (not collapsed) — dragging
        // the *middle* group's title bar must detach only that group,
        // leaving the ones above and below it exactly where they were.
        // (Multi-tab groups are covered by detach_icon_group's and
        // float_node's own tests; this one is purely about position.)
        let mut r = Rail::with(Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B]), weight: 1.0 },
                Child { node: tabs(&[C]), weight: 1.0 },
            ],
        });
        let out = r.detach_group(&NodePath(vec![1])).expect("middle group");
        assert_eq!(out, Node::Tabs { panels: vec![B], active: 0 });
        assert!(r.panels().contains(&A));
        assert!(r.panels().contains(&C));
        assert!(!r.panels().contains(&B));
        // The remaining two groups collapse down to a plain two-child
        // split, not left with a hole where the middle one was.
        assert_eq!(
            r.tree,
            Some(Node::Split {
                axis: Axis::Vertical,
                children: vec![
                    Child { node: tabs(&[A]), weight: 1.0 },
                    Child { node: tabs(&[C]), weight: 1.0 },
                ],
            })
        );
    }

    #[test]
    fn detach_group_rejects_a_path_that_is_not_a_tabs_node() {
        let mut r = Rail::with(Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B]), weight: 1.0 },
            ],
        });
        // The root here is a Split, not a Tabs — not a real group.
        assert!(r.detach_group(&NodePath(vec![])).is_none());
        assert!(r.detach_group(&NodePath(vec![9])).is_none());
    }

    #[test]
    fn remove_finds_a_panel_collapsed_to_an_icon() {
        let mut r = Rail::with(Node::Split {
            axis: Axis::Horizontal,
            children: vec![
                Child { node: tabs(&[A]), weight: 1.0 },
                Child { node: tabs(&[B, C]), weight: 1.0 },
            ],
        });
        r.collapse(&NodePath(vec![1]));
        assert!(r.remove(B));
        assert_eq!(r.icons, vec![IconColumn { node: tabs(&[C]) }]);
        assert!(r.remove(C));
        assert!(r.icons.is_empty(), "the emptied icon column is dropped");
        assert!(!r.remove(C), "second remove is a no-op");
    }
}
