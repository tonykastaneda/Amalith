//! The Artboard options-bar segment — shown while an artboard is selected
//! with the Artboard tool.
//!
//! Left → right: a Portrait / Landscape toggle (swaps the artboard's W and
//! H, not its contents), a background-fill dropdown + swatch, add / delete
//! buttons, the name field, and X / Y / W / H (with a W↔H link).

use crate::metrics::px as ui_px;

use amalith_core::xform::fmt_px;
use vello::kurbo::{BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::{Color as VColor, Fill};
use vello::Scene;

use crate::panels::{transform::ABField, Action};
use crate::text::TextContext;
use crate::theme::Theme;

use super::{baseline, draw_combo, draw_field, field, Ctx, SegKind, Segment, ID};

/// Read-only snapshot of the selected artboard for the segment.
#[derive(Clone)]
pub struct ArtboardBar {
    pub name: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub fill: Option<amalith_core::Color>,
    pub portrait: bool,
}

pub(super) const SEGMENT: Segment = Segment {
    kind: SegKind::Artboard,
    applies: |ctx| ctx.artboard.is_some(),
    measure: |_| ui_px(1112.0),
    paint,
    hit,
};

struct Parts {
    orient_p: Rect,
    orient_l: Rect,
    fill_combo: Rect,
    fill_swatch: Rect,
    plus: Rect,
    trash: Rect,
    name: Rect,
    x: (Rect, Rect, Rect),
    y: (Rect, Rect, Rect),
    w: Rect,
    link: Rect,
    h: Rect,
}

fn parts(r: Rect) -> Parts {
    let cy = r.center().y;
    let sq = |x: f64, s: f64| Rect::new(x, cy - s * 0.5, x + s, cy + s * 0.5);
    let mut x = r.x0;
    let orient_p = sq(x, ui_px(24.0));
    x = orient_p.x1 + ui_px(4.0);
    let orient_l = sq(x, ui_px(24.0));
    x = orient_l.x1 + ui_px(22.0) + ui_px(30.0); // gap + "Fill:"
    let fill_combo = Rect::new(x, cy - ui_px(11.5), x + ui_px(172.0), cy + ui_px(11.5));
    x = fill_combo.x1 + ui_px(8.0);
    let fill_swatch = sq(x, ui_px(23.0));
    x = fill_swatch.x1 + ui_px(18.0);
    let plus = sq(x, ui_px(24.0));
    x = plus.x1 + ui_px(10.0);
    let trash = sq(x, ui_px(24.0));
    x = trash.x1 + ui_px(22.0) + ui_px(44.0); // gap + "Name:"
    let name = Rect::new(x, cy - ui_px(11.5), x + ui_px(178.0), cy + ui_px(11.5));
    x = name.x1 + ui_px(22.0);
    let xf = field(x + ui_px(16.0), cy, ui_px(74.0)); // after "X:"
    x = xf.2.x1 + ui_px(12.0);
    let yf = field(x + ui_px(16.0), cy, ui_px(74.0)); // after "Y:"
    x = yf.2.x1 + ui_px(12.0);
    let w = Rect::new(x + ui_px(18.0), cy - ui_px(11.5), x + ui_px(18.0) + ui_px(74.0), cy + ui_px(11.5)); // after "W:"
    x = w.x1 + ui_px(7.0);
    let link = Rect::new(x, cy - ui_px(11.5), x + ui_px(23.0), cy + ui_px(11.5));
    x = link.x1 + ui_px(7.0) + ui_px(16.0); // gap + "H:"
    let h = Rect::new(x, cy - ui_px(11.5), x + ui_px(74.0), cy + ui_px(11.5));
    Parts {
        orient_p,
        orient_l,
        fill_combo,
        fill_swatch,
        plus,
        trash,
        name,
        x: xf,
        y: yf,
        w,
        link,
        h,
    }
}

fn fill_label(fill: Option<amalith_core::Color>) -> &'static str {
    match fill {
        None => "Transparent (Default)",
        Some(c) if c.r > 0.98 && c.g > 0.98 && c.b > 0.98 => "White",
        Some(c) if c.r < 0.02 && c.g < 0.02 && c.b < 0.02 => "Black",
        Some(_) => "Custom",
    }
}

fn editing(ctx: &Ctx, f: ABField) -> bool {
    ctx.artboard_edit.is_some_and(|(ef, _)| ef == f)
}

fn shown(ctx: &Ctx, f: ABField) -> String {
    if let Some((ef, s)) = ctx.artboard_edit {
        if ef == f {
            return s.to_string();
        }
    }
    let Some(ab) = &ctx.artboard else {
        return "—".into();
    };
    match f {
        ABField::Name => ab.name.clone(),
        ABField::X => fmt_px(ab.x),
        ABField::Y => fmt_px(ab.y),
        ABField::W => fmt_px(ab.w),
        ABField::H => fmt_px(ab.h),
    }
}

fn paint(scene: &mut Scene, text: &mut TextContext, r: Rect, ctx: &Ctx) {
    let theme = ctx.theme;
    let p = parts(r);
    let base = baseline(r);
    let ab = ctx.artboard.clone().unwrap_or_else(|| ArtboardBar {
        name: String::new(),
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        fill: None,
        portrait: true,
    });

    orient_icon(scene, p.orient_p, true, ab.portrait, theme);
    orient_icon(scene, p.orient_l, false, !ab.portrait, theme);

    text.draw(scene, "Fill:", 13.0, theme.text_dim, p.orient_l.x1 + ui_px(22.0), base);
    draw_combo(scene, text, theme, p.fill_combo, fill_label(ab.fill));
    swatch(scene, p.fill_swatch, ab.fill, theme);

    plus_icon(scene, p.plus, theme);
    trash_icon(scene, p.trash, theme);

    text.draw(scene, "Name:", 13.0, theme.text_dim, p.trash.x1 + ui_px(22.0), base);
    name_box(scene, text, theme, p.name, &shown(ctx, ABField::Name), editing(ctx, ABField::Name));

    text.draw(scene, "X:", 13.0, theme.text, p.x.0.x0 - ui_px(16.0), base);
    draw_field(scene, text, theme, p.x.0, p.x.1, p.x.2, &shown(ctx, ABField::X), editing(ctx, ABField::X));
    text.draw(scene, "Y:", 13.0, theme.text, p.y.0.x0 - ui_px(16.0), base);
    draw_field(scene, text, theme, p.y.0, p.y.1, p.y.2, &shown(ctx, ABField::Y), editing(ctx, ABField::Y));
    text.draw(scene, "W:", 13.0, theme.text, p.w.x0 - ui_px(18.0), base);
    box_field(scene, text, theme, p.w, &shown(ctx, ABField::W), editing(ctx, ABField::W));
    lock_icon(scene, p.link, ctx.artboard_link, theme);
    text.draw(scene, "H:", 13.0, theme.text, p.h.x0 - ui_px(16.0), base);
    box_field(scene, text, theme, p.h, &shown(ctx, ABField::H), editing(ctx, ABField::H));

    if ctx.artboard_fill_menu {
        fill_menu(scene, text, theme, p.fill_combo, ab.fill);
    }
}

fn hit(r: Rect, local: Point, ctx: &Ctx) -> Action {
    let p = parts(r);
    if ctx.artboard_fill_menu {
        // The open menu captures the click.
        for (i, item) in fill_menu_rects(p.fill_combo).into_iter().enumerate() {
            if item.contains(local) {
                return Action::ArtboardFillPick(i as u8);
            }
        }
        return Action::ToggleArtboardFillMenu; // click elsewhere closes it
    }
    if p.orient_p.contains(local) {
        return Action::ArtboardOrient(true);
    }
    if p.orient_l.contains(local) {
        return Action::ArtboardOrient(false);
    }
    if p.fill_combo.contains(local) || p.fill_swatch.contains(local) {
        return Action::ToggleArtboardFillMenu;
    }
    if p.plus.contains(local) {
        return Action::NewArtboard;
    }
    if p.trash.contains(local) {
        return Action::DeleteArtboard;
    }
    if p.name.contains(local) {
        return Action::BeginArtboardEdit(ABField::Name);
    }
    if p.link.contains(local) {
        return Action::ToggleArtboardLink;
    }
    for (fld, f) in [
        (p.x.0, ABField::X),
        (p.y.0, ABField::Y),
        (p.w, ABField::W),
        (p.h, ABField::H),
    ] {
        if fld.contains(local) {
            return Action::BeginArtboardEdit(f);
        }
    }
    for (up, dn, f) in [
        (p.x.1, p.x.2, ABField::X),
        (p.y.1, p.y.2, ABField::Y),
    ] {
        if up.contains(local) {
            return Action::NudgeArtboard(f, 1.0);
        }
        if dn.contains(local) {
            return Action::NudgeArtboard(f, -1.0);
        }
    }
    Action::None
}

/// Which numeric field the pointer is over, for scroll-to-nudge.
pub fn field_at(r: Rect, p: Point) -> Option<ABField> {
    let s = parts(r);
    let hit = |a: (Rect, Rect, Rect)| a.0.contains(p) || a.1.contains(p) || a.2.contains(p);
    if hit(s.x) {
        Some(ABField::X)
    } else if hit(s.y) {
        Some(ABField::Y)
    } else if s.w.contains(p) {
        Some(ABField::W)
    } else if s.h.contains(p) {
        Some(ABField::H)
    } else {
        None
    }
}

// --- widgets ----------------------------------------------------------

fn box_field(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, value: &str, highlight: bool) {
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    if highlight {
        let w = text.measure(value, 13.0);
        let band = Rect::new(r.x0 + ui_px(5.0), r.y0 + ui_px(3.0), (r.x0 + ui_px(9.0) + w).min(r.x1 - ui_px(3.0)), r.y1 - ui_px(3.0));
        crate::widgets::draw_field_highlight(scene, theme, band);
    }
    let border = if highlight { theme.accent } else { theme.text_dim.with_alpha(0.5) };
    scene.stroke(&Stroke::new(if highlight { 1.5 } else { 1.0 }), ID, border, None, &r);
    text.draw(scene, value, 13.0, theme.text, r.x0 + ui_px(7.0), r.y0 + r.height() * 0.5 + ui_px(4.5));
}

fn name_box(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, value: &str, highlight: bool) {
    box_field(scene, text, theme, r, value, highlight);
}

fn swatch(scene: &mut Scene, r: Rect, fill: Option<amalith_core::Color>, theme: &Theme) {
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.6), None, &r);
    match fill {
        Some(c) => {
            scene.fill(
                Fill::NonZero,
                ID,
                VColor::new([c.r, c.g, c.b, c.a]),
                None,
                &r.inset(ui_px(-1.0)),
            );
        }
        None => {
            scene.fill(Fill::NonZero, ID, theme.bg, None, &r.inset(ui_px(-1.0)));
            let mut d = BezPath::new();
            d.move_to((r.x1 - ui_px(2.0), r.y0 + ui_px(2.0)));
            d.line_to((r.x0 + ui_px(2.0), r.y1 - ui_px(2.0)));
            scene.stroke(&Stroke::new(ui_px(1.4)), ID, VColor::from_rgb8(0xe0, 0x40, 0x40), None, &d);
        }
    }
}

fn orient_icon(scene: &mut Scene, r: Rect, portrait: bool, on: bool, theme: &Theme) {
    let bg = if on { theme.accent } else { theme.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &r.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.6), None, &r.to_rounded_rect(ui_px(3.0)));
    let ink = if on { theme.on_accent } else { theme.text_dim };
    let c = r.center();
    let page = if portrait {
        Rect::from_center_size(c, (ui_px(10.0), ui_px(13.0)))
    } else {
        Rect::from_center_size(c, (ui_px(13.0), ui_px(10.0)))
    };
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, ink, None, &page);
}

fn plus_icon(scene: &mut Scene, r: Rect, theme: &Theme) {
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(0.6), None, &r.to_rounded_rect(ui_px(3.0)));
    let c = r.center();
    let mut p = BezPath::new();
    p.move_to((c.x - ui_px(5.0), c.y));
    p.line_to((c.x + ui_px(5.0), c.y));
    p.move_to((c.x, c.y - ui_px(5.0)));
    p.line_to((c.x, c.y + ui_px(5.0)));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, theme.text, None, &p);
}

fn trash_icon(scene: &mut Scene, r: Rect, theme: &Theme) {
    let c = r.center();
    let body = Rect::from_center_size(Point::new(c.x, c.y + 1.0), (ui_px(9.0), ui_px(11.0)));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text, None, &body);
    let mut lid = BezPath::new();
    lid.move_to((c.x - ui_px(7.0), c.y - ui_px(5.0)));
    lid.line_to((c.x + ui_px(7.0), c.y - ui_px(5.0)));
    lid.move_to((c.x - ui_px(2.5), c.y - ui_px(5.0)));
    lid.line_to((c.x - ui_px(2.5), c.y - ui_px(7.5)));
    lid.line_to((c.x + ui_px(2.5), c.y - ui_px(7.5)));
    lid.line_to((c.x + ui_px(2.5), c.y - ui_px(5.0)));
    scene.stroke(&Stroke::new(ui_px(1.3)), ID, theme.text, None, &lid);
}

fn lock_icon(scene: &mut Scene, r: Rect, on: bool, theme: &Theme) {
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &r);
    let c = r.center();
    let col = if on { theme.accent } else { theme.text_dim.with_alpha(0.7) };
    if on {
        let a = Rect::from_center_size(Point::new(c.x, c.y - ui_px(3.5)), (ui_px(8.0), ui_px(9.0)));
        let b = Rect::from_center_size(Point::new(c.x, c.y + ui_px(3.5)), (ui_px(8.0), ui_px(9.0)));
        scene.stroke(&Stroke::new(ui_px(1.4)), ID, col, None, &a.to_rounded_rect(ui_px(2.5)));
        scene.stroke(&Stroke::new(ui_px(1.4)), ID, col, None, &b.to_rounded_rect(ui_px(2.5)));
    } else {
        let a = Rect::from_center_size(c, (ui_px(9.0), ui_px(11.5)));
        scene.stroke(&Stroke::new(ui_px(1.4)), ID, col, None, &a.to_rounded_rect(ui_px(2.5)));
        let mut slash = BezPath::new();
        slash.move_to((c.x - ui_px(6.0), c.y + ui_px(7.0)));
        slash.line_to((c.x + ui_px(6.0), c.y - ui_px(7.0)));
        scene.stroke(&Stroke::new(ui_px(1.4)), ID, col, None, &slash);
    }
}

const FILL_ITEMS: [&str; 4] = ["White", "Black", "Transparent (Default)", "Other…"];

fn fill_menu_rects(combo: Rect) -> Vec<Rect> {
    let row = ui_px(26.0);
    (0..FILL_ITEMS.len())
        .map(|i| {
            let y = combo.y1 + ui_px(3.0) + i as f64 * row;
            Rect::new(combo.x0, y, combo.x0 + combo.width().max(ui_px(180.0)), y + row)
        })
        .collect()
}

fn fill_menu(scene: &mut Scene, text: &mut TextContext, theme: &Theme, combo: Rect, fill: Option<amalith_core::Color>) {
    let rects = fill_menu_rects(combo);
    let frame = Rect::new(
        rects[0].x0,
        rects[0].y0 - ui_px(3.0),
        rects[0].x1,
        rects.last().unwrap().y1 + ui_px(3.0),
    );
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &frame.to_rounded_rect(ui_px(4.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &frame.to_rounded_rect(ui_px(4.0)));
    let cur = match fill {
        Some(c) if c.r > 0.98 && c.g > 0.98 && c.b > 0.98 => 0,
        Some(c) if c.r < 0.02 && c.g < 0.02 && c.b < 0.02 => 1,
        None => 2,
        Some(_) => usize::MAX,
    };
    for (i, r) in rects.iter().enumerate() {
        let col = if i == 2 { theme.accent } else { theme.text };
        if i == cur {
            let cm = Circle::new((r.x0 + ui_px(12.0), r.center().y), ui_px(2.0));
            scene.fill(Fill::NonZero, ID, theme.text, None, &cm);
        }
        text.draw(scene, FILL_ITEMS[i], 13.0, col, r.x0 + ui_px(24.0), r.center().y + ui_px(4.5));
        if i == 2 {
            scene.fill(
                Fill::NonZero,
                ID,
                theme.border,
                None,
                &Rect::new(r.x0 + ui_px(4.0), r.y1, r.x1 - ui_px(4.0), r.y1 + 1.0),
            );
        }
    }
}
