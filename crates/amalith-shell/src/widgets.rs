//! Small chrome widgets shared across the shell's dialogs and panels, so
//! they read as one app instead of each modal having quietly grown its own
//! near-identical button.

use vello::kurbo::{Affine, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use amalith_core::MeasureKind;

use crate::text::TextContext;
use crate::theme::Theme;

/// Whether `ch` may appear while typing into a measurement field —
/// digits, a decimal point, sign, the four arithmetic operators,
/// parentheses, and a unit-suffix letter/symbol (`px`, `pt`, `in`, `mm`,
/// `cm`, `pc`, `ft`, `yd`, `m`, `°`, `%`, `"`, `'`) — matching what
/// [`amalith_core::parse_measurement`] can consume. Shared by every
/// field's per-keystroke filter so widening what's typable and widening
/// what's parseable never drift apart.
pub fn measurement_char(ch: char) -> bool {
    ch.is_ascii_digit()
        || ch.is_ascii_alphabetic()
        || matches!(ch, '.' | ',' | '-' | '+' | '*' | '/' | '(' | ')' | ' ' | '%' | '\'' | '"' | '\u{b0}')
}

/// The standard dialog-footer button: sharp corners, a solid `theme.accent`
/// fill for `primary` (Create, OK, Save, Open — the default action), a
/// `theme.strip_active` fill with a hairline border otherwise (Cancel,
/// Don't Save, Import). Established by the New Document dialog's Create /
/// Cancel pair; every other Cancel/OK-style button in the app should paint
/// through this rather than rolling its own.
pub fn button(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, primary: bool) {
    let fill = if primary { theme.accent } else { theme.strip_active };
    scene.fill(Fill::NonZero, Affine::IDENTITY, fill, None, &r);
    if !primary {
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            theme.text_dim.with_alpha(0.6),
            None,
            &r,
        );
    }
    let col = if primary { theme.on_accent } else { theme.text };
    let w = text.measure(label, 12.5);
    text.draw(
        scene,
        label,
        12.5,
        col,
        r.x0 + (r.width() - w) * 0.5,
        r.y0 + r.height() * 0.5 + 4.5,
    );
}

// -------------------------------------------------------- numeric edit fields

/// A numeric field's live edit buffer: the text typed so far, and whether
/// it's still "fresh" — true from the moment the field is clicked into
/// until the first keystroke, so typing replaces the seeded value instead
/// of appending to it (and so [`draw_field_value`] paints it as
/// selected). Every options-bar / flyout numeric field (Stroke weight,
/// Opacity, Transform X/Y/W/H, dash/gap, …) owns one of these as
/// `Option<NumEdit>` and drives it with [`edit_key`].
pub struct NumEdit {
    pub buf: String,
    pub fresh: bool,
    /// What the buffer's own (unsuffixed) numbers mean, and what other
    /// units typing a suffix (`5in`, `3pt`, …) may convert *from* — see
    /// [`amalith_core::parse_measurement`]. Nudging (Up/Down) and commit
    /// parsing both go through this, so every `NumEdit` field
    /// consistently shows and accepts its own unit's initials.
    pub kind: MeasureKind,
}

impl NumEdit {
    /// Begin editing, seeded from the field's current value — freshly
    /// selected, so the first keystroke overwrites it.
    pub fn seeded(seed: impl Into<String>, kind: MeasureKind) -> Self {
        Self { buf: seed.into(), fresh: true, kind }
    }
}

/// What one keystroke did to a [`NumEdit`] in progress.
pub enum EditOutcome {
    /// Consumed — stay in the field.
    Consumed,
    /// Enter: apply `buf`, leave edit mode, and consume the key (nothing
    /// else should react to an Enter that just committed a field).
    Commit,
    /// A non-numeric key, or one with no text at all (an arrow key, a
    /// modifier): apply `buf`, leave edit mode, but *don't* consume the
    /// key — the caller should let it fall through to whatever it would
    /// normally do (a tool shortcut, arrow-key navigation, …).
    CommitAndPassThrough,
    /// Esc: leave edit mode, discarding `buf`.
    Cancel,
}

/// Route one key event to a field's edit buffer. Digits, `.`, `-`, `+`
/// insert (clearing the buffer first if `fresh`); Backspace deletes;
/// Up / Down nudge the buffer's numeric value by 1 (5 with `shift`, 0.1
/// with `cmd` — `cmd` wins if both are held) — consumed here rather than
/// falling through, or they'd land on the canvas as an object-nudge /
/// other shortcut while a field is focused; Enter commits; Esc cancels;
/// anything else (a non-numeric character, or a key with no text at all —
/// a modifier, a function key) commits first so it can fall through to
/// whatever it would normally do. Identical across every numeric field in
/// the shell — only *what a commit applies* (the parsed value, to which
/// command) differs, which is the caller's job once this returns
/// [`EditOutcome::Commit`].
pub fn edit_key(edit: &mut NumEdit, event: &winit::event::KeyEvent, shift: bool, cmd: bool) -> EditOutcome {
    use winit::keyboard::{KeyCode, PhysicalKey};
    if !event.state.is_pressed() {
        return EditOutcome::Consumed;
    }
    match event.physical_key {
        PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => EditOutcome::Commit,
        PhysicalKey::Code(KeyCode::Escape) => EditOutcome::Cancel,
        PhysicalKey::Code(KeyCode::Backspace) => {
            edit.fresh = false;
            edit.buf.pop();
            EditOutcome::Consumed
        }
        PhysicalKey::Code(KeyCode::ArrowUp | KeyCode::ArrowDown) => {
            let dir = if event.physical_key == PhysicalKey::Code(KeyCode::ArrowUp) { 1.0 } else { -1.0 };
            let step = if cmd { 0.1 } else if shift { 5.0 } else { 1.0 };
            let cur = amalith_core::parse_measurement(&edit.buf, edit.kind).unwrap_or(0.0);
            edit.buf = format_number(cur + dir * step);
            edit.fresh = false;
            EditOutcome::Consumed
        }
        // A bare modifier keydown (Shift, held in preparation for
        // Shift+Up/Down) has no text of its own — without this it would
        // fall to the `_` arm below and be treated as "some other key",
        // committing and exiting the field right as the user presses
        // Shift, before the arrow key that was meant to land *in* it.
        PhysicalKey::Code(
            KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight,
        ) => EditOutcome::Consumed,
        _ => {
            let Some(txt) = event.text.as_ref() else {
                return EditOutcome::CommitAndPassThrough;
            };
            let numeric = txt.chars().all(measurement_char);
            if !numeric {
                return EditOutcome::CommitAndPassThrough;
            }
            for ch in txt.chars().filter(|c| !c.is_control()) {
                if edit.fresh {
                    edit.buf.clear();
                    edit.fresh = false;
                }
                edit.buf.push(ch);
            }
            EditOutcome::Consumed
        }
    }
}

/// Formats a stepped value back into a plain editable number — no unit
/// suffix, so it reads as "still being typed" rather than a committed,
/// re-suffixed display value (trailing zeros trimmed, so `12.0` becomes
/// `12`).
fn format_number(v: f64) -> String {
    let r = (v * 10_000.0).round() / 10_000.0;
    if (r - r.round()).abs() < 5e-5 {
        format!("{}", r.round() as i64)
    } else {
        let s = format!("{r:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// Selection-highlight band drawn behind a field's value while it's
/// `fresh` — the visual half of "click a number field and it's
/// highlighted, ready to type over". Draw this *before* the value text.
pub fn draw_field_highlight(scene: &mut Scene, theme: &Theme, value_rect: Rect) {
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent.with_alpha(0.35), None, &value_rect);
}
