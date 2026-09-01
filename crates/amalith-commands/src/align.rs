//! Align and distribute: document-space moves relative to the selection,
//! a key object, or an artboard.

use amalith_core::{ObjectId, Rect, Vec2};

/// Where alignment / distribution is measured from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignTo {
    Selection,
    KeyObject,
    Artboard,
}

/// One button on the Align panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlignKind {
    HLeft,
    HCenter,
    HRight,
    VTop,
    VCenter,
    VBottom,
    DistHLeft,
    DistHCenter,
    DistHRight,
    DistVTop,
    DistVCenter,
    DistVBottom,
    DistHSpace,
    DistVSpace,
}

#[derive(Clone, Copy)]
struct Item {
    id: ObjectId,
    b: Rect,
}

fn h_edge(b: Rect, kind: AlignKind) -> f64 {
    match kind {
        AlignKind::HLeft | AlignKind::DistHLeft => b.x0,
        AlignKind::HCenter | AlignKind::DistHCenter => (b.x0 + b.x1) * 0.5,
        AlignKind::HRight | AlignKind::DistHRight => b.x1,
        _ => (b.x0 + b.x1) * 0.5,
    }
}

fn v_edge(b: Rect, kind: AlignKind) -> f64 {
    match kind {
        AlignKind::VTop | AlignKind::DistVTop => b.y0,
        AlignKind::VCenter | AlignKind::DistVCenter => (b.y0 + b.y1) * 0.5,
        AlignKind::VBottom | AlignKind::DistVBottom => b.y1,
        _ => (b.y0 + b.y1) * 0.5,
    }
}

fn union(items: &[Item]) -> Option<Rect> {
    items
        .iter()
        .map(|i| i.b)
        .reduce(|a, b| a.union(b))
}

/// Document-space translation for each object. The key object (if any)
/// never moves for align; distribute spacing with a key starts from it.
pub fn deltas(
    bounds: &[(ObjectId, Rect)],
    kind: AlignKind,
    to: AlignTo,
    key: Option<ObjectId>,
    frame: Rect,
    spacing: Option<f64>,
) -> Vec<(ObjectId, Vec2)> {
    let items: Vec<Item> = bounds.iter().map(|&(id, b)| Item { id, b }).collect();
    if items.is_empty() {
        return Vec::new();
    }
    match kind {
        AlignKind::HLeft | AlignKind::HCenter | AlignKind::HRight => {
            align_h(&items, kind, to, key, frame)
        }
        AlignKind::VTop | AlignKind::VCenter | AlignKind::VBottom => {
            align_v(&items, kind, to, key, frame)
        }
        AlignKind::DistHLeft | AlignKind::DistHCenter | AlignKind::DistHRight => {
            dist_h(&items, kind, to, frame)
        }
        AlignKind::DistVTop | AlignKind::DistVCenter | AlignKind::DistVBottom => {
            dist_v(&items, kind, to, frame)
        }
        AlignKind::DistHSpace => dist_space_h(&items, to, key, frame, spacing),
        AlignKind::DistVSpace => dist_space_v(&items, to, key, frame, spacing),
    }
}

fn align_h(
    items: &[Item],
    kind: AlignKind,
    to: AlignTo,
    key: Option<ObjectId>,
    frame: Rect,
) -> Vec<(ObjectId, Vec2)> {
    let target = match to {
        AlignTo::Selection => h_edge(union(items).unwrap(), kind),
        AlignTo::KeyObject => items
            .iter()
            .find(|i| Some(i.id) == key)
            .map(|i| h_edge(i.b, kind))
            .unwrap_or_else(|| h_edge(union(items).unwrap(), kind)),
        AlignTo::Artboard => h_edge(frame, kind),
    };
    items
        .iter()
        .filter(|i| to != AlignTo::KeyObject || Some(i.id) != key)
        .map(|i| (i.id, Vec2::new(target - h_edge(i.b, kind), 0.0)))
        .filter(|(_, d)| d.x.abs() > 1e-9)
        .collect()
}

fn align_v(
    items: &[Item],
    kind: AlignKind,
    to: AlignTo,
    key: Option<ObjectId>,
    frame: Rect,
) -> Vec<(ObjectId, Vec2)> {
    let target = match to {
        AlignTo::Selection => v_edge(union(items).unwrap(), kind),
        AlignTo::KeyObject => items
            .iter()
            .find(|i| Some(i.id) == key)
            .map(|i| v_edge(i.b, kind))
            .unwrap_or_else(|| v_edge(union(items).unwrap(), kind)),
        AlignTo::Artboard => v_edge(frame, kind),
    };
    items
        .iter()
        .filter(|i| to != AlignTo::KeyObject || Some(i.id) != key)
        .map(|i| (i.id, Vec2::new(0.0, target - v_edge(i.b, kind))))
        .filter(|(_, d)| d.y.abs() > 1e-9)
        .collect()
}

fn dist_h(items: &[Item], kind: AlignKind, to: AlignTo, frame: Rect) -> Vec<(ObjectId, Vec2)> {
    let mut items = items.to_vec();
    items.sort_by(|a, b| h_edge(a.b, kind).partial_cmp(&h_edge(b.b, kind)).unwrap());
    let n = items.len();
    if n < 2 {
        return Vec::new();
    }
    // Selection/key: first & last pin, so 3+ objects. Artboard: 2 is enough
    // to stretch the pair to the frame.
    if n < 3 && !matches!(to, AlignTo::Artboard) {
        return Vec::new();
    }
    let (lo, hi) = match to {
        AlignTo::Artboard => {
            // Span the artboard with the same edge used for distribution.
            let lo = match kind {
                AlignKind::DistHLeft => frame.x0,
                AlignKind::DistHRight => frame.x1 - (items[n - 1].b.width() - items[0].b.width()).max(0.0),
                _ => frame.x0 + items[0].b.width() * 0.5,
            };
            let hi = match kind {
                AlignKind::DistHLeft => frame.x1 - items[n - 1].b.width(),
                AlignKind::DistHRight => frame.x1,
                _ => frame.x1 - items[n - 1].b.width() * 0.5,
            };
            (lo, hi)
        }
        _ => (h_edge(items[0].b, kind), h_edge(items[n - 1].b, kind)),
    };
    let span = hi - lo;
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != 0 && *i != n - 1)
        .map(|(i, it)| {
            let desired = lo + span * i as f64 / (n - 1) as f64;
            (it.id, Vec2::new(desired - h_edge(it.b, kind), 0.0))
        })
        .chain(match to {
            AlignTo::Artboard => vec![
                (items[0].id, Vec2::new(lo - h_edge(items[0].b, kind), 0.0)),
                (
                    items[n - 1].id,
                    Vec2::new(hi - h_edge(items[n - 1].b, kind), 0.0),
                ),
            ],
            _ => Vec::new(),
        })
        .filter(|(_, d)| d.x.abs() > 1e-9)
        .collect()
}

fn dist_v(items: &[Item], kind: AlignKind, to: AlignTo, frame: Rect) -> Vec<(ObjectId, Vec2)> {
    let mut items = items.to_vec();
    items.sort_by(|a, b| v_edge(a.b, kind).partial_cmp(&v_edge(b.b, kind)).unwrap());
    let n = items.len();
    if n < 2 {
        return Vec::new();
    }
    if n < 3 && !matches!(to, AlignTo::Artboard) {
        return Vec::new();
    }
    let (lo, hi) = match to {
        AlignTo::Artboard => {
            let lo = match kind {
                AlignKind::DistVTop => frame.y0,
                AlignKind::DistVBottom => frame.y1 - (items[n - 1].b.height() - items[0].b.height()).max(0.0),
                _ => frame.y0 + items[0].b.height() * 0.5,
            };
            let hi = match kind {
                AlignKind::DistVTop => frame.y1 - items[n - 1].b.height(),
                AlignKind::DistVBottom => frame.y1,
                _ => frame.y1 - items[n - 1].b.height() * 0.5,
            };
            (lo, hi)
        }
        _ => (v_edge(items[0].b, kind), v_edge(items[n - 1].b, kind)),
    };
    let span = hi - lo;
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| {
            if matches!(to, AlignTo::Artboard) {
                true
            } else {
                *i != 0 && *i != n - 1
            }
        })
        .map(|(i, it)| {
            let desired = lo + span * i as f64 / (n - 1) as f64;
            (it.id, Vec2::new(0.0, desired - v_edge(it.b, kind)))
        })
        .filter(|(_, d)| d.y.abs() > 1e-9)
        .collect()
}

fn dist_space_h(
    items: &[Item],
    to: AlignTo,
    key: Option<ObjectId>,
    frame: Rect,
    spacing: Option<f64>,
) -> Vec<(ObjectId, Vec2)> {
    let mut items = items.to_vec();
    items.sort_by(|a, b| a.b.x0.partial_cmp(&b.b.x0).unwrap());
    let n = items.len();
    if n < 2 {
        return Vec::new();
    }
    if let Some(g) = spacing {
        if let Some(kid) = key.filter(|_| to == AlignTo::KeyObject) {
            return space_from_key_h(&items, kid, g);
        }
        // Exact gap; first stays (or first goes to artboard left).
        let mut x = match to {
            AlignTo::Artboard => frame.x0,
            _ => items[0].b.x0,
        };
        let mut out = Vec::new();
        for it in &items {
            let dx = x - it.b.x0;
            if dx.abs() > 1e-9 {
                out.push((it.id, Vec2::new(dx, 0.0)));
            }
            x += it.b.width() + g;
        }
        return out;
    }
    // Auto: equal gaps, pin first & last (or artboard).
    if n < 3 && to != AlignTo::Artboard {
        return Vec::new();
    }
    let first_x0 = match to {
        AlignTo::Artboard => frame.x0,
        _ => items[0].b.x0,
    };
    let last_x1 = match to {
        AlignTo::Artboard => frame.x1,
        _ => items[n - 1].b.x1,
    };
    let sum_w: f64 = items.iter().map(|i| i.b.width()).sum();
    let gap = (last_x1 - first_x0 - sum_w) / (n - 1) as f64;
    let mut x = first_x0;
    let mut out = Vec::new();
    for it in &items {
        let dx = x - it.b.x0;
        if dx.abs() > 1e-9 {
            out.push((it.id, Vec2::new(dx, 0.0)));
        }
        x += it.b.width() + gap;
    }
    out
}

fn space_from_key_h(items: &[Item], key: ObjectId, g: f64) -> Vec<(ObjectId, Vec2)> {
    let ki = items.iter().position(|i| i.id == key).unwrap_or(0);
    let mut out = Vec::new();
    let mut x = items[ki].b.x1;
    for it in items.iter().skip(ki + 1) {
        x += g;
        let dx = x - it.b.x0;
        if dx.abs() > 1e-9 {
            out.push((it.id, Vec2::new(dx, 0.0)));
        }
        x += it.b.width();
    }
    let mut x = items[ki].b.x0;
    for it in items[..ki].iter().rev() {
        x -= g;
        let dx = (x - it.b.width()) - it.b.x0;
        if dx.abs() > 1e-9 {
            out.push((it.id, Vec2::new(dx, 0.0)));
        }
        x -= it.b.width();
    }
    out
}

fn dist_space_v(
    items: &[Item],
    to: AlignTo,
    key: Option<ObjectId>,
    frame: Rect,
    spacing: Option<f64>,
) -> Vec<(ObjectId, Vec2)> {
    let mut items = items.to_vec();
    items.sort_by(|a, b| a.b.y0.partial_cmp(&b.b.y0).unwrap());
    let n = items.len();
    if n < 2 {
        return Vec::new();
    }
    if let Some(g) = spacing {
        if let Some(kid) = key.filter(|_| to == AlignTo::KeyObject) {
            return space_from_key_v(&items, kid, g);
        }
        let mut y = match to {
            AlignTo::Artboard => frame.y0,
            _ => items[0].b.y0,
        };
        let mut out = Vec::new();
        for it in &items {
            let dy = y - it.b.y0;
            if dy.abs() > 1e-9 {
                out.push((it.id, Vec2::new(0.0, dy)));
            }
            y += it.b.height() + g;
        }
        return out;
    }
    if n < 3 && to != AlignTo::Artboard {
        return Vec::new();
    }
    let first_y0 = match to {
        AlignTo::Artboard => frame.y0,
        _ => items[0].b.y0,
    };
    let last_y1 = match to {
        AlignTo::Artboard => frame.y1,
        _ => items[n - 1].b.y1,
    };
    let sum_h: f64 = items.iter().map(|i| i.b.height()).sum();
    let gap = (last_y1 - first_y0 - sum_h) / (n - 1) as f64;
    let mut y = first_y0;
    let mut out = Vec::new();
    for it in &items {
        let dy = y - it.b.y0;
        if dy.abs() > 1e-9 {
            out.push((it.id, Vec2::new(0.0, dy)));
        }
        y += it.b.height() + gap;
    }
    out
}

fn space_from_key_v(items: &[Item], key: ObjectId, g: f64) -> Vec<(ObjectId, Vec2)> {
    let ki = items.iter().position(|i| i.id == key).unwrap_or(0);
    let mut out = Vec::new();
    let mut y = items[ki].b.y1;
    for it in items.iter().skip(ki + 1) {
        y += g;
        let dy = y - it.b.y0;
        if dy.abs() > 1e-9 {
            out.push((it.id, Vec2::new(0.0, dy)));
        }
        y += it.b.height();
    }
    let mut y = items[ki].b.y0;
    for it in items[..ki].iter().rev() {
        y -= g;
        let dy = (y - it.b.height()) - it.b.y0;
        if dy.abs() > 1e-9 {
            out.push((it.id, Vec2::new(0.0, dy)));
        }
        y -= it.b.height();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::ObjectId;

    #[test]
    fn align_left_to_selection_uses_leftmost_edge() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let bounds = [
            (a, Rect::new(10.0, 0.0, 20.0, 10.0)),
            (b, Rect::new(40.0, 0.0, 60.0, 10.0)),
        ];
        let d = deltas(
            &bounds,
            AlignKind::HLeft,
            AlignTo::Selection,
            None,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            None,
        );
        assert!(d.iter().all(|(id, v)| (*id == b && (v.x + 30.0).abs() < 1e-9) || (*id == a && v.x.abs() < 1e-9) || (*id == a)));
        let db = d.iter().find(|(id, _)| *id == b).unwrap().1;
        assert!((db.x + 30.0).abs() < 1e-9);
        assert!(d.iter().all(|(id, v)| *id != a || v.x.abs() < 1e-9));
    }

    #[test]
    fn align_left_to_key_keeps_the_key_put() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let bounds = [
            (a, Rect::new(10.0, 0.0, 20.0, 10.0)),
            (b, Rect::new(40.0, 0.0, 60.0, 10.0)),
        ];
        let d = deltas(
            &bounds,
            AlignKind::HLeft,
            AlignTo::KeyObject,
            Some(b),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            None,
        );
        assert!(d.iter().all(|(id, _)| *id != b));
        let da = d.iter().find(|(id, _)| *id == a).unwrap().1;
        assert!((da.x - 30.0).abs() < 1e-9);
    }

    #[test]
    fn align_center_to_artboard() {
        let a = ObjectId::new();
        let bounds = [(a, Rect::new(0.0, 0.0, 10.0, 10.0))];
        let d = deltas(
            &bounds,
            AlignKind::HCenter,
            AlignTo::Artboard,
            None,
            Rect::new(0.0, 0.0, 100.0, 50.0),
            None,
        );
        let da = d.iter().find(|(id, _)| *id == a).unwrap().1;
        assert!((da.x - 45.0).abs() < 1e-9);
    }

    #[test]
    fn distribute_centers_pins_ends() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let c = ObjectId::new();
        let bounds = [
            (a, Rect::new(0.0, 0.0, 10.0, 10.0)),
            (b, Rect::new(10.0, 0.0, 20.0, 10.0)),
            (c, Rect::new(90.0, 0.0, 100.0, 10.0)),
        ];
        let d = deltas(
            &bounds,
            AlignKind::DistHCenter,
            AlignTo::Selection,
            None,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            None,
        );
        let db = d.iter().find(|(id, _)| *id == b).unwrap().1;
        // centers at 5, 15, 95 → middle should go to 50.
        assert!((db.x - 35.0).abs() < 1e-9);
        assert!(d.iter().all(|(id, _)| *id == b));
    }

    #[test]
    fn distribute_space_from_key_keeps_the_key_put() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let c = ObjectId::new();
        let bounds = [
            (a, Rect::new(0.0, 0.0, 10.0, 10.0)),
            (b, Rect::new(20.0, 0.0, 30.0, 10.0)),
            (c, Rect::new(50.0, 0.0, 60.0, 10.0)),
        ];
        let d = deltas(
            &bounds,
            AlignKind::DistHSpace,
            AlignTo::KeyObject,
            Some(b),
            Rect::new(0.0, 0.0, 100.0, 100.0),
            Some(5.0),
        );
        assert!(d.iter().all(|(id, _)| *id != b));
        let da = d.iter().find(|(id, _)| *id == a).unwrap().1;
        let dc = d.iter().find(|(id, _)| *id == c).unwrap().1;
        // key stays at 20..30; left gap 5 → a at 5..15 (dx +5); right gap 5 → c at 35..45 (dx -15).
        assert!((da.x - 5.0).abs() < 1e-9);
        assert!((dc.x + 15.0).abs() < 1e-9);
    }

    #[test]
    fn distribute_space_auto_pins_first_and_last() {
        let a = ObjectId::new();
        let b = ObjectId::new();
        let c = ObjectId::new();
        let bounds = [
            (a, Rect::new(0.0, 0.0, 10.0, 10.0)),
            (b, Rect::new(10.0, 0.0, 20.0, 10.0)),
            (c, Rect::new(90.0, 0.0, 100.0, 10.0)),
        ];
        let d = deltas(
            &bounds,
            AlignKind::DistHSpace,
            AlignTo::Selection,
            None,
            Rect::new(0.0, 0.0, 100.0, 100.0),
            None,
        );
        let db = d.iter().find(|(id, _)| *id == b).unwrap().1;
        // widths 10+10+10, span 100, two gaps of 35 → b at 45 (dx +35).
        assert!((db.x - 35.0).abs() < 1e-9);
        assert!(d.iter().all(|(id, _)| *id == b));
    }
}
