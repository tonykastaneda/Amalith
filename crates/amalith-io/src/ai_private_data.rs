//! HIGHLY EXPERIMENTAL. Best-effort recovery of real Symbol identity from
//! Illustrator's undocumented native "private data" stream.
//!
//! `ai.rs`'s normal import path only reads the PDF-compatible layer of an
//! `.ai` file, which has no concept of Symbols at all — a symbol instance
//! is just a Form XObject placed with `Do`, indistinguishable from any
//! other reused piece of artwork. Real Symbol identity (names, and which
//! placements are instances of which definition) only exists in Adobe's
//! private, unpublished native-object stream, embedded in the PDF as
//! `Page/PieceInfo/Illustrator/Private/AIPrivateData<N>` — a chain of
//! numbered stream objects that must be concatenated in numeric order.
//!
//! **There is no published specification for any of this.** Everything
//! below was derived by manually decompressing one real `.ai` file
//! (Illustrator 24 / "AI5_FileFormat 14.0") and reading the result — not
//! from Adobe documentation, which doesn't exist for this format, and not
//! from a broad survey of files across Illustrator versions. Treat every
//! byte pattern here as unverified beyond that one sample:
//!
//! - The concatenated `AIPrivateData*` bytes begin with the literal ASCII
//!   marker `%AI24_ZStandard_Data`, immediately followed (no separator) by
//!   a raw Zstandard frame. Older Illustrator versions may well use a
//!   different marker/codec (a sibling open-source parser,
//!   `opendesigndev/illustrator-parser-pdfcpu`, handles a zlib variant
//!   too) — unhandled here; [`extract`] simply returns `None` for any
//!   marker other than the one observed.
//! - Once decompressed, the stream is the legacy PostScript/PGF-style
//!   document description (DSC comments), `\r`-delimited rather than
//!   `\n`-delimited.
//! - `%AI24_BeginSymbolList` / `%AI24_EndSymbolList` bracket a plain list
//!   of every symbol's name, one per line as `(Name)`. The list appeared
//!   in *descending* numeric order in the one sample checked (art number
//!   3, 2, 1, 0) — [`PrivateSymbols::name_for`] assumes that always holds
//!   and reverses it to index by ascending art number. Unverified beyond
//!   that one file.
//! - A generic `%_/ArtDictionary : ... %_;` metadata block tags
//!   individual art objects; within one, a line of the exact shape
//!   `%_(ArtNumber__<N>) /UnicodeString (AI19SymbolIDKey) ,` marks that
//!   object as symbol definition `N`'s own template artwork, and
//!   `%_(ArtNumber__<N>Instance) /UnicodeString (AI19SymbolIDKey) ,`
//!   marks a placed instance of definition `N`.
//!
//! `ai.rs` only ever treats a result from here as a *hint*: it cross-checks
//! the instance-tag count against the number of repeated `Do` placements it
//! independently found in the PDF content stream, and only promotes those
//! placements to real `SymbolDefinition`/`SymbolData` when the counts agree
//! and every placement of the same XObject maps to the same art number.
//! Any mismatch — a different Illustrator version, a file this module
//! mis-parses, a document this heuristic doesn't hold for — silently
//! disables symbol recovery for that file; import falls back to today's
//! plain flattened geometry, exactly as if this module didn't exist.

use lopdf::{Dictionary, Document as PdfDocument, Object as PdfObject, ObjectId as PdfId};

/// One `AI19SymbolIDKey` tag found in the private-data stream, in the
/// exact order it appears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SymbolTag {
    pub art_number: u32,
    /// `true` for an `...Instance` tag (a placement); `false` for a bare
    /// `ArtNumber__N` tag (that symbol's own definition template).
    pub instance: bool,
}

/// Everything recovered from one page's private-data stream.
#[derive(Debug, Clone, Default)]
pub(crate) struct PrivateSymbols {
    /// Symbol names indexed by art number (best-effort — see module doc
    /// on the descending-order assumption). A missing/out-of-range index
    /// just means "no confirmed name"; callers must handle that.
    pub names: Vec<String>,
    pub tags: Vec<SymbolTag>,
}

impl PrivateSymbols {
    /// Best-effort name for `art_number`, per the descending-list
    /// assumption documented at the top of this module. `None` if the
    /// list was empty/shorter than expected — callers fall back to a
    /// generic name in that case, never a panic or a guess past the end
    /// of the list.
    pub fn name_for(&self, art_number: u32) -> Option<&str> {
        let len = self.names.len();
        let idx = len.checked_sub(1)?.checked_sub(art_number as usize)?;
        self.names.get(idx).map(String::as_str)
    }
}

const ZSTD_MARKER: &[u8] = b"%AI24_ZStandard_Data";
const BEGIN_SYMBOL_LIST: &[u8] = b"%AI24_BeginSymbolList";
const END_SYMBOL_LIST: &[u8] = b"%AI24_EndSymbolList";
const SYMBOL_ID_KEY: &[u8] = b"AI19SymbolIDKey";
const ART_NUMBER_PREFIX: &[u8] = b"ArtNumber__";

/// Locates, concatenates, decompresses, and parses the private-data
/// stream for `page_id`. `None` on any failure — missing `/PieceInfo`,
/// an unrecognized compression marker, a corrupt stream, anything: this
/// is best-effort by design, never a hard error for the caller.
pub(crate) fn extract(pdf: &PdfDocument, page_id: PdfId) -> Option<PrivateSymbols> {
    let raw = concat_private_data(pdf, page_id)?;
    decompress(&raw)
}

fn concat_private_data(pdf: &PdfDocument, page_id: PdfId) -> Option<Vec<u8>> {
    let page = pdf.get_object(page_id).ok()?.as_dict().ok()?;
    let piece_info = deref_dict(pdf, page.get(b"PieceInfo").ok()?)?;
    let illustrator = deref_dict(pdf, piece_info.get(b"Illustrator").ok()?)?;
    let private = deref_dict(pdf, illustrator.get(b"Private").ok()?)?;

    let mut chunks: Vec<(u64, &[u8])> = Vec::new();
    for (key, _) in private.iter() {
        if let Some(n) = key.strip_prefix(b"AIPrivateData") {
            if !n.is_empty() {
                if let Ok(n) = std::str::from_utf8(n).unwrap_or("").parse::<u64>() {
                    chunks.push((n, key));
                }
            }
        }
    }
    if chunks.is_empty() {
        return None;
    }
    chunks.sort_by_key(|(n, _)| *n);

    let mut blob = Vec::new();
    for (_, key) in chunks {
        let obj = private.get(key).ok()?;
        let id = obj.as_reference().ok()?;
        let stream = pdf.get_object(id).ok()?.as_stream().ok()?;
        // These chunks carry no PDF-level `/Filter` — the compression is
        // Illustrator's own, handled below by `decompress` — so the raw
        // `content` bytes are exactly what we want. `decompressed_content()`
        // is the wrong call here: with no `/Filter` key it errors rather
        // than passing content through.
        blob.extend_from_slice(&stream.content);
    }
    Some(blob)
}

fn deref_dict<'p>(pdf: &'p PdfDocument, obj: &'p PdfObject) -> Option<&'p Dictionary> {
    match obj {
        PdfObject::Dictionary(d) => Some(d),
        PdfObject::Reference(id) => pdf.get_object(*id).ok()?.as_dict().ok(),
        _ => None,
    }
}

/// Strips the recognized marker and Zstandard-decompresses the rest,
/// streaming straight into a [`Scanner`] so a document with a huge
/// (multi-GB) decompressed size never needs to sit fully in memory at
/// once — only the handful of matched tags/names are kept.
fn decompress(blob: &[u8]) -> Option<PrivateSymbols> {
    let payload = blob.strip_prefix(ZSTD_MARKER)?;
    let mut decoder = zstd::stream::read::Decoder::new(payload).ok()?;
    let mut scanner = Scanner::default();
    let mut buf = [0u8; 1 << 20];
    loop {
        use std::io::Read;
        let n = decoder.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        scanner.feed(&buf[..n]);
    }
    scanner.finish();
    Some(scanner.into_result())
}

/// Incremental `\r`-delimited line scanner so [`decompress`] never has to
/// materialize the whole (potentially gigabyte-scale) decompressed stream
/// in memory just to look for a handful of short tags.
#[derive(Default)]
struct Scanner {
    carry: Vec<u8>,
    in_symbol_list: bool,
    names: Vec<String>,
    tags: Vec<SymbolTag>,
}

impl Scanner {
    fn feed(&mut self, chunk: &[u8]) {
        self.carry.extend_from_slice(chunk);
        // Process every complete line; keep a trailing partial line (no
        // `\r` yet) as carry for the next chunk.
        let mut start = 0;
        while let Some(rel) = self.carry[start..].iter().position(|&b| b == b'\r') {
            let end = start + rel;
            self.handle_line(&self.carry[start..end].to_vec());
            start = end + 1;
        }
        self.carry.drain(..start);
    }

    fn finish(&mut self) {
        if !self.carry.is_empty() {
            let line = std::mem::take(&mut self.carry);
            self.handle_line(&line);
        }
    }

    fn handle_line(&mut self, line: &[u8]) {
        if line == BEGIN_SYMBOL_LIST {
            self.in_symbol_list = true;
            return;
        }
        if line == END_SYMBOL_LIST {
            self.in_symbol_list = false;
            return;
        }
        if self.in_symbol_list {
            if let Some(name) = line.strip_prefix(b"(").and_then(|s| s.strip_suffix(b")")) {
                self.names.push(String::from_utf8_lossy(name).into_owned());
            }
            return;
        }
        if let Some(tag) = parse_art_number_tag(line) {
            self.tags.push(tag);
        }
    }

    fn into_result(self) -> PrivateSymbols {
        PrivateSymbols { names: self.names, tags: self.tags }
    }
}

/// Runs a [`Scanner`] over already-decompressed `data` in one shot. Kept
/// separate from [`decompress`] (which streams straight from the zstd
/// reader without ever holding all of `data` at once) purely for the
/// unit tests below, which exercise the tag grammar on small fixtures.
#[cfg(test)]
fn parse(data: &[u8]) -> PrivateSymbols {
    let mut scanner = Scanner::default();
    scanner.feed(data);
    scanner.finish();
    scanner.into_result()
}

/// Parses one line of the shape
/// `%_(ArtNumber__<N>[Instance]) /UnicodeString (AI19SymbolIDKey) ,`.
fn parse_art_number_tag(line: &[u8]) -> Option<SymbolTag> {
    if !contains(line, SYMBOL_ID_KEY) {
        return None;
    }
    let start = find(line, ART_NUMBER_PREFIX)? + ART_NUMBER_PREFIX.len();
    let rest = &line[start..];
    let digits_len = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let art_number: u32 = std::str::from_utf8(&rest[..digits_len]).ok()?.parse().ok()?;
    let instance = rest[digits_len..].starts_with(b"Instance");
    Some(SymbolTag { art_number, instance })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len().max(1)).position(|w| w == needle)
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    find(haystack, needle).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_symbol_list_in_descending_order() {
        let data = b"%AI24_BeginSymbolList\r(New Symbol 3)\r(New Symbol 2)\r(New Symbol 1)\r(New Symbol)\r%AI24_EndSymbolList\r";
        let result = parse(data);
        assert_eq!(
            result.names,
            vec!["New Symbol 3", "New Symbol 2", "New Symbol 1", "New Symbol"]
        );
    }

    #[test]
    fn parses_definition_and_instance_tags() {
        let data = b"%_(ArtNumber__3) /UnicodeString (AI19SymbolIDKey) ,\r%_(ArtNumber__1Instance) /UnicodeString (AI19SymbolIDKey) ,\r";
        let result = parse(data);
        assert_eq!(
            result.tags,
            vec![
                SymbolTag { art_number: 3, instance: false },
                SymbolTag { art_number: 1, instance: true },
            ]
        );
    }

    #[test]
    fn ignores_unrelated_lines() {
        let data = b"%AI5_File:\rf\r%_/ArtDictionary :\r%_(0fdb7650) /UnicodeString (AI24 ImageRawDataUUID) ,\r";
        let result = parse(data);
        assert!(result.tags.is_empty());
        assert!(result.names.is_empty());
    }

    #[test]
    fn handles_chunk_boundaries_mid_line() {
        // Regression check for the incremental scanner: a tag line split
        // across two `feed` calls must still parse as one line.
        let mut scanner = Scanner::default();
        scanner.feed(b"%_(ArtNumber__2Inst");
        scanner.feed(b"ance) /UnicodeString (AI19SymbolIDKey) ,\r");
        scanner.finish();
        assert_eq!(scanner.tags, vec![SymbolTag { art_number: 2, instance: true }]);
    }
}
