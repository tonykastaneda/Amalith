//! Best-effort HPGL (`.plt`) import — the plotter command language still
//! used by some vinyl cutters and older/cheaper laser cutters alongside
//! DXF/SVG. Structurally much simpler than `eps.rs`'s PostScript: no
//! stack machine, no graphics-state save/restore, no arbitrary
//! procedures — just a flat sequence of short two-letter commands
//! (`PU`/`PD`/`PA`/`PR`/`SP`/`CI`), each taking a comma-separated list of
//! numeric arguments and terminated by `;`.
//!
//! Covers the common cut/engrave subset: pen-up/pen-down moves in
//! absolute (`PA`) or relative (`PR`) coordinates, and `CI` circles.
//! `SP n` (select pen `n`) becomes its own Amalith layer per distinct
//! pen number — real laser-cutting HPGL files commonly use different
//! pens the same way a DXF file uses different layers (cut vs. engrave)
//! — rather than a guessed color, since pen-to-color mapping is a
//! physical convention that varies by plotter/vendor with no reliable
//! standard to decode. Scaling (`SC`), the input window (`IP`), line
//! type/dash (`LT`), velocity/force (`VS`/`FS`), and text (`LB`/`SI`/
//! `DT`) are silently skipped, matching every other importer in this
//! crate's best-effort philosophy.

use amalith_core::{
    Affine, Appearance, AppearanceItem, Artboard, ArtboardId, BlendMode, Color, Document, Layer, LayerId,
    Object, ObjectId, ObjectKind, ObjectParent, Paint, PathData, Point, Rect, StrokeStyle,
};
use kurbo::{BezPath, PathEl, Shape};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum PltError {
    #[error("no drawable content recovered")]
    Empty,
}

/// Plotter units traditionally run 40 per millimeter (1 unit = 0.025mm)
/// — the classic HP-GL convention this crate assumes, same as most
/// hobbyist cutters' own HPGL export. Converted to this app's canonical
/// 96px/inch.
const UNITS_PER_MM: f64 = 40.0;
const PX_PER_MM: f64 = 96.0 / 25.4;

/// Parses `bytes` (a `.plt` file's contents) and builds a fresh
/// [`Document`]: one Amalith layer per distinct `SP` pen number
/// (defaulting to a single "Pen 1" layer for a file that never
/// selects one), and one stroked path object per contiguous pen-down
/// run.
pub fn import_plt(bytes: &[u8]) -> Result<Document, PltError> {
    let text = String::from_utf8_lossy(bytes);
    let recovered = Interpreter::run(&text);
    if recovered.is_empty() {
        return Err(PltError::Empty);
    }

    let bounds = recovered
        .iter()
        .map(|r| r.geometry.bounding_box())
        .reduce(|a, b| a.union(b))
        .unwrap_or(Rect::new(0.0, 0.0, 100.0, 100.0));
    // HPGL's page space has its origin at the bottom-left with y
    // increasing upward, same as DXF/PostScript/PDF; Amalith has y
    // increasing downward.
    let base = Affine::new([1.0, 0.0, 0.0, -1.0, -bounds.x0, bounds.y1]);

    let mut document = Document::new("Imported");
    let artboard_rect = Rect::new(0.0, 0.0, bounds.width().max(1.0), bounds.height().max(1.0));
    document.insert_artboard(Artboard::new(ArtboardId::new(), "Artboard 1", artboard_rect), 0);

    let mut layer_ids: HashMap<i64, LayerId> = HashMap::new();
    let mut next_index: HashMap<LayerId, usize> = HashMap::new();
    for r in recovered {
        let layer_id = *layer_ids.entry(r.pen).or_insert_with(|| {
            let id = LayerId::new();
            document.insert_layer(Layer::new(id, format!("Pen {}", r.pen)), document.layers().len());
            id
        });
        let idx = next_index.entry(layer_id).or_insert(0);
        let transformed = PathData::from_bezpath(base * r.geometry);
        let obj = Object {
            appearance: stroke_appearance(),
            ..Object::new(ObjectId::new(), ObjectParent::Layer(layer_id), amalith_core::ObjectKind::Path(transformed))
        };
        let _ = document.insert_object(obj, *idx);
        *idx += 1;
    }
    Ok(document)
}

/// HPGL has no fill concept and no reliable pen-to-color convention (see
/// the module doc comment) — every recovered path is a plain black
/// stroke, distinguished by which layer (pen) it landed on.
fn stroke_appearance() -> Appearance {
    Appearance {
        items: vec![AppearanceItem::Stroke {
            paint: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
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

struct Recovered {
    geometry: BezPath,
    pen: i64,
}

struct Interpreter {
    pos: Point,
    absolute: bool,
    pen_down: bool,
    pen: i64,
    /// The current pen-down run, if any — flushed into `out` the moment
    /// the pen lifts, a different pen is selected, or input ends.
    run: Option<BezPath>,
    out: Vec<Recovered>,
}

impl Interpreter {
    fn run(text: &str) -> Vec<Recovered> {
        let mut it = Interpreter { pos: Point::ZERO, absolute: true, pen_down: false, pen: 1, run: None, out: Vec::new() };
        for stmt in text.split(';') {
            it.exec(stmt.trim());
        }
        it.flush();
        it.out
    }

    fn flush(&mut self) {
        if let Some(bez) = self.run.take() {
            self.out.push(Recovered { geometry: bez, pen: self.pen });
        }
    }

    fn exec(&mut self, stmt: &str) {
        if stmt.len() < 2 {
            return;
        }
        let (mnemonic, rest) = stmt.split_at(2);
        let args: Vec<f64> = rest
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        match mnemonic.to_ascii_uppercase().as_str() {
            "PA" => self.absolute = true,
            "PR" => self.absolute = false,
            "PU" => self.move_through(&args, false),
            "PD" => self.move_through(&args, true),
            "SP" => {
                self.flush();
                self.pen = args.first().copied().unwrap_or(1.0) as i64;
            }
            "CI" => {
                if let Some(&r_mm) = args.first() {
                    self.flush();
                    let r = r_mm / UNITS_PER_MM * PX_PER_MM;
                    let pts = circle_points(self.pos, r);
                    let mut bez = BezPath::new();
                    bez.move_to(pts[0]);
                    for p in &pts[1..] {
                        bez.line_to(*p);
                    }
                    bez.close_path();
                    self.out.push(Recovered { geometry: bez, pen: self.pen });
                }
            }
            // `IN` (initialize) resets pen-up/absolute state, matching
            // this interpreter's own defaults, so it's a no-op here.
            // Everything else — `SC`/`IP` (scaling/window), `LT`/`PW`
            // (dash/width), `VS`/`FS` (velocity/force), `LB`/`SI`/`DT`
            // (text) — is silently skipped per the module doc comment.
            _ => {}
        }
    }

    /// Converts each `(x, y)` pair in plotter units to px (absolute or
    /// relative to the running position per `self.absolute`) and moves
    /// through them in order. `pen_down` decides whether that's a plain
    /// reposition (`PU`) or draws a line (`PD`) — a transition to a
    /// *different* pen state than last time starts a fresh run (`PU`
    /// after `PD` ends the current stroke; `PD` after `PU` starts one at
    /// the current position).
    fn move_through(&mut self, args: &[f64], pen_down: bool) {
        if pen_down && !self.pen_down {
            let mut bez = BezPath::new();
            bez.move_to(self.pos);
            self.run = Some(bez);
        } else if !pen_down && self.pen_down {
            self.flush();
        }
        self.pen_down = pen_down;
        for pair in args.chunks_exact(2) {
            let (ux, uy) = (pair[0], pair[1]);
            let (px, py) = (ux / UNITS_PER_MM * PX_PER_MM, uy / UNITS_PER_MM * PX_PER_MM);
            self.pos = if self.absolute { Point::new(px, py) } else { Point::new(self.pos.x + px, self.pos.y + py) };
            if pen_down {
                if let Some(bez) = &mut self.run {
                    bez.line_to(self.pos);
                }
            }
        }
    }
}

fn circle_points(center: Point, r: f64) -> Vec<Point> {
    let n = 64;
    (0..=n)
        .map(|i| {
            let a = std::f64::consts::TAU * (i as f64 / n as f64);
            Point::new(center.x + r * a.cos(), center.y + r * a.sin())
        })
        .collect()
}

/// Writes `ids` (and, for a group, its full descendant tree) as HPGL:
/// `IN;PA;` then one `PU x,y;PD x,y,x,y,...;` pair per subpath (curves
/// flattened to line segments — pen plotters/cutters draw straight
/// moves only, there's no curve primitive to target) — everything on
/// pen 1, since (like `export_dxf`) the flattened `ids` list this
/// receives has no per-layer boundary left to map back onto distinct
/// pens. `None` if none of `ids` resolve to contributing geometry.
pub fn export_plt(document: &Document, ids: &[ObjectId]) -> Option<String> {
    let polylines = collect_polylines(document, ids);
    let bounds = polylines
        .iter()
        .flat_map(|(pts, _)| pts.iter().copied())
        .fold(None::<Rect>, |acc, p| {
            let r = Rect::new(p.x, p.y, p.x, p.y);
            Some(acc.map_or(r, |a| a.union(r)))
        })?;

    let to_units = |p: Point| {
        // Flip y (Amalith is y-down; HPGL's page space is y-up) and
        // shift so the content's own bounds start at the origin, same
        // framing `export_dxf`/`export_eps` use.
        let (x, y) = (p.x - bounds.x0, bounds.y1 - p.y);
        ((x / PX_PER_MM * UNITS_PER_MM).round() as i64, (y / PX_PER_MM * UNITS_PER_MM).round() as i64)
    };

    let mut out = String::from("IN;PA;");
    for (pts, closed) in &polylines {
        if pts.len() < 2 {
            continue;
        }
        let (x0, y0) = to_units(pts[0]);
        out.push_str(&format!("PU{x0},{y0};PD"));
        let mut coords: Vec<String> = pts[1..].iter().map(|&p| { let (x, y) = to_units(p); format!("{x},{y}") }).collect();
        if *closed {
            coords.push(format!("{x0},{y0}"));
        }
        out.push_str(&coords.join(","));
        out.push(';');
    }
    out.push_str("PU;");
    Some(out)
}

/// Every `Path`/`CompoundPath` under `ids`, in world space, flattened to
/// `(points, closed)` polylines — HPGL has no notion of fill, curves, or
/// per-shape color, so that's all export needs to carry.
fn collect_polylines(document: &Document, ids: &[ObjectId]) -> Vec<(Vec<Point>, bool)> {
    let mut out = Vec::new();
    for &id in ids {
        collect_one(document, id, &mut out);
    }
    out
}

fn collect_one(document: &Document, id: ObjectId, out: &mut Vec<(Vec<Point>, bool)>) {
    let Some(object) = document.object(id) else { return };
    match &object.kind {
        ObjectKind::Group(g) => {
            for &child in &g.children {
                collect_one(document, child, out);
            }
        }
        ObjectKind::Path(p) => {
            let bez = document.world_transform(id) * p.geometry.clone();
            out.extend(flatten_polylines(&bez));
        }
        ObjectKind::CompoundPath(cp) => {
            let xf = document.world_transform(id);
            for sub in &cp.subpaths {
                out.extend(flatten_polylines(&(xf * sub.clone())));
            }
        }
        ObjectKind::Text(_) | ObjectKind::Image(_) | ObjectKind::Symbol(_) | ObjectKind::Adjustment(_) | ObjectKind::Unknown { .. } => {}
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_simple_pen_down_square_recovers_as_one_stroked_path() {
        let plt = "IN;PA;PU0,0;PD40,0,40,40,0,40,0,0;PU;";
        let doc = import_plt(plt.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
        assert_eq!(doc.layers().len(), 1);
        assert_eq!(doc.layers()[0].name, "Pen 1");
    }

    #[test]
    fn distinct_pens_become_distinct_layers() {
        let plt = "IN;PA;SP1;PU0,0;PD40,0;SP2;PU0,40;PD40,40;";
        let doc = import_plt(plt.as_bytes()).unwrap();
        assert_eq!(doc.layers().len(), 2);
        let names: Vec<&str> = doc.layers().iter().map(|l| l.name.as_str()).collect();
        assert!(names.contains(&"Pen 1") && names.contains(&"Pen 2"));
    }

    #[test]
    fn pen_up_after_pen_down_ends_the_stroke_so_a_later_pen_down_starts_a_new_one() {
        let plt = "IN;PA;PU0,0;PD40,0;PU40,40;PD80,40;";
        let doc = import_plt(plt.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 2);
    }

    #[test]
    fn circle_command_recovers_a_closed_loop() {
        let plt = "IN;PA;PU20,20;CI10;";
        let doc = import_plt(plt.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn relative_mode_accumulates_from_the_running_position() {
        // PR: two 10-unit relative steps from (0,0) should reach (20,0)
        // in plotter units — converted, the object's bounds should span
        // 20 units worth of px, not 10.
        let plt = "IN;PA;PU0,0;PR;PD10,0,10,0;";
        let doc = import_plt(plt.as_bytes()).unwrap();
        let obj = doc.objects().next().unwrap();
        let amalith_core::ObjectKind::Path(p) = &obj.kind else { panic!("expected a Path") };
        let expected_w = 20.0 / UNITS_PER_MM * PX_PER_MM;
        assert!((p.local_bounds().width() - expected_w).abs() < 1e-6);
    }

    #[test]
    fn no_drawable_content_is_an_error() {
        assert_eq!(import_plt(b"IN;PA;SP1;").unwrap_err(), PltError::Empty);
    }

    #[test]
    fn export_then_reimport_round_trips_a_closed_triangle() {
        let mut doc = Document::new("Test");
        let layer_id = LayerId::new();
        doc.insert_layer(Layer::new(layer_id, "Layer 1"), 0);
        let id = ObjectId::new();
        let mut bez = BezPath::new();
        bez.move_to((0.0, 0.0));
        bez.line_to((40.0, 0.0));
        bez.line_to((20.0, 30.0));
        bez.close_path();
        let obj = Object::new(id, ObjectParent::Layer(layer_id), ObjectKind::Path(PathData::from_bezpath(bez)));
        doc.insert_object(obj, 0).unwrap();

        let plt = export_plt(&doc, &[id]).expect("export produced content");
        let reimported = import_plt(plt.as_bytes()).expect("reimport succeeded");
        let obj = reimported.objects().next().unwrap();
        let ObjectKind::Path(p) = &obj.kind else { panic!("expected a Path") };
        let b = p.local_bounds();
        assert!((b.width() - 40.0).abs() < 0.5 && (b.height() - 30.0).abs() < 0.5, "got {b:?}");
    }
}
