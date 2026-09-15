//! Canvas pane layout. Documents are referenced, never copied into panes.
//!
//! A pane always owns at least one tab — there is no "empty pane" state.
//! Splitting creates a new pane with a single chooser tab; picking New
//! Document/Terminal from it replaces that tab's content in place. Closing
//! a pane's last tab removes the pane itself from the tree (its sibling
//! takes its place) — that is the *only* way a pane goes away. The root
//! pane is special: with no sibling to collapse into, closing its last tab
//! instead resets it to a single chooser tab rather than vanishing (there
//! must always be at least one pane for the canvas area to show).
use crate::canvas::CanvasView;
use amalith_core::ObjectId;
use vello::kurbo::Rect;

/// Vello 0.10's empty append overwrites encoding flags, including the
/// transform/style reset required after deferred glyph rendering.
/// An absent pane overlay must leave the parent scene untouched.
pub(crate) fn append_overlay(scene: &mut vello::Scene, overlay: &vello::Scene) {
    if !overlay.encoding().is_empty() {
        scene.append(overlay, None);
    }
}

pub type PaneId = u64;
pub type TabId = u64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabContent {
    Document(ObjectId),
    Terminal,
    Chooser,
}

/// One tab within a pane. `view`/`scroll` are only meaningful for the
/// content kinds that use them (`Document`/`Chooser` respectively) but
/// live here unconditionally — simpler than an enum-with-payload per
/// kind, and cheap since a tab is rarely more than a handful of fields.
#[derive(Clone, Debug)]
pub struct Tab {
    pub id: TabId,
    pub content: TabContent,
    pub view: CanvasView,
    pub scroll: f64,
}
impl Tab {
    fn new(id: TabId, content: TabContent) -> Self {
        Self { id, content, view: CanvasView::default(), scroll: 0.0 }
    }
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<Tab>,
    pub active: usize,
}
impl Pane {
    pub fn active_tab(&self) -> &Tab {
        &self.tabs[self.active.min(self.tabs.len() - 1)]
    }
}

/// A split's orientation — `Horizontal` places panes side by side
/// (Split Right), `Vertical` stacks them (Split Down).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug)]
enum Node {
    Leaf(Pane),
    Split {
        axis: Axis,
        ratio: f64,
        left: Box<Node>,
        right: Box<Node>,
    },
}

#[derive(Default)]
pub struct Multiplexer {
    root: Option<Node>,
    pub focused: PaneId,
    pub prefix: bool,
    next_pane: PaneId,
    next_tab: TabId,
}

/// What happened when a tab was closed — see `Multiplexer::close_tab`.
#[derive(Debug, PartialEq, Eq)]
pub enum CloseOutcome {
    /// The pane still has tabs left. `active_changed` is `true` when the
    /// tab that was showing is gone and a neighbor took its place — the
    /// caller needs to rebind `App::doc`/`App::terminal` to match; `false`
    /// means a *different* (background) tab in the same pane closed and
    /// whatever was showing is still showing.
    TabRemoved { active_changed: bool },
    /// The pane's last tab was closed and it had a sibling to collapse
    /// into — the pane is gone. Carries the pane that should be focused
    /// next.
    PaneRemoved { next_focus: PaneId },
    /// The pane's last tab was closed, but it was the root (no sibling)
    /// — it now holds a single fresh chooser tab instead of vanishing.
    RootReset,
    /// `pane`/`tab_idx` didn't resolve to a real tab; nothing happened.
    NotFound,
}

impl Node {
    fn pane(&self, id: PaneId) -> Option<&Pane> {
        match self {
            Self::Leaf(p) => (p.id == id).then_some(p),
            Self::Split { left, right, .. } => left.pane(id).or_else(|| right.pane(id)),
        }
    }
    fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        match self {
            Self::Leaf(p) => (p.id == id).then_some(p),
            Self::Split { left, right, .. } => left.pane_mut(id).or_else(|| right.pane_mut(id)),
        }
    }
    fn layout(&self, r: Rect, out: &mut Vec<(Pane, Rect)>) {
        match self {
            Self::Leaf(p) => out.push((p.clone(), r)),
            Self::Split { axis, ratio, left, right } => match axis {
                Axis::Horizontal => {
                    let x = r.x0 + r.width() * ratio;
                    left.layout(Rect::new(r.x0, r.y0, x - 2., r.y1), out);
                    right.layout(Rect::new(x + 2., r.y0, r.x1, r.y1), out);
                }
                Axis::Vertical => {
                    let y = r.y0 + r.height() * ratio;
                    left.layout(Rect::new(r.x0, r.y0, r.x1, y - 2.), out);
                    right.layout(Rect::new(r.x0, y + 2., r.x1, r.y1), out);
                }
            },
        }
    }
    fn split(&mut self, id: PaneId, axis: Axis, new: Pane) -> bool {
        match self {
            Self::Leaf(p) if p.id == id => {
                *self = Self::Split {
                    axis,
                    ratio: 0.5,
                    left: Box::new(Self::Leaf(p.clone())),
                    right: Box::new(Self::Leaf(new)),
                };
                true
            }
            Self::Split { left, right, .. } => left.split(id, axis, new.clone()) || right.split(id, axis, new),
            _ => false,
        }
    }
    /// First (leftmost/topmost) leaf under this node — used to pick a
    /// sane focus target after a pane collapses away.
    fn first_leaf(&self) -> PaneId {
        match self {
            Self::Leaf(p) => p.id,
            Self::Split { left, .. } => left.first_leaf(),
        }
    }
    /// `true` if this exact node is the leaf `id` — i.e. `id` is the
    /// whole tree, the root with no parent to collapse into.
    fn is_lone_leaf(&self, id: PaneId) -> bool {
        matches!(self, Self::Leaf(p) if p.id == id)
    }
    /// Removes `target` from `node`, collapsing its sibling into its
    /// place. `Some` unless `node` itself *was* `target` (only possible
    /// when `target` is the lone root — callers special-case that before
    /// ever getting here, see `Multiplexer::close_tab`).
    fn remove(node: Node, target: PaneId) -> Option<Node> {
        match node {
            Self::Leaf(p) if p.id == target => None,
            Self::Leaf(p) => Some(Self::Leaf(p)),
            Self::Split { axis, ratio, left, right } => {
                if left.is_lone_leaf(target) {
                    return Some(*right);
                }
                if right.is_lone_leaf(target) {
                    return Some(*left);
                }
                let new_left = Self::remove(*left, target);
                let new_right = Self::remove(*right, target);
                match (new_left, new_right) {
                    (Some(l), Some(r)) => Some(Self::Split { axis, ratio, left: Box::new(l), right: Box::new(r) }),
                    // `target` wasn't under either side (shouldn't
                    // happen for a valid id, but don't panic over it).
                    (Some(l), None) => Some(l),
                    (None, Some(r)) => Some(r),
                    (None, None) => None,
                }
            }
        }
    }
}

impl Multiplexer {
    pub fn enabled(&self) -> bool {
        self.root.is_some()
    }
    pub fn start(&mut self, document: ObjectId, view: CanvasView) {
        if self.enabled() {
            return;
        }
        self.next_pane = 1;
        self.next_tab = 1;
        self.focused = 0;
        self.root = Some(Node::Leaf(Pane {
            id: 0,
            tabs: vec![Tab { id: 0, content: TabContent::Document(document), view, scroll: 0.0 }],
            active: 0,
        }));
    }
    pub fn pane(&self, id: PaneId) -> Option<&Pane> {
        self.root.as_ref()?.pane(id)
    }
    pub fn pane_mut(&mut self, id: PaneId) -> Option<&mut Pane> {
        self.root.as_mut()?.pane_mut(id)
    }
    pub fn active_tab_mut(&mut self, pane: PaneId) -> Option<&mut Tab> {
        let p = self.pane_mut(pane)?;
        let i = p.active.min(p.tabs.len().saturating_sub(1));
        p.tabs.get_mut(i)
    }
    pub fn layout(&self, r: Rect) -> Vec<(Pane, Rect)> {
        let mut out = Vec::new();
        if let Some(root) = &self.root {
            root.layout(r, &mut out);
        }
        out
    }
    /// Splits the focused pane along `axis`; the new pane gets one fresh
    /// chooser tab and becomes focused.
    pub fn split(&mut self, axis: Axis) -> Option<PaneId> {
        let id = self.next_pane;
        let tab = self.next_tab;
        if self.root.as_mut()?.split(self.focused, axis, Pane {
            id,
            tabs: vec![Tab::new(tab, TabContent::Chooser)],
            active: 0,
        }) {
            self.next_pane += 1;
            self.next_tab += 1;
            Some(id)
        } else {
            None
        }
    }
    /// Adds a fresh tab (`content`) to `pane` and makes it active,
    /// returning its id.
    pub fn add_tab(&mut self, pane: PaneId, content: TabContent) -> Option<TabId> {
        let id = self.next_tab;
        let p = self.pane_mut(pane)?;
        p.tabs.push(Tab::new(id, content));
        p.active = p.tabs.len() - 1;
        self.next_tab += 1;
        Some(id)
    }
    /// Closes tab `tab_idx` in `pane` — the only way a pane can close
    /// (see the module doc comment). Adjusts the pane's active index to
    /// a neighboring tab when the active one is removed.
    pub fn close_tab(&mut self, pane: PaneId, tab_idx: usize) -> CloseOutcome {
        self.remove_tab_at(pane, tab_idx).map_or(CloseOutcome::NotFound, |(_, outcome)| outcome)
    }
    /// Removes tab `tab_idx` from `pane` and hands it back (instead of
    /// discarding it) — for moving it to another pane; see
    /// `App::mux_move_tab`. Same removal/pane-collapse cascade as
    /// `close_tab`, since a pane can never end up with zero tabs either
    /// way (see the module doc comment).
    pub fn take_tab(&mut self, pane: PaneId, tab_idx: usize) -> Option<(Tab, CloseOutcome)> {
        self.remove_tab_at(pane, tab_idx)
    }
    /// Appends `tab` to `pane` and makes it active. `false` (and `tab`
    /// dropped) if `pane` doesn't exist — callers should check
    /// `pane_mut`/`layout` first so this is never the normal path.
    pub fn insert_tab(&mut self, pane: PaneId, tab: Tab) -> bool {
        let Some(p) = self.pane_mut(pane) else {
            return false;
        };
        let idx = p.tabs.len();
        p.tabs.insert(idx, tab);
        p.active = idx;
        true
    }
    /// Same as `insert_tab`, but at a specific position instead of
    /// always the end — `index` clamps to the current tab count, so
    /// "insert past the last tab" still just appends.
    pub fn insert_tab_at(&mut self, pane: PaneId, index: usize, tab: Tab) -> bool {
        let Some(p) = self.pane_mut(pane) else {
            return false;
        };
        let idx = index.min(p.tabs.len());
        p.tabs.insert(idx, tab);
        p.active = idx;
        true
    }
    /// Reorders tab `from_idx` to land at `to_idx` within the *same*
    /// pane — dragging a tab chip left/right past its neighbors. No
    /// removal cascade here, unlike `take_tab`/`close_tab`: the pane
    /// never actually loses a tab, it's just reshuffling one of its own,
    /// so none of "last tab gone" logic applies.
    pub fn reorder_tab(&mut self, pane: PaneId, from_idx: usize, to_idx: usize) -> bool {
        let Some(p) = self.pane_mut(pane) else {
            return false;
        };
        if from_idx >= p.tabs.len() {
            return false;
        }
        let to_idx = to_idx.min(p.tabs.len() - 1);
        if from_idx == to_idx {
            return true;
        }
        // Track the tab that was active by its stable id, not its index
        // — simpler and more robust than working out how the shift
        // affects `active` by hand for every from/to direction.
        let was_active_id = p.tabs[p.active].id;
        let tab = p.tabs.remove(from_idx);
        p.tabs.insert(to_idx, tab);
        p.active = p.tabs.iter().position(|t| t.id == was_active_id).unwrap_or(0);
        true
    }
    fn remove_tab_at(&mut self, pane: PaneId, tab_idx: usize) -> Option<(Tab, CloseOutcome)> {
        let p = self.pane_mut(pane)?;
        if tab_idx >= p.tabs.len() {
            return None;
        }
        let was_active = tab_idx == p.active;
        let removed = p.tabs.remove(tab_idx);
        if !p.tabs.is_empty() {
            if was_active {
                p.active = tab_idx.min(p.tabs.len() - 1);
            } else if tab_idx < p.active {
                // A tab before the active one shifted every later index
                // down by one — follow it so `active` still names the
                // same logical tab, not whatever slid into its old slot.
                p.active -= 1;
            }
            return Some((removed, CloseOutcome::TabRemoved { active_changed: was_active }));
        }
        // Last tab gone — the pane itself closes, unless it's the root
        // with nothing to collapse into.
        if self.root.as_ref().is_some_and(|r| r.is_lone_leaf(pane)) {
            let id = self.next_tab;
            self.next_tab += 1;
            let p = self.pane_mut(pane).unwrap();
            p.tabs.push(Tab::new(id, TabContent::Chooser));
            p.active = 0;
            return Some((removed, CloseOutcome::RootReset));
        }
        let root = self.root.take().unwrap();
        let next_focus_hint = self.focused == pane;
        let new_root = Node::remove(root, pane);
        let next_focus = new_root.as_ref().map(Node::first_leaf).unwrap_or(0);
        self.root = new_root;
        if next_focus_hint {
            self.focused = next_focus;
        }
        Some((removed, CloseOutcome::PaneRemoved { next_focus }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_prefix_overlay_preserves_transform_reset_after_text() {
        use vello::{
            Scene,
            kurbo::Affine,
            peniko::{Color, Fill},
        };
        let mut text = crate::text::TextContext::new();
        let mut expected = Scene::new();
        let mut actual = Scene::new();
        for scene in [&mut expected, &mut actual] {
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::BLACK,
                None,
                &Rect::new(0., 0., 800., 600.),
            );
            text.draw(scene, "Untitled document", 12., Color::WHITE, 110., 74.);
        }
        append_overlay(&mut actual, &Scene::new());
        for scene in [&mut expected, &mut actual] {
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::WHITE,
                None,
                &Rect::new(0., 60., 80., 600.),
            );
        }
        assert!(actual.encoding().path_tags == expected.encoding().path_tags);
        assert_eq!(actual.encoding().transforms, expected.encoding().transforms);
        assert_eq!(actual.encoding().styles, expected.encoding().styles);
    }

    #[test]
    fn splits_only_focused_leaf_and_keeps_independent_views() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let second = m.split(Axis::Horizontal).unwrap();
        {
            let p = m.pane_mut(second).unwrap();
            p.tabs[0].content = TabContent::Document(doc);
            p.tabs[0].view.zoom = 4.;
        }
        m.focused = second;
        m.split(Axis::Horizontal).unwrap();
        let panes = m.layout(Rect::new(0., 0., 1000., 600.));
        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].1.width(), 498.);
        assert_eq!(panes[0].0.active_tab().view.zoom, 0.5);
        assert_eq!(panes[1].0.active_tab().view.zoom, 4.);
        assert_eq!(panes[0].0.active_tab().content, panes[1].0.active_tab().content);
        assert_eq!(panes[2].0.active_tab().content, TabContent::Chooser);
    }

    #[test]
    fn vertical_split_stacks_panes() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        m.split(Axis::Vertical).unwrap();
        let panes = m.layout(Rect::new(0., 0., 1000., 600.));
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].1.width(), 1000.);
        assert_eq!(panes[0].1.height(), 298.);
        assert_eq!(panes[1].1.y0, 302.);
    }

    #[test]
    fn closing_a_panes_last_tab_removes_it_and_focuses_the_sibling() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let second = m.split(Axis::Horizontal).unwrap();
        m.focused = second;
        assert_eq!(m.layout(Rect::new(0., 0., 1000., 600.)).len(), 2);
        let outcome = m.close_tab(second, 0);
        assert_eq!(outcome, CloseOutcome::PaneRemoved { next_focus: 0 });
        assert_eq!(m.focused, 0);
        let panes = m.layout(Rect::new(0., 0., 1000., 600.));
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].0.id, 0);
    }

    #[test]
    fn closing_the_roots_last_tab_resets_to_a_fresh_chooser_instead_of_vanishing() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let outcome = m.close_tab(0, 0);
        assert_eq!(outcome, CloseOutcome::RootReset);
        assert!(m.enabled());
        let panes = m.layout(Rect::new(0., 0., 1000., 600.));
        assert_eq!(panes.len(), 1);
        assert_eq!(panes[0].0.active_tab().content, TabContent::Chooser);
    }

    #[test]
    fn a_pane_never_reports_zero_tabs() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let id = m.add_tab(0, TabContent::Terminal).unwrap();
        assert_eq!(m.pane_mut(0).unwrap().tabs.len(), 2);
        let idx = m.pane_mut(0).unwrap().tabs.iter().position(|t| t.id == id).unwrap();
        m.close_tab(0, idx);
        assert_eq!(m.pane_mut(0).unwrap().tabs.len(), 1);
    }

    #[test]
    fn take_tab_then_insert_tab_moves_it_to_another_pane_intact() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let second = m.split(Axis::Horizontal).unwrap();
        let term_id = m.add_tab(0, TabContent::Terminal).unwrap();
        let idx = m.pane_mut(0).unwrap().tabs.iter().position(|t| t.id == term_id).unwrap();
        let (tab, outcome) = m.take_tab(0, idx).unwrap();
        // `add_tab` makes the new (terminal) tab active, so removing it
        // is the "active tab changed" case.
        assert_eq!(outcome, CloseOutcome::TabRemoved { active_changed: true });
        assert_eq!(tab.id, term_id);
        assert_eq!(tab.content, TabContent::Terminal);
        assert_eq!(m.pane_mut(0).unwrap().tabs.len(), 1);
        assert!(m.insert_tab(second, tab));
        let dst = m.pane_mut(second).unwrap();
        assert_eq!(dst.tabs.len(), 2);
        assert_eq!(dst.active, 1);
        assert_eq!(dst.tabs[1].id, term_id);
    }

    #[test]
    fn reorder_tab_moves_it_without_touching_which_tab_is_active() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        let term_id = m.add_tab(0, TabContent::Terminal).unwrap();
        let chooser_id = m.add_tab(0, TabContent::Chooser).unwrap();
        // tabs: [Document, Terminal, Chooser], active = 2 (Chooser).
        assert_eq!(m.pane_mut(0).unwrap().active, 2);
        // Drag the Document tab (index 0) to the end.
        assert!(m.reorder_tab(0, 0, 2));
        let p = m.pane_mut(0).unwrap();
        assert_eq!(
            p.tabs.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![term_id, chooser_id, 0]
        );
        // The Chooser tab is still the one showing, now at index 1.
        assert_eq!(p.active, 1);
        assert_eq!(p.tabs[p.active].content, TabContent::Chooser);
    }
    #[test]
    fn insert_tab_at_lands_at_the_requested_index() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        assert!(m.insert_tab_at(0, 0, Tab::new(999, TabContent::Chooser)));
        let p = m.pane_mut(0).unwrap();
        assert_eq!(p.tabs[0].id, 999);
        assert_eq!(p.active, 0);
    }

    #[test]
    fn closing_a_background_tab_before_the_active_one_keeps_the_same_tab_showing() {
        let doc = ObjectId::new();
        let mut m = Multiplexer::default();
        m.start(doc, CanvasView::default());
        m.add_tab(0, TabContent::Terminal).unwrap();
        m.add_tab(0, TabContent::Chooser).unwrap();
        // tabs: [Document, Terminal, Chooser], active = 2 (Chooser).
        assert_eq!(m.pane_mut(0).unwrap().active, 2);
        // Close index 0 (Document) — a tab *before* the active one.
        let outcome = m.close_tab(0, 0);
        assert_eq!(outcome, CloseOutcome::TabRemoved { active_changed: false });
        let p = m.pane_mut(0).unwrap();
        assert_eq!(p.tabs.len(), 2);
        // The Chooser tab is still the one showing, just shifted to
        // index 1 — `active` must have followed it, not stayed at 2.
        assert_eq!(p.active, 1);
        assert_eq!(p.active_tab().content, TabContent::Chooser);
    }
}
