//! The path-editing tools beside the Pen: Add / Delete Anchor Point and
//! Anchor Point (the Pen flyout), Curvature, Scissors (the Eraser
//! flyout), Reshape (the Scale flyout) and Group Selection (the Direct
//! Selection flyout). Each press resolves what's under the pointer and
//! compiles to one undoable command; the drags preview through
//! `DragPreview::path` (see [`App::path_tool_preview`]) and commit once,
//! on release.

use super::*;
use amalith_core::{CurvaturePoint, HandleSide, PathData};
use amalith_commands::PathPoint;

/// Screen-px hit radius for anchors, handles and segments — the same
/// radius Direct Selection grabs nodes with.
const PATH_TOOL_HIT: f64 = 6.0;

/// The Curvature tool's drawing session: the path it's adding points to
/// (`drawing`), or the lone first click of a path not created yet
/// (`first`, document space — a one-point path would be a stray point).
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::app) struct CurvatureState {
    pub drawing: Option<ObjectId>,
    pub first: Option<Point>,
}

/// What a press with one of these tools landed on.
#[derive(Clone, Copy, Debug)]
enum PathHit {
    Handle(ObjectId, usize, HandleSide),
    Anchor(ObjectId, usize),
    Segment(ObjectId, usize, f64),
}

/// Whether `id` and every ancestor up to its layer are visible and
/// unlocked — the tools here never edit locked or hidden art.
fn editable(doc: &Document, id: ObjectId) -> bool {
    let mut cur = id;
    loop {
        let Some(o) = doc.object(cur) else { return false };
        if o.locked || !o.visible {
            return false;
        }
        match o.parent {
            amalith_core::ObjectParent::Layer(l) => return doc.layer(l).is_some_and(|l| l.visible && !l.locked),
            amalith_core::ObjectParent::Group(g) => cur = g,
            amalith_core::ObjectParent::Symbol(_) => return true,
        }
    }
}

impl App {
    fn path_tool_radius(&self) -> f64 {
        PATH_TOOL_HIT * crate::handle_scale::multiplier() / self.doc.view.zoom
    }

    /// Paths these tools act on, back to front (hit-tests scan from the
    /// end): every editable path in scope with the selected ones moved to
    /// the top so they win, or only the selected ones.
    fn path_tool_targets(&self, selected_only: bool) -> Vec<ObjectId> {
        let doc = self.doc.editor.document();
        let selected = self.node_paths();
        let mut all = match self.isolation_root() {
            Some(root) => anchors::path_leaves_in(doc, root),
            None => anchors::path_leaves(doc),
        };
        all.retain(|id| editable(doc, *id));
        if selected_only {
            all.retain(|id| selected.contains(id));
        } else {
            all.sort_by_key(|id| selected.contains(id));
        }
        all
    }

    /// The handle, anchor or segment under the pointer, in that order of
    /// preference. Handles are only offered on selected paths — the only
    /// ones that show them.
    fn path_tool_hit(&self, handles: bool, selected_only: bool) -> Option<PathHit> {
        let doc = self.doc.editor.document();
        let dp = self.doc_point(self.pointer);
        let r = self.path_tool_radius();
        let ambient = self.isolation_ambient();
        if handles {
            if let Some((id, n, side)) = anchors::handle_at(doc, &self.node_paths(), dp, r, ambient) {
                return Some(PathHit::Handle(id, n, side));
            }
        }
        let targets = self.path_tool_targets(selected_only);
        if let Some((id, n)) = anchors::topmost_anchor_among(doc, &targets, dp, r, ambient) {
            return Some(PathHit::Anchor(id, n));
        }
        anchors::segment_at(doc, &targets, dp, r, ambient).map(|(id, seg, t)| PathHit::Segment(id, seg, t))
    }

    /// Document point → `id`'s local space.
    fn to_local(&self, id: ObjectId, p: Point) -> amalith_core::Point {
        let m = convert::affine(self.doc.editor.document().world_transform(id));
        convert::point_to_core((self.isolation_ambient() * m).inverse() * p)
    }

    /// Keeps a path the tool just touched selected, so its nodes stay up.
    fn keep_path_shown(&mut self, id: ObjectId) {
        if !self.doc.selection.contains(&id) {
            self.doc.selection = vec![id];
        }
    }

    /// Press with any of these tools. Returns whether it was handled —
    /// a miss falls through to the Selection tool's own press, so a click
    /// on bare art still picks it up to work on.
    pub(in crate::app) fn path_tool_press(&mut self, double: bool) -> bool {
        let dp = self.doc_point(self.pointer);
        match self.active_tool {
            Tool::AddAnchor => {
                let Some(PathHit::Segment(id, seg, t)) = self.path_tool_hit(false, false) else {
                    return false;
                };
                let Some(pd) = self.doc.editor.document().object(id).and_then(|o| o.kind.path_data()) else {
                    return false;
                };
                let new_anchor = amalith_core::insert_anchor(&mut pd.subpaths().to_vec(), seg, t);
                if self.doc.editor.execute(Command::InsertAnchor { object: id, segment: seg, t }).is_ok() {
                    self.keep_path_shown(id);
                    self.doc.anchor_sel = new_anchor.map(|n| vec![(id, n)]).unwrap_or_default();
                }
            }
            Tool::DeleteAnchor => {
                let Some(PathHit::Anchor(id, n)) = self.path_tool_hit(false, false) else {
                    return false;
                };
                let _ = self.doc.editor.execute(Command::RemoveAnchor { object: id, anchor: n });
                self.keep_path_shown(id);
                self.doc.anchor_sel.clear();
                // The last anchors of a line take the line with them.
                self.prune_selection();
            }
            Tool::AnchorPoint => match self.path_tool_hit(true, false) {
                Some(PathHit::Handle(object, anchor, side)) => {
                    self.doc.anchor_sel = vec![(object, anchor)];
                    self.drag = Drag::ConvertHandle { object, anchor, side, start_doc: dp, last_doc: dp };
                }
                Some(PathHit::Anchor(object, anchor)) => {
                    self.keep_path_shown(object);
                    self.doc.anchor_sel = vec![(object, anchor)];
                    self.drag = Drag::PullHandles { object, anchor, last_doc: dp, moved: false };
                }
                _ => return false,
            },
            Tool::Scissors => {
                let (id, at) = match self.path_tool_hit(false, false) {
                    Some(PathHit::Anchor(id, n)) => (id, PathPoint::Anchor(n)),
                    Some(PathHit::Segment(id, segment, t)) => (id, PathPoint::Segment { segment, t }),
                    _ => return false,
                };
                if let Ok(outcome) = self.doc.editor.execute(Command::SplitPath { object: id, at }) {
                    self.doc.selection = vec![id];
                    if let CommandOutcome::Object(piece) = outcome {
                        self.doc.selection.push(piece);
                    }
                    self.doc.anchor_sel.clear();
                }
            }
            Tool::Reshape => {
                let (id, at) = match self.path_tool_hit(false, true) {
                    Some(PathHit::Anchor(id, _) | PathHit::Segment(id, ..)) => (id, self.to_local(id, dp)),
                    _ => return false,
                };
                self.drag = Drag::Reshape { object: id, at, start_doc: dp, last_doc: dp, moved: false };
            }
            Tool::GroupSelect => self.group_select_press(dp),
            Tool::Curvature => self.curvature_press(dp, double),
            _ => return false,
        }
        self.request_main_redraw();
        true
    }

    /// Group Selection: the first click picks the innermost object under
    /// the pointer, even deep inside groups; each further click on it
    /// steps the selection out to the next enclosing group. Shift toggles
    /// the innermost object. A press on nothing starts a marquee.
    fn group_select_press(&mut self, dp: Point) {
        let doc = self.doc.editor.document();
        let r = self.path_tool_radius();
        let top = match self.isolation_root() {
            Some(root) => select::topmost_in(doc, root, dp, r),
            // Only what's under the press can be hit, so cull to just
            // around it rather than to the window's canvas area.
            None => select::topmost_selectable_at(doc, dp, Rect::from_center_size(dp, (4.0 * r, 4.0 * r)), r),
        };
        let Some(top) = top else {
            if !self.shift_down {
                self.doc.selection.clear();
            }
            self.drag = Drag::Marquee { start: self.pointer };
            return;
        };
        // Outermost → innermost.
        let mut chain = vec![top];
        while let Some(amalith_core::ObjectKind::Group(_)) = doc.object(*chain.last().unwrap()).map(|o| &o.kind) {
            match select::topmost_in(doc, *chain.last().unwrap(), dp, r) {
                Some(child) if !chain.contains(&child) => chain.push(child),
                _ => break,
            }
        }
        let leaf = *chain.last().unwrap();
        let picked = if self.shift_down {
            if let Some(i) = self.doc.selection.iter().position(|&s| s == leaf) {
                self.doc.selection.remove(i);
                self.doc.anchor_sel.clear();
                return;
            }
            self.doc.selection.push(leaf);
            leaf
        } else {
            // The deepest level already selected steps out one; nothing
            // selected yet on this chain starts at the leaf.
            let next = match chain.iter().rposition(|id| self.doc.selection.contains(id)) {
                Some(0) => chain[0],
                Some(k) => chain[k - 1],
                None => leaf,
            };
            self.doc.selection = vec![next];
            next
        };
        self.doc.anchor_sel.clear();
        self.drag = Drag::MoveObjects { start_doc: dp, last_doc: dp, moved: false, hit: Some(picked) };
    }

    /// The selected path the Curvature tool edits: a lone selected path
    /// object with a single subpath.
    fn curvature_target(&self) -> Option<ObjectId> {
        let [id] = self.doc.selection[..] else { return None };
        let doc = self.doc.editor.document();
        match doc.object(id).map(|o| &o.kind) {
            Some(amalith_core::ObjectKind::Path(pd)) if pd.subpaths().len() == 1 && editable(doc, id) => Some(id),
            _ => None,
        }
    }

    /// `id`'s Curvature points (local space) and whether it's closed.
    fn curvature_points_of(&self, id: ObjectId) -> Option<(Vec<CurvaturePoint>, bool)> {
        let pd = self.doc.editor.document().object(id)?.kind.path_data()?;
        let sp = pd.subpaths().first()?;
        Some((amalith_core::curvature_points(sp), sp.closed))
    }

    fn set_curvature(&mut self, object: ObjectId, points: Vec<CurvaturePoint>, closed: bool) {
        let _ = self.doc.editor.execute(Command::SetCurvaturePath { object, subpath: 0, points, closed });
    }

    /// Curvature: click to add points along a smooth curve (the first
    /// click just plants the start); click on the curve to insert a point;
    /// drag a point to move it; Alt-click or double-click a point to
    /// toggle it between smooth and corner; click the first point to
    /// close. Esc / Enter ends the path.
    fn curvature_press(&mut self, dp: Point, double: bool) {
        let r = self.path_tool_radius();
        if let Some(id) = self.curvature_target() {
            let (mut points, closed) = self.curvature_points_of(id).unwrap_or_default();
            let local = self.to_local(id, dp);
            let doc = self.doc.editor.document();
            let ambient = self.isolation_ambient();
            if let Some((_, i)) = anchors::topmost_anchor_among(doc, &[id], dp, r, ambient) {
                if self.alt_down || double {
                    points[i].corner = !points[i].corner;
                    self.set_curvature(id, points, closed);
                } else if i == 0 && !closed && points.len() >= 3 && self.curvature.drawing == Some(id) {
                    self.set_curvature(id, points, true);
                    self.curvature = CurvatureState::default();
                } else {
                    self.doc.anchor_sel = vec![(id, i)];
                    self.drag = Drag::CurvaturePoint { object: id, index: i, last_doc: dp, moved: false };
                }
                return;
            }
            if let Some((_, seg, _)) = anchors::segment_at(doc, &[id], dp, r, ambient) {
                points.insert(seg + 1, CurvaturePoint { point: local, corner: false });
                self.set_curvature(id, points, closed);
                self.doc.anchor_sel = vec![(id, seg + 1)];
                self.drag = Drag::CurvaturePoint { object: id, index: seg + 1, last_doc: dp, moved: false };
                return;
            }
            if self.curvature.drawing == Some(id) && !closed {
                points.push(CurvaturePoint { point: local, corner: self.alt_down });
                self.set_curvature(id, points, false);
                self.doc.anchor_sel.clear();
                return;
            }
        }
        // Not continuing anything: the second click of a fresh path
        // creates it, the first just plants its start.
        let Some(first) = self.curvature.first.take() else {
            self.curvature = CurvatureState { drawing: None, first: Some(dp) };
            self.doc.selection.clear();
            self.doc.anchor_sel.clear();
            return;
        };
        let points = [first, dp].map(|p| CurvaturePoint { point: convert::point_to_core(p), corner: false });
        let path = PathData::from_subpaths(vec![amalith_core::curvature_subpath(&points, false)]);
        let (container, _) = self.ensure_container();
        if let Ok(CommandOutcome::Object(id)) =
            self.doc.execute_new_vector_object(Command::CreatePath { parent: container, path, name: None })
        {
            self.doc.selection = vec![id];
            self.apply_new_appearance(id);
            self.reparent_new_object_into_isolation(id);
            self.curvature.drawing = Some(id);
        }
    }

    /// Ends the Curvature tool's current path (Esc / Enter / tool switch).
    /// Returns whether there was one.
    pub(in crate::app) fn end_curvature(&mut self) -> bool {
        let had = self.curvature.drawing.is_some() || self.curvature.first.is_some();
        self.curvature = CurvatureState::default();
        if had {
            self.request_main_redraw();
        }
        had
    }

    /// Pointer move while one of these tools' drags is live.
    pub(in crate::app) fn path_tool_drag_move(&mut self) {
        let dp = self.doc_point(self.pointer);
        let (pointer, to_screen) = (self.pointer, self.doc.view.to_screen());
        let far = move |from: Point| (pointer - to_screen * from).hypot() > 2.0;
        match &mut self.drag {
            Drag::PullHandles { last_doc, moved, .. } | Drag::CurvaturePoint { last_doc, moved, .. } => {
                *moved = *moved || far(*last_doc);
                if *moved {
                    *last_doc = dp;
                }
            }
            Drag::ConvertHandle { last_doc, .. } => *last_doc = dp,
            Drag::Reshape { start_doc, last_doc, moved, .. } => {
                *moved |= far(*start_doc);
                *last_doc = dp;
            }
            _ => return,
        }
        self.request_main_redraw();
    }

    /// The live drag as a whole-path override for `DragPreview::path`.
    pub(in crate::app) fn path_tool_preview(&self) -> Option<(ObjectId, PathData)> {
        let doc = self.doc.editor.document();
        let (object, data) = match self.drag {
            Drag::PullHandles { object, anchor, last_doc, moved: true } => {
                let mut data = doc.object(object)?.kind.path_data()?.clone();
                let out = self.to_local(object, last_doc);
                data.edit_subpaths(|sp| amalith_core::pull_anchor_handles(sp, anchor, out));
                (object, data)
            }
            Drag::ConvertHandle { object, anchor, side, last_doc, .. } => {
                let mut data = doc.object(object)?.kind.path_data()?.clone();
                let to = self.to_local(object, last_doc);
                data.edit_subpaths(|sp| {
                    amalith_core::break_handle_mirror(sp, anchor);
                    amalith_core::set_handle(sp, anchor, side, Some(to));
                });
                (object, data)
            }
            Drag::CurvaturePoint { object, index, last_doc, moved: true } => {
                let (mut points, closed) = self.curvature_points_of(object)?;
                points.get_mut(index)?.point = self.to_local(object, last_doc);
                let mut data = doc.object(object)?.kind.path_data()?.clone();
                data.edit_subpaths(|sp| sp[0] = amalith_core::curvature_subpath(&points, closed));
                (object, data)
            }
            Drag::Reshape { object, at, start_doc, last_doc, moved: true } => {
                let mut data = doc.object(object)?.kind.path_data()?.clone();
                let (delta, tolerance) = self.reshape_params(object, start_doc, last_doc);
                data.edit_subpaths(|sp| {
                    amalith_core::reshape(sp, at, delta, tolerance);
                });
                (object, data)
            }
            _ => return None,
        };
        Some((object, data))
    }

    /// A Reshape drag's local-space delta (Shift: 45° steps) and grab
    /// tolerance.
    fn reshape_params(&self, object: ObjectId, start_doc: Point, last_doc: Point) -> (amalith_core::Vec2, f64) {
        let mut d = last_doc - start_doc;
        if self.shift_down {
            d = snap8(d);
        }
        let origin = self.to_local(object, start_doc);
        let delta = self.to_local(object, start_doc + d) - origin;
        let tolerance = (self.to_local(object, start_doc + Vec2::new(self.path_tool_radius(), 0.0)) - origin).hypot();
        (delta, tolerance)
    }

    /// Release of one of these tools' drags: commits it as one command.
    pub(in crate::app) fn path_tool_release(&mut self, drag: Drag) {
        match drag {
            Drag::PullHandles { object, anchor, last_doc, moved } => {
                if moved {
                    let handle_out = self.to_local(object, last_doc);
                    let _ = self.doc.editor.execute(Command::PullAnchorHandles { object, anchor, handle_out });
                } else if amalith_core::anchor_at(
                    self.doc.editor.document().object(object).and_then(|o| o.kind.path_data()).map_or(&[][..], |p| p.subpaths()),
                    anchor,
                )
                .is_some_and(|a| a.handle_in.is_some() || a.handle_out.is_some())
                {
                    // A plain click on a smooth point makes it a corner.
                    let _ = self.doc.editor.execute(Command::SetAnchorSmooth { object, anchor, smooth: false });
                }
            }
            Drag::ConvertHandle { object, anchor, side, start_doc, last_doc } => {
                let delta = self.to_local(object, last_doc) - self.to_local(object, start_doc);
                if delta.hypot() > 0.0 {
                    let _ = self.doc.editor.execute(Command::MoveHandle {
                        object,
                        anchor,
                        side,
                        delta,
                        break_mirror: true,
                    });
                }
            }
            Drag::CurvaturePoint { object, index, last_doc, moved: true } => {
                if let Some((mut points, closed)) = self.curvature_points_of(object) {
                    if let Some(p) = points.get_mut(index) {
                        p.point = self.to_local(object, last_doc);
                        self.set_curvature(object, points, closed);
                    }
                }
            }
            Drag::Reshape { object, at, start_doc, last_doc, moved: true } => {
                let (delta, tolerance) = self.reshape_params(object, start_doc, last_doc);
                let _ = self.doc.editor.execute(Command::ReshapePath { object, at, delta, tolerance });
                self.doc.anchor_sel.clear();
            }
            _ => return,
        }
        self.request_main_redraw();
    }

    /// The Rectangular / Polar Grid drag's live grid, in the accent.
    pub(in crate::app) fn paint_grid_preview(&mut self) {
        let Drag::DrawShape { tool: tool @ (Tool::RectangularGrid | Tool::PolarGrid), start_doc, cur_doc } = self.drag
        else {
            return;
        };
        let r = shape_rect(start_doc, cur_doc, self.shift_down, self.alt_down);
        let to_screen = self.doc.view.to_screen();
        let accent = self.theme.accent;
        for p in self.shape_params.grid_paths(tool, r) {
            let path = to_screen * convert::bez_path(&p.geometry);
            self.content.stroke(&Stroke::new(1.0), ID, accent, None, &path);
        }
    }

    /// Curvature's rubber band: the curve through the path's points and
    /// the pointer, drawn while a path is being drawn and no drag is live.
    pub(in crate::app) fn paint_curvature_preview(&mut self) {
        if self.active_tool != Tool::Curvature || !matches!(self.drag, Drag::None) {
            return;
        }
        let hover = self.doc_point(self.pointer);
        let to_screen = self.doc.view.to_screen();
        let mut points: Vec<CurvaturePoint> = match (self.curvature.first, self.curvature.drawing) {
            (Some(first), _) => vec![CurvaturePoint { point: convert::point_to_core(first), corner: false }],
            (None, Some(id)) if self.curvature_target() == Some(id) => {
                let Some((pts, false)) = self.curvature_points_of(id) else { return };
                let m = self.isolation_ambient() * convert::affine(self.doc.editor.document().world_transform(id));
                pts.into_iter()
                    .map(|p| CurvaturePoint { point: convert::point_to_core(m * convert::point(p.point)), ..p })
                    .collect()
            }
            _ => return,
        };
        points.push(CurvaturePoint { point: convert::point_to_core(hover), corner: self.alt_down });
        let sp = amalith_core::curvature_subpath(&points, false);
        let path = to_screen * convert::bez_path(&amalith_core::subpaths_to_bezpath(&[sp]));
        let accent = self.theme.accent;
        self.content.stroke(&Stroke::new(1.0), ID, accent, None, &path);
        if let Some(first) = self.curvature.first {
            let s = 7.0 * crate::handle_scale::multiplier();
            self.content.fill(Fill::NonZero, ID, accent, None, &Rect::from_center_size(to_screen * first, (s, s)));
        }
    }
}

/// Drives the real press / drag / release paths headlessly — the pointer
/// is placed in screen space over document points, exactly as a mouse
/// would put it.
#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{Anchor, HandleMode, Subpath};

    fn line(points: &[(f64, f64)], closed: bool) -> PathData {
        PathData::from_subpaths(vec![Subpath {
            anchors: points.iter().map(|&(x, y)| Anchor::corner(amalith_core::Point::new(x, y))).collect(),
            closed,
        }])
    }

    /// An app with `paths` on one layer, panned well inside the canvas.
    fn app_with(paths: Vec<PathData>) -> (App, Vec<ObjectId>) {
        let mut document = Document::new("Paths");
        let layer = LayerId::new();
        document.insert_layer(amalith_core::Layer::new(layer, "Layer"), 0);
        let mut app = App::new();
        app.doc = Doc::new(Editor::new(document));
        app.doc.view.pan = Vec2::new(400.0, 300.0);
        let ids = paths
            .into_iter()
            .map(|path| {
                let parent = amalith_core::ObjectParent::Layer(layer);
                let Ok(CommandOutcome::Object(id)) = app.doc.editor.execute(Command::CreatePath { parent, path, name: None })
                else {
                    panic!("create path")
                };
                id
            })
            .collect();
        (app, ids)
    }

    fn point_at(app: &mut App, x: f64, y: f64) {
        app.pointer = app.doc.view.to_screen() * Point::new(x, y);
    }

    fn press(app: &mut App, tool: Tool, x: f64, y: f64) -> bool {
        app.set_tool(tool);
        point_at(app, x, y);
        app.path_tool_press(false)
    }

    fn drag_to(app: &mut App, x: f64, y: f64) {
        point_at(app, x, y);
        app.path_tool_drag_move();
    }

    fn release(app: &mut App) {
        let drag = std::mem::take(&mut app.drag);
        app.path_tool_release(drag);
    }

    fn subpaths(app: &App, id: ObjectId) -> Vec<Subpath> {
        app.doc.editor.document().object(id).unwrap().kind.path_data().unwrap().subpaths().to_vec()
    }

    #[test]
    fn add_anchor_clicks_a_segment_and_selects_the_new_point() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (100., 0.)], false)]);
        assert!(press(&mut app, Tool::AddAnchor, 50.0, 1.0));
        assert_eq!(subpaths(&app, ids[0])[0].anchors.len(), 3);
        assert_eq!(app.doc.selection, vec![ids[0]]);
        assert_eq!(app.doc.anchor_sel, vec![(ids[0], 1)]);
        // Off the path it isn't the tool's press.
        assert!(!press(&mut app, Tool::AddAnchor, 50.0, 80.0));
    }

    #[test]
    fn delete_anchor_keeps_a_square_closed() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (100., 0.), (100., 100.), (0., 100.)], true)]);
        assert!(press(&mut app, Tool::DeleteAnchor, 100.0, 0.0));
        let sp = subpaths(&app, ids[0]);
        assert!(sp[0].closed);
        assert_eq!(sp[0].anchors.len(), 3);
    }

    #[test]
    fn anchor_point_drag_pulls_symmetric_handles_and_click_resets_them() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (50., 0.), (100., 0.)], false)]);
        assert!(press(&mut app, Tool::AnchorPoint, 50.0, 0.0));
        drag_to(&mut app, 80.0, 20.0);
        let (_, live) = app.path_tool_preview().expect("a live preview while dragging");
        assert_eq!(live.subpaths()[0].anchors[1].handle_out, Some(amalith_core::Point::new(80., 20.)));
        release(&mut app);
        let a = subpaths(&app, ids[0])[0].anchors[1];
        assert_eq!(a.mode, HandleMode::Symmetric);
        assert_eq!(a.handle_in, Some(amalith_core::Point::new(20., -20.)));
        // A plain click on the now-smooth point turns it back into a corner.
        assert!(press(&mut app, Tool::AnchorPoint, 50.0, 0.0));
        release(&mut app);
        let a = subpaths(&app, ids[0])[0].anchors[1];
        assert_eq!((a.handle_in, a.handle_out), (None, None));
    }

    #[test]
    fn anchor_point_handle_drag_breaks_it_from_its_partner() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (50., 0.), (100., 0.)], false)]);
        app.doc.editor
            .execute(Command::PullAnchorHandles { object: ids[0], anchor: 1, handle_out: amalith_core::Point::new(80., 0.) })
            .unwrap();
        app.doc.selection = vec![ids[0]];
        assert!(press(&mut app, Tool::AnchorPoint, 80.0, 0.0));
        assert!(matches!(app.drag, Drag::ConvertHandle { side: HandleSide::Out, .. }));
        drag_to(&mut app, 80.0, 30.0);
        release(&mut app);
        let a = subpaths(&app, ids[0])[0].anchors[1];
        assert_eq!(a.handle_out, Some(amalith_core::Point::new(80., 30.)));
        assert_eq!(a.handle_in, Some(amalith_core::Point::new(20., 0.)), "the partner stays put");
    }

    #[test]
    fn scissors_cuts_an_open_path_into_two_selected_objects() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (100., 0.), (200., 0.)], false)]);
        assert!(press(&mut app, Tool::Scissors, 100.0, 0.0));
        assert_eq!(app.doc.selection.len(), 2);
        assert_eq!(app.doc.selection[0], ids[0]);
        assert_eq!(subpaths(&app, ids[0])[0].anchors.len(), 2);
        assert_eq!(subpaths(&app, app.doc.selection[1])[0].anchors.len(), 2);
    }

    #[test]
    fn reshape_only_grabs_selected_paths_and_bends_them() {
        let (mut app, ids) = app_with(vec![line(&[(0., 0.), (200., 0.)], false)]);
        assert!(!press(&mut app, Tool::Reshape, 100.0, 0.0), "unselected paths are left alone");
        app.doc.selection = vec![ids[0]];
        assert!(press(&mut app, Tool::Reshape, 100.0, 0.0));
        drag_to(&mut app, 100.0, 30.0);
        assert!(app.path_tool_preview().is_some());
        release(&mut app);
        let sp = subpaths(&app, ids[0]);
        assert_eq!(sp[0].anchors.len(), 3);
        assert_eq!(sp[0].anchors[1].point, amalith_core::Point::new(100., 30.));
    }

    #[test]
    fn group_selection_steps_out_one_group_per_click() {
        let square = |x: f64| line(&[(x, 0.), (x + 50., 0.), (x + 50., 50.), (x, 50.)], true);
        let (mut app, ids) = app_with(vec![square(0.0), square(100.0)]);
        for &id in &ids {
            app.doc.editor.execute(Command::SetFill { objects: vec![id], paint: amalith_core::Paint::Solid(amalith_core::Color::rgb(1., 0., 0.)) }).unwrap();
        }
        let Ok(CommandOutcome::Object(group)) = app.doc.editor.execute(Command::Group { ids: ids.clone(), name: None }) else {
            panic!("group")
        };
        assert!(press(&mut app, Tool::GroupSelect, 25.0, 25.0));
        assert_eq!(app.doc.selection, vec![ids[0]], "the first click picks the shape inside the group");
        app.drag = Drag::None;
        assert!(press(&mut app, Tool::GroupSelect, 25.0, 25.0));
        assert_eq!(app.doc.selection, vec![group], "the next steps out to its group");
    }

    #[test]
    fn curvature_draws_inserts_drags_and_closes() {
        let (mut app, _) = app_with(vec![]);
        assert!(press(&mut app, Tool::Curvature, 0.0, 0.0));
        assert!(app.curvature.first.is_some(), "the first click only plants the start");
        point_at(&mut app, 100.0, 0.0);
        app.path_tool_press(false);
        let [id] = app.doc.selection[..] else { panic!("the second click creates the path") };
        assert_eq!(app.curvature.drawing, Some(id));
        point_at(&mut app, 200.0, 100.0);
        app.path_tool_press(false);
        let sp = subpaths(&app, id);
        assert_eq!(sp[0].anchors.len(), 3);
        assert_eq!(sp[0].anchors[1].mode, HandleMode::Smooth, "the old end turns smooth once passed");
        assert!(sp[0].anchors[1].handle_out.is_some());
        // Drag the middle point.
        point_at(&mut app, 100.0, 0.0);
        app.path_tool_press(false);
        drag_to(&mut app, 100.0, -40.0);
        release(&mut app);
        assert_eq!(subpaths(&app, id)[0].anchors[1].point, amalith_core::Point::new(100., -40.));
        // Clicking the first point closes the curve and ends the session.
        point_at(&mut app, 0.0, 0.0);
        app.path_tool_press(false);
        assert!(subpaths(&app, id)[0].closed);
        assert_eq!(app.curvature.drawing, None);
    }

    #[test]
    fn grids_are_one_selected_unfilled_group_using_the_remembered_counts() {
        let (mut app, _) = app_with(vec![]);
        app.shape_params.rect_grid = (100.0, 100.0, 2.0, 3.0);
        let r = amalith_core::Rect::new(0., 0., 200., 100.);
        app.create_grid(app.shape_params.grid_paths(Tool::RectangularGrid, r));
        let [group] = app.doc.selection[..] else { panic!("the grid is selected") };
        let doc = app.doc.editor.document();
        let Some(amalith_core::ObjectKind::Group(g)) = doc.object(group).map(|o| &o.kind) else { panic!("a group") };
        assert_eq!(g.children.len(), 1 + 2 + 3);
        let child = doc.object(g.children[0]).unwrap();
        assert_eq!(child.appearance.fill(), amalith_core::Paint::None);
        assert_ne!(child.appearance.stroke(), amalith_core::Paint::None);
        // One undo takes the whole grid away.
        app.doc.editor.undo().unwrap();
        assert!(app.doc.editor.document().object(group).is_none());
    }

    #[test]
    fn the_grid_dialog_builds_its_paths_at_the_click() {
        let mut params = crate::shapedialog::Params::default();
        params.polar_grid = (80.0, 80.0, 3.0, 6.0);
        let dlg = crate::shapedialog::ShapeDialog::open(Tool::PolarGrid, Point::new(10.0, 20.0), &params);
        let crate::shapedialog::Geometry::Paths(paths) = dlg.geometry() else { panic!("a grid is several paths") };
        assert_eq!(paths.len(), 4 + 6);
        let outer = paths[3].local_bounds();
        assert_eq!((outer.x0.round(), outer.y0.round(), outer.width().round()), (10.0, 20.0, 80.0));
    }
}
