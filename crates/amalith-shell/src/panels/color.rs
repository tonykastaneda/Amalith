//! Color panel: live RGB / HSB / CMYK sliders for the active fill or
//! stroke, a recent-colors row, and a hue spectrum. The hamburger switches
//! the slider set (and Invert / Complement).

use amalith_core::Paint;
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, ColorStop, Fill, Gradient};
use vello::Scene;

use crate::picker::{hsv_to_rgb, rgb_to_hsv};
use crate::text::TextContext;

use super::{draw_paint_swatch, Action, Ctx, MenuEntry, PaintSlot, ID, PAD};

pub(super) const NATURAL_H: f64 = 232.0;
const RECENT_N: usize = 12;
const RECENT_H: f64 = 16.0;
const CHIP: f64 = 26.0;
const SLIDER_H: f64 = 28.0;
const TRACK_H: f64 = 8.0;
const FIELD_W: f64 = 58.0;
const SPEC_H: f64 = 16.0;
const SLASH: Color = Color::from_rgb8(0xd0, 0x30, 0x30);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ColorSpace {
    #[default]
    Rgb,
    Hsb,
    Cmyk,
}

pub(super) fn menu(ctx: &Ctx) -> Vec<MenuEntry> {
    let m = ctx.color_mode;
    vec![
        MenuEntry::Item {
            id: "rgb",
            label: "RGB",
            checked: m == ColorSpace::Rgb,
        },
        MenuEntry::Item {
            id: "hsb",
            label: "HSB",
            checked: m == ColorSpace::Hsb,
        },
        MenuEntry::Item {
            id: "cmyk",
            label: "CMYK",
            checked: m == ColorSpace::Cmyk,
        },
        MenuEntry::Separator,
        MenuEntry::Item {
            id: "invert",
            label: "Invert",
            checked: false,
        },
        MenuEntry::Item {
            id: "complement",
            label: "Complement",
            checked: false,
        },
    ]
}

struct Lay {
    recent: Vec<Rect>,
    fill: Rect,
    stroke: Rect,
    swap: Rect,
    none: Rect,
    default: Rect,
    tracks: Vec<Rect>,
    hex: Rect,
    spectrum: Rect,
}

fn n_channels(mode: ColorSpace) -> usize {
    match mode {
        ColorSpace::Cmyk => 4,
        _ => 3,
    }
}

fn labels(mode: ColorSpace) -> &'static [&'static str] {
    match mode {
        ColorSpace::Rgb => &["R", "G", "B"],
        ColorSpace::Hsb => &["H", "S", "B"],
        ColorSpace::Cmyk => &["C", "M", "Y", "K"],
    }
}

fn layout(body: Rect, mode: ColorSpace, recent_n: usize) -> Lay {
    let x0 = body.x0 + PAD;
    let x1 = body.x1 - PAD;
    let mut y = body.y0 + 8.0;
    y += 14.0; // "Recent Colors" caption
    let bar = Rect::new(x0, y, x1, y + RECENT_H);
    let n = recent_n.max(1).min(RECENT_N);
    let sw = ((bar.width() - 4.0) / RECENT_N as f64).clamp(8.0, 18.0);
    let recent: Vec<Rect> = (0..n)
        .map(|i| {
            let x = bar.x0 + 2.0 + i as f64 * sw;
            Rect::new(x, bar.y0 + 2.0, x + sw - 2.0, bar.y1 - 2.0)
        })
        .collect();
    y = bar.y1 + 14.0;

    let fill = Rect::new(x0, y, x0 + CHIP, y + CHIP);
    let stroke = Rect::new(fill.x0 + 12.0, fill.y0 + 12.0, fill.x0 + 12.0 + CHIP, fill.y0 + 12.0 + CHIP);
    let swap = Rect::new(stroke.x1 + 4.0, fill.y0, stroke.x1 + 20.0, fill.y0 + 16.0);
    let none = Rect::new(x0, stroke.y1 + 8.0, x0 + 16.0, stroke.y1 + 24.0);
    let default = Rect::new(none.x1 + 8.0, none.y0, none.x1 + 24.0, none.y1);

    let slider_x = x0 + 56.0;
    let nch = n_channels(mode);
    let spec_y = (body.y1 - PAD - SPEC_H).max(y + nch as f64 * SLIDER_H + 36.0);
    let tracks: Vec<Rect> = (0..nch)
        .map(|i| {
            let ty = y + i as f64 * SLIDER_H + (SLIDER_H - TRACK_H) * 0.5;
            Rect::new(slider_x + 18.0, ty, x1 - FIELD_W - 8.0, ty + TRACK_H)
        })
        .collect();
    let last_y = y + nch as f64 * SLIDER_H;
    let hex = Rect::new(x1 - FIELD_W, last_y + 4.0, x1, last_y + 24.0);
    let spectrum = Rect::new(x0, spec_y, x1, spec_y + SPEC_H);

    Lay {
        recent,
        fill,
        stroke,
        swap,
        none,
        default,
        tracks,
        hex,
        spectrum,
    }
}

fn slot_paint(ctx: &Ctx) -> Paint {
    match ctx.active_slot {
        PaintSlot::Fill => ctx.representative.map(|a| a.fill).unwrap_or(ctx.cur_fill),
        PaintSlot::Stroke => ctx.representative.map(|a| a.stroke).unwrap_or(ctx.cur_stroke),
    }
}

fn rgb_of(paint: Paint) -> (f32, f32, f32) {
    paint
        .color()
        .map(|c| (c.r, c.g, c.b))
        .unwrap_or((0.0, 0.0, 0.0))
}

// RGB↔CMYK conversion lives on `amalith_core::Color` (`to_cmyk`/`from_cmyk`)
// so the PDF exporter's `DeviceCMYK` output uses the exact same formula the
// Color panel previews — one conversion, not two that could drift apart.
fn rgb_to_cmyk(r: f32, g: f32, b: f32) -> [f32; 4] {
    amalith_core::Color::rgb(r, g, b).to_cmyk()
}

fn cmyk_to_rgb(c: f32, m: f32, y: f32, k: f32) -> (f32, f32, f32) {
    let rgb = amalith_core::Color::from_cmyk(c, m, y, k);
    (rgb.r, rgb.g, rgb.b)
}

/// Channel values in 0..1 for the current mode, plus display strings.
fn channels(mode: ColorSpace, r: f32, g: f32, b: f32) -> (Vec<f32>, Vec<String>) {
    match mode {
        ColorSpace::Rgb => (
            vec![r, g, b],
            vec![
                format!("{}", (r * 255.0).round() as i32),
                format!("{}", (g * 255.0).round() as i32),
                format!("{}", (b * 255.0).round() as i32),
            ],
        ),
        ColorSpace::Hsb => {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            (
                vec![h, s, v],
                vec![
                    format!("{}", (h * 360.0).round() as i32 % 360),
                    format!("{} %", (s * 100.0).round() as i32),
                    format!("{} %", (v * 100.0).round() as i32),
                ],
            )
        }
        ColorSpace::Cmyk => {
            let c = rgb_to_cmyk(r, g, b);
            (
                c.to_vec(),
                c.iter()
                    .map(|v| format!("{:.2} %", *v * 100.0))
                    .collect(),
            )
        }
    }
}

pub fn apply_channel(mode: ColorSpace, r: f32, g: f32, b: f32, channel: u8, t: f32) -> (f32, f32, f32) {
    let t = t.clamp(0.0, 1.0);
    match mode {
        ColorSpace::Rgb => {
            let mut rgb = [r, g, b];
            rgb[(channel as usize).min(2)] = t;
            (rgb[0], rgb[1], rgb[2])
        }
        ColorSpace::Hsb => {
            let (mut h, mut s, mut v) = rgb_to_hsv(r, g, b);
            match channel {
                0 => h = t,
                1 => s = t,
                _ => v = t,
            }
            hsv_to_rgb(h, s, v)
        }
        ColorSpace::Cmyk => {
            let mut c = rgb_to_cmyk(r, g, b);
            c[(channel as usize).min(3)] = t;
            cmyk_to_rgb(c[0], c[1], c[2], c[3])
        }
    }
}

pub fn apply_spectrum(r: f32, g: f32, b: f32, t: f32) -> (f32, f32, f32) {
    let (_, s, v) = rgb_to_hsv(r, g, b);
    let s = if s < 0.08 { 1.0 } else { s };
    let v = if v < 0.08 { 1.0 } else { v };
    hsv_to_rgb(t.clamp(0.0, 1.0), s, v)
}

pub fn invert_rgb(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    (1.0 - r, 1.0 - g, 1.0 - b)
}

pub fn complement_rgb(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    hsv_to_rgb((h + 0.5).rem_euclid(1.0), s, v)
}

fn track_gradient(mode: ColorSpace, channel: usize, r: f32, g: f32, b: f32, track: Rect) -> Gradient {
    let a = track.x0;
    let y = track.y0 + track.height() * 0.5;
    let z = track.x1;
    match mode {
        ColorSpace::Rgb => {
            let mut c0 = [r, g, b];
            let mut c1 = [r, g, b];
            c0[channel.min(2)] = 0.0;
            c1[channel.min(2)] = 1.0;
            Gradient::new_linear((a, y), (z, y)).with_stops([
                Color::new([c0[0], c0[1], c0[2], 1.0]),
                Color::new([c1[0], c1[1], c1[2], 1.0]),
            ])
        }
        ColorSpace::Hsb => {
            let (h, s, v) = rgb_to_hsv(r, g, b);
            match channel {
                0 => {
                    let stops: [ColorStop; 7] = [
                        hue_stop(0.0, 0.0),
                        hue_stop(1.0 / 6.0, 1.0 / 6.0),
                        hue_stop(2.0 / 6.0, 2.0 / 6.0),
                        hue_stop(3.0 / 6.0, 3.0 / 6.0),
                        hue_stop(4.0 / 6.0, 4.0 / 6.0),
                        hue_stop(5.0 / 6.0, 5.0 / 6.0),
                        hue_stop(1.0, 1.0),
                    ];
                    Gradient::new_linear((a, y), (z, y)).with_stops(stops)
                }
                1 => {
                    let (r0, g0, b0) = hsv_to_rgb(h, 0.0, v.max(0.15));
                    let (r1, g1, b1) = hsv_to_rgb(h, 1.0, v.max(0.15));
                    Gradient::new_linear((a, y), (z, y)).with_stops([
                        Color::new([r0, g0, b0, 1.0]),
                        Color::new([r1, g1, b1, 1.0]),
                    ])
                }
                _ => {
                    let (r1, g1, b1) = hsv_to_rgb(h, s.max(0.15), 1.0);
                    Gradient::new_linear((a, y), (z, y)).with_stops([
                        Color::new([0.0, 0.0, 0.0, 1.0]),
                        Color::new([r1, g1, b1, 1.0]),
                    ])
                }
            }
        }
        ColorSpace::Cmyk => {
            let mut c = rgb_to_cmyk(r, g, b);
            let i = channel.min(3);
            c[i] = 0.0;
            let (r0, g0, b0) = cmyk_to_rgb(c[0], c[1], c[2], c[3]);
            c[i] = 1.0;
            let (r1, g1, b1) = cmyk_to_rgb(c[0], c[1], c[2], c[3]);
            Gradient::new_linear((a, y), (z, y)).with_stops([
                Color::new([r0, g0, b0, 1.0]),
                Color::new([r1, g1, b1, 1.0]),
            ])
        }
    }
}

fn hue_stop(offset: f32, hue: f32) -> ColorStop {
    let (r, g, b) = hsv_to_rgb(hue, 1.0, 1.0);
    ColorStop {
        offset,
        color: Color::new([r, g, b, 1.0]).into(),
    }
}

fn draw_thumb(scene: &mut Scene, track: Rect, t: f32, color: Color) {
    let x = track.x0 + t.clamp(0.0, 1.0) as f64 * track.width();
    let y = track.y1 + 1.0;
    let mut tri = BezPath::new();
    tri.move_to((x, y));
    tri.line_to((x - 5.0, y + 7.0));
    tri.line_to((x + 5.0, y + 7.0));
    tri.close_path();
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &tri);
}

pub(super) fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, ctx: &Ctx) {
    let th = ctx.theme;
    let l = layout(body, ctx.color_mode, ctx.recent.len().max(1));
    let (fill, stroke) = match ctx.representative {
        Some(a) => (a.fill, a.stroke),
        None => (ctx.cur_fill, ctx.cur_stroke),
    };
    let paint = slot_paint(ctx);
    let (r, g, b) = rgb_of(paint);
    let (vals, labels_v) = channels(ctx.color_mode, r, g, b);

    text.draw(
        scene,
        "Recent Colors",
        11.5,
        th.text_dim,
        body.x0 + PAD,
        body.y0 + 18.0,
    );
    let bar = Rect::new(
        l.recent.first().map(|s| s.x0 - 2.0).unwrap_or(body.x0 + PAD),
        l.recent.first().map(|s| s.y0 - 2.0).unwrap_or(body.y0),
        body.x1 - PAD,
        l.recent.first().map(|s| s.y1 + 2.0).unwrap_or(body.y0),
    );
    scene.fill(Fill::NonZero, ID, th.bg, None, &bar);
    for (i, slot) in l.recent.iter().enumerate() {
        if let Some(c) = ctx.recent.get(i) {
            scene.fill(
                Fill::NonZero,
                ID,
                crate::convert::color(*c),
                None,
                slot,
            );
        }
    }

    // Stroke behind, fill in front.
    draw_paint_swatch(
        scene,
        text,
        th,
        l.stroke,
        stroke,
        ctx.active_slot == PaintSlot::Stroke,
        ctx.stroke_mixed,
    );
    draw_paint_swatch(
        scene,
        text,
        th,
        l.fill,
        fill,
        ctx.active_slot == PaintSlot::Fill,
        ctx.fill_mixed,
    );

    // Swap arrows.
    let sc = l.swap.center();
    let mut swap = BezPath::new();
    swap.move_to((sc.x - 5.0, sc.y - 3.0));
    swap.line_to((sc.x + 2.0, sc.y - 3.0));
    swap.move_to((sc.x - 1.0, sc.y - 6.0));
    swap.line_to((sc.x + 4.0, sc.y - 3.0));
    swap.line_to((sc.x - 1.0, sc.y));
    swap.move_to((sc.x + 5.0, sc.y + 3.0));
    swap.line_to((sc.x - 2.0, sc.y + 3.0));
    swap.move_to((sc.x + 1.0, sc.y));
    swap.line_to((sc.x - 4.0, sc.y + 3.0));
    swap.line_to((sc.x + 1.0, sc.y + 6.0));
    scene.stroke(&Stroke::new(1.3), ID, th.text_dim, None, &swap);

    // None
    scene.fill(Fill::NonZero, ID, Color::WHITE, None, &l.none);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.none);
    let mut slash = BezPath::new();
    slash.move_to((l.none.x0 + 1.0, l.none.y1 - 1.0));
    slash.line_to((l.none.x1 - 1.0, l.none.y0 + 1.0));
    scene.stroke(&Stroke::new(1.6), ID, SLASH, None, &slash);

    // Default: white fill / black stroke mini.
    scene.fill(Fill::NonZero, ID, Color::WHITE, None, &l.default);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.default);
    scene.fill(
        Fill::NonZero,
        ID,
        Color::BLACK,
        None,
        &Rect::new(
            l.default.x0 + 7.0,
            l.default.y0 + 7.0,
            l.default.x1 - 1.0,
            l.default.y1 - 1.0,
        ),
    );

    let labs = labels(ctx.color_mode);
    for (i, track) in l.tracks.iter().enumerate() {
        let label = labs.get(i).copied().unwrap_or("?");
        text.draw(
            scene,
            label,
            12.0,
            th.text_dim,
            track.x0 - 16.0,
            track.y0 + 8.0,
        );
        let t = vals.get(i).copied().unwrap_or(0.0);
        let grad = track_gradient(ctx.color_mode, i, r, g, b, *track);
        scene.fill(Fill::NonZero, ID, &grad, None, track);
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, track);
        draw_thumb(scene, *track, t, th.text);
        let field = Rect::new(track.x1 + 8.0, track.y0 - 6.0, body.x1 - PAD, track.y0 + TRACK_H + 6.0);
        scene.fill(Fill::NonZero, ID, th.bg, None, &field.to_rounded_rect(3.0));
        scene.stroke(&Stroke::new(1.0), ID, th.border, None, &field.to_rounded_rect(3.0));
        if let Some(s) = labels_v.get(i) {
            text.draw(scene, s, 11.5, th.text, field.x0 + 6.0, field.y0 + 14.0);
        }
    }

    scene.fill(Fill::NonZero, ID, th.bg, None, &l.hex.to_rounded_rect(3.0));
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.hex.to_rounded_rect(3.0));
    let hex = format!(
        "# {:02X}{:02X}{:02X}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8
    );
    text.draw(scene, &hex, 11.5, th.text, l.hex.x0 + 6.0, l.hex.y0 + 14.0);

    let sy = l.spectrum.y0 + l.spectrum.height() * 0.5;
    let spec = Gradient::new_linear((l.spectrum.x0, sy), (l.spectrum.x1, sy)).with_stops([
        hue_stop(0.0, 0.0),
        hue_stop(1.0 / 6.0, 1.0 / 6.0),
        hue_stop(2.0 / 6.0, 2.0 / 6.0),
        hue_stop(3.0 / 6.0, 3.0 / 6.0),
        hue_stop(4.0 / 6.0, 4.0 / 6.0),
        hue_stop(5.0 / 6.0, 5.0 / 6.0),
        hue_stop(1.0, 1.0),
    ]);
    scene.fill(Fill::NonZero, ID, &spec, None, &l.spectrum);
    scene.stroke(&Stroke::new(1.0), ID, th.border, None, &l.spectrum);
}

pub(super) fn hit(body: Rect, local: Point, ctx: &Ctx) -> Action {
    let l = layout(body, ctx.color_mode, ctx.recent.len().max(1));
    for (i, r) in l.recent.iter().enumerate() {
        if r.contains(local) {
            if let Some(c) = ctx.recent.get(i) {
                return Action::SetPaint(Paint::Solid(*c));
            }
        }
    }
    if l.fill.contains(local)
        && !(ctx.active_slot == PaintSlot::Stroke && l.stroke.contains(local))
    {
        return Action::OpenPicker(PaintSlot::Fill);
    }
    if l.stroke.contains(local) {
        return Action::OpenPicker(PaintSlot::Stroke);
    }
    if l.swap.contains(local) {
        return Action::SwapPaints;
    }
    if l.none.contains(local) {
        return Action::SetPaint(Paint::None);
    }
    if l.default.contains(local) {
        return Action::DefaultPaints;
    }
    for (i, track) in l.tracks.iter().enumerate() {
        let grab = track.inflate(0.0, 10.0);
        if grab.contains(local) {
            let t = ((local.x - track.x0) / track.width()).clamp(0.0, 1.0) as f32;
            return Action::ColorScrub {
                channel: i as u8,
                t,
                track: *track,
            };
        }
    }
    if l.spectrum.contains(local) {
        let t = ((local.x - l.spectrum.x0) / l.spectrum.width()).clamp(0.0, 1.0) as f32;
        return Action::ColorSpectrum {
            t,
            track: l.spectrum,
        };
    }
    Action::None
}

pub(super) fn tip(body: Rect, local: Point, ctx: &Ctx) -> Option<&'static str> {
    let l = layout(body, ctx.color_mode, ctx.recent.len().max(1));
    if l.fill.contains(local) {
        return Some("Fill");
    }
    if l.stroke.contains(local) {
        return Some("Stroke");
    }
    if l.swap.contains(local) {
        return Some("Swap Fill and Stroke");
    }
    if l.none.contains(local) {
        return Some("None");
    }
    if l.default.contains(local) {
        return Some("Default Fill and Stroke");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_channel_sets_only_that_component() {
        let (r, g, b) = apply_channel(ColorSpace::Rgb, 0.2, 0.4, 0.6, 0, 1.0);
        assert!((r - 1.0).abs() < 1e-5);
        assert!((g - 0.4).abs() < 1e-5);
        assert!((b - 0.6).abs() < 1e-5);
    }

    #[test]
    fn complement_rotates_hue_halfway() {
        let (r, g, b) = complement_rgb(1.0, 0.0, 0.0);
        // Complementary of red is cyan.
        assert!(g > 0.9 && b > 0.9 && r < 0.1);
    }
}
