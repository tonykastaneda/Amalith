//! Dock geometry: turn a [`Node`] tree + a rect into concrete rectangles,
//! and turn a rect + a cursor into a [`DropTarget`].
//!
//! Pure math — no vello, no winit. The chrome renderer draws a [`Layout`];
//! the drag code calls [`hit_test`]. Both stay dumb; the rules live here.

use vello::kurbo::{Point, Rect};

use crate::dock::{Axis, DropTarget, Node, NodePath, PanelId, Side};
use crate::theme::Theme;

/// One rendered tab group: its outer bounds, the strip, the body, and the
/// clickable rect of each tab.
#[derive(Clone, Debug)]
pub struct PanelArea {
    /// Path to this `Tabs` node in the source tree.
    pub path: NodePath,
    pub bounds: Rect,
    pub tab_strip: Rect,
    pub body: Rect,
    pub tabs: Vec<TabRect>,
    pub active: usize,
}

#[derive(Clone, Debug)]
pub struct TabRect {
    pub panel: PanelId,
    pub rect: Rect,
}

/// A draggable gap between two split children.
#[derive(Clone, Debug)]
pub struct SplitterHandle {
    /// Path to the parent `Split` node.
    pub path: NodePath,
    pub axis: Axis,
    /// The gap sits after child `index`.
    pub index: usize,
    pub rect: Rect,
    /// Full extent of the child before the gap.
    pub before: Rect,
    /// Full extent of the child after the gap.
    pub after: Rect,
}

impl SplitterHandle {
    /// Fraction (0..1) along the axis that a pointer at `p` implies for the
    /// boundary between the two children — feed straight to
    /// [`crate::dock::DockModel::set_boundary`].
    pub fn frac_at(&self, p: Point) -> f32 {
        match self.axis {
            Axis::Horizontal => {
                let lo = self.before.x0;
                let hi = self.after.x1;
                (((p.x - lo) / (hi - lo)) as f32).clamp(0.0, 1.0)
            }
            Axis::Vertical => {
                let lo = self.before.y0;
                let hi = self.after.y1;
                (((p.y - lo) / (hi - lo)) as f32).clamp(0.0, 1.0)
            }
        }
    }
}

/// Everything needed to draw and hit-test a dock tree in a given rect.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub areas: Vec<PanelArea>,
    pub splitters: Vec<SplitterHandle>,
}

/// Lay `root` out within `within`. `tab_width(panel)` gives each tab's
/// pixel width — it takes `&mut` because measuring text mutates the font
/// cache; a pure estimate works too.
pub fn layout(
    root: &Node,
    within: Rect,
    theme: &Theme,
    tab_width: &mut dyn FnMut(PanelId) -> f64,
) -> Layout {
    let mut out = Layout::default();
    let mut path = Vec::new();
    layout_node(root, within, theme, tab_width, &mut path, &mut out);
    out
}

fn layout_node(
    node: &Node,
    rect: Rect,
    theme: &Theme,
    tab_width: &mut dyn FnMut(PanelId) -> f64,
    path: &mut Vec<usize>,
    out: &mut Layout,
) {
    match node {
        Node::Tabs { panels, active } => {
            let strip_y1 = (rect.y0 + theme.tab_strip_h).min(rect.y1);
            let tab_strip = Rect::new(rect.x0, rect.y0, rect.x1, strip_y1);
            let body = Rect::new(rect.x0, strip_y1, rect.x1, rect.y1);

            let mut x = rect.x0;
            let mut tabs = Vec::with_capacity(panels.len());
            for &panel in panels {
                let w = tab_width(panel).max(8.0);
                tabs.push(TabRect {
                    panel,
                    rect: Rect::new(x, tab_strip.y0, x + w, tab_strip.y1),
                });
                x += w;
            }

            out.areas.push(PanelArea {
                path: NodePath(path.clone()),
                bounds: rect,
                tab_strip,
                body,
                tabs,
                active: *active,
            });
        }
        Node::Split { axis, children } => {
            let n = children.len();
            if n == 0 {
                return;
            }
            let gap = theme.splitter_thickness;
            let weight_sum: f64 = children.iter().map(|c| (c.weight.max(0.0)) as f64).sum();
            let weight_sum = if weight_sum <= 0.0 {
                n as f64
            } else {
                weight_sum
            };

            let (span, cross_lo, cross_hi, along_lo) = match axis {
                Axis::Horizontal => (rect.width(), rect.y0, rect.y1, rect.x0),
                Axis::Vertical => (rect.height(), rect.x0, rect.x1, rect.y0),
            };
            let avail = (span - gap * (n as f64 - 1.0)).max(0.0);

            // Place every child rect first, so splitters can reference both
            // neighbours.
            let mut child_rects = Vec::with_capacity(n);
            let mut cursor = along_lo;
            for (i, child) in children.iter().enumerate() {
                let seg = avail * (child.weight.max(0.0) as f64) / weight_sum;
                let r = match axis {
                    Axis::Horizontal => Rect::new(cursor, cross_lo, cursor + seg, cross_hi),
                    Axis::Vertical => Rect::new(cross_lo, cursor, cross_hi, cursor + seg),
                };
                child_rects.push(r);
                cursor += seg + if i + 1 < n { gap } else { 0.0 };
            }

            for (i, child) in children.iter().enumerate() {
                path.push(i);
                layout_node(&child.node, child_rects[i], theme, tab_width, path, out);
                path.pop();

                if i + 1 < n {
                    let (a, b) = (child_rects[i], child_rects[i + 1]);
                    let sp = match axis {
                        Axis::Horizontal => Rect::new(a.x1, cross_lo, b.x0, cross_hi),
                        Axis::Vertical => Rect::new(cross_lo, a.y1, cross_hi, b.y0),
                    };
                    out.splitters.push(SplitterHandle {
                        path: NodePath(path.clone()),
                        axis: *axis,
                        index: i,
                        rect: sp,
                        before: a,
                        after: b,
                    });
                }
            }
        }
    }
}

/// Distance from a point to the nearest edge of `r`, ignoring whether the
/// point is inside.
fn inset_contains(r: Rect, p: Point) -> bool {
    r.x0 <= p.x && p.x <= r.x1 && r.y0 <= p.y && p.y <= r.y1
}

/// Given a laid-out dock and the cursor, decide where a dragged panel
/// lands. Illustrator-ish: near the dock perimeter → split the whole dock;
/// over a tab strip → insert as a tab; near a group's body edge → split
/// that group; over a group's body centre → tab into it; nowhere → float.
pub fn hit_test(layout: &Layout, root: Rect, p: Point, theme: &Theme) -> DropTarget {
    if !inset_contains(root, p) {
        return DropTarget::Float;
    }

    // Perimeter band → split the root.
    const EDGE: f64 = 24.0;
    let (dl, dr, dt, db) = (p.x - root.x0, root.x1 - p.x, p.y - root.y0, root.y1 - p.y);
    let m = dl.min(dr).min(dt).min(db);
    if m <= EDGE {
        return DropTarget::Split {
            path: NodePath(Vec::new()),
            side: nearest_side(dl, dr, dt, db),
        };
    }

    let Some(area) = layout.areas.iter().find(|a| inset_contains(a.bounds, p)) else {
        return DropTarget::Float;
    };

    // Over the strip → tab insert at the nearest gap.
    if inset_contains(area.tab_strip, p) {
        let mut index = area.tabs.len();
        for (i, t) in area.tabs.iter().enumerate() {
            if p.x < (t.rect.x0 + t.rect.x1) * 0.5 {
                index = i;
                break;
            }
        }
        return DropTarget::Tab {
            path: area.path.clone(),
            index,
        };
    }

    // Body: an edge band splits this group; the centre tabs into it.
    let b = area.body;
    let _ = theme; // reserved: strip height already baked into `body`
    let (bl, br, bt, bb) = (p.x - b.x0, b.x1 - p.x, p.y - b.y0, b.y1 - p.y);
    let band = (b.width().min(b.height()) * 0.22).clamp(24.0, 80.0);
    let bm = bl.min(br).min(bt).min(bb);
    if bm <= band {
        return DropTarget::Split {
            path: area.path.clone(),
            side: nearest_side(bl, br, bt, bb),
        };
    }

    DropTarget::Tab {
        path: area.path.clone(),
        index: area.tabs.len(),
    }
}

fn nearest_side(left: f64, right: f64, top: f64, bottom: f64) -> Side {
    let m = left.min(right).min(top).min(bottom);
    if m == left {
        Side::Left
    } else if m == right {
        Side::Right
    } else if m == top {
        Side::Top
    } else {
        Side::Bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dock::Child;

    const L: PanelId = PanelId("layers");
    const A: PanelId = PanelId("artboards");
    const S: PanelId = PanelId("swatches");

    fn theme() -> Theme {
        Theme::default()
    }

    fn w80(_: PanelId) -> f64 {
        80.0
    }

    fn stacked() -> Node {
        // Two groups stacked vertically, 50/50. Bottom group has two tabs.
        Node::Split {
            axis: Axis::Vertical,
            children: vec![
                Child {
                    node: Node::Tabs {
                        panels: vec![L],
                        active: 0,
                    },
                    weight: 1.0,
                },
                Child {
                    node: Node::Tabs {
                        panels: vec![A, S],
                        active: 0,
                    },
                    weight: 1.0,
                },
            ],
        }
    }

    #[test]
    fn vertical_split_stacks_two_areas_with_a_splitter_between() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));

        assert_eq!(lay.areas.len(), 2);
        assert_eq!(lay.splitters.len(), 1);

        let top = &lay.areas[0];
        let bottom = &lay.areas[1];
        assert_eq!(top.path.0, vec![0]);
        assert_eq!(bottom.path.0, vec![1]);
        // Top ends where the splitter begins; bottom starts after it.
        assert!(top.bounds.y1 <= lay.splitters[0].rect.y0 + 0.01);
        assert!(bottom.bounds.y0 >= lay.splitters[0].rect.y1 - 0.01);
        // Strip then body.
        assert_eq!(top.tab_strip.y0, top.bounds.y0);
        assert_eq!(top.body.y0, top.tab_strip.y1);
        // Bottom group's two tabs, laid left to right at 80px each.
        assert_eq!(bottom.tabs.len(), 2);
        assert_eq!(bottom.tabs[0].rect.x0, 0.0);
        assert_eq!(bottom.tabs[1].rect.x0, 80.0);
    }

    #[test]
    fn splitter_frac_at_maps_a_pointer_to_a_boundary_fraction() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let sp = &lay.splitters[0];
        assert_eq!(sp.axis, Axis::Vertical);
        // Combined span is the whole 0..400 height. A pointer a quarter of
        // the way down implies a 25% boundary.
        let f = sp.frac_at(Point::new(150.0, 100.0));
        assert!((f - 0.25).abs() < 1e-4, "got {f}");
        // Clamped past the ends.
        assert_eq!(sp.frac_at(Point::new(150.0, -50.0)), 0.0);
        assert_eq!(sp.frac_at(Point::new(150.0, 999.0)), 1.0);
    }

    #[test]
    fn cursor_near_left_edge_splits_the_whole_dock() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let t = hit_test(&lay, root, Point::new(6.0, 200.0), &theme());
        assert_eq!(
            t,
            DropTarget::Split {
                path: NodePath(vec![]),
                side: Side::Left,
            }
        );
    }

    #[test]
    fn cursor_in_a_tab_strip_inserts_a_tab_at_the_nearest_gap() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let bottom = &lay.areas[1];
        // Just past the midpoint of the first tab → insert before tab 1.
        let y = bottom.tab_strip.y0 + 4.0;
        let t = hit_test(&lay, root, Point::new(60.0, y), &theme());
        assert_eq!(
            t,
            DropTarget::Tab {
                path: NodePath(vec![1]),
                index: 1,
            }
        );
    }

    #[test]
    fn cursor_in_a_group_body_centre_tabs_into_that_group() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let bottom = &lay.areas[1];
        let c = bottom.body.center();
        let t = hit_test(&lay, root, c, &theme());
        assert_eq!(
            t,
            DropTarget::Tab {
                path: NodePath(vec![1]),
                index: 2,
            }
        );
    }

    #[test]
    fn cursor_near_a_lower_group_body_top_splits_that_group() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let bottom = &lay.areas[1];
        // A few px below the body's top edge, mid-width, well clear of the
        // dock perimeter.
        let p = Point::new(150.0, bottom.body.y0 + 5.0);
        let t = hit_test(&lay, root, p, &theme());
        assert_eq!(
            t,
            DropTarget::Split {
                path: NodePath(vec![1]),
                side: Side::Top,
            }
        );
    }

    #[test]
    fn cursor_outside_the_dock_floats() {
        let root = Rect::new(0.0, 0.0, 300.0, 400.0);
        let lay = layout(&stacked(), root, &theme(), &mut |p| w80(p));
        let t = hit_test(&lay, root, Point::new(-20.0, 200.0), &theme());
        assert_eq!(t, DropTarget::Float);
    }
}
