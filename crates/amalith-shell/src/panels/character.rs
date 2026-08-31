//! Character panel — the Illustrator type-attribute controls.
//!
//! Reads [`Ctx::text_style`] (the live text edit, else the selected text
//! object, else the new-text defaults) and emits [`Action`]s the shell
//! applies back. v1 is whole-object: no per-character runs.
//!
//! Live in v1: family, style, size, leading, tracking, and the
//! underline / strikethrough / caps / super- / sub-script toggles.
//! Greyed for now (v2): kerning mode, vertical / horizontal scale,
//! baseline shift, character rotation.

use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{Action, Ctx, FontMenu, TextFlag, ID, PAD};

const ROW_H: f64 = 30.0;
const GAP: f64 = 8.0;
const FIELD_H: f64 = 24.0;

/// Preset font sizes for the Size dropdown.
pub const SIZE_PRESETS: [f64; 16] = [
    6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0, 24.0, 30.0, 36.0, 48.0, 60.0, 72.0, 96.0, 144.0,
];

/// Human label for a weight / italic pair.
pub fn face_label(weight: u16, italic: bool) -> String {
    let w = match weight {
        0..=149 => "Thin",
        150..=249 => "Extra Light",
        250..=349 => "Light",
        350..=449 => "Regular",
        450..=549 => "Medium",
        550..=649 => "Semibold",
        650..=749 => "Bold",
        750..=849 => "Extra Bold",
        _ => "Black",
    };
    match (w, italic) {
        ("Regular", true) => "Italic".into(),
        ("Regular", false) => "Regular".into(),
        (w, true) => format!("{w} Italic"),
        (w, false) => w.to_string(),
    }
}

struct L {
    family: Rect,
    style: Rect,
    size: Field,
    leading: Field,
    kerning: Field,
    tracking: Field,
    vscale: Field,
    hscale: Field,
    baseline: Field,
    rotation: Field,
    toggles: [Rect; 6],
}

struct Field {
    /// Whole field box (label glyph + value + steppers).
    box_: Rect,
    /// Clickable value area (opens a menu for Size).
    value: Rect,
    up: Rect,
    down: Rect,
}

fn field(x: f64, y: f64, w: f64) -> Field {
    let box_ = Rect::new(x, y, x + w, y + FIELD_H);
    let sx = box_.x1 - 16.0;
    Field {
        box_,
        value: Rect::new(x + 22.0, y, sx - 2.0, y + FIELD_H),
        up: Rect::new(sx, y + 1.0, box_.x1, y + FIELD_H / 2.0),
        down: Rect::new(sx, y + FIELD_H / 2.0, box_.x1, y + FIELD_H - 1.0),
    }
}

fn layout(body: Rect) -> L {
    let x = body.x0 + PAD;
    let w = body.width() - PAD * 2.0;
    let half = (w - GAP) / 2.0;
    // Leave room under the tab strip for the header hint.
    let mut y = body.y0 + 30.0;
    let full = |y: f64| Rect::new(x, y, x + w, y + FIELD_H);

    let family = full(y);
    y += FIELD_H + GAP;
    let style = full(y);
    y += FIELD_H + 14.0;

    let row = |y: f64| (field(x, y, half), field(x + half + GAP, y, half));
    let (size, leading) = row(y);
    y += ROW_H;
    let (kerning, tracking) = row(y);
    y += ROW_H;
    let (vscale, hscale) = row(y);
    y += ROW_H;
    let (baseline, rotation) = row(y);
    y += ROW_H + 10.0;

    let tw = 30.0;
    let tgap = (w - tw * 6.0) / 5.0;
    let toggles = std::array::from_fn(|i| {
        let tx = x + i as f64 * (tw + tgap);
        Rect::new(tx, y, tx + tw, y + 26.0)
    });

    L {
        family,
        style,
        size,
        leading,
        kerning,
        tracking,
        vscale,
        hscale,
        baseline,
        rotation,
        toggles,
    }
}

fn fmt_pt(v: f64) -> String {
    if (v.fract()).abs() < 0.05 {
        format!("{} pt", v.round() as i64)
    } else {
        format!("{v:.1} pt")
    }
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = layout(body);
    let th = ctx.theme;
    let s = &ctx.text_style;

    // Header hint.
    let hint = if ctx.text_editing {
        "Character — editing"
    } else if ctx
        .selection
        .iter()
        .any(|id| matches!(ctx.doc.object(*id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Text(_))))
    {
        "Character — selection"
    } else {
        "Character — new text"
    };
    text.draw(scene, hint, 10.0, th.text_dim, body.x0 + PAD, body.y0 + 18.0);

    combo(scene, text, th, l.family, &s.family, ctx.pointer);
    combo(
        scene,
        text,
        th,
        l.style,
        &face_label(s.weight, s.italic),
        ctx.pointer,
    );

    let auto_leading = s.leading.is_none();
    let leading_val = s.leading.unwrap_or(s.size * 1.2);
    stepper(scene, text, th, &l.size, "T", &fmt_pt(s.size), true, ctx.pointer);
    stepper(
        scene,
        text,
        th,
        &l.leading,
        "A",
        &if auto_leading {
            format!("({})", fmt_pt(leading_val))
        } else {
            fmt_pt(leading_val)
        },
        true,
        ctx.pointer,
    );
    stepper(scene, text, th, &l.kerning, "VA", "Auto", false, ctx.pointer);
    stepper(
        scene,
        text,
        th,
        &l.tracking,
        "VA",
        &format!("{}", s.tracking.round() as i64),
        true,
        ctx.pointer,
    );
    stepper(scene, text, th, &l.vscale, "T", "100%", false, ctx.pointer);
    stepper(scene, text, th, &l.hscale, "T", "100%", false, ctx.pointer);
    stepper(scene, text, th, &l.baseline, "A", "0 pt", false, ctx.pointer);
    stepper(scene, text, th, &l.rotation, "T", "0°", false, ctx.pointer);

    let flags = [
        ("TT", flag_on(ctx, TextFlag::AllCaps)),
        ("Tc", flag_on(ctx, TextFlag::SmallCaps)),
        ("T¹", flag_on(ctx, TextFlag::Superscript)),
        ("T₁", flag_on(ctx, TextFlag::Subscript)),
        ("U", flag_on(ctx, TextFlag::Underline)),
        ("S", flag_on(ctx, TextFlag::Strikethrough)),
    ];
    for (r, (label, on)) in l.toggles.iter().zip(flags) {
        let hot = r.contains(ctx.pointer);
        let bg = if on {
            th.accent
        } else if hot {
            th.strip_bg
        } else {
            th.bg
        };
        scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(4.0));
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &r.to_rounded_rect(4.0));
        // Dark glyph over the gold accent, light otherwise.
        let col = if on {
            Color::from_rgb8(0x1a, 0x14, 0x00)
        } else {
            th.text
        };
        let w = text.measure(label, 11.0);
        text.draw(
            scene,
            label,
            11.0,
            col,
            r.center().x - w / 2.0,
            r.center().y + 4.0,
        );
    }
}

fn flag_on(ctx: &Ctx, f: TextFlag) -> bool {
    let s = &ctx.text_style;
    match f {
        TextFlag::Underline => s.underline,
        TextFlag::Strikethrough => s.strikethrough,
        TextFlag::SmallCaps => s.small_caps,
        TextFlag::Superscript => s.position == amalith_core::TextPosition::Superscript,
        TextFlag::Subscript => s.position == amalith_core::TextPosition::Subscript,
        TextFlag::AllCaps => false, // not modelled yet
    }
}

fn combo(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    r: Rect,
    value: &str,
    pointer: Point,
) {
    let rr = r.to_rounded_rect(4.0);
    let bg = if r.contains(pointer) { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &rr);
    text.draw(scene, value, 12.0, th.text, r.x0 + 8.0, r.center().y + 4.0);
    caret_down(scene, Point::new(r.x1 - 12.0, r.center().y), th.text_dim);
}

#[allow(clippy::too_many_arguments)]
fn stepper(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    f: &Field,
    glyph: &str,
    value: &str,
    enabled: bool,
    pointer: Point,
) {
    let rr = f.box_.to_rounded_rect(4.0);
    scene.fill(Fill::NonZero, ID, th.bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &rr);
    let label_col = if enabled { th.text_dim } else { th.border };
    text.draw(scene, glyph, 10.0, label_col, f.box_.x0 + 4.0, f.box_.center().y + 4.0);
    let val_col = if enabled { th.text } else { th.text_dim };
    text.draw(scene, value, 12.0, val_col, f.value.x0, f.box_.center().y + 4.0);
    if enabled {
        let up_hot = f.up.contains(pointer);
        let dn_hot = f.down.contains(pointer);
        tri(scene, f.up.center(), true, if up_hot { th.text } else { th.text_dim });
        tri(scene, f.down.center(), false, if dn_hot { th.text } else { th.text_dim });
    }
}

fn tri(scene: &mut Scene, c: Point, up: bool, color: Color) {
    let d = 3.0;
    let mut p = BezPath::new();
    if up {
        p.move_to((c.x - d, c.y + d * 0.6));
        p.line_to((c.x + d, c.y + d * 0.6));
        p.line_to((c.x, c.y - d * 0.6));
    } else {
        p.move_to((c.x - d, c.y - d * 0.6));
        p.line_to((c.x + d, c.y - d * 0.6));
        p.line_to((c.x, c.y + d * 0.6));
    }
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

fn caret_down(scene: &mut Scene, c: Point, color: Color) {
    let d = 3.0;
    let mut p = BezPath::new();
    p.move_to((c.x - d, c.y - d * 0.6));
    p.line_to((c.x + d, c.y - d * 0.6));
    p.line_to((c.x, c.y + d * 0.6));
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);
    let s = &ctx.text_style;

    if l.family.contains(p) {
        return Action::OpenFontMenu(FontMenu::Family, l.family);
    }
    if l.style.contains(p) {
        return Action::OpenFontMenu(FontMenu::Style, l.style);
    }

    // Size.
    if l.size.up.contains(p) {
        return Action::SetFontSize((s.size + 1.0).min(1296.0));
    }
    if l.size.down.contains(p) {
        return Action::SetFontSize((s.size - 1.0).max(1.0));
    }
    if l.size.value.contains(p) {
        return Action::OpenFontMenu(FontMenu::Size, l.size.box_);
    }

    // Leading.
    let cur = s.leading.unwrap_or(s.size * 1.2);
    if l.leading.up.contains(p) {
        return Action::SetLeading(Some(cur + 1.0));
    }
    if l.leading.down.contains(p) {
        return Action::SetLeading(Some((cur - 1.0).max(0.0)));
    }

    // Tracking (thousandths of an em; ±10 per click).
    if l.tracking.up.contains(p) {
        return Action::SetTracking(s.tracking + 10.0);
    }
    if l.tracking.down.contains(p) {
        return Action::SetTracking(s.tracking - 10.0);
    }

    let flags = [
        (l.toggles[0], TextFlag::AllCaps),
        (l.toggles[1], TextFlag::SmallCaps),
        (l.toggles[2], TextFlag::Superscript),
        (l.toggles[3], TextFlag::Subscript),
        (l.toggles[4], TextFlag::Underline),
        (l.toggles[5], TextFlag::Strikethrough),
    ];
    for (r, f) in flags {
        if r.contains(p) {
            return Action::ToggleTextFlag(f);
        }
    }
    Action::None
}

/// Hover text for every Character-panel control.
pub fn tip(body: Rect, p: Point, _ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    let fields: [(&Field, &str); 8] = [
        (&l.size, "Font Size"),
        (&l.leading, "Leading"),
        (&l.kerning, "Kerning"),
        (&l.tracking, "Tracking"),
        (&l.vscale, "Vertical Scale"),
        (&l.hscale, "Horizontal Scale"),
        (&l.baseline, "Baseline Shift"),
        (&l.rotation, "Character Rotation"),
    ];
    if l.family.contains(p) {
        return Some("Font Family");
    }
    if l.style.contains(p) {
        return Some("Font Style");
    }
    for (f, name) in fields {
        if f.box_.contains(p) {
            return Some(name);
        }
    }
    let toggles = [
        "All Caps",
        "Small Caps",
        "Superscript",
        "Subscript",
        "Underline",
        "Strikethrough",
    ];
    for (r, name) in l.toggles.iter().zip(toggles) {
        if r.contains(p) {
            return Some(name);
        }
    }
    None
}
