//! Live text editing — a thin wrapper over `parley::PlainEditor`.
//!
//! Owns the caret / selection / IME state while the Type tool has a text
//! object open. The document only ever sees the *result*: on commit the
//! shell reads [`TextEdit::to_text_data`] and files one `Command::SetText`.
//!
//! v1 is whole-object styling — `PlainEditor` carries a single `StyleSet`,
//! so the Character panel edits the object, not a character range.

use std::borrow::Cow;

use amalith_core::geom as cg;
use amalith_core::{Paragraph, TextAlign, TextData, TextKind, TextPosition, TextStyle};
use parley::layout::PositionedLayoutItem;
use parley::style::{
    FontFamily, FontFamilyName, FontFeatures, FontStyle, FontWeight, LineHeight, StyleProperty,
};
use parley::{Alignment, Layout, PlainEditor};
use skrifa::{
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    GlyphId, MetadataProvider,
};
use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

use crate::text::{TextContext, TextLayoutKey};
use crate::vertical_text::{self, VerticalLayout};

/// A caret size hint for `cursor_geometry`, in editor px.
const CARET_W: f32 = 1.5;

/// The "text can't all fit" warning: the overflow tab drawn here on an
/// Area-text box whose fixed height clips its own layout, and the overset
/// badge `canvas.rs` draws on a threaded frame's out-port. Shared because
/// both mean the same thing to the user — text didn't fit — not because
/// they're the same drawing.
pub(crate) const TEXT_OVERSET_INK: Color = Color::from_rgb8(0xd0, 0x30, 0x30);

/// One in-progress text edit.
pub struct TextEdit {
    /// The document object being edited.
    pub object: amalith_core::ObjectId,
    /// Document-space anchor: point-type baseline-left / area-type top-left.
    pub origin: amalith_core::Point,
    editor: PlainEditor<Brush>,
    kind: TextKind,
    path_geometry: Option<amalith_core::PathData>,
    style: TextStyle,
    align: TextAlign,
    paragraph: Paragraph,
    /// Thread links carried through so a commit doesn't sever them.
    thread_next: Option<amalith_core::ObjectId>,
    thread_prev: Option<amalith_core::ObjectId>,
    /// True from the first keystroke — a never-touched object is discarded
    /// on commit.
    pub touched: bool,
    /// Illustrator's Vertical Type Tool — top-to-bottom, right-to-left
    /// columns of upright glyphs, computed and hit-tested entirely outside
    /// `parley::PlainEditor` (which has no vertical-writing concept at
    /// all — see `vertical_text.rs`). `editor` still owns the actual text
    /// buffer and logical-order caret/selection (insert, delete, select
    /// all, word/char movement) even when this is `true`; only point-based
    /// hit-testing and the Up/Down/Left/Right key mapping are swapped.
    vertical: bool,
    /// Kept in sync with `editor`'s text/style after every edit — the
    /// source of truth for rendering, hit-testing, and caret/selection
    /// rects when `vertical` is set.
    v_layout: VerticalLayout,
}

impl TextEdit {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        object: amalith_core::ObjectId,
        origin: amalith_core::Point,
        kind: TextKind,
        style: TextStyle,
        align: TextAlign,
        paragraph: Paragraph,
        vertical: bool,
        seed: &str,
        tcx: &mut TextContext,
    ) -> Self {
        let mut editor = PlainEditor::<Brush>::new(style.size as f32);
        editor.set_text(seed);
        // Vertical text is never wrapped at the parley level — its own
        // column-height wrapping (`vertical_text::layout`) replaces
        // parley's width-based wrapping entirely.
        if !vertical {
            if let TextKind::Area { width, .. } = kind {
                editor.set_width(Some(width as f32));
            } else {
                editor.set_width(None);
            }
        }
        editor.set_alignment(alignment(align));
        let v_layout = vertical_text::layout(tcx, seed, &style, wrap_h_for(kind));
        let mut this = Self {
            object,
            origin,
            editor,
            kind,
            path_geometry: None,
            style: style.clone(),
            align,
            paragraph,
            thread_next: None,
            thread_prev: None,
            touched: !seed.is_empty(),
            vertical,
            v_layout,
        };
        this.apply_style(&style, tcx);
        this.set_paragraph(paragraph);
        this
    }

    /// Recomputes `v_layout` from the editor's current text/style — call
    /// after anything that changes either. A no-op for horizontal text.
    fn refresh_v_layout(&mut self, tcx: &mut TextContext) {
        if self.vertical {
            let content = self.text();
            self.v_layout = vertical_text::layout(tcx, &content, &self.style, wrap_h_for(self.kind));
        }
    }

    /// Carry the source frame's thread links so [`Self::to_text_data`]
    /// writes them back unchanged.
    pub fn set_thread(
        &mut self,
        prev: Option<amalith_core::ObjectId>,
        next: Option<amalith_core::ObjectId>,
    ) {
        self.thread_prev = prev;
        self.thread_next = next;
    }

    /// Push the whole [`TextStyle`] into the editor's `StyleSet`. Only the
    /// attributes parley supports natively land here; the geometric ones
    /// (H/V scale, baseline shift, rotation) are v2.
    pub fn apply_style(&mut self, style: &TextStyle, tcx: &mut TextContext) {
        self.style = style.clone();
        let (fc, lc) = tcx.parts();
        let s = self.editor.edit_styles();
        s.clear();
        s.insert(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
            vec![FontFamilyName::Named(Cow::Owned(style.family.clone()))],
        ))));
        s.insert(StyleProperty::FontSize(style.size as f32));
        s.insert(StyleProperty::FontWeight(FontWeight::new(
            style.weight as f32,
        )));
        s.insert(StyleProperty::FontStyle(if style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        s.insert(StyleProperty::LineHeight(match style.leading {
            Some(px) => LineHeight::Absolute(px as f32),
            None => LineHeight::FontSizeRelative(1.2),
        }));
        // Tracking is thousandths of an em → px.
        s.insert(StyleProperty::LetterSpacing(
            (style.tracking / 1000.0 * style.size) as f32,
        ));
        s.insert(StyleProperty::Underline(style.underline));
        s.insert(StyleProperty::Strikethrough(style.strikethrough));
        let feats: &'static str = if style.small_caps {
            "smcp on"
        } else {
            match style.position {
                TextPosition::Superscript => "sups on",
                TextPosition::Subscript => "subs on",
                TextPosition::Normal => "",
            }
        };
        s.insert(StyleProperty::FontFeatures(FontFeatures::from(feats)));
        self.editor.refresh_layout(fc, lc);
    }

    pub fn style(&self) -> &TextStyle {
        &self.style
    }

    pub fn align(&self) -> TextAlign {
        self.align
    }

    pub fn kind(&self) -> TextKind {
        self.kind
    }

    pub fn set_path_geometry(&mut self, path: Option<amalith_core::PathData>) {
        self.path_geometry = path;
    }

    pub fn set_align(&mut self, align: TextAlign, tcx: &mut TextContext) {
        self.align = align;
        self.editor.set_alignment(alignment(align));
        // `PlainEditor::set_alignment` records the new paragraph setting,
        // but an existing shaped layout remains in use until its next
        // refresh. A keystroke happened to trigger that refresh, making
        // placeholder text appear stuck until the user typed something.
        let (font, layout) = tcx.parts();
        self.editor.refresh_layout(font, layout);
    }

    pub fn paragraph(&self) -> Paragraph {
        self.paragraph
    }

    /// Update the paragraph attributes. Left / right indent narrow the
    /// area-text wrap width live; the rest are recorded for commit /
    /// export.
    pub fn set_paragraph(&mut self, p: Paragraph) {
        self.paragraph = p;
        // Vertical text is never wrapped at the parley level (see `new`) —
        // indent-narrowing the parley wrap width would be a no-op there
        // anyway, but skip it for clarity.
        if !self.vertical {
            if let TextKind::Area { width, .. } = self.kind {
                let inner = (width - p.indent_start - p.indent_end).max(1.0);
                self.editor.set_width(Some(inner as f32));
            }
        }
    }

    pub fn set_area_width(&mut self, width: f64) {
        if let TextKind::Area { width: w, height } = &mut self.kind {
            *w = width;
            let h = *height;
            self.kind = TextKind::Area { width, height: h };
        }
        if !self.vertical {
            self.editor.set_width(Some(width as f32));
        }
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.editor.set_scale(scale);
    }

    pub fn text(&self) -> String {
        self.editor.text().chars().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.editor.text().chars().next().is_none()
    }

    /// The current selection rectangles, in editor space (origin at the
    /// text block's top-left).
    pub fn selection_rects(&self) -> Vec<Rect> {
        if self.vertical {
            let sel = self.editor.raw_selection();
            let (a, b) = (sel.anchor().index(), sel.focus().index());
            let (lo, hi) = if a < b { (a, b) } else { (b, a) };
            return self.v_layout.selection_rects(lo..hi);
        }
        self.editor
            .selection_geometry()
            .into_iter()
            .map(|(b, _)| Rect::new(b.x0, b.y0, b.x1, b.y1))
            .collect()
    }

    /// The caret rectangle, in editor space, if the cursor is shown.
    pub fn caret_rect(&self) -> Option<Rect> {
        if self.vertical {
            return Some(self.v_layout.caret_rect(self.editor.raw_selection().focus().index()));
        }
        self.editor
            .cursor_geometry(CARET_W)
            .map(|b| Rect::new(b.x0, b.y0, b.x1, b.y1))
    }

    /// IME candidate-window anchor, editor space.
    pub fn ime_area(&self) -> Rect {
        let b = self.editor.ime_cursor_area();
        Rect::new(b.x0, b.y0, b.x1, b.y1)
    }

    // --- editing --------------------------------------------------------

    /// A key while editing. Returns whether the edit should now commit
    /// (Esc / ⌘Return / — caller also commits on click-away & tool switch).
    pub fn key(
        &mut self,
        key: &winit::keyboard::Key,
        mods: Mods,
        text: Option<&str>,
        tcx: &mut TextContext,
    ) -> KeyResult {
        use winit::keyboard::{Key, NamedKey};
        let sel = mods.shift;
        // Vertical text swaps the visual meaning of the arrow keys before
        // anything else runs: Up/Down move within the current column
        // (parley's own Left/Right — moving one character forward/back in
        // logical order is layout-direction-agnostic), while Left/Right
        // jump to the same row in the neighboring column — something
        // parley has no concept of at all, computed from `v_layout`
        // instead. Everything else (typing, Backspace/Delete, Enter,
        // Escape, ⌘A, ...) is identical to horizontal text and falls
        // through to the unchanged match below.
        if self.vertical {
            if let Key::Named(named) = key {
                match named {
                    NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::ArrowUp | NamedKey::ArrowDown
                        if mods.alt =>
                    {
                        return KeyResult::Ignored;
                    }
                    NamedKey::ArrowUp => {
                        let (fc, lc) = tcx.parts();
                        let mut drv = self.editor.driver(fc, lc);
                        if mods.meta {
                            if sel { drv.select_to_text_start(); } else { drv.move_to_text_start(); }
                        } else if sel {
                            drv.select_left();
                        } else {
                            drv.move_left();
                        }
                        return KeyResult::Handled;
                    }
                    NamedKey::ArrowDown => {
                        let (fc, lc) = tcx.parts();
                        let mut drv = self.editor.driver(fc, lc);
                        if mods.meta {
                            if sel { drv.select_to_text_end(); } else { drv.move_to_text_end(); }
                        } else if sel {
                            drv.select_right();
                        } else {
                            drv.move_right();
                        }
                        return KeyResult::Handled;
                    }
                    NamedKey::ArrowLeft | NamedKey::ArrowRight => {
                        let focus = self.editor.raw_selection().focus().index();
                        let (col, row) = self.v_layout.column_row_of(focus);
                        // Column 0 sits at local x=0 (rightmost); higher
                        // indices sit further left. So Left = a higher
                        // column index, Right = a lower one.
                        let target = if matches!(named, NamedKey::ArrowLeft) {
                            col + 1
                        } else {
                            col.saturating_sub(1)
                        };
                        let byte = self.v_layout.byte_at(target, row);
                        let (fc, lc) = tcx.parts();
                        let mut drv = self.editor.driver(fc, lc);
                        if sel { drv.extend_selection_to_byte(byte); } else { drv.move_to_byte(byte); }
                        return KeyResult::Handled;
                    }
                    NamedKey::Home | NamedKey::End => {
                        let focus = self.editor.raw_selection().focus().index();
                        let (col, _) = self.v_layout.column_row_of(focus);
                        let bounds = self.v_layout.column_bounds(col);
                        let byte = if matches!(named, NamedKey::Home) { bounds.start } else { bounds.end };
                        let (fc, lc) = tcx.parts();
                        let mut drv = self.editor.driver(fc, lc);
                        if sel { drv.extend_selection_to_byte(byte); } else { drv.move_to_byte(byte); }
                        return KeyResult::Handled;
                    }
                    _ => {}
                }
            }
        }
        let (fc, lc) = tcx.parts();
        let mut drv = self.editor.driver(fc, lc);
        match key {
            Key::Named(NamedKey::Escape) => return KeyResult::Commit,
            Key::Named(NamedKey::Enter) if mods.meta => return KeyResult::Commit,
            Key::Named(NamedKey::Enter) => {
                drv.insert_or_replace_selection("\n");
                self.touched = true;
            }
            Key::Named(NamedKey::Tab) => {
                drv.insert_or_replace_selection("\t");
                self.touched = true;
            }
            Key::Named(NamedKey::Backspace) => {
                if mods.alt {
                    drv.backdelete_word();
                } else {
                    drv.backdelete();
                }
                self.touched = true;
            }
            Key::Named(NamedKey::Delete) => {
                if mods.alt {
                    drv.delete_word();
                } else {
                    drv.delete();
                }
                self.touched = true;
            }
            // Option is reserved for kerning/tracking (Left/Right),
            // leading (Up/Down), and — with Shift too — baseline shift
            // (Up/Down), matching Illustrator: unlike a plain text field,
            // its Type tool doesn't use Option for word navigation. Bail
            // out untouched so the shell's `action_keys` dispatch (see
            // `keyboard.rs`) gets the keystroke instead.
            Key::Named(NamedKey::ArrowLeft) if mods.alt => return KeyResult::Ignored,
            Key::Named(NamedKey::ArrowRight) if mods.alt => return KeyResult::Ignored,
            Key::Named(NamedKey::ArrowUp) if mods.alt => return KeyResult::Ignored,
            Key::Named(NamedKey::ArrowDown) if mods.alt => return KeyResult::Ignored,
            Key::Named(NamedKey::ArrowLeft) => match (sel, mods.meta) {
                (false, false) => drv.move_left(),
                (true, false) => drv.select_left(),
                (false, true) => drv.move_to_line_start(),
                (true, true) => drv.select_to_line_start(),
            },
            Key::Named(NamedKey::ArrowRight) => match (sel, mods.meta) {
                (false, false) => drv.move_right(),
                (true, false) => drv.select_right(),
                (false, true) => drv.move_to_line_end(),
                (true, true) => drv.select_to_line_end(),
            },
            Key::Named(NamedKey::ArrowUp) => {
                if mods.meta {
                    if sel {
                        drv.select_to_text_start();
                    } else {
                        drv.move_to_text_start();
                    }
                } else if sel {
                    drv.select_up();
                } else {
                    drv.move_up();
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if mods.meta {
                    if sel {
                        drv.select_to_text_end();
                    } else {
                        drv.move_to_text_end();
                    }
                } else if sel {
                    drv.select_down();
                } else {
                    drv.move_down();
                }
            }
            Key::Named(NamedKey::Home) => {
                if sel {
                    drv.select_to_line_start();
                } else {
                    drv.move_to_line_start();
                }
            }
            Key::Named(NamedKey::End) => {
                if sel {
                    drv.select_to_line_end();
                } else {
                    drv.move_to_line_end();
                }
            }
            Key::Character(c) if mods.meta => match c.as_str() {
                "a" | "A" => drv.select_all(),
                _ => return KeyResult::PassThrough,
            },
            _ => {
                if let Some(t) = text {
                    let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        drv.insert_or_replace_selection(&clean);
                        self.touched = true;
                    }
                }
            }
        }
        drop(drv);
        self.refresh_v_layout(tcx);
        KeyResult::Handled
    }

    pub fn insert_str(&mut self, s: &str, tcx: &mut TextContext) {
        let (fc, lc) = tcx.parts();
        self.editor.driver(fc, lc).insert_or_replace_selection(s);
        self.touched = true;
        self.refresh_v_layout(tcx);
    }

    pub fn select_all(&mut self, tcx: &mut TextContext) {
        let (fc, lc) = tcx.parts();
        self.editor.driver(fc, lc).select_all();
    }

    // --- IME -----------------------------------------------------------

    pub fn ime(&mut self, ime: &winit::event::Ime, tcx: &mut TextContext) {
        use winit::event::Ime;
        let (fc, lc) = tcx.parts();
        let mut drv = self.editor.driver(fc, lc);
        match ime {
            Ime::Enabled | Ime::Disabled => drv.clear_compose(),
            Ime::Preedit(s, cursor) => {
                if s.is_empty() {
                    drv.clear_compose();
                } else {
                    drv.set_compose(s, *cursor);
                }
            }
            Ime::Commit(s) => {
                drv.clear_compose();
                drv.insert_or_replace_selection(s);
                self.touched = true;
            }
        }
    }

    pub fn is_composing(&self) -> bool {
        self.editor.is_composing()
    }

    // --- pointer -----------------------------------------------------------

    /// `p` is in editor space (already offset by the text block origin and
    /// un-zoomed). `clicks` = 1 caret, 2 word, 3+ the whole text.
    pub fn pointer_down(&mut self, p: (f32, f32), clicks: u32, tcx: &mut TextContext) {
        if self.vertical {
            // Parley's own point-based hit-testing assumes its (unused
            // for vertical) horizontal layout — resolve against the real
            // column layout instead, then just move the underlying
            // editor's logical caret to that byte offset.
            let byte = self.v_layout.hit_test(Point::new(p.0 as f64, p.1 as f64));
            let (fc, lc) = tcx.parts();
            let mut drv = self.editor.driver(fc, lc);
            if clicks >= 3 {
                drv.select_all();
            } else {
                drv.move_to_byte(byte);
            }
            return;
        }
        let (fc, lc) = tcx.parts();
        let mut drv = self.editor.driver(fc, lc);
        match clicks {
            0 | 1 => drv.move_to_point(p.0, p.1),
            2 => drv.select_word_at_point(p.0, p.1),
            _ => drv.select_all(),
        }
    }

    pub fn pointer_drag(&mut self, p: (f32, f32), tcx: &mut TextContext) {
        if self.vertical {
            let byte = self.v_layout.hit_test(Point::new(p.0 as f64, p.1 as f64));
            let (fc, lc) = tcx.parts();
            self.editor.driver(fc, lc).extend_selection_to_byte(byte);
            return;
        }
        let (fc, lc) = tcx.parts();
        self.editor
            .driver(fc, lc)
            .extend_selection_to_point(p.0, p.1);
    }

    // --- render / commit -------------------------------------------------

    pub fn path_editor_point(
        &self,
        arc: &amalith_core::ArcLengthPath,
        pt: &amalith_core::PathTextData,
        p: amalith_core::Point,
    ) -> (f32, f32) {
        let width = self
            .editor
            .try_layout()
            .and_then(|l| l.lines().next())
            .map(|l| l.metrics().advance as f64)
            .unwrap_or(0.0);
        let offset = crate::pathtext::paragraph_offset(self.align, pt.end - pt.start, width);
        let distance = crate::pathtext::nearest_unwrapped(arc, p, (pt.start + pt.end) / 2.0);
        let x = if pt.flip {
            pt.end - distance
        } else {
            distance - pt.start
        } - offset;
        let baseline = self
            .editor
            .try_layout()
            .and_then(|l| l.lines().next())
            .map(|l| l.metrics().baseline)
            .unwrap_or(self.style.size as f32);
        (x as f32, baseline - self.style.size as f32 * 0.3)
    }

    /// Project the editor's selection and caret onto the same baseline as
    /// the live glyphs, keeping typing and pointer selection on the curve.
    #[allow(clippy::too_many_arguments)]
    pub fn render_path(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        arc: &amalith_core::ArcLengthPath,
        rel: amalith_core::Affine,
        xf: Affine,
        color: Color,
        caret_on: bool,
        accent: Color,
    ) {
        let data = self.to_text_data(tcx);
        let TextKind::Path(pt) = data.kind else {
            return;
        };
        let layout = self.editor.try_layout().unwrap();
        let offset = crate::pathtext::paragraph_offset(
            self.align,
            pt.end - pt.start,
            layout
                .lines()
                .next()
                .map(|line| line.metrics().advance as f64)
                .unwrap_or(0.0),
        );
        let baseline = layout
            .lines()
            .next()
            .map(|l| l.metrics().baseline as f64)
            .unwrap_or(self.style.size);
        let vertical_shift = layout
            .lines()
            .next()
            .and_then(|line| {
                line.items().find_map(|item| match item {
                    PositionedLayoutItem::GlyphRun(run) => {
                        let metrics = run.run().metrics();
                        Some(match pt.align {
                            amalith_core::PathTextAlign::Baseline => 0.0,
                            amalith_core::PathTextAlign::Ascender => metrics.ascent as f64,
                            amalith_core::PathTextAlign::Descender => -(metrics.descent as f64),
                            amalith_core::PathTextAlign::Center => {
                                (metrics.ascent - metrics.descent) as f64 / 2.0
                            }
                        })
                    }
                    _ => None,
                })
            })
            .unwrap_or(0.0);
        let path_xf = xf * crate::convert::affine(rel);
        let project = |x: f64, y: f64| {
            let d = if pt.flip {
                pt.end - offset - x
            } else {
                pt.start + offset + x
            };
            let (p, angle) = arc.point_and_tangent(d);
            let angle = angle + if pt.flip { std::f64::consts::PI } else { 0.0 };
            path_xf
                * Affine::translate((p.x, p.y))
                * Affine::rotate(angle)
                * vello::kurbo::Point::new(
                    0.0,
                    y - baseline + vertical_shift - self.style.baseline_shift,
                )
        };
        for r in self.selection_rects() {
            let x0 = r.x0.max(-offset);
            let x1 = r.x1.min(pt.end - pt.start - offset);
            if x1 <= x0 {
                continue;
            }
            let steps = ((x1 - x0) / 3.0).ceil().clamp(1.0, 4096.0) as usize;
            let mut shape = vello::kurbo::BezPath::new();
            for i in 0..=steps {
                let p = project(x0 + (x1 - x0) * i as f64 / steps as f64, r.y0);
                if i == 0 {
                    shape.move_to(p);
                } else {
                    shape.line_to(p);
                }
            }
            for i in (0..=steps).rev() {
                shape.line_to(project(x0 + (x1 - x0) * i as f64 / steps as f64, r.y1));
            }
            shape.close_path();
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                accent.multiply_alpha(0.35),
                None,
                &shape,
            );
        }
        crate::pathtext::paint_path_text(scene, tcx, &data, &pt, arc, rel, xf, color);
        if caret_on && !self.is_composing() {
            if let Some(c) = self.caret_rect() {
                let x =
                    c.x0.clamp(-offset, (pt.end - pt.start - offset).max(-offset));
                scene.stroke(
                    &Stroke::new(1.5),
                    Affine::IDENTITY,
                    color,
                    None,
                    &vello::kurbo::Line::new(project(x, c.y0), project(x, c.y1)),
                );
            }
        }
    }

    /// Draw the live text, its selection, and (when `caret_on`) the caret.
    /// `xf` maps editor space → screen; `color` is the text colour.
    pub fn render(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        xf: Affine,
        color: Color,
        caret_on: bool,
        theme_blue: Color,
    ) {
        // Baseline shift is a pure local-space vertical offset (positive
        // = up, so subtracted — local space is y-down), applied before
        // the object's own world transform like everything else.
        let xf = xf * Affine::translate((0.0, -self.style.baseline_shift));
        if self.vertical {
            self.render_vertical(scene, tcx, xf, color, caret_on, theme_blue);
            return;
        }
        // Refresh the editor's layout up front. Driver ops (typing,
        // select-all, arrow keys) mark it dirty but don't rebuild, so
        // `selection_geometry` / `cursor_geometry` would otherwise read a
        // stale layout while the glyphs below draw from the fresh one —
        // which showed up as a selection box covering only part of the text.
        {
            let (fc, lc) = tcx.parts();
            self.editor.refresh_layout(fc, lc);
        }

        // Point text anchors on the click point per its alignment; shift the
        // whole editor (selection, glyphs, caret) so a live edit previews it.
        // Path text previews straight (like point text, anchored at the
        // click) while actively being typed — it only follows the curve
        // once committed. Curving a live caret/IME/selection is future
        // work; see `pathtext.rs` for the committed render.
        let xf = match self.kind {
            TextKind::Point | TextKind::Path(_) => {
                let w = self.editor.try_layout().map(|l| l.width()).unwrap_or(0.0);
                xf * Affine::translate((point_align_dx(self.align, w), 0.0))
            }
            TextKind::Area { .. } => xf,
        };

        // A fixed-height area box hides its text (and selection) past the
        // bottom edge — the box stays the size you drew, overflow is the
        // red tab's job to flag.
        let box_clip = match self.kind {
            TextKind::Area {
                width,
                height: Some(h),
            } => {
                scene.push_clip_layer(Fill::NonZero, xf, &Rect::new(0.0, 0.0, width, h));
                true
            }
            _ => false,
        };

        // Selection under the glyphs.
        let sel = self.selection_rects();
        for r in &sel {
            let mut c = theme_blue;
            c = c.multiply_alpha(0.35);
            scene.fill(Fill::NonZero, xf, c, None, r);
        }

        let (fc, lc) = tcx.parts();
        let layout = self.editor.layout(fc, lc);
        draw_glyph_runs(scene, layout, xf, color);

        if caret_on && !self.is_composing() {
            if let Some(c) = self.caret_rect() {
                scene.fill(Fill::NonZero, xf, color, None, &c);
            }
        }

        if box_clip {
            scene.pop_layer();
        }

        // Area-text box outline (always shown while editing) + a red
        // overflow tab when a fixed-height box can't fit its text.
        if let TextKind::Area { width, height } = self.kind {
            let content_h = self
                .editor
                .try_layout()
                .map(|l| l.height() as f64)
                .unwrap_or(0.0);
            let box_h = height.unwrap_or(content_h).max(1.0);
            scene.stroke(
                &Stroke::new(1.0),
                xf,
                theme_blue,
                None,
                &Rect::new(0.0, 0.0, width, box_h),
            );
            if let Some(fixed) = height {
                if content_h > fixed {
                    let m = Rect::new(width, fixed - 6.0, width + 6.0, fixed);
                    scene.fill(
                        Fill::NonZero,
                        xf,
                        TEXT_OVERSET_INK,
                        None,
                        &m,
                    );
                }
            }
        }
    }

    /// [`Self::render`]'s vertical-text counterpart — drawn entirely from
    /// `v_layout` rather than `editor.layout(..)`, which has no useful
    /// vertical positions to offer (see the module doc comment).
    fn render_vertical(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        xf: Affine,
        color: Color,
        caret_on: bool,
        theme_blue: Color,
    ) {
        self.refresh_v_layout(tcx);
        let box_clip = match self.kind {
            TextKind::Area { width, height: Some(h) } => {
                scene.push_clip_layer(Fill::NonZero, xf, &Rect::new(-width, 0.0, 0.0, h));
                true
            }
            _ => false,
        };
        for r in self.selection_rects() {
            scene.fill(Fill::NonZero, xf, theme_blue.multiply_alpha(0.35), None, &r);
        }
        self.v_layout.draw(scene, xf, color);
        if caret_on && !self.is_composing() {
            if let Some(c) = self.caret_rect() {
                scene.fill(Fill::NonZero, xf, color, None, &c);
            }
        }
        if box_clip {
            scene.pop_layer();
        }
        if let TextKind::Area { width, height } = self.kind {
            let box_h = height.unwrap_or_else(|| self.v_layout.height()).max(1.0);
            scene.stroke(&Stroke::new(1.0), xf, theme_blue, None, &Rect::new(-width, 0.0, 0.0, box_h));
            if let Some(fixed) = height {
                if self.v_layout.height() > fixed {
                    // Overflow past the box's LEFT edge (columns march
                    // leftward) — the vertical analogue of the horizontal
                    // overset tab past the bottom edge.
                    let m = Rect::new(-width - 6.0, 0.0, -width, 6.0);
                    scene.fill(Fill::NonZero, xf, TEXT_OVERSET_INK, None, &m);
                }
            }
        }
    }

    /// Recompute bounds from the current layout and produce the committed
    /// [`TextData`].
    pub fn to_text_data(&mut self, tcx: &mut TextContext) -> TextData {
        let content = self.text();
        let bounds = if self.vertical {
            self.refresh_v_layout(tcx);
            match self.kind {
                // A vertical area box keeps its drawn size (top-right
                // anchored, extending left to `-width`), same as a
                // horizontal box keeps its own drawn width/height —
                // content that overflows it isn't reflected here.
                TextKind::Area { width, height } => amalith_core::Rect::new(
                    -width,
                    0.0,
                    0.0,
                    height.unwrap_or_else(|| self.v_layout.height()),
                ),
                TextKind::Point | TextKind::Path(_) => {
                    let b = self.v_layout.bounds();
                    amalith_core::Rect::new(b.x0, b.y0, b.x1, b.y1)
                }
            }
        } else {
            let (fc, lc) = tcx.parts();
            let layout = self.editor.layout(fc, lc);
            let w = layout.width() as f64;
            let h = layout.height() as f64;
            match self.kind {
                // Path text's real bounds (the curved footprint) need the
                // followed path's geometry, which `TextEdit` doesn't have.
                // This straight-line approximation stands in for now — it's
                // only used for the selection outline / bounding-box handles,
                // never for painting (see `pathtext::paint_path_text`, which
                // ignores `local_bounds` entirely).
                TextKind::Point | TextKind::Path(_) => {
                    // Same anchor offset `paint_text_data` / `measure_text_data`
                    // apply, so the committed object's bounds wrap its glyphs.
                    let dx = point_align_dx(self.align, layout.width());
                    amalith_core::Rect::new(dx, 0.0, dx + w, h)
                }
                TextKind::Area { width, height } => {
                    // A fixed-height box keeps its drawn size regardless of how
                    // much text it holds; an auto box (height None) grows to
                    // the content.
                    amalith_core::Rect::new(0.0, 0.0, width, height.unwrap_or(h))
                }
            }
        };
        TextData {
            content,
            path_geometry: self.path_geometry.clone(),
            kind: self.kind,
            style: self.style.clone(),
            align: self.align,
            paragraph: self.paragraph,
            vertical: self.vertical,
            local_bounds: bounds,
            thread_next: self.thread_next,
            thread_prev: self.thread_prev,
        }
    }
}

/// Modifier snapshot for [`TextEdit::key`].
#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

pub enum KeyResult {
    /// Consumed by the editor.
    Handled,
    /// Commit and exit edit mode.
    Commit,
    /// Not an editing key — let the shell handle it (e.g. ⌘Z, ⌘S). Exits
    /// edit mode first (unlike `Ignored`), since the shell-level action
    /// this hands off to is generally not text-editing-aware.
    PassThrough,
    /// Not an editing key, but — unlike `PassThrough` — one the shell can
    /// act on *without* leaving edit mode (the Option-held text-
    /// formatting nudges: kerning/tracking, leading, baseline shift).
    Ignored,
}

/// Line height for a [`TextStyle`] as parley's [`LineHeight`].
fn line_height(style: &TextStyle) -> LineHeight {
    match style.leading {
        Some(px) => LineHeight::Absolute(px as f32),
        None => LineHeight::FontSizeRelative(1.2),
    }
}

fn features(style: &TextStyle) -> &'static str {
    if style.small_caps {
        "smcp on"
    } else {
        match style.position {
            TextPosition::Superscript => "sups on",
            TextPosition::Subscript => "subs on",
            TextPosition::Normal => "",
        }
    }
}

fn alignment(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Start => Alignment::Start,
        TextAlign::Center => Alignment::Center,
        TextAlign::End => Alignment::End,
        // parley has one Justify; the last-line variants are recorded on
        // the model and honoured on export, not in the live layout yet.
        TextAlign::JustifyLeft
        | TextAlign::JustifyCenter
        | TextAlign::JustifyRight
        | TextAlign::JustifyAll => Alignment::Justify,
    }
}

/// The height budget `vertical_text::layout` should wrap columns to:
/// an area box's own (always-fixed, for vertical text — see
/// `vertical_text.rs`'s doc comment) height, or unbounded for point text.
fn wrap_h_for(kind: TextKind) -> Option<f64> {
    match kind {
        TextKind::Area { height, .. } => height,
        TextKind::Point | TextKind::Path(_) => None,
    }
}

/// Anchor offset for point text: the click point is the left edge for
/// left-align, the centre for centre-align, the right edge for right-align
/// (Illustrator point-type behaviour). `w` is the laid-out text width.
/// Area text anchors at its box's top-left, so this only applies to
/// [`TextKind::Point`].
pub fn point_align_dx(align: TextAlign, w: f32) -> f64 {
    match align {
        TextAlign::Center | TextAlign::JustifyCenter => -(w as f64) / 2.0,
        TextAlign::End | TextAlign::JustifyRight => -(w as f64),
        _ => 0.0,
    }
}

/// A committed [`TextData`]'s parley layout, from the cache or freshly
/// built and filed. Re-shaping a paragraph every frame is the dominant
/// per-frame cost of a text box on the canvas, so this is memoized by
/// everything that affects shaping (see [`TextLayoutKey`]).
pub fn td_layout<'a>(tcx: &'a mut TextContext, td: &TextData) -> &'a Layout<Brush> {
    let key = TextLayoutKey::of(td);
    if tcx.td_cached(&key).is_none() {
        let width = match td.kind {
            TextKind::Area { width, .. } => Some(width as f32),
            TextKind::Point | TextKind::Path(_) => None,
        };
        let (fc, lc) = tcx.parts();
        let mut b = lc.ranged_builder(fc, &td.content, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
            vec![FontFamilyName::Named(Cow::Owned(td.style.family.clone()))],
        ))));
        b.push_default(StyleProperty::FontSize(td.style.size as f32));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(
            td.style.weight as f32,
        )));
        b.push_default(StyleProperty::FontStyle(if td.style.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        b.push_default(StyleProperty::LineHeight(line_height(&td.style)));
        b.push_default(StyleProperty::LetterSpacing(
            (td.style.tracking / 1000.0 * td.style.size) as f32,
        ));
        b.push_default(StyleProperty::Underline(td.style.underline));
        b.push_default(StyleProperty::Strikethrough(td.style.strikethrough));
        b.push_default(StyleProperty::FontFeatures(FontFeatures::from(features(
            &td.style,
        ))));
        let mut layout = b.build(&td.content);
        layout.break_all_lines(width);
        layout.align(
            alignment(td.align),
            parley::layout::AlignmentOptions::default(),
        );
        tcx.td_store(key.clone(), layout);
    }
    tcx.td_cached(&key).expect("just stored")
}

/// Lay out a committed [`TextData`] and draw it with transform `xf`.
pub fn paint_text_data(
    scene: &mut Scene,
    tcx: &mut TextContext,
    td: &TextData,
    xf: Affine,
    color: Color,
) {
    if td.content.is_empty() {
        return;
    }
    if td.vertical {
        let v = vertical_text::layout(tcx, &td.content, &td.style, wrap_h_for(td.kind));
        let clip = match td.kind {
            TextKind::Area { width, height: Some(h) } if v.height() > h + 0.5 => {
                scene.push_clip_layer(Fill::NonZero, xf, &Rect::new(-width, 0.0, 0.0, h));
                true
            }
            _ => false,
        };
        v.draw(scene, xf, color);
        if clip {
            scene.pop_layer();
        }
        return;
    }
    let layout = td_layout(tcx, td);
    // `TextKind::Path` never reaches here — canvas.rs routes it to
    // `pathtext::paint_path_text` instead, which needs the followed
    // path's geometry this function doesn't have. The arm below only
    // exists to keep the match exhaustive.
    let xf = match td.kind {
        TextKind::Point => xf * Affine::translate((point_align_dx(td.align, layout.width()), 0.0)),
        TextKind::Area { .. } | TextKind::Path(_) => xf,
    };
    // A fixed-height area box hides text past its bottom edge — but only
    // pay for the GPU clip layer when something actually overflows.
    let clip = match td.kind {
        TextKind::Area {
            width,
            height: Some(h),
        } if layout.height() as f64 > h + 0.5 => {
            scene.push_clip_layer(Fill::NonZero, xf, &Rect::new(0.0, 0.0, width, h));
            true
        }
        _ => false,
    };
    draw_glyph_runs(scene, layout, xf, color);
    if clip {
        scene.pop_layer();
    }
}

/// Lay out `td` and return its local bounds (top-left at the origin).
pub fn measure_text_data(td: &TextData, tcx: &mut TextContext) -> amalith_core::Rect {
    if let (TextKind::Path(pt), Some(pd)) = (td.kind, td.path_geometry.as_ref()) {
        if let Some(points) = pd.flattened_points(0.05).first() {
            let arc = amalith_core::ArcLengthPath::new(
                points,
                pd.subpaths().first().is_some_and(|s| s.closed),
            );
            return crate::pathtext::text_bounds(tcx, td, &pt, &arc, cg::Affine::IDENTITY);
        }
    }
    if td.vertical {
        let wrap_h = wrap_h_for(td.kind);
        let v = vertical_text::layout(tcx, &td.content, &td.style, wrap_h);
        return match td.kind {
            TextKind::Area { width, height } => {
                amalith_core::Rect::new(-width, 0.0, 0.0, height.unwrap_or_else(|| v.height()))
            }
            TextKind::Point | TextKind::Path(_) => {
                let b = v.bounds();
                amalith_core::Rect::new(b.x0, b.y0, b.x1.max(b.x0 + 1.0), b.y1.max(b.y0 + 1.0))
            }
        };
    }
    let layout = td_layout(tcx, td);
    let w = layout.width() as f64;
    let h = layout.height() as f64;
    match td.kind {
        TextKind::Point => {
            // Match the anchor offset applied in `paint_text_data` so the
            // local bounds track the drawn glyphs (selection box, hit test).
            let dx = point_align_dx(td.align, layout.width());
            amalith_core::Rect::new(dx, 0.0, dx + w.max(1.0), h.max(1.0))
        }
        TextKind::Area { width, height } => {
            amalith_core::Rect::new(0.0, 0.0, width, height.unwrap_or(h))
        }
        // Straight-line approximation — see the matching note in
        // `to_text_data` above. Selection-box use only, never painting.
        TextKind::Path(_) => amalith_core::Rect::new(0.0, 0.0, w.max(1.0), h.max(1.0)),
    }
}

/// `td`'s text with every soft wrap baked into a hard newline — for
/// converting area type to point type so the visible line layout is kept
/// (a 3-line wrapped paragraph becomes 3 lines with returns, not one).
pub fn hard_wrapped_content(td: &TextData, tcx: &mut TextContext) -> String {
    let layout = td_layout(tcx, td);
    let src = td.content.clone();
    let mut lines = layout.lines().peekable();
    let mut out = String::new();
    while let Some(line) = lines.next() {
        let seg = src
            .get(line.text_range())
            .unwrap_or("")
            .trim_end_matches(['\n', '\r', ' ', '\t']);
        out.push_str(seg);
        if lines.peek().is_some() {
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_alignment_is_reflected_without_a_keystroke() {
        let mut tcx = TextContext::new();
        let mut edit = TextEdit::new(
            amalith_core::ObjectId::new(),
            amalith_core::Point::ORIGIN,
            TextKind::Path(amalith_core::PathTextData {
                path: amalith_core::ObjectId::new(),
                start: 0.0,
                end: 500.0,
                align: amalith_core::PathTextAlign::Baseline,
                flip: false,
            }),
            TextStyle::default(),
            TextAlign::Start,
            Paragraph::default(),
            false,
            "Lorem ipsum",
            &mut tcx,
        );

        edit.set_align(TextAlign::End, &mut tcx);

        assert_eq!(edit.align(), TextAlign::End);
        assert_eq!(edit.to_text_data(&mut tcx).align, TextAlign::End);
        assert!(edit.editor.try_layout().is_some());
    }

    #[test]
    fn vertical_up_down_move_within_a_column_left_right_jump_columns() {
        let mut tcx = TextContext::new();
        let mut edit = TextEdit::new(
            amalith_core::ObjectId::new(),
            amalith_core::Point::ORIGIN,
            TextKind::Point,
            TextStyle::default(),
            TextAlign::Start,
            Paragraph::default(),
            true,
            "AB\nCD",
            &mut tcx,
        );
        // Start of the text (byte 0, 'A' in column 0).
        {
            let (fc, lc) = tcx.parts();
            edit.editor.driver(fc, lc).move_to_byte(0);
        }
        let mods = Mods::default();
        // Down moves within the column: 'A' (0) -> 'B' (1).
        edit.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowDown), mods, None, &mut tcx);
        assert_eq!(edit.editor.raw_selection().focus().index(), 1);
        // Left jumps to the same row in the next (further-left) column:
        // 'B' (column 0, row 1) -> 'D' (column 1, row 1, byte 4).
        edit.key(&winit::keyboard::Key::Named(winit::keyboard::NamedKey::ArrowLeft), mods, None, &mut tcx);
        assert_eq!(edit.editor.raw_selection().focus().index(), 4);
    }

    #[test]
    fn vertical_to_text_data_round_trips_the_flag_and_content() {
        let mut tcx = TextContext::new();
        let mut edit = TextEdit::new(
            amalith_core::ObjectId::new(),
            amalith_core::Point::ORIGIN,
            TextKind::Point,
            TextStyle::default(),
            TextAlign::Start,
            Paragraph::default(),
            true,
            "Hi",
            &mut tcx,
        );
        let data = edit.to_text_data(&mut tcx);
        assert!(data.vertical);
        assert_eq!(data.content, "Hi");
    }
}

/// Sinks one glyph's contours into a core [`BezPath`], each point pushed
/// through `xf`.
pub(crate) struct OutlineSink<'a> {
    pub(crate) path: &'a mut cg::BezPath,
    pub(crate) xf: cg::Affine,
    pub(crate) started: bool,
}

impl OutlinePen for OutlineSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.started {
            self.path.close_path();
        }
        self.path
            .move_to(self.xf * cg::Point::new(x as f64, y as f64));
        self.started = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path
            .line_to(self.xf * cg::Point::new(x as f64, y as f64));
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.path.quad_to(
            self.xf * cg::Point::new(cx as f64, cy as f64),
            self.xf * cg::Point::new(x as f64, y as f64),
        );
    }
    fn curve_to(&mut self, c0x: f32, c0y: f32, c1x: f32, c1y: f32, x: f32, y: f32) {
        self.path.curve_to(
            self.xf * cg::Point::new(c0x as f64, c0y as f64),
            self.xf * cg::Point::new(c1x as f64, c1y as f64),
            self.xf * cg::Point::new(x as f64, y as f64),
        );
    }
    fn close(&mut self) {
        if self.started {
            self.path.close_path();
            self.started = false;
        }
    }
}

/// Convert a committed [`TextData`]'s glyphs into one filled path in the
/// text object's local space — the same frame [`paint_text_data`] draws
/// in, so the result drops straight under the text object's transform.
/// Used by Type ▸ Create Outlines (⌘⇧O).
pub fn outline_text_data(td: &TextData, tcx: &mut TextContext) -> cg::BezPath {
    if let (TextKind::Path(pt), Some(pd)) = (td.kind, td.path_geometry.as_ref()) {
        if let Some(points) = pd.flattened_points(0.05).into_iter().next() {
            let arc = amalith_core::ArcLengthPath::new(
                &points,
                pd.subpaths().first().is_some_and(|s| s.closed),
            );
            return crate::pathtext::outline_path_text(tcx, td, &pt, &arc, cg::Affine::IDENTITY);
        }
    }
    let mut out = cg::BezPath::new();
    if td.content.is_empty() {
        return out;
    }
    let width = match td.kind {
        TextKind::Area { width, .. } => Some(width as f32),
        TextKind::Point | TextKind::Path(_) => None,
    };
    let (fc, lc) = tcx.parts();
    let mut b = lc.ranged_builder(fc, &td.content, 1.0, true);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(
        vec![FontFamilyName::Named(Cow::Owned(td.style.family.clone()))],
    ))));
    b.push_default(StyleProperty::FontSize(td.style.size as f32));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(
        td.style.weight as f32,
    )));
    b.push_default(StyleProperty::FontStyle(if td.style.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }));
    b.push_default(StyleProperty::LineHeight(line_height(&td.style)));
    b.push_default(StyleProperty::LetterSpacing(
        (td.style.tracking / 1000.0 * td.style.size) as f32,
    ));
    b.push_default(StyleProperty::FontFeatures(FontFeatures::from(features(
        &td.style,
    ))));
    let mut layout = b.build(&td.content);
    layout.break_all_lines(width);
    layout.align(
        alignment(td.align),
        parley::layout::AlignmentOptions::default(),
    );

    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let mut gx = run.offset();
            let gy = run.baseline();
            let r = run.run();
            let font = r.font();
            let font_size = r.font_size();
            // parley hands back the raw fixed-point bits; skrifa wants its
            // `NormalizedCoord` newtype. Usually empty (non-variable font).
            let loc: Vec<skrifa::instance::NormalizedCoord> = r
                .normalized_coords()
                .iter()
                .map(|&c| skrifa::instance::NormalizedCoord::from_bits(c))
                .collect();
            let Ok(font_ref) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else {
                continue;
            };
            let glyphs = font_ref.outline_glyphs();
            for g in run.glyphs() {
                let x = (gx + g.x) as f64;
                let y = (gy - g.y) as f64;
                gx += g.advance;
                let Some(glyph) = glyphs.get(GlyphId::new(g.id as u32)) else {
                    continue;
                };
                // skrifa emits px, y-up, glyph origin; the layout frame is
                // y-down with this glyph's baseline at `y`.
                let xf = cg::Affine::new([1.0, 0.0, 0.0, -1.0, x, y]);
                let mut sink = OutlineSink {
                    path: &mut out,
                    xf,
                    started: false,
                };
                let settings = DrawSettings::unhinted(Size::new(font_size), LocationRef::new(&loc));
                let _ = glyph.draw(settings, &mut sink);
                sink.close();
            }
        }
    }
    out
}

/// The shared vello glyph-run loop.
pub fn draw_glyph_runs<B: parley::style::Brush>(
    scene: &mut Scene,
    layout: &parley::Layout<B>,
    xf: Affine,
    color: Color,
) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(run) = item else {
                continue;
            };
            let mut gx = run.offset();
            let gy = run.baseline();
            let r = run.run();
            let font = r.font();
            let size = r.font_size();
            let coords = r.normalized_coords();
            let skew = r
                .synthesis()
                .skew()
                .map(|a| Affine::skew(a.to_radians().tan() as f64, 0.0));
            scene
                .draw_glyphs(font)
                .brush(&Brush::Solid(color))
                // Unhinted: hinting re-runs per frame at every pan offset and
                // was ~30 ms/frame for a paragraph. Canvas text isn't hinted
                // in Illustrator either.
                .hint(false)
                .transform(xf)
                .glyph_transform(skew)
                .font_size(size)
                .normalized_coords(coords)
                .draw(
                    Fill::NonZero,
                    run.glyphs().map(|g| {
                        let x = gx + g.x;
                        let y = gy - g.y;
                        gx += g.advance;
                        Glyph {
                            id: g.id as u32,
                            x,
                            y,
                        }
                    }),
                );
        }
    }
}
