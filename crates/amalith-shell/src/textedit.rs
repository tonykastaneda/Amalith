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
use skrifa::{
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    GlyphId, MetadataProvider,
};
use parley::style::{
    FontFamily, FontFamilyName, FontFeatures, FontStyle, FontWeight, LineHeight, StyleProperty,
};
use parley::{Alignment, Layout, PlainEditor};
use vello::kurbo::{Affine, Rect, Stroke};
use vello::peniko::{Brush, Color, Fill};
use vello::{Glyph, Scene};

use crate::text::{TextContext, TextLayoutKey};

/// A caret size hint for `cursor_geometry`, in editor px.
const CARET_W: f32 = 1.5;

/// One in-progress text edit.
pub struct TextEdit {
    /// The document object being edited.
    pub object: amalith_core::ObjectId,
    /// Document-space anchor: point-type baseline-left / area-type top-left.
    pub origin: amalith_core::Point,
    editor: PlainEditor<Brush>,
    kind: TextKind,
    style: TextStyle,
    align: TextAlign,
    paragraph: Paragraph,
    /// Thread links carried through so a commit doesn't sever them.
    thread_next: Option<amalith_core::ObjectId>,
    thread_prev: Option<amalith_core::ObjectId>,
    /// True from the first keystroke — a never-touched object is discarded
    /// on commit.
    pub touched: bool,
}

impl TextEdit {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        object: amalith_core::ObjectId,
        origin: amalith_core::Point,
        kind: TextKind,
        style: TextStyle,
        align: TextAlign,
        paragraph: Paragraph,
        seed: &str,
        tcx: &mut TextContext,
    ) -> Self {
        let mut editor = PlainEditor::<Brush>::new(style.size as f32);
        editor.set_text(seed);
        if let TextKind::Area { width, .. } = kind {
            editor.set_width(Some(width as f32));
        } else {
            editor.set_width(None);
        }
        editor.set_alignment(alignment(align));
        let mut this = Self {
            object,
            origin,
            editor,
            kind,
            style: style.clone(),
            align,
            paragraph,
            thread_next: None,
            thread_prev: None,
            touched: !seed.is_empty(),
        };
        this.apply_style(&style, tcx);
        this.set_paragraph(paragraph);
        this
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
        s.insert(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(vec![
            FontFamilyName::Named(Cow::Owned(style.family.clone())),
        ]))));
        s.insert(StyleProperty::FontSize(style.size as f32));
        s.insert(StyleProperty::FontWeight(FontWeight::new(style.weight as f32)));
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

    pub fn set_align(&mut self, align: TextAlign) {
        self.align = align;
        self.editor.set_alignment(alignment(align));
    }

    pub fn paragraph(&self) -> Paragraph {
        self.paragraph
    }

    /// Update the paragraph attributes. Left / right indent narrow the
    /// area-text wrap width live; the rest are recorded for commit /
    /// export.
    pub fn set_paragraph(&mut self, p: Paragraph) {
        self.paragraph = p;
        if let TextKind::Area { width, .. } = self.kind {
            let inner = (width - p.indent_start - p.indent_end).max(1.0);
            self.editor.set_width(Some(inner as f32));
        }
    }

    pub fn set_area_width(&mut self, width: f64) {
        if let TextKind::Area { width: w, height } = &mut self.kind {
            *w = width;
            let h = *height;
            self.kind = TextKind::Area { width, height: h };
        }
        self.editor.set_width(Some(width as f32));
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
        self.editor
            .selection_geometry()
            .into_iter()
            .map(|(b, _)| Rect::new(b.x0, b.y0, b.x1, b.y1))
            .collect()
    }

    /// The caret rectangle, in editor space, if the cursor is shown.
    pub fn caret_rect(&self) -> Option<Rect> {
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
        let (fc, lc) = tcx.parts();
        let mut drv = self.editor.driver(fc, lc);
        let sel = mods.shift;
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
            Key::Named(NamedKey::ArrowLeft) => match (sel, mods.alt, mods.meta) {
                (false, false, false) => drv.move_left(),
                (true, false, false) => drv.select_left(),
                (false, true, _) => drv.move_word_left(),
                (true, true, _) => drv.select_word_left(),
                (false, _, true) => drv.move_to_line_start(),
                (true, _, true) => drv.select_to_line_start(),
            },
            Key::Named(NamedKey::ArrowRight) => match (sel, mods.alt, mods.meta) {
                (false, false, false) => drv.move_right(),
                (true, false, false) => drv.select_right(),
                (false, true, _) => drv.move_word_right(),
                (true, true, _) => drv.select_word_right(),
                (false, _, true) => drv.move_to_line_end(),
                (true, _, true) => drv.select_to_line_end(),
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
            Key::Character(c) if mods.meta => {
                match c.as_str() {
                    "a" | "A" => drv.select_all(),
                    _ => return KeyResult::PassThrough,
                }
            }
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
        KeyResult::Handled
    }

    pub fn insert_str(&mut self, s: &str, tcx: &mut TextContext) {
        let (fc, lc) = tcx.parts();
        self.editor
            .driver(fc, lc)
            .insert_or_replace_selection(s);
        self.touched = true;
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
        let (fc, lc) = tcx.parts();
        let mut drv = self.editor.driver(fc, lc);
        match clicks {
            0 | 1 => drv.move_to_point(p.0, p.1),
            2 => drv.select_word_at_point(p.0, p.1),
            _ => drv.select_all(),
        }
    }

    pub fn pointer_drag(&mut self, p: (f32, f32), tcx: &mut TextContext) {
        let (fc, lc) = tcx.parts();
        self.editor
            .driver(fc, lc)
            .extend_selection_to_point(p.0, p.1);
    }

    // --- render / commit -------------------------------------------------

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
        let xf = match self.kind {
            TextKind::Point => {
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
                        Color::from_rgb8(0xd0, 0x30, 0x30),
                        None,
                        &m,
                    );
                }
            }
        }
    }

    /// Recompute bounds from the current layout and produce the committed
    /// [`TextData`].
    pub fn to_text_data(&mut self, tcx: &mut TextContext) -> TextData {
        let content = self.text();
        let (fc, lc) = tcx.parts();
        let layout = self.editor.layout(fc, lc);
        let w = layout.width() as f64;
        let h = layout.height() as f64;
        let bounds = match self.kind {
            TextKind::Point => {
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
        };
        TextData {
            content,
            kind: self.kind,
            style: self.style.clone(),
            align: self.align,
            paragraph: self.paragraph,
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
    /// Not an editing key — let the shell handle it (e.g. ⌘Z, ⌘S).
    PassThrough,
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
            TextKind::Point => None,
        };
        let (fc, lc) = tcx.parts();
        let mut b = lc.ranged_builder(fc, &td.content, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(vec![
            FontFamilyName::Named(Cow::Owned(td.style.family.clone())),
        ]))));
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
    let layout = td_layout(tcx, td);
    let xf = match td.kind {
        TextKind::Point => xf * Affine::translate((point_align_dx(td.align, layout.width()), 0.0)),
        TextKind::Area { .. } => xf,
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
    }
}

/// Sinks one glyph's contours into a core [`BezPath`], each point pushed
/// through `xf`.
struct OutlineSink<'a> {
    path: &'a mut cg::BezPath,
    xf: cg::Affine,
    started: bool,
}

impl OutlinePen for OutlineSink<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.started {
            self.path.close_path();
        }
        self.path.move_to(self.xf * cg::Point::new(x as f64, y as f64));
        self.started = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to(self.xf * cg::Point::new(x as f64, y as f64));
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
    let mut out = cg::BezPath::new();
    if td.content.is_empty() {
        return out;
    }
    let width = match td.kind {
        TextKind::Area { width, .. } => Some(width as f32),
        TextKind::Point => None,
    };
    let (fc, lc) = tcx.parts();
    let mut b = lc.ranged_builder(fc, &td.content, 1.0, true);
    b.push_default(StyleProperty::FontFamily(FontFamily::List(Cow::Owned(vec![
        FontFamilyName::Named(Cow::Owned(td.style.family.clone())),
    ]))));
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
                let settings =
                    DrawSettings::unhinted(Size::new(font_size), LocationRef::new(&loc));
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
