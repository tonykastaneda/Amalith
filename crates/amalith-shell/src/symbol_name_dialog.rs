//! Name entry for a pending symbol conversion. Artwork is captured on open.
use crate::{metrics::px, text::TextContext, theme::Theme};
use amalith_core::ObjectId;
use vello::{
    kurbo::{Affine, Point, Rect, Stroke},
    peniko::Fill,
    Scene,
};
pub struct SymbolNameDialog {
    pub ids: Vec<ObjectId>,
    pub buf: String,
    pub fresh: bool,
}
pub fn width() -> f64 {
    px(320.0)
}
pub fn height() -> f64 {
    px(126.0)
}
fn buttons(body: Rect) -> [Rect; 2] {
    let y = body.y1 - px(38.0);
    [
        Rect::new(body.x1 - px(190.0), y, body.x1 - px(104.0), y + px(26.0)),
        Rect::new(body.x1 - px(94.0), y, body.x1 - px(8.0), y + px(26.0)),
    ]
}
pub fn hit(body: Rect, p: Point) -> Option<bool> {
    let [cancel, ok] = buttons(body);
    if cancel.contains(p) {
        Some(false)
    } else if ok.contains(p) {
        Some(true)
    } else {
        None
    }
}
pub fn paint(
    scene: &mut Scene,
    d: &SymbolNameDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
) {
    text.draw(
        scene,
        "Symbol name",
        12.0,
        theme.text,
        body.x0 + px(12.0),
        body.y0 + px(22.0),
    );
    let field = Rect::new(
        body.x0 + px(12.0),
        body.y0 + px(32.0),
        body.x1 - px(12.0),
        body.y0 + px(60.0),
    );
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field);
    scene.stroke(
        &Stroke::new(px(1.0)),
        Affine::IDENTITY,
        theme.accent,
        None,
        &field,
    );
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &field.inflate(-px(3.0), -px(3.0)));
    let w = text.measure(&d.buf, 12.0);
    let x = (field.x0 + px(6.0)).min(field.x1 - px(8.0) - w);
    if d.fresh {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.accent.with_alpha(0.3),
            None,
            &Rect::new(x, field.y0 + px(4.0), x + w, field.y1 - px(4.0)),
        );
    }
    text.draw(scene, &d.buf, 12.0, theme.text, x, field.y0 + px(19.0));
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.text,
        None,
        &Rect::new(
            x + w + 1.0,
            field.y0 + px(5.0),
            x + w + px(2.0),
            field.y1 - px(5.0),
        ),
    );
    scene.pop_layer();
    let [cancel, ok] = buttons(body);
    crate::widgets::button(scene, text, theme, cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, ok, "OK", true);
}
