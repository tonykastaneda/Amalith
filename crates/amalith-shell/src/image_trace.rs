//! Image Trace panel state and shared paint/hit geometry.
use crate::{metrics::px, text::TextContext, theme::Theme};
use amalith_trace::{Mode, Options};
use serde::{Deserialize, Serialize};
use vello::{
    kurbo::{Affine, Line, Point, Rect, Stroke},
    peniko::Fill,
    Scene,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Result,
    ResultOutline,
    Outline,
    SourceOutline,
    Original,
}
impl View {
    pub const ALL: [Self; 5] = [
        Self::Result,
        Self::ResultOutline,
        Self::Outline,
        Self::SourceOutline,
        Self::Original,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Result => "Tracing Result",
            Self::ResultOutline => "Result with Outlines",
            Self::Outline => "Outlines",
            Self::SourceOutline => "Source with Outlines",
            Self::Original => "Original Image",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Amount,
    Paths,
    Corners,
    Noise,
    IgnoreColor,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dropdown {
    Preset,
    View,
    Mode,
    Palette,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Dropdown(Dropdown),
    Choose(Dropdown, usize),
    Field(Field),
    Slider(Field, f64, f64, f64),
    Advanced,
    Transparency,
    Ignore,
    Preview,
    Trace,
    Expand,
    Compare,
    PickIgnore,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub options: Options,
}
#[derive(Clone)]
pub struct Panel {
    pub options: Options,
    pub view: View,
    pub preview: bool,
    pub advanced: bool,
    pub dropdown: Option<Dropdown>,
    pub presets: Vec<Preset>,
    pub preset: Option<usize>,
    pub edit: Option<(Field, String, bool)>,
    pub name_edit: Option<(String, bool, bool)>,
    pub enabled: bool,
    pub busy: bool,
    pub can_expand: bool,
    pub status: String,
    pub counts: Option<(usize, usize, usize)>,
    pub comparing: bool,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            options: Options::default(),
            view: View::Result,
            preview: true,
            advanced: true,
            dropdown: None,
            presets: Options::presets()
                .into_iter()
                .map(|(name, options)| Preset {
                    name: name.into(),
                    options,
                })
                .collect(),
            preset: Some(0),
            edit: None,
            name_edit: None,
            enabled: false,
            busy: false,
            can_expand: false,
            status: "Select one image to trace".into(),
            counts: None,
            comparing: false,
        }
    }
}
impl Panel {
    pub fn choices(&self, d: Dropdown) -> Vec<String> {
        match d {
            Dropdown::Preset => self.presets.iter().map(|p| p.name.clone()).collect(),
            Dropdown::View => View::ALL.iter().map(|v| v.label().into()).collect(),
            Dropdown::Mode => ["Black and White", "Grayscale", "Color"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            Dropdown::Palette => ["Limited", "Full Tone", "Document Swatches"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
    pub fn value(&self, f: Field) -> f64 {
        match f {
            Field::Amount => {
                if self.options.mode == Mode::Color && self.options.palette_mode == 1 {
                    return self.options.tone_detail as f64;
                }
                if self.options.mode == Mode::BlackWhite {
                    self.options.threshold as f64
                } else {
                    self.options.colors as f64
                }
            }
            Field::Paths => self.options.paths,
            Field::Corners => self.options.corners,
            Field::Noise => self.options.noise as f64,
            Field::IgnoreColor => 0.,
        }
    }
    pub fn range(&self, f: Field) -> (f64, f64) {
        match f {
            Field::Amount => {
                if self.options.mode == Mode::Color && self.options.palette_mode == 1 {
                    return (0., 100.);
                }
                if self.options.mode == Mode::BlackWhite {
                    (0., 255.)
                } else {
                    (2., 256.)
                }
            }
            Field::Paths | Field::Corners => (0., 100.),
            Field::Noise => (1., 100.),
            Field::IgnoreColor => (0., 0.),
        }
    }
    pub fn set(&mut self, f: Field, v: f64) {
        if !v.is_finite() {
            return;
        }
        let (min, max) = self.range(f);
        let v = v.clamp(min, max);
        match f {
            Field::Amount => {
                if self.options.mode == Mode::Color && self.options.palette_mode == 1 {
                    self.options.tone_detail = v.round() as u8;
                    return;
                }
                if self.options.mode == Mode::BlackWhite {
                    self.options.threshold = v.round() as u8
                } else {
                    self.options.colors = v.round() as u16
                }
            }
            Field::Paths => self.options.paths = v,
            Field::Corners => self.options.corners = v,
            Field::Noise => self.options.noise = v.round() as u16,
            Field::IgnoreColor => {}
        }
    }
}
pub fn height() -> f64 {
    px(610.)
}
fn row(body: Rect, y: f64) -> Rect {
    Rect::new(
        body.x0 + px(12.),
        body.y0 + px(y),
        body.x1 - px(12.),
        body.y0 + px(y + 26.),
    )
}
fn field(r: Rect) -> Rect {
    Rect::new(r.x0 + px(78.), r.y0, r.x1, r.y1)
}
fn drop_row(body: Rect, d: Dropdown) -> Rect {
    field(row(
        body,
        match d {
            Dropdown::Preset => 12.,
            Dropdown::View => 48.,
            Dropdown::Mode => 84.,
            Dropdown::Palette => 120.,
        },
    ))
}
fn slider_row(body: Rect, f: Field) -> Rect {
    row(
        body,
        match f {
            Field::Amount => 160.,
            Field::Paths => 244.,
            Field::Corners => 290.,
            Field::Noise => 336.,
            Field::IgnoreColor => 448.,
        },
    )
}
fn track(r: Rect) -> Rect {
    Rect::new(r.x0 + px(78.), r.y0, r.x1 - px(65.), r.y1)
}
fn numeric(r: Rect) -> Rect {
    Rect::new(r.x1 - px(57.), r.y0, r.x1, r.y1)
}
fn menu_rows(body: Rect, p: &Panel, d: Dropdown) -> Vec<Rect> {
    let r = drop_row(body, d);
    let choices = p.choices(d);
    let h = px(25.);
    let y = (r.y1 + px(2.)).min(body.y0 + height() - h * choices.len() as f64 - px(6.));
    choices
        .iter()
        .enumerate()
        .map(|(i, _)| Rect::new(r.x0, y + i as f64 * h, r.x1, y + (i + 1) as f64 * h))
        .collect()
}
fn label(scene: &mut Scene, text: &mut TextContext, t: &Theme, s: &str, r: Rect) {
    text.draw(scene, s, 12., t.text, r.x0, r.center().y + px(4.));
}
fn checkbox(scene: &mut Scene, text: &mut TextContext, t: &Theme, s: &str, r: Rect, on: bool) {
    let b = Rect::new(r.x0, r.y0 + px(5.), r.x0 + px(16.), r.y0 + px(21.));
    scene.stroke(&Stroke::new(px(1.)), Affine::IDENTITY, t.border, None, &b);
    if on {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            t.accent,
            None,
            &b.inflate(-px(3.), -px(3.)),
        );
    }
    label(
        scene,
        text,
        t,
        s,
        Rect::new(r.x0 + px(24.), r.y0, r.x1, r.y1),
    );
}
pub fn paint(scene: &mut Scene, text: &mut TextContext, body: Rect, t: &Theme, p: &Panel) {
    for (d, name, value) in [
        (
            Dropdown::Preset,
            "Preset:",
            p.preset
                .and_then(|i| p.presets.get(i))
                .map_or("Custom".into(), |v| v.name.clone()),
        ),
        (Dropdown::View, "View:", p.view.label().into()),
        (
            Dropdown::Mode,
            "Mode:",
            match p.options.mode {
                Mode::BlackWhite => "Black and White",
                Mode::Grayscale => "Grayscale",
                Mode::Color => "Color",
            }
            .into(),
        ),
        (
            Dropdown::Palette,
            "Palette:",
            match p.options.palette_mode {
                0 => "Limited",
                1 => "Full Tone",
                _ => "Document Swatches",
            }
            .into(),
        ),
    ] {
        let r = drop_row(body, d);
        label(
            scene,
            text,
            t,
            name,
            Rect::new(body.x0 + px(12.), r.y0, r.x0, r.y1),
        );
        scene.fill(Fill::NonZero, Affine::IDENTITY, t.bg, None, &r);
        scene.stroke(&Stroke::new(px(1.)), Affine::IDENTITY, t.border, None, &r);
        let shown = if d == Dropdown::Preset {
            p.name_edit
                .as_ref()
                .map(|(s, _, _)| s.as_str())
                .unwrap_or(&value)
        } else {
            &value
        };
        scene.push_clip_layer(
            Fill::NonZero,
            Affine::IDENTITY,
            &r.inflate(-px(4.), -px(2.)),
        );
        label(
            scene,
            text,
            t,
            shown,
            Rect::new(r.x0 + px(7.), r.y0, r.x1 - px(16.), r.y1),
        );
        if d == Dropdown::Preset {
            if let Some((_, fresh, _)) = &p.name_edit {
                let x = (r.x0 + px(7.) + text.measure(shown, 12.)).min(r.x1 - px(6.));
                if *fresh {
                    scene.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        t.accent.with_alpha(0.25),
                        None,
                        &Rect::new(r.x0 + px(5.), r.y0 + px(3.), x, r.y1 - px(3.)),
                    );
                }
                scene.stroke(
                    &Stroke::new(px(1.)),
                    Affine::IDENTITY,
                    t.text,
                    None,
                    &Line::new((x, r.y0 + px(4.)), (x, r.y1 - px(4.))),
                );
            }
        }
        scene.pop_layer();
        text.draw(scene, "▾", 12., t.text_dim, r.x1 - px(15.), r.y0 + px(18.));
    }
    if p.options.mode != Mode::Color {
        let r = drop_row(body, Dropdown::Palette);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            t.panel_bg.with_alpha(0.55),
            None,
            &r,
        );
    }
    for f in [Field::Amount, Field::Paths, Field::Corners, Field::Noise] {
        if !p.advanced && f != Field::Amount {
            continue;
        }
        let r = slider_row(body, f);
        let label_ = match f {
            Field::Amount => match p.options.mode {
                Mode::BlackWhite => "Threshold:",
                Mode::Grayscale => "Grays:",
                Mode::Color => {
                    if p.options.palette_mode == 1 {
                        "Detail:"
                    } else {
                        "Colors:"
                    }
                }
            },
            Field::Paths => "Paths:",
            Field::Corners => "Corners:",
            _ => "Noise:",
        };
        label(scene, text, t, label_, r);
        let tr = track(r);
        let (min, max) = p.range(f);
        let x = tr.x0 + (tr.width() * (p.value(f) - min) / (max - min));
        scene.stroke(
            &Stroke::new(px(2.)),
            Affine::IDENTITY,
            t.border,
            None,
            &Line::new((tr.x0, tr.center().y), (tr.x1, tr.center().y)),
        );
        scene.stroke(
            &Stroke::new(px(2.)),
            Affine::IDENTITY,
            t.text_dim,
            None,
            &vello::kurbo::Circle::new((x, tr.center().y), px(5.)),
        );
        let n = numeric(r);
        scene.fill(Fill::NonZero, Affine::IDENTITY, t.bg, None, &n);
        let value = p
            .edit
            .as_ref()
            .filter(|(which, _, _)| *which == f)
            .map(|(_, s, _)| s.clone())
            .unwrap_or_else(|| {
                format!(
                    "{:.0}{}",
                    p.value(f),
                    if matches!(f, Field::Paths | Field::Corners) {
                        "%"
                    } else if f == Field::Noise {
                        " px"
                    } else {
                        ""
                    }
                )
            });
        label(
            scene,
            text,
            t,
            &value,
            Rect::new(n.x0 + px(4.), n.y0, n.x1, n.y1),
        );
        if p.edit.as_ref().is_some_and(|(which, _, _)| *which == f) {
            scene.stroke(&Stroke::new(px(1.)), Affine::IDENTITY, t.accent, None, &n);
        }
    }
    label(
        scene,
        text,
        t,
        if p.advanced {
            "▾ Advanced"
        } else {
            "▸ Advanced"
        },
        row(body, 206.),
    );
    if p.advanced {
        label(scene, text, t, "Create: Filled paths", row(body, 386.));
        checkbox(
            scene,
            text,
            t,
            "Transparency",
            row(body, 418.),
            p.options.transparency,
        );
        checkbox(
            scene,
            text,
            t,
            "Ignore Color",
            row(body, 450.),
            p.options.ignore,
        );
        let pick = Rect::new(
            body.x1 - px(115.),
            body.y0 + px(450.),
            body.x1 - px(76.),
            body.y0 + px(476.),
        );
        crate::widgets::button(scene, text, t, pick, "Pick", false);
        let n = numeric(row(body, 450.));
        let c = p.options.ignore_color;
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            vello::peniko::Color::from_rgb8(c[0], c[1], c[2]),
            None,
            &n,
        );
        if let Some((Field::IgnoreColor, s, _)) = &p.edit {
            scene.fill(Fill::NonZero, Affine::IDENTITY, t.bg, None, &n);
            label(scene, text, t, s, n);
        }
    }
    if p.options.mode == Mode::Color && p.options.palette_mode == 2 {
        let r = slider_row(body, Field::Amount);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            t.panel_bg.with_alpha(0.65),
            None,
            &r,
        );
    }
    let info = p
        .counts
        .map(|(a, b, c)| format!("Paths: {a}    Anchors: {b}    Colors: {c}"))
        .unwrap_or_else(|| "Paths: —    Anchors: —    Colors: —".into());
    text.draw(
        scene,
        &info,
        10.,
        t.text_dim,
        body.x0 + px(12.),
        body.y0 + px(504.),
    );
    let status = row(body, 512.);
    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &status);
    text.draw(
        scene,
        &p.status,
        10.,
        t.text_dim,
        status.x0,
        status.y0 + px(15.),
    );
    scene.pop_layer();
    let r = row(body, 544.);
    checkbox(scene, text, t, "Preview", r, p.preview);
    let b = Rect::new(r.x1 - px(95.), r.y0, r.x1, r.y1);
    crate::widgets::button(scene, text, t, b, "Expand", p.can_expand);
    let r = row(body, 578.);
    crate::widgets::button(
        scene,
        text,
        t,
        Rect::new(r.x0, r.y0, r.x0 + px(110.), r.y1),
        "Hold Original",
        false,
    );
    crate::widgets::button(
        scene,
        text,
        t,
        Rect::new(r.x1 - px(95.), r.y0, r.x1, r.y1),
        if p.busy { "Cancel" } else { "Image Trace" },
        false,
    );
    if let Some(d) = p.dropdown {
        let rows = menu_rows(body, p, d);
        for (r, s) in rows.iter().zip(p.choices(d)) {
            scene.fill(Fill::NonZero, Affine::IDENTITY, t.strip_bg, None, r);
            scene.stroke(&Stroke::new(px(1.)), Affine::IDENTITY, t.border, None, r);
            scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, r);
            label(
                scene,
                text,
                t,
                &s,
                Rect::new(r.x0 + px(5.), r.y0, r.x1, r.y1),
            );
            scene.pop_layer();
        }
    }
}
pub fn hit(body: Rect, point: Point, p: &Panel) -> Hit {
    if let Some(d) = p.dropdown {
        return menu_rows(body, p, d)
            .iter()
            .position(|r| r.contains(point))
            .map_or(Hit::Dropdown(d), |i| Hit::Choose(d, i));
    }
    for d in [
        Dropdown::Preset,
        Dropdown::View,
        Dropdown::Mode,
        Dropdown::Palette,
    ] {
        if drop_row(body, d).contains(point) {
            if d == Dropdown::Palette && p.options.mode != Mode::Color {
                return Hit::None;
            }
            return Hit::Dropdown(d);
        }
    }
    for f in [Field::Amount, Field::Paths, Field::Corners, Field::Noise] {
        if !p.advanced && f != Field::Amount {
            continue;
        }
        if f == Field::Amount && p.options.mode == Mode::Color && p.options.palette_mode == 2 {
            continue;
        }
        let r = slider_row(body, f);
        if numeric(r).contains(point) {
            return Hit::Field(f);
        }
        let tr = track(r);
        if tr.contains(point) {
            return Hit::Slider(f, point.x, tr.x0, tr.x1);
        }
    }
    if row(body, 206.).contains(point) {
        return Hit::Advanced;
    }
    if p.advanced {
        if row(body, 418.).contains(point) {
            return Hit::Transparency;
        }
        if Rect::new(
            body.x1 - px(115.),
            body.y0 + px(450.),
            body.x1 - px(76.),
            body.y0 + px(476.),
        )
        .contains(point)
        {
            return Hit::PickIgnore;
        }
        if numeric(row(body, 450.)).contains(point) {
            return Hit::Field(Field::IgnoreColor);
        }
        if row(body, 450.).contains(point) {
            return Hit::Ignore;
        }
    }
    let r = row(body, 544.);
    if r.contains(point) {
        return if point.x > r.x1 - px(95.) {
            Hit::Expand
        } else {
            Hit::Preview
        };
    }
    let r = row(body, 578.);
    if r.contains(point) {
        return if point.x > r.x1 - px(95.) {
            Hit::Trace
        } else {
            Hit::Compare
        };
    }
    Hit::None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropdowns_and_fields_use_the_same_geometry() {
        let mut panel = Panel::default();
        let body = Rect::new(20., 30., 340., 640.);
        for d in [Dropdown::Preset, Dropdown::View, Dropdown::Mode] {
            assert_eq!(
                hit(body, drop_row(body, d).center(), &panel),
                Hit::Dropdown(d)
            );
            panel.dropdown = Some(d);
            for (i, r) in menu_rows(body, &panel, d).iter().enumerate() {
                assert_eq!(hit(body, r.center(), &panel), Hit::Choose(d, i));
            }
            panel.dropdown = None;
        }
        for f in [Field::Amount, Field::Paths, Field::Corners, Field::Noise] {
            assert_eq!(
                hit(body, numeric(slider_row(body, f)).center(), &panel),
                Hit::Field(f)
            );
        }
        panel.advanced = false;
        assert_eq!(
            hit(
                body,
                numeric(slider_row(body, Field::Paths)).center(),
                &panel
            ),
            Hit::None
        );
    }
    #[test]
    fn amount_control_tracks_the_selected_mode() {
        let mut p = Panel::default();
        p.set(Field::Amount, 200.);
        assert_eq!(p.options.threshold, 200);
        p.options.mode = Mode::Color;
        p.set(Field::Amount, 6.);
        assert_eq!(p.options.colors, 6);
        p.options.palette_mode = 1;
        p.set(Field::Amount, 90.);
        assert_eq!(p.options.tone_detail, 90);
        assert_eq!(p.options.colors, 6);
    }
}
