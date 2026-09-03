//! A single-line editable text field — the reusable widget the ad-hoc
//! "append `event.text`, pop on Backspace" input handlers around the app
//! should migrate to.
//!
//! Backed by `parley::PlainEditor` (same engine as the canvas Type tool),
//! so it gets a real caret, mouse-driven selection, word / line cursor
//! motion, and clipboard for free. It owns only the text + caret; the
//! caller draws the box and decides what Enter / Esc / Tab mean.

use parley::PlainEditor;
use vello::kurbo::{Affine, Point, Rect};
use vello::peniko::{Brush, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::textedit::{draw_glyph_runs, Mods};
use crate::theme::Theme;

/// Left / right padding between the box edge and the text, in px.
const INSET: f64 = 10.0;
const FONT_PX: f32 = 13.5;

/// What a keystroke asked the caller to do.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resp {
    /// The text changed.
    Changed,
    /// Handled (caret move, selection, …); text unchanged.
    Handled,
    /// Enter — commit.
    Submit,
    /// Esc — cancel.
    Cancel,
    /// Tab — move focus (`true` = Shift+Tab, backward).
    Tab(bool),
    /// Not consumed — the caller may act on it.
    Pass,
}

pub struct TextField {
    ed: PlainEditor<Brush>,
    /// Screen rect of the whole box from the last `paint`, for hit-testing.
    rect: Rect,
}

impl TextField {
    pub fn new(initial: &str) -> Self {
        let mut ed = PlainEditor::<Brush>::new(FONT_PX);
        ed.set_text(initial);
        ed.set_width(None); // single line, no wrap
        Self {
            ed,
            rect: Rect::ZERO,
        }
    }

    pub fn text(&self) -> String {
        self.ed.text().chars().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.ed.text().chars().next().is_none()
    }

    pub fn set_text(&mut self, s: &str) {
        self.ed.set_text(s);
    }

    /// True if `p` (screen px) is inside the field's box.
    pub fn contains(&self, p: Point) -> bool {
        self.rect.contains(p)
    }

    /// Put the caret past the end and select everything — the usual state
    /// when a field gains focus.
    pub fn select_all(&mut self, tcx: &mut TextContext) {
        let (fc, lc) = tcx.parts();
        self.ed.driver(fc, lc).select_all();
    }

    /// A press at `p` (screen px). `clicks`: 1 caret, 2 word, 3+ all.
    pub fn pointer_down(&mut self, p: Point, clicks: u32, tcx: &mut TextContext) {
        let (x, y) = self.local(p);
        let (fc, lc) = tcx.parts();
        let mut drv = self.ed.driver(fc, lc);
        match clicks {
            0 | 1 => drv.move_to_point(x, y),
            2 => drv.select_word_at_point(x, y),
            _ => drv.select_all(),
        }
    }

    pub fn pointer_drag(&mut self, p: Point, tcx: &mut TextContext) {
        let (x, y) = self.local(p);
        let (fc, lc) = tcx.parts();
        self.ed.driver(fc, lc).extend_selection_to_point(x, y);
    }

    fn local(&self, p: Point) -> (f32, f32) {
        (
            (p.x - self.rect.x0 - INSET) as f32,
            (p.y - self.rect.y0 - self.rect.height() * 0.5 + FONT_PX as f64 * 0.5) as f32,
        )
    }

    /// Feed one key. `text` is `event.text` (typed characters).
    pub fn key(
        &mut self,
        key: &winit::keyboard::Key,
        mods: Mods,
        text: Option<&str>,
        clip: Option<&mut arboard::Clipboard>,
        tcx: &mut TextContext,
    ) -> Resp {
        use winit::keyboard::{Key, NamedKey};
        let (fc, lc) = tcx.parts();
        let mut drv = self.ed.driver(fc, lc);
        let sel = mods.shift;
        match key {
            Key::Named(NamedKey::Escape) => return Resp::Cancel,
            Key::Named(NamedKey::Enter) => return Resp::Submit,
            Key::Named(NamedKey::Tab) => return Resp::Tab(mods.shift),
            Key::Named(NamedKey::Backspace) => {
                if mods.alt {
                    drv.backdelete_word();
                } else {
                    drv.backdelete();
                }
                return Resp::Changed;
            }
            Key::Named(NamedKey::Delete) => {
                if mods.alt {
                    drv.delete_word();
                } else {
                    drv.delete();
                }
                return Resp::Changed;
            }
            Key::Named(NamedKey::ArrowLeft) => match (sel, mods.alt || mods.meta) {
                (false, false) => drv.move_left(),
                (true, false) => drv.select_left(),
                (false, true) => drv.move_word_left(),
                (true, true) => drv.select_word_left(),
            },
            Key::Named(NamedKey::ArrowRight) => match (sel, mods.alt || mods.meta) {
                (false, false) => drv.move_right(),
                (true, false) => drv.select_right(),
                (false, true) => drv.move_word_right(),
                (true, true) => drv.select_word_right(),
            },
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
                drop(drv);
                return self.cmd_combo(c.as_str(), clip, tcx);
            }
            _ => {
                if let Some(t) = text {
                    let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        drv.insert_or_replace_selection(&clean);
                        return Resp::Changed;
                    }
                }
                return Resp::Pass;
            }
        }
        Resp::Handled
    }

    fn cmd_combo(
        &mut self,
        c: &str,
        clip: Option<&mut arboard::Clipboard>,
        tcx: &mut TextContext,
    ) -> Resp {
        let (fc, lc) = tcx.parts();
        match c {
            "a" | "A" => {
                self.ed.driver(fc, lc).select_all();
                Resp::Handled
            }
            "c" | "C" => {
                if let (Some(cb), Some(s)) = (clip, self.ed.selected_text()) {
                    let _ = cb.set_text(s.to_string());
                }
                Resp::Handled
            }
            "x" | "X" => {
                let cut = self.ed.selected_text().map(|s| s.to_string());
                if let (Some(cb), Some(s)) = (clip, cut) {
                    let _ = cb.set_text(s);
                    self.ed.driver(fc, lc).delete_selection();
                    return Resp::Changed;
                }
                Resp::Handled
            }
            "v" | "V" => {
                let paste = clip.and_then(|cb| cb.get_text().ok());
                if let Some(t) = paste {
                    let clean: String = t.chars().filter(|c| !c.is_control()).collect();
                    if !clean.is_empty() {
                        self.ed.driver(fc, lc).insert_or_replace_selection(&clean);
                        return Resp::Changed;
                    }
                }
                Resp::Handled
            }
            _ => Resp::Pass,
        }
    }

    /// Draw the text (selection + glyphs + caret) inside `box_` (screen
    /// px). The caller draws the box background / border. `placeholder`
    /// shows dim when the field is empty. `caret_on` blinks the caret.
    pub fn paint(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        box_: Rect,
        placeholder: &str,
        caret_on: bool,
    ) {
        self.rect = box_;
        let inner = Rect::new(box_.x0 + INSET, box_.y0, box_.x1 - INSET, box_.y1);
        let baseline_y = box_.y0 + box_.height() * 0.5 - FONT_PX as f64 * 0.5;
        let xf = Affine::translate((inner.x0, baseline_y));

        if self.is_empty() {
            tcx.draw(
                scene,
                placeholder,
                FONT_PX,
                theme.text_dim,
                inner.x0,
                box_.y0 + box_.height() * 0.5 + 4.5,
            );
            return;
        }

        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &inner);
        {
            let (fc, lc) = tcx.parts();
            self.ed.refresh_layout(fc, lc);
        }
        let hl = theme.accent.multiply_alpha(0.35);
        for (b, _) in self.ed.selection_geometry() {
            scene.fill(
                Fill::NonZero,
                xf,
                hl,
                None,
                &Rect::new(b.x0, b.y0, b.x1, b.y1),
            );
        }
        let (fc, lc) = tcx.parts();
        let layout = self.ed.layout(fc, lc);
        draw_glyph_runs(scene, layout, xf, theme.text);
        if caret_on {
            if let Some(b) = self.ed.cursor_geometry(1.5) {
                scene.fill(
                    Fill::NonZero,
                    xf,
                    theme.text,
                    None,
                    &Rect::new(b.x0, b.y0, b.x1, b.y1),
                );
            }
        }
        scene.pop_layer();
    }
}
