//! Command palette (⌘K): fuzzy-search every menu command, tool and
//! toggle, then run it. A hand-drawn modal — one implementation for every
//! platform — and on Linux, where there is no menu bar yet, the main way
//! to reach those commands.
//!
//! This module owns the modal's UI and matching only. The shell builds
//! the command list (display text here, the real action kept parallel in
//! `App`) and runs whatever row the palette reports as chosen.

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;

const PW: f64 = 560.0;
const FIELD_H: f64 = 42.0;
const ROW_H: f64 = 30.0;
const MAX_ROWS: usize = 9;
const PAD: f64 = 8.0;
const TOP_FRAC: f64 = 0.13;

/// One searchable row's display text. `hint` is the small right-aligned
/// category ("File", "Tool", "View", …).
#[derive(Clone)]
pub struct Entry {
    pub title: String,
    pub hint: String,
}

/// What a press resolved to.
pub enum Hit {
    /// Run the entry with this original index.
    Row(usize),
    /// Inside the panel but not on a row — swallow, stay open.
    Panel,
    /// Outside — dismiss.
    Outside,
}

pub struct Palette {
    pub field: crate::text_field::TextField,
    entries: Vec<Entry>,
    /// Original indices that match `query`, best score first.
    filtered: Vec<usize>,
    /// Index into `filtered`.
    sel: usize,
    /// First visible row (index into `filtered`).
    top: usize,
    // Refreshed every paint for hit-testing.
    panel: Rect,
    rows: Vec<(Rect, usize)>,
}

impl Palette {
    pub fn new(entries: Vec<Entry>) -> Self {
        let filtered = (0..entries.len()).collect();
        Self {
            field: crate::text_field::TextField::new(""),
            entries,
            filtered,
            sel: 0,
            top: 0,
            panel: Rect::ZERO,
            rows: Vec::new(),
        }
    }

    /// Recompute the match list from the field's current text.
    pub fn refilter(&mut self) {
        let q = self.field.text().trim().to_lowercase();
        let mut scored: Vec<(i64, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| score(&q, &e.title).map(|s| (s, i)))
            .collect();
        // Higher score first; ties keep source order (stable sort).
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        self.sel = 0;
        self.top = 0;
    }

    pub fn move_sel(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let n = self.filtered.len() as i32;
        self.sel = (self.sel as i32 + delta).rem_euclid(n) as usize;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + MAX_ROWS {
            self.top = self.sel + 1 - MAX_ROWS;
        }
    }

    pub fn scroll(&mut self, dy: f64) {
        if self.filtered.len() <= MAX_ROWS {
            return;
        }
        let max = self.filtered.len() - MAX_ROWS;
        let step = if dy > 0.0 { -1i32 } else { 1 };
        self.top = (self.top as i32 + step).clamp(0, max as i32) as usize;
    }

    /// The original entry index currently highlighted.
    pub fn selected(&self) -> Option<usize> {
        self.filtered.get(self.sel).copied()
    }

    pub fn hit(&self, p: Point) -> Hit {
        if !self.panel.contains(p) {
            return Hit::Outside;
        }
        for (r, idx) in &self.rows {
            if r.contains(p) {
                return Hit::Row(*idx);
            }
        }
        Hit::Panel
    }

    /// Hovering a row selects it (VS Code-style). Returns whether the
    /// selection moved (caller repaints).
    pub fn hover(&mut self, p: Point) -> bool {
        for (r, orig) in &self.rows {
            if r.contains(p) {
                if let Some(pos) = self.filtered.iter().position(|i| i == orig) {
                    if pos != self.sel {
                        self.sel = pos;
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn paint(&mut self, scene: &mut Scene, text: &mut TextContext, theme: &Theme, wl: f64, hl: f64) {
        scene.fill(
            Fill::NonZero,
            ID,
            Color::from_rgba8(0, 0, 0, 120),
            None,
            &Rect::new(0.0, 0.0, wl, hl),
        );

        let visible = self.filtered.len().min(MAX_ROWS);
        let body_h = FIELD_H + PAD + visible as f64 * ROW_H + PAD;
        let x0 = ((wl - PW) * 0.5).round().max(0.0);
        let y0 = (hl * TOP_FRAC).round();
        self.panel = Rect::new(x0, y0, x0 + PW, y0 + body_h);
        scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &self.panel.to_rounded_rect(10.0));
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            theme.border,
            None,
            &self.panel.to_rounded_rect(10.0),
        );

        // Search field — a real editable text field.
        let field = Rect::new(x0 + PAD, y0 + PAD, x0 + PW - PAD, y0 + FIELD_H - PAD * 0.5);
        scene.fill(Fill::NonZero, ID, theme.bg, None, &field.to_rounded_rect(6.0));
        self.field.paint(scene, text, theme, field, "Search commands…", true);
        scene.fill(
            Fill::NonZero,
            ID,
            theme.border,
            None,
            &Rect::new(x0 + PAD, field.y1 + PAD * 0.5, x0 + PW - PAD, field.y1 + PAD * 0.5 + 1.0),
        );

        // Rows.
        self.rows.clear();
        let list_top = y0 + FIELD_H + PAD;
        if self.filtered.is_empty() {
            text.draw(
                scene,
                "No matching commands",
                13.0,
                theme.text_dim,
                x0 + PAD + 12.0,
                list_top + ROW_H * 0.5 + 4.0,
            );
        }
        for row in 0..visible {
            let fi = self.top + row;
            let Some(&orig) = self.filtered.get(fi) else { break };
            let e = &self.entries[orig];
            let r = Rect::new(x0 + PAD * 0.5, list_top + row as f64 * ROW_H, x0 + PW - PAD * 0.5, list_top + (row as f64 + 1.0) * ROW_H);
            let on = fi == self.sel;
            if on {
                scene.fill(Fill::NonZero, ID, theme.accent, None, &r.to_rounded_rect(5.0));
            }
            let title_col = if on { theme.on_accent } else { theme.text };
            let hint_col = if on {
                theme.on_accent
            } else {
                theme.text_dim
            };
            text.draw(scene, &e.title, 13.5, title_col, r.x0 + 12.0, r.center().y + 4.5);
            if !e.hint.is_empty() {
                let hw = text.measure(&e.hint, 11.5);
                text.draw(scene, &e.hint, 11.5, hint_col, r.x1 - 12.0 - hw, r.center().y + 4.0);
            }
            self.rows.push((r, orig));
        }

        // Scroll indicator.
        if self.filtered.len() > MAX_ROWS {
            let track = Rect::new(self.panel.x1 - 5.0, list_top, self.panel.x1 - 2.0, list_top + MAX_ROWS as f64 * ROW_H);
            let frac = MAX_ROWS as f64 / self.filtered.len() as f64;
            let th = (track.height() * frac).max(20.0);
            let denom = (self.filtered.len() - MAX_ROWS) as f64;
            let ty = track.y0 + (track.height() - th) * (self.top as f64 / denom);
            scene.fill(
                Fill::NonZero,
                ID,
                Color::from_rgba8(0x9a, 0x9a, 0x9a, 0x88),
                None,
                &Rect::new(track.x0, ty, track.x1, ty + th).to_rounded_rect(1.5),
            );
        }
    }
}

/// Subsequence fuzzy score, higher is better. `q` must be pre-lowercased;
/// empty `q` matches everything at score 0. Bonuses for word-start and
/// contiguous hits; a penalty for skipped characters.
fn score(q: &str, s: &str) -> Option<i64> {
    if q.is_empty() {
        return Some(0);
    }
    let sb = s.to_lowercase().into_bytes();
    let mut si = 0usize;
    let mut total = 0i64;
    let mut last: Option<usize> = None;
    for qc in q.bytes() {
        let m = (si..sb.len()).find(|&j| sb[j] == qc)?;
        if last.map(|l| l + 1) == Some(m) {
            total += 6;
        }
        if m == 0 || sb[m - 1] == b' ' {
            total += 12;
        }
        total -= (m - si) as i64;
        last = Some(m);
        si = m + 1;
    }
    Some(total)
}
