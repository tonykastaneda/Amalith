//! Reading Illustrator's PDF-compatible layer of an `.ai` file.
//!
//! Since Illustrator 9 / CS, every `.ai` file *is* a valid PDF with an
//! additional, undocumented, Adobe-only "native object" stream layered on
//! top for Illustrator's own internal use (live effects, symbols, exact
//! layer structure, editable blends). That stream has never been
//! published, and nobody outside Adobe has fully reverse-engineered it —
//! this reads the PDF layer instead, which is what every other
//! non-Adobe `.ai` reader actually does. That recovers real vector
//! geometry, fills/strokes, and artboards (one per PDF page) well; it
//! does *not* recover live effects or editable blends (Illustrator
//! already flattened those to their final static shapes before writing
//! the PDF layer), and — for now — it doesn't recover text at all (see
//! the module-level TODO below); raster images are skipped too.
//!
//! Best-effort throughout, matching `svg.rs`'s philosophy: an
//! unsupported operator, color space, or resource is silently skipped
//! rather than a hard error, so a complex real-world file still recovers
//! whatever Amalith *can* represent instead of failing outright.
//!
//! TODO: recover text. Illustrator's PDF output usually embeds a
//! `/ToUnicode` CMap per font, which would let real character content
//! come back as an editable Amalith `Text` object (at a generic font,
//! since matching the exact original font isn't realistic); without one,
//! that run has to stay unrecovered — turning the shown bytes into
//! glyph outlines directly would need a full embedded-font (TrueType/
//! CFF) parser, which is out of scope.

use amalith_core::{
    Affine, Anchor, Appearance, Artboard, ArtboardId, Color, Document, LayerId, Object, ObjectId,
    ObjectKind, ObjectParent, Paint, PathData, Point, Rect, Subpath,
};
use lopdf::content::Operation;
use lopdf::{Dictionary, Document as PdfDocument, Object as PdfObject, ObjectId as PdfId};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("malformed PDF-compatible layer: {0}")]
    Pdf(#[from] lopdf::Error),
    #[error("no pages found in the PDF-compatible layer")]
    NoPages,
}

/// Parses `bytes` (an `.ai` file's contents) as its PDF-compatible layer
/// and builds a fresh [`Document`] from it — one artboard per PDF page,
/// laid out left to right, and one layer holding every recovered path.
pub fn import_ai(bytes: &[u8]) -> Result<Document, AiError> {
    let pdf = PdfDocument::load_mem(bytes)?;
    let pages = pdf.get_pages();
    if pages.is_empty() {
        return Err(AiError::NoPages);
    }

    let mut document = Document::new("Imported");
    let layer_id = LayerId::new();
    document.insert_layer(amalith_core::Layer::new(layer_id, "Layer 1"), 0);

    const GAP: f64 = 60.0;
    let mut x_cursor = 0.0;
    let mut next_index = 0usize;
    for (i, (_num, page_id)) in pages.iter().enumerate() {
        let page_id = *page_id;
        let src_rect = page_rect(&pdf, page_id).unwrap_or(Rect::new(0.0, 0.0, 800.0, 600.0));
        let (w, h) = (src_rect.width().max(1.0), src_rect.height().max(1.0));
        let artboard_rect = Rect::new(x_cursor, 0.0, x_cursor + w, h);
        document.insert_artboard(
            Artboard::new(ArtboardId::new(), format!("Artboard {}", i + 1), artboard_rect),
            i,
        );
        x_cursor += w + GAP;

        // PDF space has its origin at the page's bottom-left with y
        // increasing upward; Amalith (like SVG) has y increasing
        // downward. Flip vertically and shift into this artboard's own
        // placement in the shared document space.
        let base = Affine::new([1.0, 0.0, 0.0, -1.0, artboard_rect.x0 - src_rect.x0, artboard_rect.y0 + src_rect.y1]);

        let Ok(content_bytes) = pdf.get_page_content(page_id) else { continue };
        let Ok(content) = lopdf::content::Content::decode(&content_bytes) else { continue };
        let resources = pdf
            .get_page_resources(page_id)
            .ok()
            .and_then(|(d, _)| d.cloned())
            .unwrap_or_default();

        let mut out = Vec::new();
        let gs = GState { ctm: base, ..GState::default() };
        interpret(&pdf, &content.operations, gs, &resources, &mut out, 0);
        for kind in out {
            let id = ObjectId::new();
            let mut obj = Object::new(id, ObjectParent::Layer(layer_id), kind.kind);
            obj.appearance = kind.appearance;
            if document.insert_object(obj, next_index).is_ok() {
                next_index += 1;
            }
        }
    }
    Ok(document)
}

/// `/ArtBox` (Illustrator's own artboard bounds) if present, else
/// `/MediaBox`, walking up `/Parent` for either when the page itself
/// doesn't carry one (both are inheritable per the PDF spec).
fn page_rect(pdf: &PdfDocument, page_id: PdfId) -> Option<Rect> {
    let dict = |id: PdfId| pdf.get_object(id).ok()?.as_dict().ok();
    let find = |key: &[u8]| -> Option<Rect> {
        let mut cur = Some(page_id);
        let mut depth = 0;
        while let Some(id) = cur {
            if depth > 16 {
                break;
            }
            depth += 1;
            let d = dict(id)?;
            if let Ok(arr) = d.get(key).and_then(PdfObject::as_array) {
                if arr.len() == 4 {
                    let n = |o: &PdfObject| num(o);
                    let (x0, y0, x1, y1) = (n(&arr[0]), n(&arr[1]), n(&arr[2]), n(&arr[3]));
                    return Some(Rect::new(x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)));
                }
            }
            cur = d.get(b"Parent").and_then(PdfObject::as_reference).ok();
        }
        None
    };
    find(b"ArtBox").or_else(|| find(b"MediaBox"))
}

fn num(o: &PdfObject) -> f64 {
    match o {
        PdfObject::Integer(i) => *i as f64,
        PdfObject::Real(f) => *f as f64,
        _ => 0.0,
    }
}

#[derive(Clone)]
struct GState {
    ctm: Affine,
    fill: Option<Color>,
    stroke: Option<Color>,
    line_width: f64,
}

impl Default for GState {
    fn default() -> Self {
        Self {
            ctm: Affine::IDENTITY,
            fill: Some(Color::rgb(0.0, 0.0, 0.0)),
            stroke: None,
            line_width: 1.0,
        }
    }
}

/// One recovered shape, ready to become an `Object` once the caller
/// knows its final id / parent.
struct Recovered {
    kind: ObjectKind,
    appearance: Appearance,
}

/// A subpath under construction: anchors so far, plus whether a
/// `h` (closepath) was seen.
struct BuildingSubpath {
    anchors: Vec<Anchor>,
    closed: bool,
}

fn interpret(
    pdf: &PdfDocument,
    ops: &[Operation],
    mut gs: GState,
    resources: &Dictionary,
    out: &mut Vec<Recovered>,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let mut stack: Vec<GState> = Vec::new();
    let mut subpaths: Vec<BuildingSubpath> = Vec::new();
    let mut current: Option<BuildingSubpath> = None;

    macro_rules! pt {
        ($x:expr, $y:expr) => {
            gs.ctm * Point::new(num($x), num($y))
        };
    }

    let finish_current = |current: &mut Option<BuildingSubpath>, subpaths: &mut Vec<BuildingSubpath>| {
        if let Some(sp) = current.take() {
            if sp.anchors.len() >= 2 {
                subpaths.push(sp);
            }
        }
    };

    for op in ops {
        let a = &op.operands;
        match op.operator.as_str() {
            "m" if a.len() >= 2 => {
                finish_current(&mut current, &mut subpaths);
                current = Some(BuildingSubpath {
                    anchors: vec![Anchor::corner(pt!(&a[0], &a[1]))],
                    closed: false,
                });
            }
            "l" if a.len() >= 2 => {
                if let Some(sp) = &mut current {
                    sp.anchors.push(Anchor::corner(pt!(&a[0], &a[1])));
                }
            }
            "c" if a.len() >= 6 => {
                if let Some(sp) = &mut current {
                    let (c1, c2, p3) = (pt!(&a[0], &a[1]), pt!(&a[2], &a[3]), pt!(&a[4], &a[5]));
                    if let Some(last) = sp.anchors.last_mut() {
                        last.handle_out = Some(c1);
                    }
                    sp.anchors.push(Anchor {
                        point: p3,
                        handle_in: Some(c2),
                        handle_out: None,
                        mode: amalith_core::HandleMode::Corner,
                    });
                }
            }
            "v" if a.len() >= 4 => {
                // Same as `c`, with the first control point implicitly
                // equal to the current point.
                if let Some(sp) = &mut current {
                    let (c2, p3) = (pt!(&a[0], &a[1]), pt!(&a[2], &a[3]));
                    sp.anchors.push(Anchor {
                        point: p3,
                        handle_in: Some(c2),
                        handle_out: None,
                        mode: amalith_core::HandleMode::Corner,
                    });
                }
            }
            "y" if a.len() >= 4 => {
                // Same as `c`, with the second control point implicitly
                // equal to the segment's endpoint.
                if let Some(sp) = &mut current {
                    let (c1, p3) = (pt!(&a[0], &a[1]), pt!(&a[2], &a[3]));
                    if let Some(last) = sp.anchors.last_mut() {
                        last.handle_out = Some(c1);
                    }
                    sp.anchors.push(Anchor::corner(p3));
                }
            }
            "h" => {
                if let Some(sp) = &mut current {
                    sp.closed = true;
                }
            }
            "re" if a.len() >= 4 => {
                finish_current(&mut current, &mut subpaths);
                let (x, y, w, h) = (num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3]));
                let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                subpaths.push(BuildingSubpath {
                    anchors: corners
                        .iter()
                        .map(|&(cx, cy)| Anchor::corner(gs.ctm * Point::new(cx, cy)))
                        .collect(),
                    closed: true,
                });
            }
            "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "S" | "s" | "n" => {
                finish_current(&mut current, &mut subpaths);
                let is_close_stroke = matches!(op.operator.as_str(), "s" | "b" | "b*");
                let force_closed = !matches!(op.operator.as_str(), "S" | "n");
                let paints = !matches!(op.operator.as_str(), "n");
                if paints && !subpaths.is_empty() {
                    let fills = matches!(op.operator.as_str(), "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                    let strokes = matches!(op.operator.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                    let built: Vec<Subpath> = subpaths
                        .into_iter()
                        .map(|sp| Subpath {
                            anchors: sp.anchors,
                            closed: sp.closed || force_closed || is_close_stroke,
                        })
                        .collect();
                    out.push(Recovered {
                        kind: ObjectKind::Path(PathData::from_subpaths(built)),
                        appearance: Appearance {
                            fill: fills.then_some(gs.fill).flatten().map(Paint::Solid).unwrap_or(Paint::None),
                            stroke: strokes.then_some(gs.stroke).flatten().map(Paint::Solid).unwrap_or(Paint::None),
                            stroke_width: gs.line_width,
                            ..Appearance::default()
                        },
                    });
                }
                subpaths = Vec::new();
            }
            "W" | "W*" => {
                // Clipping isn't modeled — the `n` (or paint op) that
                // normally follows still runs and just paints/discards
                // the path as usual, unclipped.
            }
            "q" => stack.push(gs.clone()),
            "Q" => {
                if let Some(prev) = stack.pop() {
                    gs = prev;
                }
            }
            "cm" if a.len() >= 6 => {
                let m = Affine::new([num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3]), num(&a[4]), num(&a[5])]);
                gs.ctm *= m;
            }
            "w" if !a.is_empty() => gs.line_width = num(&a[0]),
            "rg" if a.len() >= 3 => gs.fill = Some(Color::rgb(num(&a[0]) as f32, num(&a[1]) as f32, num(&a[2]) as f32)),
            "RG" if a.len() >= 3 => gs.stroke = Some(Color::rgb(num(&a[0]) as f32, num(&a[1]) as f32, num(&a[2]) as f32)),
            "g" if !a.is_empty() => {
                let v = num(&a[0]) as f32;
                gs.fill = Some(Color::rgb(v, v, v));
            }
            "G" if !a.is_empty() => {
                let v = num(&a[0]) as f32;
                gs.stroke = Some(Color::rgb(v, v, v));
            }
            "k" if a.len() >= 4 => gs.fill = Some(cmyk(&a[0], &a[1], &a[2], &a[3])),
            "K" if a.len() >= 4 => gs.stroke = Some(cmyk(&a[0], &a[1], &a[2], &a[3])),
            "sc" | "scn" => {
                if let Some(c) = scn_color(a) {
                    gs.fill = Some(c);
                }
            }
            "SC" | "SCN" => {
                if let Some(c) = scn_color(a) {
                    gs.stroke = Some(c);
                }
            }
            "Do" if a.len() == 1 => {
                if let PdfObject::Name(name) = &a[0] {
                    run_xobject(pdf, name, &gs, resources, out, depth);
                }
            }
            // Text, shading patterns, marked content, and everything else
            // are silently unsupported (see the module TODO for text).
            _ => {}
        }
    }
}

/// `Do /Name` for a Form XObject: recurse into its own content stream
/// with `/Matrix` and its own (or the caller's) `/Resources`. An Image
/// XObject is skipped — raster import isn't in scope here.
fn run_xobject(
    pdf: &PdfDocument,
    name: &[u8],
    gs: &GState,
    resources: &Dictionary,
    out: &mut Vec<Recovered>,
    depth: u32,
) {
    let Ok(xobjects) = resources.get(b"XObject").and_then(PdfObject::as_dict) else {
        return;
    };
    let Ok(xobj_ref) = xobjects.get(name) else { return };
    let Ok(id) = xobj_ref.as_reference() else { return };
    let Ok(obj) = pdf.get_object(id) else { return };
    let Ok(stream) = obj.as_stream() else { return };
    let is_form = stream
        .dict
        .get(b"Subtype")
        .and_then(PdfObject::as_name)
        .is_ok_and(|s| s == b"Form");
    if !is_form {
        return;
    }
    let matrix = stream
        .dict
        .get(b"Matrix")
        .and_then(PdfObject::as_array)
        .map(|arr| {
            Affine::new([num(&arr[0]), num(&arr[1]), num(&arr[2]), num(&arr[3]), num(&arr[4]), num(&arr[5])])
        })
        .unwrap_or(Affine::IDENTITY);
    let form_resources = stream
        .dict
        .get(b"Resources")
        .and_then(PdfObject::as_dict)
        .cloned()
        .unwrap_or_else(|_| resources.clone());
    let Ok(content_bytes) = stream.decompressed_content() else { return };
    let Ok(content) = lopdf::content::Content::decode(&content_bytes) else { return };
    let mut inner = gs.clone();
    inner.ctm *= matrix;
    interpret(pdf, &content.operations, inner, &form_resources, out, depth + 1);
}

fn cmyk(c: &PdfObject, m: &PdfObject, y: &PdfObject, k: &PdfObject) -> Color {
    let (c, m, y, k) = (num(c), num(m), num(y), num(k));
    Color::rgb(
        ((1.0 - c) * (1.0 - k)) as f32,
        ((1.0 - m) * (1.0 - k)) as f32,
        ((1.0 - y) * (1.0 - k)) as f32,
    )
}

/// Best-effort `sc`/`scn`/`SC`/`SCN`: only plain numeric operands (a
/// gray, RGB, or CMYK value in whatever color space is current) are
/// understood. A pattern/named-colorspace operand (its last operand is a
/// `/Name`) is left as-is — resolving those needs an actual color-space
/// table lookup, which is out of scope.
fn scn_color(operands: &[PdfObject]) -> Option<Color> {
    let nums: Vec<f64> = operands.iter().take_while(|o| matches!(o, PdfObject::Integer(_) | PdfObject::Real(_))).map(num).collect();
    match nums.len() {
        1 => Some(Color::rgb(nums[0] as f32, nums[0] as f32, nums[0] as f32)),
        3 => Some(Color::rgb(nums[0] as f32, nums[1] as f32, nums[2] as f32)),
        4 => Some(cmyk(
            &PdfObject::Real(nums[0] as f32),
            &PdfObject::Real(nums[1] as f32),
            &PdfObject::Real(nums[2] as f32),
            &PdfObject::Real(nums[3] as f32),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real `.ai` files checked into the repo (branding source art) —
    /// the best available end-to-end fixtures, since nobody publishes a
    /// small canonical `.ai` test corpus. Skipped (not failed) if a repo
    /// layout change ever moves them, so this doesn't become spuriously
    /// flaky in some other checkout shape.
    fn fixture(rel: &str) -> Option<Vec<u8>> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../").join(rel);
        std::fs::read(path).ok()
    }

    #[test]
    fn imports_real_ai_files_with_at_least_one_artboard_and_object() {
        for rel in [
            "branding/WebArt/website-assets.ai",
            "branding/NewDocument/landing.ai",
            "branding/ToolIcons/icons.ai",
        ] {
            let Some(bytes) = fixture(rel) else { continue };
            let doc = import_ai(&bytes).unwrap_or_else(|e| panic!("{rel}: {e}"));
            assert!(!doc.artboards().is_empty(), "{rel}: no artboards recovered");
            assert!(doc.objects().next().is_some(), "{rel}: no objects recovered");
        }
    }
}
