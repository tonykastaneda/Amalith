//! The path-editing tools that sit beside the Pen: Delete Anchor Point,
//! Anchor Point, Scissors, Curvature and Reshape — each one undoable step.
use amalith_commands::{Command, CommandOutcome, Editor, PathPoint};
use amalith_core::*;

fn editor_with(path: PathData) -> (Editor, LayerId, ObjectId) {
    let mut doc = Document::new("Paths");
    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer"), 0);
    let id = ObjectId::new();
    doc.insert_object(Object::new(id, ObjectParent::Layer(layer), ObjectKind::Path(path)), 0).unwrap();
    (Editor::new(doc), layer, id)
}

fn polyline(points: &[(f64, f64)], closed: bool) -> PathData {
    PathData::from_subpaths(vec![Subpath {
        anchors: points.iter().map(|&(x, y)| Anchor::corner(Point::new(x, y))).collect(),
        closed,
    }])
}

fn subpaths(editor: &Editor, id: ObjectId) -> Vec<Subpath> {
    editor.document().object(id).unwrap().kind.path_data().unwrap().subpaths().to_vec()
}

#[test]
fn remove_anchor_keeps_the_shape_closed_and_undoes() {
    let square = polyline(&[(0., 0.), (100., 0.), (100., 100.), (0., 100.)], true);
    let (mut editor, _, id) = editor_with(square.clone());
    editor.execute(Command::RemoveAnchor { object: id, anchor: 1 }).unwrap();
    let after = subpaths(&editor, id);
    assert_eq!(after.len(), 1);
    assert!(after[0].closed, "Delete Anchor Point must not open the shape");
    assert_eq!(after[0].anchors.len(), 3);
    editor.undo().unwrap();
    assert_eq!(subpaths(&editor, id), square.subpaths());
}

#[test]
fn removing_a_lines_anchor_removes_the_line() {
    let (mut editor, layer, id) = editor_with(polyline(&[(0., 0.), (100., 0.)], false));
    editor.execute(Command::RemoveAnchor { object: id, anchor: 0 }).unwrap();
    assert!(editor.document().object(id).is_none());
    assert!(editor.document().layer(layer).unwrap().children.is_empty());
    editor.undo().unwrap();
    assert_eq!(subpaths(&editor, id)[0].anchors.len(), 2);
}

#[test]
fn pull_anchor_handles_makes_a_symmetric_point() {
    let (mut editor, _, id) = editor_with(polyline(&[(0., 0.), (50., 50.), (100., 0.)], false));
    editor
        .execute(Command::PullAnchorHandles { object: id, anchor: 1, handle_out: Point::new(80., 50.) })
        .unwrap();
    let a = subpaths(&editor, id)[0].anchors[1];
    assert_eq!(a.mode, HandleMode::Symmetric);
    assert_eq!(a.handle_out, Some(Point::new(80., 50.)));
    assert_eq!(a.handle_in, Some(Point::new(20., 50.)));
    // Dragging back onto the anchor itself leaves a corner.
    editor
        .execute(Command::PullAnchorHandles { object: id, anchor: 1, handle_out: Point::new(50., 50.) })
        .unwrap();
    let a = subpaths(&editor, id)[0].anchors[1];
    assert_eq!((a.handle_in, a.handle_out, a.mode), (None, None, HandleMode::Corner));
}

#[test]
fn scissors_on_a_closed_path_opens_it_in_place() {
    let (mut editor, layer, id) = editor_with(polyline(&[(0., 0.), (100., 0.), (100., 100.), (0., 100.)], true));
    let out = editor
        .execute(Command::SplitPath { object: id, at: PathPoint::Segment { segment: 0, t: 0.5 } })
        .unwrap();
    assert_eq!(out, CommandOutcome::None);
    let sp = subpaths(&editor, id);
    assert!(!sp[0].closed);
    assert_eq!(sp[0].anchors.first().unwrap().point, Point::new(50., 0.));
    assert_eq!(sp[0].anchors.last().unwrap().point, Point::new(50., 0.));
    assert_eq!(editor.document().layer(layer).unwrap().children, vec![id]);
}

#[test]
fn scissors_on_an_open_path_detaches_a_new_object_above_it() {
    let (mut editor, layer, id) = editor_with(polyline(&[(0., 0.), (100., 0.), (200., 0.)], false));
    editor.execute(Command::SetStrokeWidth { objects: vec![id], width: 7.0 }).unwrap();
    let before = editor.document().object(id).unwrap().clone();
    let CommandOutcome::Object(piece) =
        editor.execute(Command::SplitPath { object: id, at: PathPoint::Anchor(1) }).unwrap()
    else {
        panic!("an open-path cut should create the cut-off piece");
    };
    assert_eq!(editor.document().layer(layer).unwrap().children, vec![id, piece]);
    let head = subpaths(&editor, id);
    let tail = subpaths(&editor, piece);
    assert_eq!(head[0].anchors.last().unwrap().point, Point::new(100., 0.));
    assert_eq!(tail[0].anchors.first().unwrap().point, Point::new(100., 0.));
    assert_eq!(tail[0].anchors.last().unwrap().point, Point::new(200., 0.));
    let piece_obj = editor.document().object(piece).unwrap();
    assert_eq!(piece_obj.appearance, before.appearance);
    assert_eq!(piece_obj.transform, before.transform);
    // One undo step puts everything back.
    editor.undo().unwrap();
    assert_eq!(editor.document().object(id), Some(&before));
    assert!(editor.document().object(piece).is_none());
}

#[test]
fn scissors_on_an_endpoint_is_a_no_op() {
    let (mut editor, _, id) = editor_with(polyline(&[(0., 0.), (100., 0.)], false));
    let can_undo = editor.can_undo();
    let out = editor.execute(Command::SplitPath { object: id, at: PathPoint::Anchor(0) }).unwrap();
    assert_eq!(out, CommandOutcome::None);
    assert_eq!(editor.can_undo(), can_undo, "a no-op cut must not leave an undo step");
}

#[test]
fn curvature_rebuilds_the_subpath_through_its_points() {
    let (mut editor, _, id) = editor_with(polyline(&[(0., 0.), (100., 0.)], false));
    let points: Vec<CurvaturePoint> = [(0., 0.), (50., 40.), (100., 0.)]
        .iter()
        .map(|&(x, y)| CurvaturePoint { point: Point::new(x, y), corner: false })
        .collect();
    editor
        .execute(Command::SetCurvaturePath { object: id, subpath: 0, points: points.clone(), closed: false })
        .unwrap();
    let sp = &subpaths(&editor, id)[0];
    assert_eq!(sp.anchors.len(), 3);
    assert_eq!(sp.anchors[1].point, Point::new(50., 40.));
    assert_eq!(sp.anchors[1].mode, HandleMode::Smooth);
    assert!(sp.anchors[1].handle_in.is_some() && sp.anchors[1].handle_out.is_some());
    // Reading the model back off the path round-trips, ends included.
    assert_eq!(curvature_points(sp), points);
    editor.undo().unwrap();
    assert_eq!(subpaths(&editor, id)[0].anchors.len(), 2);
}

#[test]
fn reshape_drags_a_point_on_the_curve_with_falloff() {
    let (mut editor, _, id) = editor_with(polyline(&[(0., 0.), (200., 0.)], false));
    editor
        .execute(Command::ReshapePath {
            object: id,
            at: Point::new(100., 0.),
            delta: Vec2::new(0., 30.),
            tolerance: 2.0,
        })
        .unwrap();
    let sp = &subpaths(&editor, id)[0];
    assert_eq!(sp.anchors.len(), 3, "a grab between anchors inserts one");
    assert_eq!(sp.anchors[1].point, Point::new(100., 30.));
    assert!(sp.anchors[0].point.y < 30.0 && sp.anchors[2].point.y < 30.0);
    editor.undo().unwrap();
    assert_eq!(subpaths(&editor, id)[0].anchors.len(), 2);
}

#[test]
fn a_grid_is_one_group_of_paths_in_one_undo_step() {
    let mut doc = Document::new("Grid");
    let layer = LayerId::new();
    doc.insert_layer(Layer::new(layer, "Layer"), 0);
    let mut editor = Editor::new(doc);
    let mut look = Appearance::default();
    look.set_fill(Paint::None);
    let paths = grid::rectangular_grid(Rect::new(0., 0., 100., 100.), 2, 2);
    let CommandOutcome::Object(group) = editor
        .execute(Command::CreatePathGroup {
            parent: ObjectParent::Layer(layer),
            paths,
            appearance: Some(look.clone()),
            name: None,
        })
        .unwrap()
    else {
        panic!("the outcome is the group");
    };
    let ObjectKind::Group(g) = &editor.document().object(group).unwrap().kind else { panic!("a group") };
    assert_eq!(g.children.len(), 5);
    for child in &g.children {
        let c = editor.document().object(*child).unwrap();
        assert_eq!(c.parent, ObjectParent::Group(group));
        assert_eq!(c.appearance, look);
    }
    editor.undo().unwrap();
    assert!(editor.document().layer(layer).unwrap().children.is_empty());
    assert!(!editor.can_undo());
}

#[test]
fn a_liquify_stroke_is_one_deterministic_undo_step_in_document_space() {
    use amalith_core::liquify::{LiquifyKind, LiquifyParams};
    let square = polyline(&[(0., 0.), (200., 0.), (200., 200.), (0., 200.)], true);
    let (mut editor, _, id) = editor_with(square.clone());
    // Move the square so local and document space differ.
    editor.execute(Command::SetTransform { object: id, transform: Affine::translate((1000., 0.)) }).unwrap();
    let params = LiquifyParams { intensity: 1.0, width: 100.0, height: 100.0, ..LiquifyParams::new(LiquifyKind::Bloat) };
    let stroke = vec![Point::new(1180., 100.); 3];
    let run = |editor: &mut Editor| {
        editor.execute(Command::Liquify { objects: vec![id], stroke: stroke.clone(), params }).unwrap();
        subpaths(editor, id)
    };
    let first = run(&mut editor);
    let right = first[0].anchors.iter().map(|a| a.point.x).fold(f64::MIN, f64::max);
    assert!(right > 201.0, "the brush, placed in document space, bulged the right edge: {right}");
    editor.undo().unwrap();
    assert_eq!(subpaths(&editor, id), square.subpaths());
    assert_eq!(run(&mut editor), first, "replaying the stroke gives the same path");
    // A stroke nowhere near the path leaves no undo step.
    editor.undo().unwrap();
    let far = vec![Point::new(-5000., -5000.)];
    editor.execute(Command::Liquify { objects: vec![id], stroke: far, params }).unwrap();
    assert!(!editor.can_undo() || subpaths(&editor, id) == square.subpaths());
}
