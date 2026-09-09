//! Free Transform's on-canvas modes and press-time gesture geometry.
//! Corners scale or distort, side handles scale or modifier-shear, and
//! the rotation halo uses the shared rotation gesture.

use super::*;
use std::collections::HashSet;

/// Which Free Transform sub-mode the on-canvas flyout has selected.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(in crate::app) enum FreeTransformMode {
    #[default]
    Transform,
    Perspective,
    FreeDistort,
}

/// One button in the flyout, for painting/hit-testing without repeating
/// the match over all four every time.
#[derive(Clone, Copy)]
enum FlyoutButton {
    Constrain,
    Transform,
    Perspective,
    FreeDistort,
}

const FT_BTN_W: f64 = 30.0;
const FT_BTN_H: f64 = 26.0;
const FT_BTN_GAP: f64 = 4.0;
const FT_PAD: f64 = 6.0;

/// The flyout's screen rects, anchored below the selection's bbox.
pub(in crate::app) struct FreeTransformFlyout {
    pub panel: Rect,
    pub constrain: Rect,
    pub transform: Rect,
    pub perspective: Rect,
    pub free_distort: Rect,
}

impl FreeTransformFlyout {
    fn buttons(&self) -> [(Rect, FlyoutButton); 4] {
        [
            (self.constrain, FlyoutButton::Constrain),
            (self.transform, FlyoutButton::Transform),
            (self.perspective, FlyoutButton::Perspective),
            (self.free_distort, FlyoutButton::FreeDistort),
        ]
    }
}

/// Which corner of `Handle::ALL`'s quad convention (Nw, Ne, Se, Sw = 0..4
/// clockwise from top-left) `h` is. Only ever called with a corner
/// handle — edges and `N`/`E`/`S`/`W` never reach a warp drag.
fn corner_index(h: Handle) -> usize {
    match h {
        Handle::Nw => 0,
        Handle::Ne => 1,
        Handle::Se => 2,
        Handle::Sw => 3,
        _ => 0,
    }
}

/// The two corners adjacent to `idx` around the quad — its horizontal-edge
/// partner, then its vertical-edge partner. The corner diagonally
/// opposite (`idx + 2 mod 4`) has no partner and never moves.
fn edge_partners(idx: usize) -> (usize, usize) {
    match idx {
        0 => (1, 3),
        1 => (0, 2),
        2 => (3, 1),
        _ => (2, 0),
    }
}

/// Selection-edge coordinates also handle rotated and sheared parent groups.
fn quad_frame(q: [Point; 4]) -> Affine {
    let x = q[1] - q[0];
    let y = q[3] - q[0];
    Affine::new([x.x, x.y, y.x, y.y, q[0].x, q[0].y])
}

/// Perspective is a one-axis, symmetric edge taper. Constrained Free
/// Distort locks the dragged corner to one edge axis without moving a partner.
fn warp_dst_quad(mode: FreeTransformMode, constrain: bool, src: [Point; 4], handle: Handle, pointer: Point) -> [Point; 4] {
    let idx = corner_index(handle);
    let frame = quad_frame(src);
    if !frame.determinant().is_finite() || frame.determinant().abs() < 1e-20 { return src; }
    let d = frame.inverse() * pointer - frame.inverse() * src[idx];
    let x = src[1] - src[0];
    let y = src[3] - src[0];
    let horizontal = (d.x * x.hypot()).abs() >= (d.y * y.hypot()).abs();
    let delta = if mode == FreeTransformMode::Perspective || constrain {
        if horizontal { x * d.x } else { y * d.y }
    } else { pointer - src[idx] };
    let mut dst = src;
    dst[idx] += delta;
    if mode == FreeTransformMode::Perspective {
        let partners = edge_partners(idx);
        let partner = if horizontal { partners.0 } else { partners.1 };
        dst[partner] -= delta;
    }
    dst
}

fn affine_drag(src: [Point; 4], handle: Handle, pointer: Point, constrain: bool, center: bool, shear: bool) -> Affine {
    let frame = quad_frame(src);
    if (pointer-handles::handle_pos(src,handle)).hypot() < 1e-9 || frame.determinant().abs() < 1e-20 { return Affine::IDENTITY; }
    let unit = handles::rect_quad(Rect::new(0.0, 0.0, 1.0, 1.0));
    let start = handles::handle_pos(unit, handle);
    let p = frame.inverse() * pointer;
    let pivot = if center && !shear { Point::new(0.5, 0.5) } else { Point::new(1.0-start.x, 1.0-start.y) };
    let dx = start.x - pivot.x;
    let dy = start.y - pivot.y;
    let mut sx = if dx != 0.0 { (p.x-pivot.x)/dx } else { 1.0 };
    let mut sy = if dy != 0.0 { (p.y-pivot.y)/dy } else { 1.0 };
    let mut xy = 0.0;
    let mut yx = 0.0;
    if shear {
        if dy != 0.0 { xy = (p.x-start.x)/dy; if constrain { sy = 1.0; } }
        else { yx = (p.y-start.y)/dx; if constrain { sx = 1.0; } }
    } else if constrain {
        let scale = if dx == 0.0 { sy } else if dy == 0.0 || (sx-1.0).abs() >= (sy-1.0).abs() { sx } else { sy };
        sx = scale; sy = scale;
    }
    // Permit reflection while retaining an invertible transform at the crossing.
    let nonzero = |s: f64| if s.abs() < 1e-6 { if s < 0.0 { -1e-6 } else { 1e-6 } } else { s };
    sx = nonzero(sx); sy = nonzero(sy);
    frame * Affine::translate(pivot.to_vec2()) * Affine::new([sx,yx,xy,sy,0.0,0.0])
        * Affine::translate(-pivot.to_vec2()) * frame.inverse()
}

fn warp_targets(document: &amalith_core::Document, selection: &[ObjectId]) -> Vec<ObjectId> {
    fn visit(doc: &amalith_core::Document, id: ObjectId, seen: &mut HashSet<ObjectId>, ids: &mut Vec<ObjectId>) {
        if !seen.insert(id) { return; }
        let Some(object) = doc.object(id) else { return; };
        match &object.kind {
            amalith_core::ObjectKind::Group(g) => for &child in &g.children { visit(doc, child, seen, ids); },
            amalith_core::ObjectKind::Path(_) => ids.push(id),
            _ => {},
        }
    }
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for &id in selection { visit(document, id, &mut seen, &mut ids); }
    ids
}

/// A finite projective image of a filled rectangle must remain convex.
fn valid_quad(q: [Point; 4]) -> bool {
    let mut sign = 0.0_f64;
    for i in 0..4 {
        let a = q[(i+1)%4] - q[i];
        let b = q[(i+2)%4] - q[(i+1)%4];
        let c = a.x*b.y-a.y*b.x;
        if !c.is_finite() || c.abs() <= 1e-9*a.hypot()*b.hypot() { return false; }
        if sign != 0.0 && c.signum() != sign { return false; }
        sign = c.signum();
    }
    true
}

impl App {
    /// The flyout's screen layout — `None` when it shouldn't show (wrong
    /// tool, or nothing selected).
    pub(in crate::app) fn free_transform_flyout_layout(&self) -> Option<FreeTransformFlyout> {
        if self.active_tool != Tool::FreeTransform || self.doc.selection.is_empty() {
            return None;
        }
        let quad = select::selection_quad(self.doc.editor.document(), &self.doc.selection)?;
        let to_screen = self.doc.view.to_screen();
        let scr = quad.map(|p| to_screen * p);
        let (mut x0, mut y0, mut x1, mut y1) =
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in scr {
            x0 = x0.min(p.x);
            y0 = y0.min(p.y);
            x1 = x1.max(p.x);
            y1 = y1.max(p.y);
        }
        let w = 4.0 * FT_BTN_W + 3.0 * FT_BTN_GAP + 2.0 * FT_PAD;
        let h = FT_BTN_H + 2.0 * FT_PAD;
        let viewport = self.canvas_viewport();
        let cx = (x0 + x1) * 0.5;
        let x = (cx - w * 0.5).max(viewport.x0 + 4.0).min((viewport.x1 - w - 4.0).max(viewport.x0 + 4.0));
        let y = (y1 + 10.0).max(viewport.y0 + 4.0).min((viewport.y1 - h - 4.0).max(viewport.y0 + 4.0));
        let panel = Rect::new(x, y, x + w, y + h);
        let row: [Rect; 4] = std::array::from_fn(|i| {
            let bx = panel.x0 + FT_PAD + i as f64 * (FT_BTN_W + FT_BTN_GAP);
            Rect::new(bx, panel.y0 + FT_PAD, bx + FT_BTN_W, panel.y0 + FT_PAD + FT_BTN_H)
        });
        Some(FreeTransformFlyout {
            panel,
            constrain: row[0],
            transform: row[1],
            perspective: row[2],
            free_distort: row[3],
        })
    }

    /// Capture the entire painted panel, including button gaps and padding.
    pub(in crate::app) fn free_transform_flyout_press(&mut self) -> bool {
        let Some(lay) = self.free_transform_flyout_layout() else { return false };
        for (r, btn) in lay.buttons() {
            if r.contains(self.pointer) {
                match btn {
                    FlyoutButton::Constrain => {
                        if self.free_transform_mode != FreeTransformMode::Perspective {
                            self.free_transform_constrain = !self.free_transform_constrain;
                        }
                    }
                    FlyoutButton::Transform => self.free_transform_mode = FreeTransformMode::Transform,
                    FlyoutButton::Perspective => {
                        self.free_transform_mode = FreeTransformMode::Perspective;
                    }
                    FlyoutButton::FreeDistort => {
                        self.free_transform_mode = FreeTransformMode::FreeDistort;
                    }
                }
                self.request_main_redraw();
                return true;
            }
        }
        lay.panel.contains(self.pointer)
    }

    /// Capture handles in every mode so Cmd/Ctrl can switch a running
    /// scale to distortion. Unsupported selections retain affine handling.
    pub(in crate::app) fn free_transform_warp_press(&mut self) -> bool {
        if self.doc.selection.is_empty() {
            return false;
        }
        let document = self.doc.editor.document();
        let Some(quad) = select::selection_quad(document, &self.doc.selection) else {
            return false;
        };
        let to_screen = self.doc.view.to_screen();
        let scr = quad.map(|p| to_screen * p);
        let Some(handle) = handles::hit_handle(self.pointer, scr) else { return false; };
        if !valid_quad(quad) { return false; }
        let objects = warp_targets(document, &self.doc.selection);
        let start_xf = self.doc.selection.iter().filter_map(|&id| document.object(id).map(|o| (id, convert::affine(o.transform)))).collect::<HashMap<_,_>>();
        self.drag = Drag::Warp {
            handle, src_quad: quad, dst_quad: quad, objects,
            press_doc: self.doc_point(self.pointer),
            preview: start_xf.clone(), start_xf, warping: false,
        };
        self.request_main_redraw();
        true
    }

    /// Recompute from press-time geometry so modifier changes never accumulate edits.
    pub(in crate::app) fn update_free_transform_drag(&mut self) {
        let Drag::Warp { handle, src_quad, objects, press_doc, start_xf, .. } = &self.drag else { return; };
        let (handle, src, press) = (*handle, *src_quad, *press_doc);
        let (objects, start_xf) = (objects.clone(), start_xf.clone());
        let pointer = handles::handle_pos(src, handle) + (self.doc_point(self.pointer)-press);
        let corner = matches!(handle, Handle::Nw | Handle::Ne | Handle::Se | Handle::Sw);
        let mode = if self.cmd_down && corner {
            if self.shift_down && self.alt_down { FreeTransformMode::Perspective } else { FreeTransformMode::FreeDistort }
        } else { self.free_transform_mode };
        let warping = corner && mode != FreeTransformMode::Transform && !objects.is_empty();
        let constrain = self.shift_down || self.free_transform_constrain;
        let mut dst = src;
        let mut preview = start_xf.clone();
        if warping {
            dst = warp_dst_quad(mode, constrain, src, handle, pointer);
            if !valid_quad(dst) { return; } // Keep the last finite preview at a collapsed/crossed edge.
            let Some(h) = amalith_core::Homography::try_solve(src.map(convert::point_to_core), dst.map(convert::point_to_core)) else { return; };
            let doc = self.doc.editor.document();
            if objects.iter().any(|&id| {
                let world = doc.world_transform(id);
                doc.object(id).and_then(|o| o.kind.path_data()).is_none_or(|p| h.conjugate(world, world.inverse()).warp_path(p).is_none())
            }) { return; }
        } else {
            let shear = !corner && self.cmd_down && self.alt_down;
            let m = affine_drag(src, handle, pointer, constrain, self.alt_down, shear);
            let doc = self.doc.editor.document();
            preview = start_xf.iter().map(|(&id,&s)| {
                let parent = match doc.object(id).map(|o| o.parent) {
                    Some(amalith_core::ObjectParent::Group(id)) => doc.world_transform(id),
                    _ => amalith_core::Affine::IDENTITY,
                };
                let parent = convert::affine(parent);
                (id, parent.inverse()*m*parent*s)
            }).collect();
        }
        self.drag = Drag::Warp { handle, src_quad: src, dst_quad: dst, objects, press_doc: press, start_xf, preview, warping };
        self.request_main_redraw();
    }

    pub(in crate::app) fn cancel_free_transform_drag(&mut self) -> bool {
        if matches!(self.drag, Drag::Warp { .. }) || (self.active_tool == Tool::FreeTransform && matches!(self.drag, Drag::Rotate { .. })) {
            self.drag = Drag::None;
            self.request_main_redraw();
            true
        } else { false }
    }

    /// Commits a finished `Drag::Warp` gesture: solves one homography in
    /// document space from `src_quad` to `dst_quad`, re-expresses it in
    /// each affected object's own local space (conjugated by that
    /// object's world transform — see `Homography::conjugate`), and
    /// applies all of them as one undoable `Command::WarpPaths`.
    pub(in crate::app) fn commit_warp(
        &mut self,
        src_quad: [Point; 4],
        dst_quad: [Point; 4],
        objects: Vec<ObjectId>,
    ) {
        if objects.is_empty() || src_quad == dst_quad {
            return;
        }
        let src_core = src_quad.map(convert::point_to_core);
        let dst_core = dst_quad.map(convert::point_to_core);
        let Some(h_doc) = amalith_core::Homography::try_solve(src_core, dst_core) else { return };
        let document = self.doc.editor.document();
        let items: Vec<(ObjectId, amalith_core::Homography)> = objects
            .iter()
            .map(|&id| {
                let world = document.world_transform(id);
                (id, h_doc.conjugate(world, world.inverse()))
            })
            .collect();
        let _ = self.doc.editor.execute(Command::WarpPaths { items });
        self.request_main_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square() -> [Point;4] { handles::rect_quad(Rect::new(0.,0.,100.,100.)) }
    fn close(a: Point,b: Point) { assert!((a-b).hypot()<1e-7,"{a:?} != {b:?}"); }

    #[test]
    fn perspective_moves_only_one_edge_pair_on_every_corner() {
        let src=square();
        for handle in [Handle::Nw,Handle::Ne,Handle::Se,Handle::Sw] {
            let i=corner_index(handle);
            for (delta,partner) in [(Vec2::new(10.,0.),edge_partners(i).0),(Vec2::new(0.,10.),edge_partners(i).1)] {
                let dst=warp_dst_quad(FreeTransformMode::Perspective,false,src,handle,src[i]+delta);
                for j in 0..4 { close(dst[j],if j==i {src[j]+delta} else if j==partner {src[j]-delta} else {src[j]}); }
            }
        }
    }

    #[test]
    fn perspective_locks_diagonal_drag_in_rotated_sheared_frame() {
        let frame=Affine::translate((20.,30.))*Affine::rotate(0.7)*Affine::new([1.,0.,0.3,1.,0.,0.]);
        let src=square().map(|p|frame*p);
        let dst=warp_dst_quad(FreeTransformMode::Perspective,false,src,Handle::Nw,frame*Point::new(20.,2.));
        let expected=[Point::new(20.,0.),Point::new(80.,0.),Point::new(100.,100.),Point::new(0.,100.)];
        for i in 0..4 {close(dst[i],frame*expected[i]);}
    }

    #[test]
    fn free_distort_constraint_never_moves_another_corner() {
        for constrain in [false,true] {
            let src=square();
            let dst=warp_dst_quad(FreeTransformMode::FreeDistort,constrain,src,Handle::Nw,Point::new(20.,5.));
            close(dst[0],Point::new(20.,if constrain {0.} else {5.}));
            assert_eq!(&dst[1..],&src[1..]);
        }
    }

    #[test]
    fn rotated_scale_keeps_opposite_corner_and_tracks_pointer() {
        let frame=Affine::translate((20.,30.))*Affine::rotate(0.8);
        let src=square().map(|p|frame*p);
        let pointer=frame*Point::new(50.,30.);
        let m=affine_drag(src,Handle::Nw,pointer,false,false,false);
        close(m*src[0],pointer);close(m*src[2],src[2]);
        close(m*src[1],frame*Point::new(100.,30.));
    }

    #[test]
    fn centered_scale_and_constrained_shear_keep_their_pivots() {
        let src=square();
        let m=affine_drag(src,Handle::Se,Point::new(150.,150.),true,true,false);
        close(m*Point::new(50.,50.),Point::new(50.,50.));
        close(m*src[0],Point::new(-50.,-50.));
        let shear=affine_drag(src,Handle::N,Point::new(70.,10.),true,false,true);
        close(shear*src[0],Point::new(20.,0.));
        close(shear*src[3],src[3]);
    }

    #[test]
    fn collapsed_and_crossed_quads_are_not_committable() {
        let mut q=square();assert!(valid_quad(q));
        q[0]=q[1];assert!(!valid_quad(q));
        q=square();q[0]=Point::new(120.,120.);assert!(!valid_quad(q));
    }

    #[test]
    fn selected_groups_expand_to_unique_path_leaves() {
        let mut editor=Editor::new(amalith_core::Document::new("warp test"));
        let CommandOutcome::Layer(layer)=editor.execute(Command::CreateLayer {name:"test".into(),index:None}).unwrap() else {panic!()};
        let CommandOutcome::Object(path)=editor.execute(Command::CreateRect {layer,rect:amalith_core::Rect::new(0.,0.,100.,100.),name:None}).unwrap() else {panic!()};
        let CommandOutcome::Object(group)=editor.execute(Command::Group {ids:vec![path],name:None}).unwrap() else {panic!()};
        assert_eq!(warp_targets(editor.document(),&[group,path]),vec![path]);
        assert!(warp_targets(editor.document(),&[]).is_empty());
    }
}
