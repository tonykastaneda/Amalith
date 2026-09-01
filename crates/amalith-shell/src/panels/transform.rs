//! Transform panel — X / Y / W / H, rotation, shear, 9-point origin.
//!
//! Reads the selection through [`amalith_core::xform`] and emits
//! [`Action`]s the shell turns into `SetTransform`.

use amalith_core::xform::{self, RefPoint, TransformValues};
use vello::kurbo::{BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;

use super::{Action, Ctx, ID, PAD};

const FIELD_H: f64 = 22.0;
const GAP: f64 = 8.0;
const LOC: f64 = 44.0;

/// Which numeric field the pointer is on / the user is typing into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XformField {
    X,
    Y,
    W,
    H,
    Rotation,
    Shear,
}

struct L {
    locator: Rect,
    cells: [Rect; 9],
    x: Rect,
    y: Rect,
    w: Rect,
    h: Rect,
    link: Rect,
    rotation: Rect,
    shear: Rect,
    bottom: f64,
}

fn layout(body: Rect) -> L {
    let x0 = body.x0 + PAD;
    let y0 = body.y0 + 10.0;
    let x1 = body.x1 - PAD;
    let locator = Rect::new(x0, y0 + 4.0, x0 + LOC, y0 + 4.0 + LOC);

    let link_w = 28.0;
    let link = Rect::new(x1 - link_w, y0, x1, y0 + FIELD_H * 2.0 + GAP);
    let rest_l = locator.x1 + 10.0;
    let rest_r = link.x0 - 8.0;
    let col_w = ((rest_r - rest_l) - GAP).max(40.0) / 2.0;

    let x = Rect::new(rest_l, y0, rest_l + col_w, y0 + FIELD_H);
    let w = Rect::new(rest_l + col_w + GAP, y0, rest_r, y0 + FIELD_H);
    let y = Rect::new(rest_l, y0 + FIELD_H + GAP, rest_l + col_w, y0 + FIELD_H * 2.0 + GAP);
    let h = Rect::new(
        rest_l + col_w + GAP,
        y0 + FIELD_H + GAP,
        rest_r,
        y0 + FIELD_H * 2.0 + GAP,
    );

    let mut cells = [Rect::ZERO; 9];
    let cell = 8.0;
    let cg = (LOC - 8.0 - cell * 3.0) / 2.0;
    for row in 0..3 {
        for col in 0..3 {
            let cx = locator.x0 + 4.0 + col as f64 * (cell + cg);
            let cy = locator.y0 + 4.0 + row as f64 * (cell + cg);
            cells[row * 3 + col] = Rect::new(cx, cy, cx + cell, cy + cell);
        }
    }

    let ry = y0 + FIELD_H * 2.0 + GAP + 14.0;
    let half = ((x1 - x0) - GAP).max(40.0) / 2.0;
    let rotation = Rect::new(x0, ry, x0 + half, ry + FIELD_H);
    let shear = Rect::new(x0 + half + GAP, ry, x1, ry + FIELD_H);

    L {
        locator,
        cells,
        x,
        y,
        w,
        h,
        link,
        rotation,
        shear,
        bottom: ry + FIELD_H + PAD,
    }
}

pub fn natural_height() -> f64 {
    layout(Rect::new(0.0, 0.0, 240.0, 400.0)).bottom
}

fn readout(ctx: &Ctx) -> Option<TransformValues> {
    let id = *ctx.selection.first()?;
    let b = ctx.doc.local_bounds_of(id)?;
    let world = ctx.doc.world_transform(id);
    Some(xform::values(world, b, ctx.xform_ref))
}

pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let l = layout(body);
    let th = ctx.theme;
    let v = readout(ctx);
    let on = v.is_some();

    paint_locator(scene, &l, ctx);

    let edit = |f: XformField| {
        ctx.xform_edit
            .filter(|(ef, _)| *ef == f)
            .map(|(_, s)| s.to_string())
    };
    let px = |f: XformField| {
        edit(f).unwrap_or_else(|| {
            v.map_or("—".into(), |v| match f {
                XformField::X => xform::fmt_px(v.x),
                XformField::Y => xform::fmt_px(v.y),
                XformField::W => xform::fmt_px(v.w),
                XformField::H => xform::fmt_px(v.h),
                _ => "—".into(),
            })
        })
    };
    let deg = |f: XformField| {
        edit(f).unwrap_or_else(|| {
            v.map_or("—".into(), |v| match f {
                XformField::Rotation => xform::fmt_deg(v.rotation_deg),
                XformField::Shear => xform::fmt_deg(v.shear_deg),
                _ => "—".into(),
            })
        })
    };

    labeled_field(scene, text, th, l.x, "X:", &px(XformField::X), on, ctx.pointer);
    labeled_field(scene, text, th, l.y, "Y:", &px(XformField::Y), on, ctx.pointer);
    labeled_field(scene, text, th, l.w, "W:", &px(XformField::W), on, ctx.pointer);
    labeled_field(scene, text, th, l.h, "H:", &px(XformField::H), on, ctx.pointer);

    paint_link(scene, l.link, ctx.xform_constrain, th, ctx.pointer);

    icon_field(
        scene,
        text,
        th,
        l.rotation,
        rot_icon,
        &deg(XformField::Rotation),
        on,
        ctx.pointer,
    );
    icon_field(
        scene,
        text,
        th,
        l.shear,
        shear_icon,
        &deg(XformField::Shear),
        on,
        ctx.pointer,
    );
}

fn paint_locator(scene: &mut Scene, l: &L, ctx: &Ctx) {
    let th = ctx.theme;
    let rp = ctx.xform_ref;
    for row in 0..3u8 {
        for col in 0..3u8 {
            let r = l.cells[(row * 3 + col) as usize];
            let on = rp.col == col && rp.row == row;
            let hot = r.contains(ctx.pointer);
            let fill = if on {
                th.accent
            } else if hot {
                th.text_dim
            } else {
                th.border
            };
            scene.fill(Fill::NonZero, ID, fill, None, &r.to_rounded_rect(1.5));
            if !on {
                scene.fill(
                    Fill::NonZero,
                    ID,
                    th.panel_bg,
                    None,
                    &r.inset(1.5).to_rounded_rect(1.0),
                );
            }
        }
    }
}

fn paint_link(scene: &mut Scene, r: Rect, on: bool, th: &crate::theme::Theme, pointer: Point) {
    let rr = r.to_rounded_rect(4.0);
    let hot = r.contains(pointer);
    let bg = if on {
        th.accent.with_alpha(0.22)
    } else if hot {
        th.strip_bg
    } else {
        th.bg
    };
    scene.fill(Fill::NonZero, ID, bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &rr);
    let c = r.center();
    let col = if on { th.accent } else { th.text_dim };
    // Two overlapping rings — a chain link.
    let a = Rect::from_center_size(Point::new(c.x, c.y - 5.0), (9.0, 11.0));
    let b = Rect::from_center_size(Point::new(c.x, c.y + 5.0), (9.0, 11.0));
    scene.stroke(&Stroke::new(1.6), ID, col, None, &a.to_rounded_rect(3.0));
    scene.stroke(&Stroke::new(1.6), ID, col, None, &b.to_rounded_rect(3.0));
}

fn labeled_field(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    r: Rect,
    label: &str,
    value: &str,
    enabled: bool,
    pointer: Point,
) {
    let lw = text.measure(label, 11.0) + 4.0;
    text.draw(
        scene,
        label,
        11.0,
        th.text_dim,
        r.x0,
        r.center().y + 4.0,
    );
    let box_ = Rect::new(r.x0 + lw, r.y0, r.x1, r.y1);
    field_box(scene, text, th, box_, value, enabled, pointer);
}

fn icon_field(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    r: Rect,
    icon: fn(&mut Scene, Point, Color),
    value: &str,
    enabled: bool,
    pointer: Point,
) {
    icon(scene, Point::new(r.x0 + 8.0, r.center().y), th.text_dim);
    let box_ = Rect::new(r.x0 + 18.0, r.y0, r.x1, r.y1);
    field_box(scene, text, th, box_, value, enabled, pointer);
    caret_down(scene, Point::new(box_.x1 - 10.0, box_.center().y), th.text_dim);
}

fn field_box(
    scene: &mut Scene,
    text: &mut TextContext,
    th: &crate::theme::Theme,
    r: Rect,
    value: &str,
    enabled: bool,
    pointer: Point,
) {
    let rr = r.to_rounded_rect(3.0);
    let hot = r.contains(pointer);
    let bg = if hot && enabled { th.strip_bg } else { th.bg };
    scene.fill(Fill::NonZero, ID, bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &rr);
    let col = if enabled { th.text } else { th.text_dim };
    text.draw(
        scene,
        value,
        11.5,
        col,
        r.x0 + 6.0,
        r.center().y + 4.0,
    );
}

fn rot_icon(scene: &mut Scene, c: Point, color: Color) {
    let mut p = BezPath::new();
    p.move_to((c.x - 5.0, c.y + 4.0));
    p.line_to((c.x + 5.0, c.y + 4.0));
    p.line_to((c.x, c.y - 5.0));
    p.close_path();
    scene.stroke(&Stroke::new(1.2), ID, color, None, &p);
}

fn shear_icon(scene: &mut Scene, c: Point, color: Color) {
    let mut p = BezPath::new();
    p.move_to((c.x - 4.0, c.y + 5.0));
    p.line_to((c.x + 2.0, c.y + 5.0));
    p.line_to((c.x + 6.0, c.y - 5.0));
    p.line_to((c.x, c.y - 5.0));
    p.close_path();
    scene.stroke(&Stroke::new(1.2), ID, color, None, &p);
}

fn caret_down(scene: &mut Scene, c: Point, color: Color) {
    let mut p = BezPath::new();
    p.move_to((c.x - 3.0, c.y - 1.5));
    p.line_to((c.x + 3.0, c.y - 1.5));
    p.line_to((c.x, c.y + 2.0));
    p.close_path();
    scene.fill(Fill::NonZero, ID, color, None, &p);
}

pub fn hit(body: Rect, p: Point, ctx: &Ctx) -> Action {
    let l = layout(body);
    for row in 0..3u8 {
        for col in 0..3u8 {
            if l.cells[(row * 3 + col) as usize].contains(p) {
                return Action::SetXformRef(RefPoint { col, row });
            }
        }
    }
    if l.link.contains(p) {
        return Action::ToggleXformConstrain;
    }
    if readout(ctx).is_none() {
        return Action::None;
    }
    let fields = [
        (l.x, XformField::X),
        (l.y, XformField::Y),
        (l.w, XformField::W),
        (l.h, XformField::H),
        (l.rotation, XformField::Rotation),
        (l.shear, XformField::Shear),
    ];
    for (r, f) in fields {
        if r.contains(p) {
            return Action::BeginXformEdit(f);
        }
    }
    Action::None
}

/// Field under `p`, for scroll-to-nudge.
pub fn field_at(body: Rect, p: Point) -> Option<XformField> {
    let l = layout(body);
    let fields = [
        (l.x, XformField::X),
        (l.y, XformField::Y),
        (l.w, XformField::W),
        (l.h, XformField::H),
        (l.rotation, XformField::Rotation),
        (l.shear, XformField::Shear),
    ];
    fields.into_iter().find(|(r, _)| r.contains(p)).map(|(_, f)| f)
}

pub fn tip(body: Rect, p: Point, _ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body);
    if l.locator.contains(p) {
        return Some("Reference Point");
    }
    if l.link.contains(p) {
        return Some("Constrain Width and Height");
    }
    if l.x.contains(p) {
        return Some("X");
    }
    if l.y.contains(p) {
        return Some("Y");
    }
    if l.w.contains(p) {
        return Some("Width");
    }
    if l.h.contains(p) {
        return Some("Height");
    }
    if l.rotation.contains(p) {
        return Some("Rotate");
    }
    if l.shear.contains(p) {
        return Some("Shear");
    }
    None
}

pub fn menu(_ctx: &Ctx) -> Vec<super::MenuEntry> {
    vec![
        super::MenuEntry::Item {
            id: "flip-h",
            label: "Flip Horizontal",
            checked: false,
        },
        super::MenuEntry::Item {
            id: "flip-v",
            label: "Flip Vertical",
            checked: false,
        },
    ]
}


