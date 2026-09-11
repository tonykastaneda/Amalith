//! Cut open centerlines at brush crossings without flattening their Béziers.
use kurbo::{BezPath, Line, ParamCurve, PathEl, PathSeg, Shape};

pub(crate) fn erase(path: &BezPath, area: &BezPath) -> Option<Vec<BezPath>> {
    let contours = crate::pathfinder::flatten_path(area);
    let mask = crate::pathfinder::polygon_path(&contours).geometry;
    let edges: Vec<_> = mask.segments().map(|s| Line::new(s.start(), s.end())).collect();
    let mut subpaths = Vec::new();
    let mut current = BezPath::new();
    for &el in path.elements() {
        if matches!(el, PathEl::MoveTo(_)) && !current.elements().is_empty() {
            subpaths.push(std::mem::take(&mut current));
        }
        current.push(el);
    }
    if !current.elements().is_empty() { subpaths.push(current); }
    let mut closed = BezPath::new();
    let mut result = Vec::new();
    let mut changed = false;
    for sub in subpaths {
        if matches!(sub.elements().last(), Some(PathEl::ClosePath)) {
            closed.extend(sub.elements().iter().copied());
            continue;
        }
        let mut piece = BezPath::new();
        for seg in sub.segments() {
            let mut ts = vec![0.0, 1.0];
            for edge in &edges {
                ts.extend(seg.intersect_line(*edge).iter().map(|hit| hit.segment_t.clamp(0.0, 1.0)));
            }
            ts.sort_by(f64::total_cmp);
            ts.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            for pair in ts.windows(2) {
                if mask.winding(seg.eval((pair[0] + pair[1]) * 0.5)) != 0 {
                    changed = true;
                    if !piece.elements().is_empty() { result.push(std::mem::take(&mut piece)); }
                } else {
                    let span = seg.subsegment(pair[0]..pair[1]);
                    if piece.elements().is_empty() { piece.move_to(span.start()); }
                    match span {
                        PathSeg::Line(l) => piece.line_to(l.p1),
                        PathSeg::Quad(q) => piece.quad_to(q.p1, q.p2),
                        PathSeg::Cubic(c) => piece.curve_to(c.p1, c.p2, c.p3),
                    }
                }
            }
        }
        if !piece.elements().is_empty() { result.push(piece); }
    }
    if !closed.elements().is_empty() {
        if let Some(pieces) = crate::pathfinder::erase_closed(&closed, area, &contours) {
            changed = true;
            result.extend(pieces);
        } else { result.push(closed); }
    }
    changed.then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crossing_splits_a_line_into_two_open_paths() {
        let mut line = BezPath::new();
        line.move_to((0.0, 0.0)); line.line_to((100.0, 0.0));
        let mask = kurbo::Rect::new(40.0, -10.0, 60.0, 10.0).to_path(0.01);
        let pieces = erase(&line, &mask).unwrap();
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].segments().last().unwrap().end().x, 40.0);
        assert_eq!(pieces[1].segments().next().unwrap().start().x, 60.0);
        assert!(pieces.iter().all(|p| p.elements().len() == 2));
        assert!(erase(&pieces[0], &kurbo::Rect::new(45.0, -5.0, 55.0, 5.0).to_path(0.01)).is_none());
    }
    #[test]
    fn closed_shape_splits_into_separate_closed_pieces() {
        let path = kurbo::Rect::new(0.0, 0.0, 100.0, 100.0).to_path(0.01);
        let mask = kurbo::Rect::new(40.0, -10.0, 60.0, 110.0).to_path(0.01);
        let pieces = erase(&path, &mask).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|p| matches!(p.elements().last(), Some(PathEl::ClosePath))));
        assert!((pieces.iter().map(|p| p.area().abs()).sum::<f64>() - 8000.0).abs() < 0.01);
    }

    #[test]
    fn cut_preserves_exact_cubic_spans() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((30.0, -40.0), (70.0, 40.0), (100.0, 0.0));
        let pieces = erase(&path, &kurbo::Rect::new(45.0, -100.0, 55.0, 100.0).to_path(0.01)).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|p| matches!(p.elements()[1], PathEl::CurveTo(..))));
        assert!((pieces[0].segments().next().unwrap().end().x - 45.0).abs() < 1e-7);
        assert!((pieces[1].segments().next().unwrap().start().x - 55.0).abs() < 1e-7);
    }
}
