//! Text threading — linked area-text frames that share one story.
//!
//! The story's text lives on the head frame (`thread_prev == None`); each
//! downstream frame keeps an empty `content` and shows the overflow of its
//! predecessor. This module walks a chain and works out which byte range
//! of the story each frame displays.

use std::collections::HashMap;

use amalith_core::{Document, ObjectId, ObjectKind, TextData, TextKind};

use crate::text::TextContext;
use crate::textedit;

/// Where one frame's visible text starts within the story, and whether the
/// last frame still can't fit everything.
#[derive(Clone, Copy, Debug)]
pub struct ThreadSlice {
    /// Byte offset into the head frame's `content`.
    pub start: usize,
    /// Only ever true for the final frame: text remains past its box.
    pub overset: bool,
}

fn text_of(doc: &Document, id: ObjectId) -> Option<&TextData> {
    match &doc.object(id)?.kind {
        ObjectKind::Text(t) => Some(t),
        _ => None,
    }
}

/// True when frame `id` can't fit all the text it's meant to show — the
/// last frame of a thread with story left over, or a lone fixed-height
/// area box whose own text overflows. Drives the red "+" out-port.
pub fn frame_overset(doc: &Document, tcx: &mut TextContext, id: ObjectId) -> bool {
    let Some(td) = text_of(doc, id) else {
        return false;
    };
    let TextKind::Area {
        width,
        height: Some(h),
    } = td.kind
    else {
        return false;
    };
    if td.is_threaded() {
        return head(doc, id)
            .map(|hd| slices(doc, hd, tcx).get(&id).is_some_and(|s| s.overset))
            .unwrap_or(false);
    }
    let probe = TextData {
        content: td.content.clone(),
        kind: TextKind::Area {
            width,
            height: None,
        },
        style: td.style.clone(),
        align: td.align,
        paragraph: td.paragraph,
        local_bounds: amalith_core::Rect::ZERO,
        thread_next: None,
        thread_prev: None,
    };
    textedit::td_layout(tcx, &probe).height() as f64 > h + 0.5
}

/// The head (first) frame of `id`'s thread — `id` itself when it's a lone
/// frame or already the head. `None` if `id` isn't a text object.
pub fn head(doc: &Document, id: ObjectId) -> Option<ObjectId> {
    let mut cur = id;
    text_of(doc, cur)?;
    for _ in 0..4096 {
        match text_of(doc, cur).and_then(|t| t.thread_prev) {
            Some(prev) if text_of(doc, prev).is_some() => cur = prev,
            _ => return Some(cur),
        }
    }
    Some(cur)
}

/// Frames of a thread, head first. A single-frame "thread" returns one id.
pub fn chain(doc: &Document, head_id: ObjectId) -> Vec<ObjectId> {
    let mut out = Vec::new();
    let mut cur = Some(head_id);
    while let Some(id) = cur {
        if text_of(doc, id).is_none() || out.contains(&id) {
            break;
        }
        out.push(id);
        cur = text_of(doc, id).and_then(|t| t.thread_next);
    }
    out
}

/// For every frame in `head_id`'s thread, the byte offset its text starts
/// at and whether it's overset. The tail after each frame's box is
/// re-flowed at the *next* frame's width, so frames may differ in size.
pub fn slices(
    doc: &Document,
    head_id: ObjectId,
    tcx: &mut TextContext,
) -> HashMap<ObjectId, ThreadSlice> {
    let mut map = HashMap::new();
    let Some(head_td) = text_of(doc, head_id).cloned() else {
        return map;
    };
    let content = head_td.content.clone();
    let frames = chain(doc, head_id);
    let mut cursor = 0usize;

    for (i, &fid) in frames.iter().enumerate() {
        let is_last = i + 1 == frames.len();
        let Some(ftd) = text_of(doc, fid) else { continue };
        let (fw, fh) = match ftd.kind {
            TextKind::Area { width, height } => (width, height),
            TextKind::Point => (head_td.local_bounds.width().max(1.0), None),
        };

        if cursor >= content.len() {
            map.insert(fid, ThreadSlice { start: cursor, overset: false });
            continue;
        }

        // Lay out the remaining story at this frame's width and find the
        // first line whose bottom crosses the box floor.
        let probe = TextData {
            content: content[cursor..].to_string(),
            kind: TextKind::Area {
                width: fw,
                height: None,
            },
            style: head_td.style.clone(),
            align: head_td.align,
            paragraph: head_td.paragraph,
            local_bounds: amalith_core::Rect::ZERO,
            thread_next: None,
            thread_prev: None,
        };
        let layout = textedit::td_layout(tcx, &probe);

        let mut consumed = content.len() - cursor; // default: it all fits
        if let Some(limit) = fh {
            let mut y = 0.0f64;
            for line in layout.lines() {
                let line_h = line.metrics().line_height as f64;
                if y + line_h > limit + 0.5 {
                    consumed = line.text_range().start;
                    break;
                }
                y += line_h;
            }
        }

        map.insert(fid, ThreadSlice { start: cursor, overset: false });
        cursor += consumed;

        if is_last {
            map.insert(
                fid,
                ThreadSlice {
                    start: map[&fid].start,
                    overset: cursor < content.len(),
                },
            );
        }
    }
    map
}
