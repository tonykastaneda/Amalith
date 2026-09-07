//! Small chrome widgets shared across the shell's dialogs and panels, so
//! they read as one app instead of each modal having quietly grown its own
//! near-identical button.

use vello::kurbo::{Affine, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

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
