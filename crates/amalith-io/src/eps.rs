//! Best-effort EPS import: a small stack-based interpreter over the
//! common subset of PostScript real-world design tools actually emit —
//! path construction, fill/stroke paint, and basic graphics state
//! (`gsave`/`grestore`, `translate`/`scale`/`rotate`/`concat`) — not a
//! general PostScript engine. PostScript is a full programming language
//! (procedures, conditionals, loops); implementing all of it is out of
//! scope for this project (that's Ghostscript-sized). Matching
//! `ai.rs`/`svg.rs`'s philosophy, anything past this subset — custom
//! `def`initions actually invoked, `if`/`ifelse`/`for`/`repeat` actually
//! executing their procedure bodies, patterns, images, text, dash
//! patterns — is silently skipped rather than a hard error, so a complex
//! real-world file still recovers whatever straight-line drawing it can
//! instead of failing outright.
//!
//! A `{ ... }` procedure body is never executed: it's tokenized into one
//! opaque placeholder value, so `if`/`ifelse`/`for`/`repeat`/`exec` just
//! consume their operands as normal stack traffic and do nothing further.
//! This is what makes a flat, non-recursive single pass over the token
//! stream safe — there's no risk of a malformed `for`/`loop` hanging the
//! importer, at the cost of not recovering whatever drawing lived inside
//! a conditional or a repeated block.

use amalith_core::{
    Affine, Appearance, AppearanceItem, Artboard, ArtboardId, Color, Document, LayerId, Object,
    ObjectId, ObjectKind, ObjectParent, Paint, PathData, Point, Rect,
};
use kurbo::{BezPath, PathEl, Shape};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum EpsError {
    #[error("not readable as text (or as a DOS binary EPS header)")]
    NotText,
    #[error("no drawable content recovered")]
    Empty,
}

/// Parses `bytes` (an `.eps` file's contents) and builds a fresh
/// [`Document`] from it: one artboard sized from `%%BoundingBox` (a
/// default Letter-ish size if that's missing or deferred `(atend)` with
/// no later numeric value), one layer, and one object per `fill`/`stroke`
/// call recovered from the interpreter above.
pub fn import_eps(bytes: &[u8]) -> Result<Document, EpsError> {
    let text = extract_postscript_text(bytes).ok_or(EpsError::NotText)?;
    let bbox = find_bounding_box(&text).unwrap_or(Rect::new(0.0, 0.0, 612.0, 792.0));
    let (w, h) = (bbox.width().max(1.0), bbox.height().max(1.0));

    let mut document = Document::new("Imported");
    let layer_id = LayerId::new();
    document.insert_layer(amalith_core::Layer::new(layer_id, "Layer 1"), 0);
    let artboard_rect = Rect::new(0.0, 0.0, w, h);
    document.insert_artboard(Artboard::new(ArtboardId::new(), "Artboard 1", artboard_rect), 0);

    // PostScript space has its origin at the bounding box's bottom-left
    // with y increasing upward; Amalith (like SVG/PDF) has y increasing
    // downward. Flip vertically and shift so `bbox`'s bottom-left lands
    // at this artboard's origin.
    let base = Affine::new([1.0, 0.0, 0.0, -1.0, -bbox.x0, bbox.y1]);

    let paths = Interpreter::run(&text, base);
    if paths.is_empty() {
        return Err(EpsError::Empty);
    }
    for (i, recovered) in paths.into_iter().enumerate() {
        let obj = Object {
            appearance: recovered.appearance,
            ..Object::new(ObjectId::new(), ObjectParent::Layer(layer_id), amalith_core::ObjectKind::Path(PathData::from_bezpath(recovered.geometry)))
        };
        let _ = document.insert_object(obj, i);
    }
    Ok(document)
}

/// A DOS-format binary EPS (common from older Windows/Mac Illustrator
/// exports) wraps the actual PostScript in a fixed 30-byte header
/// alongside optional WMF/TIFF previews, marked by this 4-byte magic.
const BINARY_EPS_MAGIC: [u8; 4] = [0xC5, 0xD0, 0xD3, 0xC6];

/// Strips a DOS binary-EPS header if present (using its embedded PS
/// offset/length fields) and decodes the rest as text. Plain ASCII/UTF-8
/// EPS (the vast majority of real files) has no such header — the whole
/// input is the PostScript text.
fn extract_postscript_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() >= 30 && bytes[0..4] == BINARY_EPS_MAGIC {
        let read_u32 = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let (ps_start, ps_len) = (read_u32(4), read_u32(8));
        let end = ps_start.checked_add(ps_len)?;
        let slice = bytes.get(ps_start..end)?;
        return Some(String::from_utf8_lossy(slice).into_owned());
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Scans every `%%BoundingBox:` DSC comment line and keeps the last one
/// that parses as four numbers — real files sometimes state `(atend)`
/// early (the true box is written once the content's real extent is
/// known) and repeat the comment near the trailer with real numbers.
fn find_bounding_box(text: &str) -> Option<Rect> {
    let mut found = None;
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("%%BoundingBox:") else { continue };
        let nums: Vec<f64> = rest.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        if let [llx, lly, urx, ury] = nums[..] {
            found = Some(Rect::new(llx, lly, urx, ury));
        }
    }
    found
}

/// One recovered `fill`/`stroke` call: already-CTM-transformed geometry
/// (document space) plus the single Fill or Stroke item that painted it.
struct Recovered {
    geometry: BezPath,
    appearance: Appearance,
}

/// An interpreter operand: PostScript is dynamically typed, so unlike
/// `ai.rs`'s PDF operand stack (numbers only) this needs a few more
/// shapes — a numeric array for `concat`'s matrix operand, and an opaque
/// placeholder for anything else (a literal name, or a `{...}`
/// procedure body — never executed, see the module doc comment) so
/// operators that don't care about a value's real type (`pop`, `dup`
/// consumers that just discard, `if`/`ifelse`'s condition+bodies) still
/// have *something* to pop without the interpreter needing to model it.
#[derive(Clone, Debug)]
enum Value {
    Num(f64),
    Arr(Vec<f64>),
    Other,
}

#[derive(Clone, Copy)]
struct GState {
    ctm: Affine,
    fill: Paint,
    stroke: Option<Paint>,
    line_width: f64,
}

impl Default for GState {
    fn default() -> Self {
        Self { ctm: Affine::IDENTITY, fill: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)), stroke: None, line_width: 1.0 }
    }
}

struct Interpreter {
    stack: Vec<Value>,
    gs: GState,
    gstack: Vec<GState>,
    /// The current path, already transformed into document space (each
    /// point is baked through `gs.ctm` the moment it's emitted, rather
    /// than stored in user space with one transform applied at the end —
    /// simpler, and still correct even across a mid-path `concat`/
    /// `translate`, which real PostScript allows).
    path: BezPath,
    /// Current point and the active subpath's start, in *user* space
    /// (pre-CTM) — relative operators (`rlineto`, …) and `closepath`
    /// need these un-transformed.
    cur: Point,
    sub_start: Point,
    out: Vec<Recovered>,
}

impl Interpreter {
    fn run(text: &str, base: Affine) -> Vec<Recovered> {
        let mut it = Interpreter {
            stack: Vec::new(),
            gs: GState { ctm: base, ..GState::default() },
            gstack: Vec::new(),
            path: BezPath::new(),
            cur: Point::ZERO,
            sub_start: Point::ZERO,
            out: Vec::new(),
        };
        for tok in tokenize(text) {
            it.step(tok);
        }
        it.out
    }

    fn pop_num(&mut self) -> Option<f64> {
        match self.stack.pop()? {
            Value::Num(n) => Some(n),
            _ => None,
        }
    }
    fn pop_point_user(&mut self) -> Option<Point> {
        let y = self.pop_num()?;
        let x = self.pop_num()?;
        Some(Point::new(x, y))
    }

    fn step(&mut self, tok: Tok) {
        match tok {
            Tok::Num(n) => self.stack.push(Value::Num(n)),
            Tok::Arr(a) => self.stack.push(Value::Arr(a)),
            Tok::Other => self.stack.push(Value::Other),
            Tok::Name(name) => self.op(&name),
        }
    }

    fn op(&mut self, name: &str) {
        match name {
            "moveto" => {
                if let Some(p) = self.pop_point_user() {
                    self.cur = p;
                    self.sub_start = p;
                    self.path.move_to(self.gs.ctm * p);
                }
            }
            "lineto" => {
                if let Some(p) = self.pop_point_user() {
                    self.cur = p;
                    self.path.line_to(self.gs.ctm * p);
                }
            }
            "rmoveto" => {
                if let Some(d) = self.pop_point_user() {
                    let p = Point::new(self.cur.x + d.x, self.cur.y + d.y);
                    self.cur = p;
                    self.sub_start = p;
                    self.path.move_to(self.gs.ctm * p);
                }
            }
            "rlineto" => {
                if let Some(d) = self.pop_point_user() {
                    let p = Point::new(self.cur.x + d.x, self.cur.y + d.y);
                    self.cur = p;
                    self.path.line_to(self.gs.ctm * p);
                }
            }
            "curveto" => {
                if let (Some(p3), Some(p2), Some(p1)) = (self.pop_point_user(), self.pop_point_user(), self.pop_point_user()) {
                    self.path.curve_to(self.gs.ctm * p1, self.gs.ctm * p2, self.gs.ctm * p3);
                    self.cur = p3;
                }
            }
            "rcurveto" => {
                if let (Some(d3), Some(d2), Some(d1)) = (self.pop_point_user(), self.pop_point_user(), self.pop_point_user()) {
                    let p1 = Point::new(self.cur.x + d1.x, self.cur.y + d1.y);
                    let p2 = Point::new(p1.x + d2.x, p1.y + d2.y);
                    let p3 = Point::new(p2.x + d3.x, p2.y + d3.y);
                    self.path.curve_to(self.gs.ctm * p1, self.gs.ctm * p2, self.gs.ctm * p3);
                    self.cur = p3;
                }
            }
            "closepath" => {
                self.path.close_path();
                self.cur = self.sub_start;
            }
            "newpath" => {
                self.path = BezPath::new();
            }
            "fill" | "eofill" => self.paint(true),
            "stroke" => self.paint(false),
            "setlinewidth" => {
                if let Some(w) = self.pop_num() {
                    self.gs.line_width = w;
                }
            }
            "setgray" => {
                if let Some(g) = self.pop_num() {
                    self.set_fill_stroke_color(Color::rgb(g as f32, g as f32, g as f32));
                }
            }
            "setrgbcolor" => {
                if let (Some(b), Some(g), Some(r)) = (self.pop_num(), self.pop_num(), self.pop_num()) {
                    self.set_fill_stroke_color(Color::rgb(r as f32, g as f32, b as f32));
                }
            }
            "setcmykcolor" => {
                if let (Some(k), Some(y), Some(m), Some(c)) = (self.pop_num(), self.pop_num(), self.pop_num(), self.pop_num()) {
                    self.set_fill_stroke_color(Color::from_cmyk(c as f32, m as f32, y as f32, k as f32));
                }
            }
            "translate" => {
                if let Some(d) = self.pop_point_user() {
                    self.gs.ctm *= Affine::translate((d.x, d.y));
                }
            }
            "scale" => {
                if let (Some(sy), Some(sx)) = (self.pop_num(), self.pop_num()) {
                    self.gs.ctm *= Affine::scale_non_uniform(sx, sy);
                }
            }
            "rotate" => {
                if let Some(deg) = self.pop_num() {
                    self.gs.ctm *= Affine::rotate(deg.to_radians());
                }
            }
            "concat" => {
                if let Some(Value::Arr(a)) = self.stack.pop() {
                    if let [a0, a1, a2, a3, a4, a5] = a[..] {
                        self.gs.ctm *= Affine::new([a0, a1, a2, a3, a4, a5]);
                    }
                }
            }
            "gsave" => self.gstack.push(self.gs),
            "grestore" => {
                if let Some(gs) = self.gstack.pop() {
                    self.gs = gs;
                }
            }
            // Everything else — `def`, `dict`/`begin`/`end`, `show` and
            // friends (no text support), `setdash`, `clip`, image/pattern
            // operators, arithmetic, any user-defined/unrecognized name —
            // is silently skipped per the module doc comment. Operands
            // already on the stack are simply left there (or fall off
            // the bottom on the next unrelated push); nothing here
            // inspects stack depth, so leftover junk is harmless.
            _ => {}
        }
    }

    /// `fill`/`stroke` paint the *current* path without clearing it —
    /// real PostScript semantics (a script that wants a fresh path calls
    /// `newpath` itself, and most generated EPS does exactly that before
    /// every shape). Painting fill and stroke both on the same path
    /// means two `Recovered` entries land on identical geometry — fine
    /// visually, just not the most compact possible recovery.
    fn paint(&mut self, filling: bool) {
        if self.path.elements().is_empty() {
            return;
        }
        let item = if filling {
            AppearanceItem::Fill { paint: self.gs.fill, opacity: 1.0, visible: true, effects: Vec::new() }
        } else {
            let paint = self.gs.stroke.unwrap_or(self.gs.fill);
            AppearanceItem::Stroke {
                paint,
                width: self.gs.line_width.max(0.01),
                style: amalith_core::StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
                effects: Vec::new(),
            }
        };
        self.out.push(Recovered {
            geometry: self.path.clone(),
            appearance: Appearance { items: vec![item], opacity: 1.0 },
        });
    }

    /// PostScript has one "current color" used for whichever of
    /// fill/stroke paints next — unlike Amalith's own fill+stroke-at-once
    /// model. Setting it updates both, which is exactly right for
    /// `fill`/`stroke` calls issued back-to-back with a color change in
    /// between (the common case); a script that explicitly wants
    /// different fill and stroke colors sets one, paints, sets the
    /// other, paints again — also handled correctly, just as two
    /// separate `Recovered` entries via `paint`.
    fn set_fill_stroke_color(&mut self, c: Color) {
        self.gs.fill = Paint::Solid(c);
        self.gs.stroke = Some(Paint::Solid(c));
    }
}

#[derive(Debug)]
enum Tok {
    Num(f64),
    Name(String),
    Arr(Vec<f64>),
    /// A literal name (`/foo`) or an un-executed `{...}` procedure body —
    /// see the module doc comment.
    Other,
}

/// Tokenizes `text`, skipping comments (`%` to end of line — including
/// `%%BoundingBox`, already handled separately by `find_bounding_box`)
/// and string literals (`(...)`/`<...>`, balanced/escape-aware but never
/// inspected — no text support). A `{ ... }` procedure body collapses to
/// one `Tok::Other`, nested braces included, so the interpreter's main
/// loop never needs to recurse.
fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => i += 1,
            '%' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            '(' => {
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '\\' => i += 1, // skip the escaped char too
                        '(' => depth += 1,
                        ')' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
            }
            '<' => {
                i += 1;
                while i < chars.len() && chars[i] != '>' {
                    i += 1;
                }
                i += 1;
            }
            '{' => {
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    match chars[i] {
                        '{' => depth += 1,
                        '}' => depth -= 1,
                        _ => {}
                    }
                    i += 1;
                }
                out.push(Tok::Other);
            }
            '}' => i += 1, // an unmatched close (malformed input) — ignore
            '[' => {
                let mut nums = Vec::new();
                i += 1;
                let mut buf = String::new();
                while i < chars.len() && chars[i] != ']' {
                    if chars[i].is_whitespace() {
                        if let Ok(n) = buf.parse() {
                            nums.push(n);
                        }
                        buf.clear();
                    } else {
                        buf.push(chars[i]);
                    }
                    i += 1;
                }
                if let Ok(n) = buf.parse() {
                    nums.push(n);
                }
                i += 1; // the ']'
                out.push(Tok::Arr(nums));
            }
            ']' => i += 1,
            '/' => {
                i += 1;
                while i < chars.len() && !chars[i].is_whitespace() && !"()<>{}[]/%".contains(chars[i]) {
                    i += 1;
                }
                out.push(Tok::Other);
            }
            _ => {
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() && !"()<>{}[]/%".contains(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                if let Ok(n) = word.parse::<f64>() {
                    out.push(Tok::Num(n));
                } else if !word.is_empty() {
                    out.push(Tok::Name(word));
                }
            }
        }
    }
    out
}

/// Writes `ids` (and, for a group, its full descendant tree — same
/// traversal `svg.rs`'s own exporter uses) to a standalone EPS document,
/// tight-cropped to the recovered content's own bounds rather than any
/// artboard size, which is what a laser/cut workflow actually wants.
/// `None` if none of `ids` resolve to a `Path`/`CompoundPath` with
/// contributing geometry — `Text`/`Image`/`Symbol` have no export here,
/// matching this crate's import side not recovering them either.
pub fn export_eps(document: &Document, ids: &[ObjectId]) -> Option<String> {
    let shapes = collect_shapes(document, ids);
    let bounds = shapes.iter().map(|(b, _)| b.bounding_box()).reduce(|a, b| a.union(b))?;

    let mut out = String::new();
    out.push_str("%!PS-Adobe-3.0 EPSF-3.0\n");
    out.push_str(&format!(
        "%%BoundingBox: 0 0 {} {}\n%%EndComments\n",
        bounds.width().ceil() as i64,
        bounds.height().ceil() as i64
    ));
    for (bez, appearance) in &shapes {
        let ops = path_ops(bez, bounds);
        if let Some(c) = appearance.fill().color() {
            out.push_str(&format!("{} {} {} setrgbcolor\n{ops}fill\n", c.r, c.g, c.b));
        }
        if let Some(c) = appearance.stroke().color() {
            out.push_str(&format!(
                "{} {} {} setrgbcolor\n{} setlinewidth\n{ops}stroke\n",
                c.r,
                c.g,
                c.b,
                appearance.stroke_width()
            ));
        }
    }
    out.push_str("%%EOF\n");
    Some(out)
}

/// Every `Path`/`CompoundPath` under `ids`, already in world (document)
/// space — `Object::transform` composed all the way up, per
/// `Document::world_transform` — paired with its own appearance. A
/// `Group` recurses into its children rather than contributing geometry
/// itself.
fn collect_shapes(document: &Document, ids: &[ObjectId]) -> Vec<(BezPath, Appearance)> {
    let mut out = Vec::new();
    for &id in ids {
        collect_one(document, id, &mut out);
    }
    out
}

fn collect_one(document: &Document, id: ObjectId, out: &mut Vec<(BezPath, Appearance)>) {
    let Some(object) = document.object(id) else { return };
    match &object.kind {
        ObjectKind::Group(g) => {
            for &child in &g.children {
                collect_one(document, child, out);
            }
        }
        ObjectKind::Path(p) => {
            out.push((document.world_transform(id) * p.geometry.clone(), object.appearance.clone()));
        }
        ObjectKind::CompoundPath(cp) => {
            let xf = document.world_transform(id);
            for sub in &cp.subpaths {
                out.push((xf * sub.clone(), object.appearance.clone()));
            }
        }
        ObjectKind::Text(_) | ObjectKind::Image(_) | ObjectKind::Symbol(_) | ObjectKind::Unknown { .. } => {}
    }
}

/// Emits `moveto`/`lineto`/`curveto`/`closepath` for `bez`, flipping y
/// (Amalith is y-down; PostScript is y-up) against `bounds`'s own
/// height so the result sits right-side-up with its origin at
/// `bounds`'s bottom-left — matching the `%%BoundingBox` `export_eps`
/// writes (always `0 0 w h`). A `QuadTo` (never actually produced by
/// Amalith's own cubic-only anchor model, but handled for robustness)
/// degree-elevates to the equivalent cubic rather than approximating it
/// as a straight line.
fn path_ops(bez: &BezPath, bounds: Rect) -> String {
    let flip = |p: Point| (p.x - bounds.x0, bounds.y1 - p.y);
    let mut s = String::new();
    let mut cur = Point::ZERO;
    for el in bez.elements() {
        match *el {
            PathEl::MoveTo(p) => {
                let (x, y) = flip(p);
                s.push_str(&format!("{x} {y} moveto\n"));
                cur = p;
            }
            PathEl::LineTo(p) => {
                let (x, y) = flip(p);
                s.push_str(&format!("{x} {y} lineto\n"));
                cur = p;
            }
            PathEl::QuadTo(c, p) => {
                let c1 = Point::new(cur.x + 2.0 / 3.0 * (c.x - cur.x), cur.y + 2.0 / 3.0 * (c.y - cur.y));
                let c2 = Point::new(p.x + 2.0 / 3.0 * (c.x - p.x), p.y + 2.0 / 3.0 * (c.y - p.y));
                let (x1, y1) = flip(c1);
                let (x2, y2) = flip(c2);
                let (x3, y3) = flip(p);
                s.push_str(&format!("{x1} {y1} {x2} {y2} {x3} {y3} curveto\n"));
                cur = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let (x1, y1) = flip(c1);
                let (x2, y2) = flip(c2);
                let (x3, y3) = flip(p);
                s.push_str(&format!("{x1} {y1} {x2} {y2} {x3} {y3} curveto\n"));
                cur = p;
            }
            PathEl::ClosePath => s.push_str("closepath\n"),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_eps() -> String {
        "%!PS-Adobe-3.0 EPSF-3.0\n%%BoundingBox: 0 0 100 100\n\
         1 0 0 setrgbcolor\n\
         10 10 moveto 90 10 lineto 90 90 lineto 10 90 lineto closepath fill\n\
         %%EOF\n"
            .to_string()
    }

    #[test]
    fn recovers_bounding_box_and_a_filled_square() {
        let doc = import_eps(square_eps().as_bytes()).unwrap();
        assert_eq!(doc.artboards().len(), 1);
        let ab = &doc.artboards()[0];
        assert_eq!(ab.rect.width(), 100.0);
        assert_eq!(ab.rect.height(), 100.0);
        let objects: Vec<_> = doc.objects().collect();
        assert_eq!(objects.len(), 1);
        match &objects[0].appearance.items[..] {
            [AppearanceItem::Fill { paint: Paint::Solid(c), .. }] => {
                assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0));
            }
            other => panic!("expected one Fill item, got {other:?}"),
        }
    }

    #[test]
    fn relative_operators_track_the_current_point() {
        let eps = "%%BoundingBox: 0 0 50 50\n0 0 moveto 10 0 rlineto 0 10 rlineto stroke\n";
        let doc = import_eps(eps.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn an_unexecuted_procedure_does_not_confuse_the_stack() {
        // `{ ... } if` never runs the body (see the module doc comment) —
        // the moveto/lineto/fill inside it must not appear, but parsing
        // still has to get through it cleanly and recover the fill after.
        let eps = "%%BoundingBox: 0 0 10 10\n\
                   true { 0 0 moveto 5 5 lineto fill } if\n\
                   1 1 moveto 9 1 lineto 9 9 lineto fill\n";
        let doc = import_eps(eps.as_bytes()).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn binary_dos_header_is_stripped() {
        let ps = square_eps();
        let mut bytes = BINARY_EPS_MAGIC.to_vec();
        bytes.extend_from_slice(&(30u32).to_le_bytes()); // ps start
        bytes.extend_from_slice(&(ps.len() as u32).to_le_bytes()); // ps length
        bytes.extend_from_slice(&[0u8; 22]); // rest of the 30-byte header
        bytes.extend_from_slice(ps.as_bytes());
        let doc = import_eps(&bytes).unwrap();
        assert_eq!(doc.objects().count(), 1);
    }

    #[test]
    fn gsave_grestore_isolates_color_across_two_shapes_with_a_curve() {
        // `newpath` before the second shape matters: PostScript's
        // `fill`/`stroke` don't clear the current path (real semantics,
        // needed so a plain `fill stroke` pair paints the *same* shape
        // twice) — a well-formed file starts every new, unrelated shape
        // with an explicit `newpath`, same as this one does.
        let eps = "%%BoundingBox: 0 0 200 200\n\
                   gsave 0.2 0.4 0.8 setrgbcolor \
                   20 20 moveto 180 20 lineto 180 180 lineto 20 180 lineto closepath fill grestore\n\
                   newpath\n\
                   gsave 1 0 0 setrgbcolor 5 setlinewidth \
                   50 50 moveto 150 50 100 150 50 150 curveto stroke grestore\n";
        let doc = import_eps(eps.as_bytes()).unwrap();
        let objects: Vec<_> = doc.objects().collect();
        assert_eq!(objects.len(), 2);
        // `Document::objects()` iterates its arena (a `HashMap`), not
        // paint order — find each by its appearance instead of position.
        let filled = objects
            .iter()
            .find(|o| matches!(o.appearance.items[..], [AppearanceItem::Fill { .. }]))
            .expect("no filled object recovered");
        match filled.appearance.items[0] {
            AppearanceItem::Fill { paint: Paint::Solid(c), .. } => {
                assert!((c.r - 0.2).abs() < 1e-6 && (c.g - 0.4).abs() < 1e-6 && (c.b - 0.8).abs() < 1e-6);
            }
            _ => unreachable!(),
        }
        let stroked = objects
            .iter()
            .find(|o| matches!(o.appearance.items[..], [AppearanceItem::Stroke { .. }]))
            .expect("no stroked object recovered");
        match stroked.appearance.items[0] {
            AppearanceItem::Stroke { paint: Paint::Solid(c), width, .. } => {
                assert_eq!((c.r, c.g, c.b), (1.0, 0.0, 0.0));
                assert_eq!(width, 5.0);
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn fill_then_stroke_with_no_newpath_between_paints_the_same_shape_twice() {
        // Real PostScript semantics: `fill`/`stroke` don't clear the
        // current path, so this common idiom (a filled *and* outlined
        // shape) paints the identical geometry twice — once per call.
        let eps = "%%BoundingBox: 0 0 100 100\n\
                   0 1 0 setrgbcolor\n\
                   10 10 moveto 90 10 lineto 90 90 lineto 10 90 lineto closepath\n\
                   fill\n\
                   0 0 0 setrgbcolor stroke\n";
        let doc = import_eps(eps.as_bytes()).unwrap();
        let objects: Vec<_> = doc.objects().collect();
        assert_eq!(objects.len(), 2);
        assert!(objects.iter().any(|o| matches!(o.appearance.items[..], [AppearanceItem::Fill { .. }])));
        assert!(objects.iter().any(|o| matches!(o.appearance.items[..], [AppearanceItem::Stroke { .. }])));
        // Same geometry both times, not a leftover-plus-more mixup.
        assert_eq!(objects[0].kind, objects[1].kind);
    }

    #[test]
    fn missing_bounding_box_falls_back_to_a_default_size() {
        let doc = import_eps(b"0 0 moveto 10 10 lineto stroke\n").unwrap();
        let ab = &doc.artboards()[0];
        assert_eq!((ab.rect.width(), ab.rect.height()), (612.0, 792.0));
    }

    #[test]
    fn no_drawable_content_is_an_error() {
        assert_eq!(import_eps(b"%%BoundingBox: 0 0 10 10\n").unwrap_err(), EpsError::Empty);
    }

    #[test]
    fn export_then_reimport_round_trips_a_filled_rectangle() {
        let mut doc = Document::new("Test");
        let layer_id = LayerId::new();
        doc.insert_layer(amalith_core::Layer::new(layer_id, "Layer 1"), 0);
        let id = ObjectId::new();
        let mut obj = Object::new(id, ObjectParent::Layer(layer_id), ObjectKind::Path(PathData::rectangle(Rect::new(0.0, 0.0, 50.0, 30.0))));
        obj.appearance = Appearance {
            items: vec![AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(0.0, 0.5, 1.0)), opacity: 1.0, visible: true, effects: Vec::new() }],
            opacity: 1.0,
        };
        doc.insert_object(obj, 0).unwrap();

        let eps = export_eps(&doc, &[id]).expect("export produced content");
        let reimported = import_eps(eps.as_bytes()).expect("reimport succeeded");
        let obj = reimported.objects().next().unwrap();
        match obj.appearance.items[..] {
            [AppearanceItem::Fill { paint: Paint::Solid(c), .. }] => {
                assert!((c.r - 0.0).abs() < 1e-3 && (c.g - 0.5).abs() < 1e-3 && (c.b - 1.0).abs() < 1e-3);
            }
            _ => panic!("expected one Fill item, got {:?}", obj.appearance.items),
        }
        let ObjectKind::Path(p) = &obj.kind else { panic!("expected a Path") };
        let b = p.local_bounds();
        assert!((b.width() - 50.0).abs() < 0.5 && (b.height() - 30.0).abs() < 0.5, "got {b:?}");
    }
}
