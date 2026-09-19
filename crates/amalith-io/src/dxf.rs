//! Best-effort DXF import: reads the `ENTITIES` section of an ASCII DXF
//! file (the near-universal interchange format for laser/CNC cutters —
//! LightBurn, RDWorks, Epilog, Trotec, and every CAD package all read and
//! write it) and maps its 2D drawing entities onto Amalith objects.
//!
//! Unlike `ai.rs`/`eps.rs`, there's no imperative graphics-state machine
//! to interpret here — DXF is a flat, declarative list of entities (each
//! just a run of group-code/value pairs), closer in shape to `svg.rs`'s
//! "iterate elements, map each to a `PathData`" than to a stack-based
//! interpreter. `LINE`, `CIRCLE`, `ARC`, `LWPOLYLINE` (bulge-arc segments
//! included), and `ELLIPSE` cover the vast majority of real 2D
//! cut/engrave plans; anything else (`SPLINE`, `HATCH`, `TEXT`/`MTEXT`,
//! `INSERT` block references, the older `POLYLINE`/`VERTEX`/`SEQEND`
//! form) is silently skipped rather than a hard error, matching every
//! other importer in this crate.
//!
//! Binary DXF (`AutoCAD binary DXF`, a distinct on-disk encoding of the
//! same group-code model) isn't handled — virtually every real-world
//! export from laser/CNC software is ASCII DXF, binary DXF being rare
//! outside AutoCAD-native workflows.

use amalith_core::{
    Affine, Appearance, AppearanceItem, Artboard, ArtboardId, BlendMode, Color, Document, Layer, LayerId,
    Object, ObjectId, ObjectKind, ObjectParent, Paint, PathData, Point, Rect, StrokeStyle,
};
use kurbo::{BezPath, PathEl};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum DxfError {
    #[error("no ENTITIES section found")]
    NoEntities,
    #[error("no drawable content recovered")]
    Empty,
}

/// Parses `bytes` (an `.dxf` file's contents) and builds a fresh
/// [`Document`]: one Amalith layer per distinct DXF layer name
/// encountered (DXF's own layer model maps directly onto Amalith's
/// `Document -> Layer -> Object` tree), sized to the recovered
/// geometry's own bounds (DXF carries no page/artboard concept — a
/// drawing is just entities in modelspace).
pub fn import_dxf(bytes: &[u8]) -> Result<Document, DxfError> {
    let text = String::from_utf8_lossy(bytes);
    let pairs = parse_pairs(&text);
    let section = entities_section(&pairs).ok_or(DxfError::NoEntities)?;
    let entities = split_entities(section);

    // DXF's y-axis points up, same as PostScript/PDF; Amalith (like SVG)
    // has y increasing downward. The final flip-and-place happens once
    // bounds are known, below — entities are recovered in raw DXF space
    // first.
    let mut recovered: Vec<(String, PathData, Option<[f32; 3]>)> = Vec::new();
    for (kind, e) in &entities {
        let layer = get_str(e, 8).unwrap_or("0").to_string();
        let color = aci_rgb(get_i64(e, 62)).or_else(|| get_i64(e, 420).map(truecolor_rgb));
        let Some(bez) = build_entity(kind, e) else { continue };
        recovered.push((layer, PathData::from_bezpath(bez), color));
    }
    if recovered.is_empty() {
        return Err(DxfError::Empty);
    }

    let bounds = recovered
        .iter()
        .map(|(_, p, _)| p.local_bounds())
        .reduce(|a, b| a.union(b))
        .unwrap_or(Rect::new(0.0, 0.0, 100.0, 100.0));
    // Flip y and shift so the recovered content's own top-left lands at
    // the origin — there's no DXF-native page size to target instead.
    let base = Affine::new([1.0, 0.0, 0.0, -1.0, -bounds.x0, bounds.y1]);

    let mut document = Document::new("Imported");
    let artboard_rect = Rect::new(0.0, 0.0, bounds.width().max(1.0), bounds.height().max(1.0));
    document.insert_artboard(Artboard::new(ArtboardId::new(), "Artboard 1", artboard_rect), 0);

    let mut layer_ids: HashMap<String, LayerId> = HashMap::new();
    let mut next_index: HashMap<LayerId, usize> = HashMap::new();
    for (layer_name, path, color) in recovered {
        let layer_id = *layer_ids.entry(layer_name.clone()).or_insert_with(|| {
            let id = LayerId::new();
            document.insert_layer(Layer::new(id, layer_name), document.layers().len());
            id
        });
        let idx = next_index.entry(layer_id).or_insert(0);
        // Baking `base` into the geometry itself (rather than setting it
        // as `Object::transform`) keeps every layer's content in the one
        // shared document space `base` already targets, no per-object
        // transform bookkeeping needed.
        let transformed = PathData::from_bezpath(base * path.geometry.clone());
        let obj = Object {
            appearance: dxf_appearance(color),
            ..Object::new(ObjectId::new(), ObjectParent::Layer(layer_id), amalith_core::ObjectKind::Path(transformed))
        };
        let _ = document.insert_object(obj, *idx);
        *idx += 1;
    }
    Ok(document)
}

/// DXF entities have no fill concept at all (barring the `HATCH` entity,
/// not recovered here) — every one is a stroked line/curve. `color`, if
/// recovered from group code 62 (ACI) or 420 (true color), becomes the
/// stroke color; `None` (no color override, i.e. `BYLAYER`) defaults to
/// black, matching how a plain, colorless cut plan reads on white paper.
fn dxf_appearance(color: Option<[f32; 3]>) -> Appearance {
    let [r, g, b] = color.unwrap_or([0.0, 0.0, 0.0]);
    Appearance {
        items: vec![AppearanceItem::Stroke {
            paint: Paint::Solid(Color::rgb(r, g, b)),
            width: 1.0,
            style: StrokeStyle::default(),
            opacity: 1.0,
            visible: true,
            effects: Vec::new(),
            blend_mode: BlendMode::Normal,
        }],
        opacity: 1.0,
    }
}

fn build_entity(kind: &str, e: &[(i32, String)]) -> Option<BezPath> {
    match kind {
        "LINE" => {
            let p0 = Point::new(get_f64(e, 10)?, get_f64(e, 20)?);
            let p1 = Point::new(get_f64(e, 11)?, get_f64(e, 21)?);
            let mut bez = BezPath::new();
            bez.move_to(p0);
            bez.line_to(p1);
            Some(bez)
        }
        "CIRCLE" => {
            let c = Point::new(get_f64(e, 10)?, get_f64(e, 20)?);
            let r = get_f64(e, 40)?;
            Some(polyline(&arc_points(c, r, 0.0, 360.0)))
        }
        "ARC" => {
            let c = Point::new(get_f64(e, 10)?, get_f64(e, 20)?);
            let r = get_f64(e, 40)?;
            let a0 = get_f64(e, 50).unwrap_or(0.0);
            let a1 = get_f64(e, 51).unwrap_or(360.0);
            let mut bez = BezPath::new();
            let pts = arc_points(c, r, a0, a1);
            bez.move_to(pts[0]);
            for p in &pts[1..] {
                bez.line_to(*p);
            }
            Some(bez)
        }
        "LWPOLYLINE" => {
            let verts = lwpolyline_vertices(e);
            if verts.len() < 2 {
                return None;
            }
            let closed = get_i64(e, 70).is_some_and(|f| f & 1 != 0);
            Some(polyline_with_bulges(&verts, closed))
        }
        "ELLIPSE" => {
            let c = Point::new(get_f64(e, 10)?, get_f64(e, 20)?);
            let major = Point::new(get_f64(e, 11)?, get_f64(e, 21)?);
            let ratio = get_f64(e, 40).unwrap_or(1.0);
            let t0 = get_f64(e, 41).unwrap_or(0.0);
            let t1 = get_f64(e, 42).unwrap_or(std::f64::consts::TAU);
            Some(ellipse_points(c, major, ratio, t0, t1))
        }
        // Best-effort: SPLINE (NURBS evaluation), HATCH (boundary-fill
        // definitions), TEXT/MTEXT (no text recovery, matching
        // `ai.rs`/`svg.rs`), INSERT (block/symbol references), POINT (no
        // stroke/fill equivalent), and the older POLYLINE/VERTEX/SEQEND
        // triple (a stateful multi-entity run, unlike every other kind
        // here) are all silently skipped.
        _ => None,
    }
}

fn polyline(pts: &[Point]) -> BezPath {
    let mut bez = BezPath::new();
    bez.move_to(pts[0]);
    for p in &pts[1..] {
        bez.line_to(*p);
    }
    bez
}

/// Samples `n` points per full turn along the arc from `start_deg` to
/// `end_deg` (DXF's own convention: always sweeping counter-clockwise
/// from start to end) — a polyline approximation, not true bezier arcs,
/// matching `eps.rs`'s same simplification for its own (unimplemented)
/// arc operator.
fn arc_points(center: Point, r: f64, start_deg: f64, end_deg: f64) -> Vec<Point> {
    let a0 = start_deg.to_radians();
    let mut a1 = end_deg.to_radians();
    if a1 <= a0 {
        a1 += std::f64::consts::TAU;
    }
    let span = a1 - a0;
    let n = ((span.abs() / std::f64::consts::TAU) * 128.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = a0 + span * (i as f64 / n as f64);
            Point::new(center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect()
}

fn ellipse_points(center: Point, major: Point, ratio: f64, t0: f64, t1: f64) -> BezPath {
    let minor = Point::new(-major.y * ratio, major.x * ratio);
    let mut t1 = t1;
    if t1 <= t0 {
        t1 += std::f64::consts::TAU;
    }
    let span = t1 - t0;
    let n = ((span.abs() / std::f64::consts::TAU) * 128.0).ceil().max(2.0) as usize;
    let pt = |t: f64| Point::new(
        center.x + major.x * t.cos() + minor.x * t.sin(),
        center.y + major.y * t.cos() + minor.y * t.sin(),
    );
    let pts: Vec<Point> = (0..=n).map(|i| pt(t0 + span * (i as f64 / n as f64))).collect();
    let mut bez = polyline(&pts);
    if (span - std::f64::consts::TAU).abs() < 1e-6 {
        bez.close_path();
    }
    bez
}

/// `(point, bulge)` per vertex, in encounter order — group codes 10/20
/// start a new vertex, 42 (if present before the next 10) is that
/// vertex's bulge (the arc-to-the-*next*-vertex indicator; see
/// `bulge_arc_points`).
fn lwpolyline_vertices(e: &[(i32, String)]) -> Vec<(Point, f64)> {
    let mut verts = Vec::new();
    let mut cur: Option<(f64, f64, f64)> = None;
    for (code, val) in e {
        match *code {
            10 => {
                if let Some((x, y, b)) = cur.take() {
                    verts.push((Point::new(x, y), b));
                }
                cur = Some((val.trim().parse().unwrap_or(0.0), 0.0, 0.0));
            }
            20 => {
                if let Some(c) = &mut cur {
                    c.1 = val.trim().parse().unwrap_or(0.0);
                }
            }
            42 => {
                if let Some(c) = &mut cur {
                    c.2 = val.trim().parse().unwrap_or(0.0);
                }
            }
            _ => {}
        }
    }
    if let Some((x, y, b)) = cur {
        verts.push((Point::new(x, y), b));
    }
    verts
}

fn polyline_with_bulges(verts: &[(Point, f64)], closed: bool) -> BezPath {
    let mut bez = BezPath::new();
    bez.move_to(verts[0].0);
    for w in verts.windows(2) {
        let ((p0, bulge), (p1, _)) = (w[0], w[1]);
        for p in bulge_arc_points(p0, p1, bulge) {
            bez.line_to(p);
        }
    }
    if closed {
        let (last, bulge) = *verts.last().unwrap();
        for p in bulge_arc_points(last, verts[0].0, bulge) {
            bez.line_to(p);
        }
        bez.close_path();
    }
    bez
}

/// DXF's "bulge" is `tan(included_angle / 4)`, signed (positive = the
/// arc from `p0` to `p1` sweeps counter-clockwise) — equivalently,
/// exactly `2 * sagitta / chord_length` per the DXF spec's own
/// definition, which is what this uses directly rather than going
/// through the angle. Returns sampled points *excluding* `p0` (the
/// caller already has it) — a straight `[p1]` when `bulge` is ~0.
fn bulge_arc_points(p0: Point, p1: Point, bulge: f64) -> Vec<Point> {
    if bulge.abs() < 1e-9 {
        return vec![p1];
    }
    let chord = p1 - p0;
    let chord_len = chord.hypot();
    if chord_len < 1e-9 {
        return vec![p1];
    }
    let sagitta = bulge * chord_len / 2.0;
    let r = (chord_len * chord_len / 4.0 + sagitta * sagitta) / (2.0 * sagitta.abs());
    let mid = p0.midpoint(p1);
    // Rotate the chord direction 90° CCW to get the direction from the
    // midpoint toward the center, for a positive (CCW, per the DXF
    // spec's own "negative = clockwise from start to end" definition)
    // bulge. Note this is about rotational sense from `p0` to `p1`, not
    // "which side the arc visually bulges to" — for the exact-semicircle
    // case (bulge = ±1) those aren't the same intuition: going strictly
    // counter-clockwise from the west point of a circle to its east
    // point passes through the *south* point, not the north one. Pinned
    // by this module's `bulge_one_is_an_exact_semicircle` test, and
    // cross-checked against a non-degenerate quarter-circle case where
    // the correct center is unambiguous by inspection.
    let (dx, dy) = (chord.x / chord_len, chord.y / chord_len);
    let perp = Point::new(-dy, dx);
    let offset = (r - sagitta.abs()) * bulge.signum();
    let center = Point::new(mid.x + perp.x * offset, mid.y + perp.y * offset);

    let a0 = (p0.y - center.y).atan2(p0.x - center.x).to_degrees();
    let mut a1 = (p1.y - center.y).atan2(p1.x - center.x).to_degrees();
    if bulge > 0.0 {
        // CCW: sweep upward in angle from a0 to a1.
        if a1 <= a0 {
            a1 += 360.0;
        }
        arc_points(center, r, a0, a1)
    } else {
        // CW: sweep downward — `arc_points` only sweeps CCW, so ask it
        // for the CCW arc from `a1` to `a0` instead and reverse it.
        let mut a0r = a0;
        if a0r <= a1 {
            a0r += 360.0;
        }
        let mut pts = arc_points(center, r, a1, a0r);
        pts.reverse();
        pts
    }
    .into_iter()
    .skip(1) // caller already has p0
    .collect()
}

/// A short, hand-picked slice of the 256-entry AutoCAD Color Index (ACI)
/// palette — the handful of indices real-world files overwhelmingly
/// actually use (the 7 basic hues, plus black/white at the two common
/// "default" slots). An unlisted index, `BYLAYER`(256)/`BYBLOCK`(0), or
/// no color at all falls back to `None` (→ plain black in
/// `dxf_appearance`) rather than the full palette table.
fn aci_rgb(index: Option<i64>) -> Option<[f32; 3]> {
    match index? {
        1 => Some([1.0, 0.0, 0.0]),       // red
        2 => Some([1.0, 1.0, 0.0]),       // yellow
        3 => Some([0.0, 1.0, 0.0]),       // green
        4 => Some([0.0, 1.0, 1.0]),       // cyan
        5 => Some([0.0, 0.0, 1.0]),       // blue
        6 => Some([1.0, 0.0, 1.0]),       // magenta
        7 => Some([0.0, 0.0, 0.0]),       // black/white default (index 7 is context-dependent; treated as black on our always-white canvas)
        _ => None,
    }
}

/// Group code 420: a packed 24-bit `0x00RRGGBB` true-color value —
/// present in files from newer CAD/CAM tools instead of (or alongside)
/// the legacy ACI index.
fn truecolor_rgb(packed: i64) -> [f32; 3] {
    let packed = packed as u32;
    [
        ((packed >> 16) & 0xFF) as f32 / 255.0,
        ((packed >> 8) & 0xFF) as f32 / 255.0,
        (packed & 0xFF) as f32 / 255.0,
    ]
}

/// Splits `text` into `(group code, value)` pairs — two lines each,
/// exactly DXF's on-disk shape. A malformed/non-numeric code line (rare
/// outside a corrupt file) just drops that one pair rather than failing
/// the whole parse.
fn parse_pairs(text: &str) -> Vec<(i32, String)> {
    let mut lines = text.lines();
    let mut out = Vec::new();
    loop {
        let Some(code_line) = lines.next() else { break };
        let Some(value_line) = lines.next() else { break };
        if let Ok(code) = code_line.trim().parse::<i32>() {
            out.push((code, value_line.trim_end_matches('\r').to_string()));
        }
    }
    out
}

/// The `(0 SECTION)(2 ENTITIES) ... (0 ENDSEC)` slice, exclusive of its
/// own delimiters.
fn entities_section(pairs: &[(i32, String)]) -> Option<&[(i32, String)]> {
    let mut i = 0;
    while i + 1 < pairs.len() {
        if pairs[i].0 == 0 && pairs[i].1 == "SECTION" && pairs[i + 1].0 == 2 && pairs[i + 1].1 == "ENTITIES" {
            let start = i + 2;
            let end = pairs[start..].iter().position(|(c, v)| *c == 0 && v == "ENDSEC").map_or(pairs.len(), |p| start + p);
            return Some(&pairs[start..end]);
        }
        i += 1;
    }
    None
}

/// Splits an entities section into `(kind, its own pairs)` runs, each
/// starting right after a `(0, kind)` marker and ending right before the
/// next one.
fn split_entities(section: &[(i32, String)]) -> Vec<(&str, &[(i32, String)])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < section.len() {
        if section[i].0 == 0 {
            let kind = section[i].1.as_str();
            let start = i + 1;
            let mut j = start;
            while j < section.len() && section[j].0 != 0 {
                j += 1;
            }
            out.push((kind, &section[start..j]));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn get_str<'a>(e: &'a [(i32, String)], code: i32) -> Option<&'a str> {
    e.iter().find(|(c, _)| *c == code).map(|(_, v)| v.trim())
}
fn get_f64(e: &[(i32, String)], code: i32) -> Option<f64> {
    get_str(e, code)?.parse().ok()
}
fn get_i64(e: &[(i32, String)], code: i32) -> Option<i64> {
    get_str(e, code)?.parse().ok()
}

/// Writes `ids` (and, for a group, its full descendant tree) as a
/// minimal but valid ASCII DXF: just `0 SECTION / 2 ENTITIES / ... /
/// 0 ENDSEC / 0 EOF`, no `HEADER`/`TABLES`/`BLOCKS` — every laser/CNC
/// tool this format targets reads that shape fine, and it sidesteps
/// needing a real DXF layer-table writer. Every path becomes one
/// `LWPOLYLINE` per subpath (curves flattened to line segments — DXF's
/// arc/bulge encoding is for *circular* arcs specifically, not a general
/// bezier, so round-tripping an arbitrary curve back into one isn't
/// possible; a dense polyline is the honest equivalent). `None` if none
/// of `ids` resolve to contributing geometry.
///
/// Every recovered layer collapses onto DXF layer `"0"` — the `ids`
/// list this receives (built by the caller from every visible layer's
/// children already flattened together) has no per-layer boundary left
/// to preserve by the time it gets here; unlike `import_dxf`'s
/// layer-per-name recovery, this isn't a two-way mapping.
pub fn export_dxf(document: &Document, ids: &[ObjectId]) -> Option<String> {
    let shapes = collect_shapes(document, ids);
    if shapes.is_empty() {
        return None;
    }
    let bounds = shapes
        .iter()
        .flat_map(|(polys, _)| polys.iter().flat_map(|(pts, _)| pts.iter().copied()))
        .fold(None::<Rect>, |acc, p| {
            let r = Rect::new(p.x, p.y, p.x, p.y);
            Some(acc.map_or(r, |a| a.union(r)))
        })?;

    let mut out = String::from("0\nSECTION\n2\nENTITIES\n");
    for (polylines, color) in &shapes {
        let aci = color.map(nearest_aci).unwrap_or(7);
        for (pts, closed) in polylines {
            if pts.len() < 2 {
                continue;
            }
            out.push_str(&format!("0\nLWPOLYLINE\n8\n0\n62\n{aci}\n90\n{}\n70\n{}\n", pts.len(), if *closed { 1 } else { 0 }));
            for p in pts {
                // Flip y (Amalith is y-down; DXF is y-up) and shift so
                // the content's own bounds start at (0, 0).
                out.push_str(&format!("10\n{}\n20\n{}\n", p.x - bounds.x0, bounds.y1 - p.y));
            }
        }
    }
    out.push_str("0\nENDSEC\n0\nEOF\n");
    Some(out)
}

/// Every `Path`/`CompoundPath` under `ids`, in world space, flattened to
/// polylines (`(points, closed)` per subpath) paired with a
/// representative color (stroke if any, else fill) — DXF entities have
/// no fill concept, so only the *presence* of a color matters, not
/// which appearance item it came from.
fn collect_shapes(document: &Document, ids: &[ObjectId]) -> Vec<(Vec<(Vec<Point>, bool)>, Option<Color>)> {
    let mut out = Vec::new();
    for &id in ids {
        collect_one(document, id, &mut out);
    }
    out
}

fn collect_one(document: &Document, id: ObjectId, out: &mut Vec<(Vec<(Vec<Point>, bool)>, Option<Color>)>) {
    let Some(object) = document.object(id) else { return };
    let color = |a: &Appearance| a.stroke().color().or_else(|| a.fill().color());
    match &object.kind {
        ObjectKind::Group(g) => {
            for &child in &g.children {
                collect_one(document, child, out);
            }
        }
        ObjectKind::Path(p) => {
            let bez = document.world_transform(id) * p.geometry.clone();
            out.push((flatten_polylines(&bez), color(&object.appearance)));
        }
        ObjectKind::CompoundPath(cp) => {
            let xf = document.world_transform(id);
            let polys = cp.subpaths.iter().flat_map(|s| flatten_polylines(&(xf * s.clone()))).collect();
            out.push((polys, color(&object.appearance)));
        }
        ObjectKind::Text(_) | ObjectKind::Image(_) | ObjectKind::Symbol(_) | ObjectKind::Unknown { .. } => {}
    }
}

/// Flattens `bez` (curves included) to line segments at a tolerance
/// tight enough that the polyline reads as smooth at normal cut-plan
/// zoom levels, returning `(points, closed)` per subpath.
fn flatten_polylines(bez: &BezPath) -> Vec<(Vec<Point>, bool)> {
    let mut polylines = Vec::new();
    let mut cur: Vec<Point> = Vec::new();
    let mut closed = false;
    kurbo::flatten(bez, 0.1, |el| match el {
        PathEl::MoveTo(p) => {
            if !cur.is_empty() {
                polylines.push((std::mem::take(&mut cur), closed));
            }
            closed = false;
            cur.push(p);
        }
        PathEl::LineTo(p) => cur.push(p),
        PathEl::ClosePath => closed = true,
        _ => {}
    });
    if !cur.is_empty() {
        polylines.push((cur, closed));
    }
    polylines
}

/// The closest of `aci_rgb`'s handful of entries to `c`, by plain
/// squared RGB distance — good enough for "roughly the right color",
/// which is all a 7-entry reverse lookup can honestly promise.
fn nearest_aci(c: Color) -> i64 {
    [1, 2, 3, 4, 5, 6]
        .into_iter()
        .min_by(|&a, &b| {
            let dist = |idx: i64| {
                let [r, g, bl] = aci_rgb(Some(idx)).unwrap();
                (r - c.r).powi(2) + (g - c.g).powi(2) + (bl - c.b).powi(2)
            };
            dist(a).total_cmp(&dist(b))
        })
        .unwrap_or(7)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dxf(entities: &str) -> String {
        format!("0\nSECTION\n2\nENTITIES\n{entities}0\nENDSEC\n0\nEOF\n")
    }

    #[test]
    fn recovers_a_line() {
        let src = dxf("0\nLINE\n8\n0\n10\n0.0\n20\n0.0\n11\n10.0\n21\n0.0\n");
        let doc = import_dxf(src.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn recovers_a_circle_as_a_closed_polyline_approximation() {
        let src = dxf("0\nCIRCLE\n8\n0\n10\n5.0\n20\n5.0\n40\n5.0\n");
        let doc = import_dxf(src.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn distinct_dxf_layers_become_distinct_amalith_layers() {
        let src = dxf(
            "0\nLINE\n8\nCUT\n10\n0\n20\n0\n11\n10\n21\n0\n\
             0\nLINE\n8\nENGRAVE\n10\n0\n20\n0\n11\n0\n21\n10\n",
        );
        let doc = import_dxf(src.as_bytes()).unwrap();
        assert_eq!(doc.layers().len(), 2);
        let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"CUT") && names.contains(&"ENGRAVE"));
    }

    #[test]
    fn aci_red_recovers_as_a_red_stroke() {
        let src = dxf("0\nLINE\n8\n0\n62\n1\n10\n0\n20\n0\n11\n10\n21\n0\n");
        let doc = import_dxf(src.as_bytes()).unwrap();
        let obj = doc.objects().next().unwrap();
        match obj.appearance.items[0] {
            AppearanceItem::Stroke { paint: Paint::Solid(c), .. } => assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0)),
            _ => panic!("expected a Stroke item"),
        }
    }

    #[test]
    fn positive_bulge_quarter_circle_matches_hand_computed_geometry() {
        // Unambiguous by inspection, unlike the exact-semicircle case
        // below: a positive (CCW) bulge from (1,0) to (0,1) with included
        // angle 90° is exactly the standard upper-right unit-circle
        // quarter-arc centered at the origin, passing through
        // (cos45°, sin45°).
        let bulge = (std::f64::consts::FRAC_PI_2 / 4.0).tan(); // tan(22.5°)
        let pts = bulge_arc_points(Point::new(1.0, 0.0), Point::new(0.0, 1.0), bulge);
        let k = std::f64::consts::FRAC_1_SQRT_2;
        let target = Point::new(k, k);
        // The closest sample, not a fixed index — `arc_points`' segment
        // count is a `ceil()` over a float ratio, so exactly which index
        // lands nearest the arc's true midpoint can shift by one sample
        // with floating-point noise; that's fine; the arc still has to
        // actually pass close to (k, k).
        let closest = pts.iter().copied().min_by(|a, b| a.distance(target).total_cmp(&b.distance(target))).unwrap();
        assert!(closest.distance(target) < 0.05, "expected a sample near ({k}, {k}), closest was {closest:?}");
    }

    #[test]
    fn bulge_one_is_an_exact_semicircle() {
        // bulge = 1.0 ⇒ included angle 180°: the arc from (0,0) to (2,0)
        // is a semicircle centered at (1,0), radius 1. Which half (upper
        // or lower) is *not* the intuitive "bulges toward positive y" —
        // per the DXF spec, positive bulge means strictly counter-
        // clockwise from start to end, and going CCW from the west point
        // of a circle to its east point passes through the *south*
        // point (standard angle order is E(0°)→N(90°)→W(180°)→S(270°)→
        // E(360°); starting at W and continuing that same order reaches
        // S next, not N) — so this is the *lower* half, confirmed
        // correct against the unambiguous quarter-circle case above.
        let pts = bulge_arc_points(Point::new(0.0, 0.0), Point::new(2.0, 0.0), 1.0);
        let mid = &pts[pts.len() / 2 - 1];
        assert!((mid.x - 1.0).abs() < 0.05, "midpoint x should be ~1.0, got {}", mid.x);
        assert!((mid.y + 1.0).abs() < 0.05, "midpoint y should be ~-1.0 (the lower half), got {}", mid.y);
        let last = pts.last().unwrap();
        assert!((last.x - 2.0).abs() < 1e-6 && last.y.abs() < 1e-6);
    }

    #[test]
    fn negative_bulge_arcs_the_other_way() {
        // The mirror image of `bulge_one_is_an_exact_semicircle`: CW
        // instead of CCW, so the *upper* half this time.
        let pts = bulge_arc_points(Point::new(0.0, 0.0), Point::new(2.0, 0.0), -1.0);
        let mid = &pts[pts.len() / 2 - 1];
        assert!((mid.x - 1.0).abs() < 0.05);
        assert!((mid.y - 1.0).abs() < 0.05, "midpoint y should be ~1.0 (the upper half), got {}", mid.y);
    }

    #[test]
    fn lwpolyline_with_a_bulge_segment_recovers() {
        let src = dxf(
            "0\nLWPOLYLINE\n8\n0\n70\n0\n\
             10\n0\n20\n0\n42\n1.0\n\
             10\n2\n20\n0\n",
        );
        let doc = import_dxf(src.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn no_entities_section_is_an_error() {
        assert_eq!(import_dxf(b"0\nSECTION\n2\nHEADER\n0\nENDSEC\n0\nEOF\n").unwrap_err(), DxfError::NoEntities);
    }

    #[test]
    fn an_empty_entities_section_is_an_error() {
        assert_eq!(import_dxf(dxf("").as_bytes()).unwrap_err(), DxfError::Empty);
    }

    #[test]
    fn export_then_reimport_round_trips_a_red_triangle() {
        let mut doc = Document::new("Test");
        let layer_id = LayerId::new();
        doc.insert_layer(Layer::new(layer_id, "Layer 1"), 0);
        let id = ObjectId::new();
        let mut bez = BezPath::new();
        bez.move_to((0.0, 0.0));
        bez.line_to((40.0, 0.0));
        bez.line_to((20.0, 30.0));
        bez.close_path();
        let mut obj = Object::new(id, ObjectParent::Layer(layer_id), ObjectKind::Path(PathData::from_bezpath(bez)));
        obj.appearance = Appearance {
            items: vec![AppearanceItem::Stroke {
                paint: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)),
                width: 1.0,
                style: StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                effects: Vec::new(),
                blend_mode: BlendMode::Normal,
            }],
            opacity: 1.0,
        };
        doc.insert_object(obj, 0).unwrap();

        let dxf_text = export_dxf(&doc, &[id]).expect("export produced content");
        let reimported = import_dxf(dxf_text.as_bytes()).expect("reimport succeeded");
        let obj = reimported.objects().next().unwrap();
        match obj.appearance.items[0] {
            AppearanceItem::Stroke { paint: Paint::Solid(c), .. } => assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0)),
            _ => panic!("expected a red Stroke item"),
        }
        let ObjectKind::Path(p) = &obj.kind else { panic!("expected a Path") };
        let b = p.local_bounds();
        assert!((b.width() - 40.0).abs() < 0.5 && (b.height() - 30.0).abs() < 0.5, "got {b:?}");
    }
}
