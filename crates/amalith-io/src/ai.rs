//! Reading Illustrator's PDF-compatible layer of an `.ai` file.
//!
//! Since Illustrator 9 / CS, every `.ai` file *is* a valid PDF with an
//! additional, undocumented, Adobe-only "native object" stream layered on
//! top for Illustrator's own internal use (live effects, symbols, exact
//! layer structure, editable blends). That stream has never been
//! published, and nobody outside Adobe has fully reverse-engineered it —
//! this reads the PDF layer instead, which is what every other
//! non-Adobe `.ai` reader actually does. That recovers real vector
//! geometry, fills/strokes, raster images, simple gradients, opacity, and
//! clipping masks, and artboards (one per PDF page) reasonably well; it
//! does *not* recover live effects or editable blends (Illustrator
//! already flattened those to their final static shapes before writing
//! the PDF layer), or text (see the module-level TODO below).
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
//!
//! Clipping (`W`/`W*`) is recovered as a real
//! [`amalith_core::object::GroupData::clip`] group: the path marked by
//! `W`/`W*` becomes the group's clip-mask child, and everything painted
//! between that and the matching `Q` (see [`ClipEvent`]/[`apply_clips`])
//! becomes the rest of the group's children. This assumes clip scopes are
//! well-nested via ordinary `q`/`Q` pairs (true for anything Illustrator
//! itself writes) and that a real intersection with an already-active
//! *outer* clip isn't needed — setting a new clip while one is already
//! active replaces it for grouping purposes rather than intersecting, so
//! a doubly-clipped region can render slightly larger than the original
//! (the outer clip's shape, not the true intersection).
//!
//! Gradients recovered from a PDF shading pattern (see
//! [`resolve_shading_gradient`]) keep the *colors* right but not
//! necessarily the *geometry*: Amalith stores gradient axes in
//! object-bounding-box unit space, while a PDF axial/radial shading's
//! `/Coords` live in "pattern space", anchored to the default coordinate
//! system of whichever content stream the pattern is used in — exactly
//! reconstructing that (especially inside a nested Form XObject) is out
//! of scope, so a recovered gradient keeps Amalith's own default axis
//! orientation instead of the original's exact angle/position.
//!
//! HIGHLY EXPERIMENTAL addition: repeated `Do` placements of the same
//! Form XObject are cross-checked against `ai_private_data`'s best-effort
//! read of Illustrator's undocumented native-object stream, and promoted
//! to real named `SymbolDefinition`/`SymbolData` when that check confirms
//! them. See `ai_private_data.rs`'s module doc for exactly how fragile
//! that is; any mismatch silently falls back to the plain flattened
//! geometry below, per this module's usual best-effort philosophy.

use crate::ai_private_data::{self, PrivateSymbols};
use crate::AssetStore;
use amalith_core::{
    Affine, Anchor, Appearance, AppearanceItem, Artboard, ArtboardId, Asset, AssetId, AssetKind, BlendMode,
    Color, Document, Gradient, GradientId, GradientKind, GradientStop, GroupData, ImageData,
    LayerId, Object, ObjectId, ObjectKind, ObjectParent, Paint, PathData, Point, Rect, StrokeStyle,
    Subpath, SymbolData, SymbolDefinition, SymbolId,
};
use lopdf::content::Operation;
use lopdf::{Dictionary, Document as PdfDocument, Object as PdfObject, ObjectId as PdfId};
use std::collections::HashMap;
use std::rc::Rc;
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
/// laid out left to right, and one layer holding every recovered object —
/// plus an [`AssetStore`] holding any raster images it recovered.
pub fn import_ai(bytes: &[u8]) -> Result<(Document, AssetStore), AiError> {
    let pdf = PdfDocument::load_mem(bytes)?;
    let pages = pdf.get_pages();
    if pages.is_empty() {
        return Err(AiError::NoPages);
    }

    let mut document = Document::new("Imported");
    let mut assets = AssetStore::new();
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
        let mut events = Vec::new();
        let mut images = Vec::new();
        let mut gradients = Vec::new();
        let mut clip_events = Vec::new();
        let mut sink =
            Sink { out: &mut out, events: &mut events, images: &mut images, gradients: &mut gradients, clip_events: &mut clip_events };
        let gs = GState { ctm: base, ..GState::default() };
        interpret(&pdf, &content.operations, gs, &resources, &mut sink, 0);

        for (_, gradient) in gradients {
            document.add_gradient(gradient);
        }
        for (asset_id, bytes, ext) in images {
            let container_path = format!("images/ai-import-{asset_id}.{ext}");
            assets.insert(container_path.clone(), bytes);
            document.add_asset(Asset::embedded(asset_id, "Image", AssetKind::Image, container_path));
        }
        let (out, events) = apply_clips(&mut document, layer_id, out, clip_events, events, artboard_rect);

        // HIGHLY EXPERIMENTAL: see the module doc and `ai_private_data.rs`.
        // `confirmed` is `None` the moment anything about this looks
        // untrustworthy, and `emit_objects` falls back to plain flattened
        // geometry in that case — exactly today's behavior. Skip the
        // (potentially gigabytes-decompressing) private-data read
        // entirely unless there's at least one repeated `Do` placement
        // for it to possibly confirm — most real Illustrator exports
        // flatten symbol instances as plain inline path operators with
        // no XObject reuse at all, so this is the common case.
        let has_repeated_xobject = {
            let mut counts: HashMap<PdfId, usize> = HashMap::new();
            for ev in &events {
                *counts.entry(ev.pdf_id).or_default() += 1;
            }
            counts.values().any(|&c| c >= 2)
        };
        let symbols = has_repeated_xobject.then(|| ai_private_data::extract(&pdf, page_id)).flatten();
        let confirmed = symbols
            .as_ref()
            .and_then(|s| confirm_symbols(&events, s))
            .map(|art_numbers| (art_numbers, symbols.unwrap()));

        next_index = emit_objects(&mut document, layer_id, out, events, confirmed, next_index);
    }
    Ok((document, assets))
}

/// Plain fallback: one `Object` per recovered path, in paint order —
/// exactly what `import_ai` did before experimental symbol recovery
/// existed.
fn insert_recovered(document: &mut Document, layer_id: LayerId, rec: Recovered, next_index: usize) -> usize {
    let id = rec.preserve_id.unwrap_or_else(ObjectId::new);
    let mut obj = Object::new(id, ObjectParent::Layer(layer_id), rec.kind);
    obj.appearance = rec.appearance;
    if document.insert_object(obj, next_index).is_ok() {
        next_index + 1
    } else {
        next_index
    }
}

/// Materializes every closed [`ClipEvent`] into a real clip [`GroupData`]:
/// the mask becomes a plain `Path` child, everything in the event's range
/// becomes the rest of the children (already-collapsed inner clip groups
/// included, since `clip_events` closes innermost-first — see the module
/// doc), and the whole range collapses to that one group in the returned
/// `out`. `events` (the symbol-confirmation `Do` placements) are remapped
/// onto the new, shorter index space; a `DoEvent` whose range only
/// *partially* survived (some but not all of its flattened paths got
/// absorbed into a clip group — structurally shouldn't happen given both
/// kinds of range come from well-nested `q`/`Q` scopes, but this is
/// defensive) is dropped rather than kept with a wrong range, consistent
/// with disabling the experimental symbol path whenever something looks
/// inconsistent.
fn apply_clips(
    document: &mut Document,
    layer_id: LayerId,
    out: Vec<Recovered>,
    clip_events: Vec<ClipEvent>,
    events: Vec<DoEvent>,
    artboard_rect: Rect,
) -> (Vec<Recovered>, Vec<DoEvent>) {
    if clip_events.is_empty() {
        return (out, events);
    }

    let mut slots: Vec<Option<Recovered>> = out.into_iter().map(Some).collect();
    for clip in clip_events {
        if clip.range.is_empty() {
            // A clip was established but nothing was painted under it
            // before the matching `Q` — nothing to group, so there's
            // nothing to do here.
            continue;
        }
        // Illustrator (like most PDF writers) routinely wraps an entire
        // page's content in a clip matching the page/artboard rect
        // itself, purely so nothing bleeds past the page edge — boilerplate
        // from the output format's own mechanics, not a real, user-made
        // clipping mask (Illustrator's own Layers panel never shows this
        // as a clip group). Recovering it as one would wrap the *whole*
        // imported document in a single meaningless clip group — reported
        // as "the entire thing wrapped in a clipping mask". Skip it: treat
        // the range as if it were never clipped at all.
        if clip_covers_rect(&clip.mask, artboard_rect) {
            continue;
        }
        let group_id = ObjectId::new();
        // `insert_object` validates an `ObjectParent::Group` target by
        // looking the group up in the arena, so the group has to exist
        // *before* any child can reference it — reserve it here (with a
        // throwaway `ObjectParent::Layer` placement, since `insert_object`
        // always needs *some* valid parent) and let `insert_object`'s own
        // parent-children bookkeeping build up its real children as they
        // go in. `remove_object` then pulls the fully-populated group
        // back out as a plain value, ready for `emit_objects` to insert
        // for real at this range's actual final position — exactly like
        // any other recovered object, just built a little differently.
        let placeholder = Object::new(group_id, ObjectParent::Layer(layer_id), ObjectKind::Group(GroupData::default()));
        document.insert_object(placeholder, 0).expect("layer exists");

        let mask_id = ObjectId::new();
        let mut mask_obj = Object::new(mask_id, ObjectParent::Group(group_id), ObjectKind::Path(clip.mask));
        mask_obj.appearance = Appearance { items: Vec::new(), opacity: 1.0 };
        document.insert_object(mask_obj, 0).expect("group just registered");

        for (child_index, slot) in slots[clip.range.clone()].iter_mut().enumerate() {
            let Some(rec) = slot.take() else { continue };
            let child_id = ObjectId::new();
            let mut obj = Object::new(child_id, ObjectParent::Group(group_id), rec.kind);
            obj.appearance = rec.appearance;
            document.insert_object(obj, child_index + 1).expect("group just registered");
        }

        let (mut group_obj, _) = document.remove_object(group_id).expect("just inserted above");
        if let ObjectKind::Group(g) = &mut group_obj.kind {
            g.clip = Some(mask_id);
        }
        slots[clip.range.start] =
            Some(Recovered { kind: group_obj.kind, appearance: group_obj.appearance, preserve_id: Some(group_obj.id) });
        for slot in &mut slots[clip.range.start + 1..clip.range.end] {
            *slot = None;
        }
    }

    let mut remap: Vec<Option<usize>> = vec![None; slots.len()];
    let mut new_out = Vec::with_capacity(slots.len());
    for (i, slot) in slots.into_iter().enumerate() {
        if let Some(rec) = slot {
            remap[i] = Some(new_out.len());
            new_out.push(rec);
        }
    }

    let new_events = events
        .into_iter()
        .filter_map(|ev| {
            let new_start = remap[ev.range.start]?;
            let new_end = remap[ev.range.end - 1]? + 1;
            (new_end - new_start == ev.range.len()).then_some(DoEvent { range: new_start..new_end, ..ev })
        })
        .collect();

    (new_out, new_events)
}

/// Whether `mask`'s bounds cover all of `rect` (with a small tolerance
/// for floating-point noise) — the signature of a page/artboard-bounding
/// "don't bleed past the edge" clip rather than a real, deliberately
/// smaller user-made clipping mask. Also true, harmlessly, for a mask
/// that's larger than the artboard: clipping to something bigger than
/// what's visible changes nothing observable.
fn clip_covers_rect(mask: &PathData, rect: Rect) -> bool {
    const TOL: f64 = 1.0;
    let b = mask.local_bounds();
    b.x0 <= rect.x0 + TOL && b.y0 <= rect.y0 + TOL && b.x1 >= rect.x1 - TOL && b.y1 >= rect.y1 - TOL
}

/// HIGHLY EXPERIMENTAL. Cross-checks the private-data stream's ordered
/// `...Instance` tags against every `Do` placement this page made of a
/// repeated Form XObject, in encounter order. Returns a confirmed
/// `PdfId -> art number` map only when:
///
/// - the two sequences have exactly the same length, and
/// - every placement of a given `PdfId` maps to the *same* art number.
///
/// Either check failing means this page's repeated XObjects don't line
/// up with what the private-data stream claims — a different Illustrator
/// version, a `Do` this parser doesn't handle the same way the private
/// data does, or this module's tag grammar just being wrong for this
/// file — so the whole correlation is discarded rather than trusted
/// partially. See `ai_private_data.rs`'s module doc for why this
/// (reasonable but unverified) ordering assumption is the best available
/// without a real spec.
fn confirm_symbols(events: &[DoEvent], symbols: &PrivateSymbols) -> Option<HashMap<PdfId, u32>> {
    let mut counts: HashMap<PdfId, usize> = HashMap::new();
    for ev in events {
        *counts.entry(ev.pdf_id).or_default() += 1;
    }
    let repeated: Vec<&DoEvent> = events.iter().filter(|ev| counts[&ev.pdf_id] >= 2).collect();

    let instance_numbers: Vec<u32> =
        symbols.tags.iter().filter(|t| t.instance).map(|t| t.art_number).collect();
    if repeated.is_empty() || repeated.len() != instance_numbers.len() {
        return None;
    }

    let mut confirmed: HashMap<PdfId, u32> = HashMap::new();
    for (ev, &art_number) in repeated.iter().zip(&instance_numbers) {
        match confirmed.get(&ev.pdf_id) {
            Some(&existing) if existing != art_number => return None,
            _ => {
                confirmed.insert(ev.pdf_id, art_number);
            }
        }
    }
    Some(confirmed)
}

/// Walks `out` in paint order, collapsing every confirmed symbol
/// placement's flattened paths into one `SymbolDefinition` (built once,
/// from the first placement, un-transformed back into local space) plus
/// one `ObjectKind::Symbol` instance per placement; everything else is
/// inserted exactly as `insert_recovered` always has been.
fn emit_objects(
    document: &mut Document,
    layer_id: LayerId,
    out: Vec<Recovered>,
    events: Vec<DoEvent>,
    confirmed: Option<(HashMap<PdfId, u32>, PrivateSymbols)>,
    mut next_index: usize,
) -> usize {
    let Some((art_numbers, symbols)) = confirmed else {
        for rec in out {
            next_index = insert_recovered(document, layer_id, rec, next_index);
        }
        return next_index;
    };

    let mut range_starts: HashMap<usize, usize> = HashMap::new();
    for (ei, ev) in events.iter().enumerate() {
        if art_numbers.contains_key(&ev.pdf_id) && !ev.range.is_empty() {
            range_starts.insert(ev.range.start, ei);
        }
    }

    let mut slots: Vec<Option<Recovered>> = out.into_iter().map(Some).collect();
    let mut definitions: HashMap<PdfId, (SymbolId, Rect)> = HashMap::new();
    let mut i = 0;
    while i < slots.len() {
        let Some(&ei) = range_starts.get(&i) else {
            if let Some(rec) = slots[i].take() {
                next_index = insert_recovered(document, layer_id, rec, next_index);
            }
            i += 1;
            continue;
        };

        let ev = &events[ei];
        let art_number = art_numbers[&ev.pdf_id];
        let (symbol_id, local_bounds) = if let Some(&existing) = definitions.get(&ev.pdf_id) {
            existing
        } else {
            let local = ev.transform.inverse();
            let symbol_id = SymbolId::new();
            let name = symbols
                .name_for(art_number)
                .map(str::to_string)
                .unwrap_or_else(|| format!("Symbol {}", art_number + 1));
            // Register the (empty) definition *before* inserting any
            // children: `Document::insert_object` validates a
            // `ObjectParent::Symbol` target by looking the symbol up, so
            // a child inserted before its definition exists would be
            // silently rejected. `insert_object` maintains
            // `SymbolDefinition::children` itself as each child goes in,
            // so there's no separate list to build here.
            document.add_symbol(SymbolDefinition { id: symbol_id, name, children: Vec::new() });
            let mut bounds: Option<Rect> = None;
            for (child_index, slot) in slots[ev.range.clone()].iter_mut().enumerate() {
                let Some(rec) = slot.take() else { continue };
                let preserve_id = rec.preserve_id;
                let local_kind = transform_kind(rec.kind, local);
                if let Some(b) = kind_bounds(&local_kind) {
                    bounds = amalith_core::geom::union_bounds(bounds, Some(b));
                }
                let child_id = preserve_id.unwrap_or_else(ObjectId::new);
                let mut obj = Object::new(child_id, ObjectParent::Symbol(symbol_id), local_kind);
                obj.appearance = rec.appearance;
                document.insert_object(obj, child_index).expect("symbol just registered");
            }
            let local_bounds = bounds.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
            definitions.insert(ev.pdf_id, (symbol_id, local_bounds));
            (symbol_id, local_bounds)
        };

        let instance_id = ObjectId::new();
        let mut obj = Object::new(
            instance_id,
            ObjectParent::Layer(layer_id),
            ObjectKind::Symbol(SymbolData { definition: symbol_id, local_bounds }),
        );
        obj.transform = ev.transform;
        if document.insert_object(obj, next_index).is_ok() {
            next_index += 1;
        }
        i = ev.range.end.max(i + 1);
    }
    next_index
}

fn transform_path_data(path: &PathData, t: Affine) -> PathData {
    let mapped: Vec<Subpath> = path
        .subpaths()
        .iter()
        .map(|sp| Subpath {
            anchors: sp
                .anchors
                .iter()
                .map(|a| Anchor {
                    point: t * a.point,
                    handle_in: a.handle_in.map(|p| t * p),
                    handle_out: a.handle_out.map(|p| t * p),
                    mode: a.mode,
                })
                .collect(),
            closed: sp.closed,
        })
        .collect();
    PathData::from_subpaths(mapped)
}

/// Re-expresses `kind`'s geometry under transform `t` — used when moving a
/// symbol placement's flattened content back into the definition's own
/// local space. `ObjectKind::Path` remaps every anchor/handle;
/// `ObjectKind::Image` remaps its `local_bounds`; anything else passes
/// through unchanged (nothing else is ever produced by this module today).
fn transform_kind(kind: ObjectKind, t: Affine) -> ObjectKind {
    match kind {
        ObjectKind::Path(path) => ObjectKind::Path(transform_path_data(&path, t)),
        ObjectKind::Image(img) => ObjectKind::Image(ImageData {
            asset: img.asset,
            local_bounds: amalith_core::geom::transformed_bounds(t, img.local_bounds),
        }),
        other => other,
    }
}

/// Bounding box of `kind`'s own geometry, in whatever space it's
/// currently expressed — used to fold a symbol definition's `local_bounds`
/// together from its children after `transform_kind` moves them into
/// local space.
fn kind_bounds(kind: &ObjectKind) -> Option<Rect> {
    match kind {
        ObjectKind::Path(p) => Some(amalith_core::geom::bez_path_bounds(&p.geometry)),
        ObjectKind::Image(img) => Some(img.local_bounds),
        _ => None,
    }
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
    /// Set instead of `fill`/`stroke` by `scn`/`SCN` when the last operand
    /// is a `/Name` (a Pattern colorspace reference) rather than plain
    /// numeric components. Resolved lazily at paint time — see
    /// `resolve_paint` — since resolving it needs `resources`, which
    /// isn't part of `GState`.
    fill_pattern: Option<Vec<u8>>,
    stroke_pattern: Option<Vec<u8>>,
    line_width: f64,
    /// From the most recently applied `/ExtGState`'s `/ca`/`/CA` (fill and
    /// stroke alpha). Blend mode (`/BM`) isn't tracked — see the module
    /// doc.
    fill_opacity: f32,
    stroke_opacity: f32,
}

impl Default for GState {
    fn default() -> Self {
        Self {
            ctm: Affine::IDENTITY,
            fill: Some(Color::rgb(0.0, 0.0, 0.0)),
            stroke: None,
            fill_pattern: None,
            stroke_pattern: None,
            line_width: 1.0,
            fill_opacity: 1.0,
            stroke_opacity: 1.0,
        }
    }
}

/// One recovered shape, ready to become an `Object` once the caller
/// knows its final id / parent.
struct Recovered {
    kind: ObjectKind,
    appearance: Appearance,
    /// `Some` only for a clip group [`apply_clips`] already materialized:
    /// its children were inserted under this exact id (see
    /// `apply_clips`'s doc comment), so whatever finally inserts this
    /// `Recovered` for real must reuse it rather than minting a fresh one
    /// — otherwise the children's `ObjectParent::Group` would point at an
    /// id nothing in the document actually has. `None` (the common case)
    /// means "fresh object, any id will do."
    preserve_id: Option<ObjectId>,
}

/// One `Do` invocation of a Form XObject, recorded in encounter order.
/// HIGHLY EXPERIMENTAL bookkeeping: exists purely so `confirm_symbols`/
/// `emit_objects` can retroactively decide whether a repeated XObject is
/// a confirmed Symbol — it plays no part in the plain flattened-geometry
/// path, which behaves exactly as it did before this existed.
struct DoEvent {
    pdf_id: PdfId,
    /// The `out` index range this placement flattened into.
    range: std::ops::Range<usize>,
    /// Maps the XObject's own local space (after its `/Matrix`) directly
    /// into this page's space for this specific placement.
    transform: Affine,
}

/// One clip scope closed while interpreting a page: everything painted
/// between the `W`/`W*` that established `mask` and the matching `Q`
/// that ended its graphics-state scope, as an `out` index range —
/// exactly the [`DoEvent`]/symbol-confirmation shape, reused here for
/// clip groups instead. Recorded in closing order, which is always
/// innermost-first (see [`apply_clips`]).
struct ClipEvent {
    range: std::ops::Range<usize>,
    mask: PathData,
}

/// Every accumulator `interpret`/`run_xobject` fill in as they walk a
/// page's (or a nested Form XObject's) content stream. Bundled into one
/// struct instead of positional `&mut` parameters purely to keep those
/// two functions' signatures readable.
struct Sink<'a> {
    out: &'a mut Vec<Recovered>,
    events: &'a mut Vec<DoEvent>,
    /// Recovered raster images, keyed by the `AssetId` already baked into
    /// the matching `Recovered::kind`'s `ObjectKind::Image` — filled in
    /// alongside `out` so paint order is preserved automatically, drained
    /// into the `Document`'s asset pool once the whole page is done.
    images: &'a mut Vec<(AssetId, Vec<u8>, &'static str)>,
    /// Recovered gradients, keyed the same way by the `GradientId` already
    /// baked into whatever `Paint::Gradient` references them.
    gradients: &'a mut Vec<(GradientId, Gradient)>,
    clip_events: &'a mut Vec<ClipEvent>,
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
    sink: &mut Sink,
    depth: u32,
) {
    if depth > 8 {
        return;
    }
    let mut stack: Vec<GState> = Vec::new();
    let mut subpaths: Vec<BuildingSubpath> = Vec::new();
    let mut current: Option<BuildingSubpath> = None;
    // Clip bookkeeping (see `ClipEvent`/`apply_clips`): `W`/`W*` stages a
    // pending mask here; the next paint op (almost always `n`, clip-only)
    // promotes it into `active_clips`, tagged with the `q`-nesting depth
    // it was established at, so the `Q` that returns to that same depth
    // — and only that one — closes it.
    let mut pending_clip: Option<PathData> = None;
    let mut active_clips: Vec<(usize, usize, Rc<PathData>)> = Vec::new();

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
                // A pending clip (from `W`/`W*` on this same path) takes
                // effect now, regardless of whether this op also paints —
                // see the module doc for the "replaces rather than
                // intersects an outer clip" simplification.
                if let Some(mask) = pending_clip.take() {
                    active_clips.push((stack.len(), sink.out.len(), Rc::new(mask)));
                }
                let is_close_stroke = matches!(op.operator.as_str(), "s" | "b" | "b*");
                let paints = !matches!(op.operator.as_str(), "n");
                if paints && !subpaths.is_empty() {
                    let fills = matches!(op.operator.as_str(), "f" | "F" | "f*" | "B" | "B*" | "b" | "b*");
                    let strokes = matches!(op.operator.as_str(), "S" | "s" | "B" | "B*" | "b" | "b*");
                    // Only force a subpath closed here when it's safe to:
                    // a fill-only op (`f`/`F`/`f*`) has no stroke for a
                    // synthetic closing edge to leak into, so marking it
                    // closed is harmless and matches this module's usual
                    // behavior. `s`/`b`/`b*` genuinely close the path
                    // (`is_close_stroke`). But a bare `B`/`B*` fills *and*
                    // strokes with no explicit `h` — per the PDF spec `B`
                    // is just "f then S", and unlike `f`, `S` never closes
                    // on its own. Amalith renders a subpath's fill and
                    // stroke from the same geometry, but the *renderer*
                    // (vello) still treats them differently per draw call:
                    // it always closes a subpath for filling regardless of
                    // an explicit close, but only closes it for stroking
                    // when one was actually recorded. So leaving this
                    // subpath's own `closed` at its true state (not forced)
                    // still fills correctly while keeping the stroke
                    // faithful to what was actually drawn (no phantom edge
                    // closing an open "L"-bracket or "V"-chevron into a
                    // triangle) — and keeps it the single open-path object
                    // Illustrator itself models it as, rather than two.
                    let force_closed = fills && !strokes;
                    let built: Vec<Subpath> = subpaths
                        .into_iter()
                        .map(|sp| Subpath {
                            anchors: sp.anchors,
                            closed: sp.closed || force_closed || is_close_stroke,
                        })
                        .collect();
                    let fill_paint = if fills {
                        resolve_paint(pdf, resources, &gs.fill_pattern, gs.fill, sink.gradients)
                    } else {
                        Paint::None
                    };
                    let stroke_paint = if strokes {
                        resolve_paint(pdf, resources, &gs.stroke_pattern, gs.stroke, sink.gradients)
                    } else {
                        Paint::None
                    };
                    sink.out.push(Recovered {
                        kind: ObjectKind::Path(PathData::from_subpaths(built)),
                        appearance: Appearance {
                            items: vec![
                                AppearanceItem::Fill {
                                    paint: fill_paint,
                                    opacity: gs.fill_opacity,
                                    visible: true,
                                    effects: Vec::new(),
                                    blend_mode: BlendMode::Normal,
                                },
                                AppearanceItem::Stroke {
                                    paint: stroke_paint,
                                    width: gs.line_width,
                                    style: StrokeStyle::default(),
                                    opacity: gs.stroke_opacity,
                                    visible: true,
                                    effects: Vec::new(),
                                    blend_mode: BlendMode::Normal,
                                },
                            ],
                            opacity: 1.0,
                        },
                        preserve_id: None,
                    });
                }
                subpaths = Vec::new();
            }
            "W" | "W*" => {
                // Stages the current path as the pending clip mask; it
                // becomes active once the next painting operator (below)
                // runs. `finish_current` (not clearing `subpaths`) mirrors
                // exactly what the paint-op arm does before consuming
                // them for real, so the snapshot sees the complete path.
                finish_current(&mut current, &mut subpaths);
                if !subpaths.is_empty() {
                    let snapshot: Vec<Subpath> = subpaths
                        .iter()
                        .map(|sp| Subpath {
                            anchors: sp.anchors.iter().map(|a| Anchor { point: a.point, handle_in: a.handle_in, handle_out: a.handle_out, mode: a.mode }).collect(),
                            closed: sp.closed,
                        })
                        .collect();
                    pending_clip = Some(PathData::from_subpaths(snapshot));
                }
            }
            "q" => stack.push(gs.clone()),
            "Q" => {
                if !stack.is_empty() {
                    // Close every clip scope established at the depth
                    // we're about to return from — see `active_clips`'s
                    // doc comment. Almost always at most one.
                    while let Some(&(depth, _, _)) = active_clips.last() {
                        if depth != stack.len() {
                            break;
                        }
                        let (_, start, mask) = active_clips.pop().unwrap();
                        sink.clip_events.push(ClipEvent { range: start..sink.out.len(), mask: (*mask).clone() });
                    }
                    gs = stack.pop().unwrap();
                }
            }
            "cm" if a.len() >= 6 => {
                let m = Affine::new([num(&a[0]), num(&a[1]), num(&a[2]), num(&a[3]), num(&a[4]), num(&a[5])]);
                gs.ctm *= m;
            }
            "w" if !a.is_empty() => gs.line_width = num(&a[0]),
            "rg" if a.len() >= 3 => {
                gs.fill = Some(Color::rgb(num(&a[0]) as f32, num(&a[1]) as f32, num(&a[2]) as f32));
                gs.fill_pattern = None;
            }
            "RG" if a.len() >= 3 => {
                gs.stroke = Some(Color::rgb(num(&a[0]) as f32, num(&a[1]) as f32, num(&a[2]) as f32));
                gs.stroke_pattern = None;
            }
            "g" if !a.is_empty() => {
                let v = num(&a[0]) as f32;
                gs.fill = Some(Color::rgb(v, v, v));
                gs.fill_pattern = None;
            }
            "G" if !a.is_empty() => {
                let v = num(&a[0]) as f32;
                gs.stroke = Some(Color::rgb(v, v, v));
                gs.stroke_pattern = None;
            }
            "k" if a.len() >= 4 => {
                gs.fill = Some(cmyk(&a[0], &a[1], &a[2], &a[3]));
                gs.fill_pattern = None;
            }
            "K" if a.len() >= 4 => {
                gs.stroke = Some(cmyk(&a[0], &a[1], &a[2], &a[3]));
                gs.stroke_pattern = None;
            }
            "sc" | "scn" => {
                if let Some(name) = pattern_name(a) {
                    gs.fill_pattern = Some(name);
                } else if let Some(c) = scn_color(a) {
                    gs.fill = Some(c);
                    gs.fill_pattern = None;
                }
            }
            "SC" | "SCN" => {
                if let Some(name) = pattern_name(a) {
                    gs.stroke_pattern = Some(name);
                } else if let Some(c) = scn_color(a) {
                    gs.stroke = Some(c);
                    gs.stroke_pattern = None;
                }
            }
            "gs" if a.len() == 1 => {
                if let PdfObject::Name(name) = &a[0] {
                    apply_ext_gstate(resources, name, &mut gs);
                }
            }
            "Do" if a.len() == 1 => {
                if let PdfObject::Name(name) = &a[0] {
                    let before = sink.out.len();
                    if let Some((pdf_id, transform)) = run_xobject(pdf, name, &gs, resources, sink, depth) {
                        if sink.out.len() > before {
                            sink.events.push(DoEvent { pdf_id, range: before..sink.out.len(), transform });
                        }
                    }
                }
            }
            // Text, shading patterns used as fills without paint (rare),
            // marked content, and everything else are silently
            // unsupported (see the module TODO for text).
            _ => {}
        }
    }
}

/// `Do /Name` for a Form or Image XObject.
///
/// A Form XObject recurses into its own content stream with `/Matrix`
/// and its own (or the caller's) `/Resources`, returning the XObject's
/// `PdfId` and this placement's absolute transform (page space, after
/// applying both the outer CTM and the XObject's own `/Matrix`) purely
/// for `interpret`'s `DoEvent` bookkeeping — see the module doc's note on
/// experimental symbol recovery.
///
/// An Image XObject is recovered best-effort via [`recover_image`] and
/// pushed straight into `sink.out`/`sink.images`; it never produces a
/// `DoEvent` (images aren't symbol candidates), so this always returns
/// `None` for that case.
fn run_xobject(pdf: &PdfDocument, name: &[u8], gs: &GState, resources: &Dictionary, sink: &mut Sink, depth: u32) -> Option<(PdfId, Affine)> {
    let xobjects = resources.get(b"XObject").and_then(PdfObject::as_dict).ok()?;
    let xobj_ref = xobjects.get(name).ok()?;
    let id = xobj_ref.as_reference().ok()?;
    let obj = pdf.get_object(id).ok()?;
    let stream = obj.as_stream().ok()?;
    let subtype = stream.dict.get(b"Subtype").and_then(PdfObject::as_name).unwrap_or(b"");

    if subtype == b"Image" {
        if let Some((bytes, ext)) = recover_image(pdf, stream) {
            let asset_id = AssetId::new();
            let rect = amalith_core::geom::transformed_bounds(gs.ctm, Rect::new(0.0, 0.0, 1.0, 1.0));
            sink.out.push(Recovered {
                kind: ObjectKind::Image(ImageData { asset: asset_id, local_bounds: rect }),
                appearance: Appearance { items: Vec::new(), opacity: 1.0 },
                preserve_id: None,
            });
            sink.images.push((asset_id, bytes, ext));
        }
        return None;
    }
    if subtype != b"Form" {
        return None;
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
    let content_bytes = stream.decompressed_content().ok()?;
    let content = lopdf::content::Content::decode(&content_bytes).ok()?;
    let mut inner = gs.clone();
    inner.ctm *= matrix;
    let transform = inner.ctm;
    interpret(pdf, &content.operations, inner, &form_resources, sink, depth + 1);
    Some((id, transform))
}

/// Best-effort raster recovery for an Image XObject stream. Handles the
/// two common cases — a DCTDecode (JPEG) stream, passed through as-is,
/// and an uncompressed/Flate-decoded 8-bit DeviceGray/RGB/CMYK raster,
/// re-encoded as PNG — and returns `None` for everything else (an
/// indexed color space, 16-bit samples, JPEG2000, CCITT fax, or any
/// `/SMask` alpha channel, which is dropped rather than composited).
fn recover_image(pdf: &PdfDocument, stream: &lopdf::Stream) -> Option<(Vec<u8>, &'static str)> {
    let filters = stream.filters().unwrap_or_default();
    if filters.last().copied() == Some(b"DCTDecode") {
        return Some((stream.content.clone(), "jpg"));
    }
    let width = stream.dict.get(b"Width").and_then(PdfObject::as_i64).ok()? as u32;
    let height = stream.dict.get(b"Height").and_then(PdfObject::as_i64).ok()? as u32;
    let bpc = stream.dict.get(b"BitsPerComponent").and_then(PdfObject::as_i64).unwrap_or(8);
    if bpc != 8 || width == 0 || height == 0 {
        return None;
    }
    let components = color_space_components(pdf, stream.dict.get(b"ColorSpace").ok()?)?;
    let raw = stream.decompressed_content().ok()?;
    let rgb: Vec<u8> = match components {
        1 => raw.iter().flat_map(|&g| [g, g, g]).collect(),
        3 => raw.clone(),
        4 => raw
            .chunks_exact(4)
            .flat_map(|c| {
                let (cc, m, y, k) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0);
                [
                    (255.0 * (1.0 - cc) * (1.0 - k)) as u8,
                    (255.0 * (1.0 - m) * (1.0 - k)) as u8,
                    (255.0 * (1.0 - y) * (1.0 - k)) as u8,
                ]
            })
            .collect(),
        _ => return None,
    };
    if rgb.len() != (width as usize) * (height as usize) * 3 {
        return None;
    }
    let img = image::RgbImage::from_raw(width, height, rgb)?;
    let mut png_bytes = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
        .ok()?;
    Some((png_bytes, "png"))
}

/// Number of color components a PDF `/ColorSpace` value implies, needed
/// to interpret an image's raw decoded samples. Handles the plain-Name
/// device/calibrated spaces directly, and — since Illustrator almost
/// always tags raster art with an embedded ICC profile rather than a bare
/// `/DeviceRGB`/`/DeviceGray`/`/DeviceCMYK` name — the array form
/// `[/ICCBased <profile stream>]` by reading the profile stream's own
/// `/N` (component count), ignoring the actual profile data (no color
/// management, just enough to know RGB vs. Gray vs. CMYK). `/Indexed`,
/// `/Separation`, and `/DeviceN` aren't handled.
fn color_space_components(pdf: &PdfDocument, obj: &PdfObject) -> Option<u32> {
    let resolved;
    let obj = match obj {
        PdfObject::Reference(id) => {
            resolved = pdf.get_object(*id).ok()?;
            &resolved
        }
        other => other,
    };
    if let Ok(name) = obj.as_name() {
        return match name {
            b"DeviceGray" | b"CalGray" => Some(1),
            b"DeviceRGB" | b"CalRGB" | b"Lab" => Some(3),
            b"DeviceCMYK" => Some(4),
            _ => None,
        };
    }
    let arr = obj.as_array().ok()?;
    let family = arr.first()?.as_name().ok()?;
    match family {
        b"ICCBased" => {
            let profile = deref_pdf_dict(pdf, arr.get(1)?)?;
            profile.get(b"N").and_then(PdfObject::as_i64).ok().map(|n| n as u32)
        }
        b"CalRGB" | b"Lab" => Some(3),
        b"CalGray" => Some(1),
        _ => None,
    }
}

/// Reads `/ca`/`/CA` (fill/stroke alpha) from the `/ExtGState` resource
/// named `name`, applying them to `gs`. `/BM` (blend mode) isn't read —
/// see the module doc.
fn apply_ext_gstate(resources: &Dictionary, name: &[u8], gs: &mut GState) {
    let Ok(states) = resources.get(b"ExtGState").and_then(PdfObject::as_dict) else { return };
    let Ok(state) = states.get(name).and_then(PdfObject::as_dict) else { return };
    if let Ok(ca) = state.get(b"ca").map(num) {
        gs.fill_opacity = ca as f32;
    }
    if let Ok(ca) = state.get(b"CA").map(num) {
        gs.stroke_opacity = ca as f32;
    }
}

fn cmyk(c: &PdfObject, m: &PdfObject, y: &PdfObject, k: &PdfObject) -> Color {
    let (c, m, y, k) = (num(c), num(m), num(y), num(k));
    Color::rgb(
        ((1.0 - c) * (1.0 - k)) as f32,
        ((1.0 - m) * (1.0 - k)) as f32,
        ((1.0 - y) * (1.0 - k)) as f32,
    )
}

fn color_from_components(nums: &[f64]) -> Option<Color> {
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

/// Best-effort `sc`/`scn`/`SC`/`SCN`: only plain numeric operands (a
/// gray, RGB, or CMYK value in whatever color space is current) are
/// understood here — a pattern/named-colorspace operand is handled
/// separately by `pattern_name`/`resolve_paint`.
fn scn_color(operands: &[PdfObject]) -> Option<Color> {
    let nums: Vec<f64> = operands.iter().take_while(|o| matches!(o, PdfObject::Integer(_) | PdfObject::Real(_))).map(num).collect();
    color_from_components(&nums)
}

/// `scn`/`SCN`'s trailing `/Name` operand, present when the current
/// colorspace is `/Pattern` (colored or uncolored tiling, or a shading
/// pattern) rather than a plain numeric color.
fn pattern_name(operands: &[PdfObject]) -> Option<Vec<u8>> {
    match operands.last()? {
        PdfObject::Name(n) => Some(n.clone()),
        _ => None,
    }
}

/// Resolves a fill/stroke's actual `Paint`: a pending pattern name (from
/// `scn`/`SCN`) is looked up and, if it turns out to be a shading pattern
/// this module understands, registered into `gradients` as a real
/// `Paint::Gradient`; otherwise (a tiling pattern, an unsupported shading
/// type, or no pattern at all) falls back to the last plain color, same
/// as before pattern support existed.
fn resolve_paint(
    pdf: &PdfDocument,
    resources: &Dictionary,
    pattern: &Option<Vec<u8>>,
    color: Option<Color>,
    gradients: &mut Vec<(GradientId, Gradient)>,
) -> Paint {
    if let Some(name) = pattern {
        if let Some(gradient) = resolve_shading_gradient(pdf, resources, name) {
            let id = gradient.id;
            gradients.push((id, gradient));
            return Paint::Gradient(id);
        }
    }
    color.map(Paint::Solid).unwrap_or(Paint::None)
}

fn deref_pdf_dict<'p>(pdf: &'p PdfDocument, obj: &'p PdfObject) -> Option<&'p Dictionary> {
    match obj {
        PdfObject::Dictionary(d) => Some(d),
        PdfObject::Stream(s) => Some(&s.dict),
        // A reference can point at either a plain Dictionary or (as with
        // an ICCBased colorspace's profile, or a sampled Type-0 function)
        // a Stream — recurse so both resolve the same way instead of only
        // handling the Dictionary case.
        PdfObject::Reference(id) => deref_pdf_dict(pdf, pdf.get_object(*id).ok()?),
        _ => None,
    }
}

/// Recovers a real gradient (colors right, geometry approximate — see the
/// module doc) from a `/Pattern` resource, if it's a shading pattern
/// (`/PatternType 2`) whose shading is axial or radial (`/ShadingType`
/// `2`/`3`) with a `/Function` this module's [`function_stops`]
/// understands. A tiling pattern (`/PatternType 1`), an unsupported
/// shading type, or an unsupported function all fall back to `None`.
fn resolve_shading_gradient(pdf: &PdfDocument, resources: &Dictionary, pattern_name: &[u8]) -> Option<Gradient> {
    let patterns = resources.get(b"Pattern").and_then(PdfObject::as_dict).ok()?;
    let pattern_ref = patterns.get(pattern_name).ok()?;
    let pattern_dict = deref_pdf_dict(pdf, pattern_ref)?;
    let pattern_type = pattern_dict.get(b"PatternType").and_then(PdfObject::as_i64).unwrap_or(0);
    if pattern_type != 2 {
        return None;
    }
    let shading_obj = pattern_dict.get(b"Shading").ok()?;
    let shading = deref_pdf_dict(pdf, shading_obj)?;
    let shading_type = shading.get(b"ShadingType").and_then(PdfObject::as_i64).ok()?;
    let kind = match shading_type {
        2 => GradientKind::Linear,
        3 => GradientKind::Radial,
        _ => return None,
    };
    let domain = shading
        .get(b"Domain")
        .and_then(PdfObject::as_array)
        .map(|a| (num(&a[0]), num(&a[1])))
        .unwrap_or((0.0, 1.0));
    let function = shading.get(b"Function").ok()?;
    let raw_stops = function_stops(pdf, function, domain)?;
    if raw_stops.len() < 2 {
        return None;
    }
    let span = (domain.1 - domain.0).max(1e-9);
    let id = GradientId::new();
    let mut gradient = match kind {
        GradientKind::Linear => Gradient::linear(id),
        GradientKind::Radial => Gradient::radial(id),
        GradientKind::Freeform => return None,
    };
    gradient.stops = raw_stops
        .into_iter()
        .map(|(t, color)| GradientStop::new(((t as f64 - domain.0) / span) as f32, color))
        .collect();
    Some(gradient)
}

/// Recovers `(offset, color)` stops from a PDF `/Function` entry (a
/// plain dictionary or, for a sampled function, a stream) — Type 2
/// (Exponential) directly as two stops, or Type 3 (Stitching) by walking
/// its sub-functions recursively. Sampled (Type 0) and PostScript
/// calculator (Type 4) functions, and a `/Function` given as an array of
/// single-output functions rather than one multi-output function, aren't
/// handled — `None` in every other case.
fn function_stops(pdf: &PdfDocument, func: &PdfObject, domain: (f64, f64)) -> Option<Vec<(f32, Color)>> {
    let dict = deref_pdf_dict(pdf, func)?;
    let function_type = dict.get(b"FunctionType").and_then(PdfObject::as_i64).ok()?;
    match function_type {
        2 => {
            let comps = |key: &[u8], default: f64| -> Vec<f64> {
                dict.get(key)
                    .and_then(PdfObject::as_array)
                    .map(|a| a.iter().map(num).collect())
                    .unwrap_or_else(|_| vec![default])
            };
            let c0 = color_from_components(&comps(b"C0", 0.0))?;
            let c1 = color_from_components(&comps(b"C1", 1.0))?;
            Some(vec![(domain.0 as f32, c0), (domain.1 as f32, c1)])
        }
        3 => {
            let functions = dict.get(b"Functions").and_then(PdfObject::as_array).ok()?;
            let bounds: Vec<f64> =
                dict.get(b"Bounds").and_then(PdfObject::as_array).map(|a| a.iter().map(num).collect()).unwrap_or_default();
            let mut stops = Vec::new();
            let mut start = domain.0;
            for (i, sub) in functions.iter().enumerate() {
                let end = bounds.get(i).copied().unwrap_or(domain.1);
                stops.extend(function_stops(pdf, sub, (start, end))?);
                start = end;
            }
            Some(stops)
        }
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
            let (doc, _assets) = import_ai(&bytes).unwrap_or_else(|e| panic!("{rel}: {e}"));
            assert!(!doc.artboards().is_empty(), "{rel}: no artboards recovered");
            assert!(doc.objects().next().is_some(), "{rel}: no objects recovered");
        }
    }

    /// Builds a minimal, valid, from-scratch PDF (no `.ai`-specific
    /// wrapping needed — the importer only reads the PDF-compatible
    /// layer, so a plain PDF exercises it identically) with `content` as
    /// its single page's content stream, on a 100x100 `MediaBox`.
    fn pdf_with_content(content: &[u8]) -> Vec<u8> {
        use lopdf::{dictionary, Document as PdfDoc, Stream};

        let mut doc = PdfDoc::with_version("1.5");
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let resources_id = doc.add_object(dictionary! {});
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Contents" => content_id,
            "Resources" => resources_id,
        });
        let pages_id = doc.add_object(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        });
        doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap().set("Parent", pages_id);
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog_id);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn clipped_rect_becomes_a_clip_group_not_two_flat_objects() {
        // `q 0 0 50 50 re W n 1 0 0 rg 0 0 100 100 re f Q` — clips a
        // filled rect to a genuinely smaller one, so this really is a
        // deliberate clip, not a page-bounding one.
        let bytes = pdf_with_content(b"q\n0 0 50 50 re\nW n\n1 0 0 rg\n0 0 100 100 re\nf\nQ");
        let (doc, _assets) = import_ai(&bytes).unwrap();

        let top_level: Vec<_> = doc.objects().filter(|o| matches!(o.parent, ObjectParent::Layer(_))).collect();
        assert_eq!(top_level.len(), 1, "expected exactly one top-level object (the clip group), got {top_level:?}");

        let ObjectKind::Group(group) = &top_level[0].kind else {
            panic!("expected a Group, got {:?}", top_level[0].kind);
        };
        assert_eq!(group.children.len(), 2, "expected mask + one filled rect as children");
        let mask_id = group.clip.expect("group should carry a clip mask");
        assert_eq!(group.children[0], mask_id, "mask should be the first child");

        let mask_obj = doc.object(mask_id).unwrap();
        assert!(matches!(mask_obj.kind, ObjectKind::Path(_)));
        assert_eq!(mask_obj.parent, ObjectParent::Group(top_level[0].id));

        let content_id = group.children[1];
        let content_obj = doc.object(content_id).unwrap();
        assert_eq!(content_obj.parent, ObjectParent::Group(top_level[0].id));
        assert!(
            matches!(&content_obj.appearance.items[0], AppearanceItem::Fill { paint: Paint::Solid(c), .. } if c.r > 0.9 && c.g < 0.1),
            "the filled rect's red fill should have survived being grouped: {:?}",
            content_obj.appearance.items
        );
    }

    /// Regression test for "the entire thing wrapped in a clipping mask"
    /// — Illustrator-style boilerplate clipping the whole page to its
    /// own `MediaBox` (exactly matching it here, `0 0 100 100`, same as
    /// `pdf_with_content`'s fixed page size) must not produce a clip
    /// group at all; the content should land as a plain top-level object.
    #[test]
    fn a_clip_matching_the_whole_page_is_not_recovered_as_a_clip_group() {
        let bytes = pdf_with_content(b"q\n0 0 100 100 re\nW n\n1 0 0 rg\n10 10 20 20 re\nf\nQ");
        let (doc, _assets) = import_ai(&bytes).unwrap();

        let top_level: Vec<_> = doc.objects().filter(|o| matches!(o.parent, ObjectParent::Layer(_))).collect();
        assert_eq!(top_level.len(), 1, "expected exactly one top-level object (the rect, no wrapping group), got {top_level:?}");
        assert!(
            matches!(top_level[0].kind, ObjectKind::Path(_)),
            "expected a plain Path, not a clip Group: {:?}",
            top_level[0].kind
        );
        assert!(matches!(
            &top_level[0].appearance.items[0],
            AppearanceItem::Fill { paint: Paint::Solid(c), .. } if c.r > 0.9 && c.g < 0.1
        ));
    }

    /// A clip mask *larger* than the page (a generous bleed box some
    /// writers emit) is just as meaningless to keep as an exact match —
    /// clipping to something bigger than what's visible changes nothing.
    #[test]
    fn a_clip_larger_than_the_page_is_also_not_recovered_as_a_clip_group() {
        let bytes = pdf_with_content(b"q\n-50 -50 200 200 re\nW n\n1 0 0 rg\n10 10 20 20 re\nf\nQ");
        let (doc, _assets) = import_ai(&bytes).unwrap();

        let top_level: Vec<_> = doc.objects().filter(|o| matches!(o.parent, ObjectParent::Layer(_))).collect();
        assert_eq!(top_level.len(), 1, "expected exactly one top-level object (the rect, no wrapping group), got {top_level:?}");
        assert!(matches!(top_level[0].kind, ObjectKind::Path(_)));
    }

    /// Regression test: an open 3-point path (`m`/`l`/`l`, no `h`) painted
    /// with `B` (fill *and* stroke, no explicit close) must not gain a
    /// phantom closing edge in its *stroke* — only the fill is implicitly
    /// closed per the PDF spec (`B` is "f then S", and `S` never closes
    /// on its own). An "L"-shaped bracket like this is common in
    /// Illustrator-authored diagrams/icons; getting this wrong turns it
    /// into a fully closed triangle outline instead of an open corner.
    #[test]
    fn an_open_path_painted_with_b_keeps_an_open_stroke_and_closed_fill() {
        let bytes = pdf_with_content(b"1 1 1 rg\n0 0 0 RG\n1 w\n0 0 m\n0 -20 l\n20 -20 l\nB");
        let (doc, _assets) = import_ai(&bytes).unwrap();

        // Illustrator models this as a single open Path carrying both a
        // fill and a stroke — not two separate objects.
        let top_level: Vec<_> = doc.objects().filter(|o| matches!(o.parent, ObjectParent::Layer(_))).collect();
        assert_eq!(top_level.len(), 1, "expected one open Path object with both a fill and a stroke, got {top_level:?}");

        let ObjectKind::Path(path) = &top_level[0].kind else { panic!("expected a Path") };
        assert!(
            !path.subpaths()[0].closed,
            "the path must stay open — `B` without an explicit `h` never closes it, only its fill"
        );
        assert_eq!(
            path.subpaths()[0].anchors.len(),
            3,
            "should keep exactly the 3 drawn points, no synthetic closing anchor"
        );
        assert!(matches!(top_level[0].appearance.items[0], AppearanceItem::Fill { .. }));
        assert!(matches!(top_level[0].appearance.items[1], AppearanceItem::Stroke { .. }));
    }
}


