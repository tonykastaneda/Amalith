//! Recolor Artwork's Assign view. UI state is separate from the live document.
use crate::{metrics::px, text::TextContext, Theme};
use amalith_core::{Color, Document, ObjectId};
use vello::{
    kurbo::{Affine, Point, Rect},
    peniko::Fill,
    Scene,
};

pub fn width() -> f64 {
    px(520.0)
}
pub fn height() -> f64 {
    px(530.0)
}
const PAGE: usize = 8;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    Row(usize),
    Toggle(usize),
    Picker(usize),
    Hex,
    Step(usize, f32),
    Previous,
    Next,
    Reset,
    Shuffle,
    Preview,
    Ok,
    Cancel,
}
pub struct RecolorDialog {
    pub document: ObjectId,
    pub revision: u64,
    pub ids: Vec<ObjectId>,
    pub original: Document,
    pub rendered: Document,
    pub source: Vec<Color>,
    pub replacement: Vec<Color>,
    pub enabled: Vec<bool>,
    pub selected: usize,
    pub page: usize,
    pub preview: bool,
    pub hex: String,
    pub editing: bool,
    pub fresh: bool,
    pub error: Option<String>,
}
impl RecolorDialog {
    pub fn open(
        document: ObjectId,
        revision: u64,
        original: Document,
        ids: Vec<ObjectId>,
    ) -> Option<Self> {
        let source = amalith_commands::recolor::colors(&original, &ids);
        if source.is_empty() {
            return None;
        }
        let mut d = Self {
            document,
            revision,
            ids,
            rendered: original.clone(),
            original,
            replacement: source.clone(),
            enabled: vec![true; source.len()],
            source,
            selected: 0,
            page: 0,
            preview: true,
            hex: String::new(),
            editing: false,
            fresh: true,
            error: None,
        };
        d.sync_hex();
        Some(d)
    }
    pub fn mapping(&self) -> Vec<(Color, Color)> {
        self.source
            .iter()
            .zip(&self.replacement)
            .zip(&self.enabled)
            .filter_map(|((&a, &b), &on)| (on && a != b).then_some((a, b)))
            .collect()
    }
    pub fn refresh(&mut self) {
        let mut editor = amalith_commands::Editor::new(self.original.clone());
        match editor.execute(amalith_commands::Command::RecolorArtwork {
            objects: self.ids.clone(),
            colors: self.mapping(),
        }) {
            Ok(_) => {
                self.rendered = editor.document().clone();
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.sync_hex();
    }
    pub fn sync_hex(&mut self) {
        let c = self.replacement[self.selected];
        self.hex = format!(
            "{:02X}{:02X}{:02X}",
            (c.r * 255.).round() as u8,
            (c.g * 255.).round() as u8,
            (c.b * 255.).round() as u8
        );
    }
    pub fn commit_hex(&mut self) -> bool {
        if self.hex.len() != 6 {
            self.error = Some("Enter six hexadecimal digits.".into());
            return false;
        }
        let Ok(v) = u32::from_str_radix(&self.hex, 16) else {
            return false;
        };
        let a = self.replacement[self.selected].a;
        self.replacement[self.selected] = Color::rgba(
            ((v >> 16) & 255) as f32 / 255.,
            ((v >> 8) & 255) as f32 / 255.,
            (v & 255) as f32 / 255.,
            a,
        );
        self.editing = false;
        self.refresh();
        true
    }
}
fn rect(body: Rect, x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(
        body.x0 + px(x),
        body.y0 + px(y),
        body.x0 + px(x + w),
        body.y0 + px(y + h),
    )
}
fn controls(d: &RecolorDialog, body: Rect) -> Vec<(Rect, Hit)> {
    let mut out = vec![
        (rect(body, 464., 16., 36., 28.), Hit::Reset),
        (rect(body, 20., 320., 138., 28.), Hit::Shuffle),
    ];
    for i in d.page * PAGE..((d.page + 1) * PAGE).min(d.source.len()) {
        let y = 100. + (i % PAGE) as f64 * 26.;
        out.extend([
            (rect(body, 24., y, 292., 26.), Hit::Row(i)),
            (rect(body, 324., y, 32., 26.), Hit::Toggle(i)),
            (rect(body, 364., y, 132., 26.), Hit::Picker(i)),
        ]);
    }
    if d.page > 0 {
        out.push((rect(body, 432., 320., 28., 28.), Hit::Previous));
    }
    if (d.page + 1) * PAGE < d.source.len() {
        out.push((rect(body, 472., 320., 28., 28.), Hit::Next));
    }
    out.push((rect(body, 110., 378., 154., 28.), Hit::Hex));
    out.push((rect(body, 20., 378., 72., 72.), Hit::Picker(d.selected)));
    for i in 0..3 {
        let x = 110. + i as f64 * 134.;
        out.extend([
            (rect(body, x + 92., 420., 22., 15.), Hit::Step(i, 1.)),
            (rect(body, x + 92., 435., 22., 15.), Hit::Step(i, -1.)),
        ]);
    }
    out.extend([
        (rect(body, 20., 486., 148., 28.), Hit::Preview),
        (rect(body, 304., 482., 92., 32.), Hit::Cancel),
        (rect(body, 408., 482., 92., 32.), Hit::Ok),
    ]);
    out
}
pub fn hit(d: &RecolorDialog, body: Rect, p: Point) -> Option<Hit> {
    controls(d, body)
        .into_iter()
        .find(|(r, _)| r.contains(p))
        .map(|(_, h)| h)
}
pub fn tip(d: &RecolorDialog, body: Rect, p: Point) -> Option<String> {
    Some(
        match hit(d, body, p)? {
            Hit::Row(_) => "Select a color mapping",
            Hit::Picker(_) => "Edit replacement color in the Color Picker",
            Hit::Toggle(i) => {
                if d.enabled[i] {
                    "Keep this original color unchanged"
                } else {
                    "Enable recoloring for this color"
                }
            }
            Hit::Hex => "Edit replacement hex color",
            Hit::Step(_, v) => {
                if v > 0. {
                    "Increase channel by 1"
                } else {
                    "Decrease channel by 1"
                }
            }
            Hit::Previous => "Previous colors",
            Hit::Next => "Next colors",
            Hit::Reset => "Reset all color assignments",
            Hit::Shuffle => "Cycle replacement colors",
            Hit::Preview => "Preview recoloring on the canvas",
            Hit::Ok => "Apply recoloring",
            Hit::Cancel => "Cancel without changing the artwork",
        }
        .into(),
    )
}

fn line(scene: &mut Scene, points: &[(f64, f64)], body: Rect, color: vello::peniko::Color) {
    let mut path = vello::kurbo::BezPath::new();
    for (i, &(x, y)) in points.iter().enumerate() {
        let p = (body.x0 + px(x), body.y0 + px(y));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    scene.stroke(
        &vello::kurbo::Stroke::new(px(1.4)),
        Affine::IDENTITY,
        color,
        None,
        &path,
    );
}
fn swatch(scene: &mut Scene, theme: &Theme, r: Rect, c: Color) {
    // Checkerboard makes retained opacity visible, including transparent colors.
    let size = px(5.);
    for row in 0..if c.a < 1. {
        (r.height() / size).ceil() as usize
    } else {
        0
    } {
        for col in 0..(r.width() / size).ceil() as usize {
            let tile = Rect::new(
                r.x0 + col as f64 * size,
                r.y0 + row as f64 * size,
                (r.x0 + (col + 1) as f64 * size).min(r.x1),
                (r.y0 + (row + 1) as f64 * size).min(r.y1),
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                if (row + col) % 2 == 0 {
                    theme.border
                } else {
                    theme.panel_bg
                },
                None,
                &tile,
            );
        }
    }
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        crate::convert::color(c),
        None,
        &r,
    );
    scene.stroke(
        &vello::kurbo::Stroke::new(px(1.)),
        Affine::IDENTITY,
        theme.border,
        None,
        &r,
    );
}
fn icon(scene: &mut Scene, theme: &Theme, r: Rect, h: Hit, active: bool) {
    let b = Rect::from_origin_size(
        (r.center().x - px(10.), r.center().y - px(10.)),
        (px(20.), px(20.)),
    );
    let ink = if active { theme.text } else { theme.text_dim };
    match h {
        Hit::Reset => {
            let mut p = vello::kurbo::BezPath::new();
            p.move_to((5., 5.));
            p.curve_to((12., -1.), (21., 7.), (16., 14.));
            p.curve_to((13., 19.), (5., 18.), (3., 12.));
            scene.stroke(
                &vello::kurbo::Stroke::new(px(1.5)),
                Affine::translate((b.x0, b.y0)) * Affine::scale(px(1.)),
                ink,
                None,
                &p,
            );
            line(scene, &[(5., 1.), (5., 6.), (10., 6.)], b, ink);
        }
        Hit::Shuffle => {
            line(scene, &[(3., 6.), (17., 6.), (14., 3.)], b, ink);
            line(scene, &[(17., 14.), (3., 14.), (6., 17.)], b, ink);
            for x in [5., 9., 13.] {
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    ink,
                    None,
                    &rect(b, x, 9., 2., 2.),
                );
            }
        }
        Hit::Toggle(_) if active => {
            line(scene, &[(4., 10.), (16., 10.), (12., 6.)], b, ink);
            line(scene, &[(16., 10.), (12., 14.)], b, ink);
        }
        Hit::Toggle(_) => line(scene, &[(6., 10.), (14., 10.)], b, ink),
        Hit::Previous => line(scene, &[(12., 5.), (7., 10.), (12., 15.)], b, ink),
        Hit::Next => line(scene, &[(8., 5.), (13., 10.), (8., 15.)], b, ink),
        Hit::Step(_, v) => {
            let y = if v > 0. { 8. } else { 12. };
            line(scene, &[(7., 20. - y), (10., y), (13., 20. - y)], b, ink);
        }
        _ => {}
    }
}
pub fn paint(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    body: Rect,
    d: &RecolorDialog,
) {
    let label = |scene: &mut Scene, text: &mut TextContext, s: &str, x: f64, y: f64, dim: bool| {
        text.draw(
            scene,
            s,
            12.,
            if dim { theme.text_dim } else { theme.text },
            body.x0 + px(x),
            body.y0 + px(y),
        );
    };
    label(scene, text, "Artwork colors", 20., 33., false);
    let n = d.source.len().min(24);
    for i in 0..n {
        let r = rect(
            body,
            130. + i as f64 * 316. / n as f64,
            18.,
            316. / n as f64,
            22.,
        );
        swatch(scene, theme, r, d.source[i]);
    }
    line(scene, &[(20., 58.), (500., 58.)], body, theme.border);
    label(
        scene,
        text,
        &format!("Current colors ({})", d.source.len()),
        24.,
        86.,
        false,
    );
    label(scene, text, "New", 364., 86., false);
    let table = rect(body, 20., 96., 480., 214.);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.strip_bg,
        None,
        &table,
    );
    scene.stroke(
        &vello::kurbo::Stroke::new(px(1.)),
        Affine::IDENTITY,
        theme.border,
        None,
        &table,
    );
    for i in d.page * PAGE..((d.page + 1) * PAGE).min(d.source.len()) {
        let y = 100. + (i % PAGE) as f64 * 26.;
        if i == d.selected {
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.marquee_fill,
                None,
                &rect(body, 21., y, 478., 26.),
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.accent,
                None,
                &rect(body, 21., y, 2., 26.),
            );
        }
        swatch(
            scene,
            theme,
            rect(body, 28., y + 4., 284., 18.),
            d.source[i],
        );
        swatch(
            scene,
            theme,
            rect(body, 368., y + 4., 96., 18.),
            if d.enabled[i] {
                d.replacement[i]
            } else {
                d.source[i]
            },
        );
        if d.enabled[i] {
            crate::icons::draw(
                scene,
                crate::icons::Icon::Pencil,
                rect(body, 474., y + 6., 14., 14.),
                theme.text_dim,
            );
        }
    }
    label(scene, text, "Cycle colors", 56., 338., true);
    label(
        scene,
        text,
        &format!(
            "{}–{} of {}",
            d.page * PAGE + 1,
            ((d.page + 1) * PAGE).min(d.source.len()),
            d.source.len()
        ),
        320.,
        338.,
        true,
    );
    line(scene, &[(20., 358.), (500., 358.)], body, theme.border);
    label(scene, text, "Replacement color", 20., 374., false);
    label(scene, text, "Hex", 280., 396., true);
    for (i, (name, value)) in [
        ("R", d.replacement[d.selected].r),
        ("G", d.replacement[d.selected].g),
        ("B", d.replacement[d.selected].b),
    ]
    .iter()
    .enumerate()
    {
        let x = 110. + i as f64 * 134.;
        let r = rect(body, x, 420., 114., 30.);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.strip_bg,
            None,
            &r.to_rounded_rect(px(3.)),
        );
        scene.stroke(
            &vello::kurbo::Stroke::new(px(1.)),
            Affine::IDENTITY,
            theme.border,
            None,
            &r.to_rounded_rect(px(3.)),
        );
        label(scene, text, name, x + 8., 439., true);
        label(
            scene,
            text,
            &format!("{}", (value * 255.).round() as u8),
            x + 36.,
            439.,
            false,
        );
        line(
            scene,
            &[(x + 92., 421.), (x + 92., 449.)],
            body,
            theme.border,
        );
    }
    for (r, h) in controls(d, body) {
        match h {
            Hit::Row(_) => {}
            Hit::Picker(i) => {
                if r.height() > px(30.) {
                    swatch(scene, theme, r, d.replacement[i]);
                }
            }
            Hit::Shuffle => {
                icon(
                    scene,
                    theme,
                    Rect::from_origin_size(r.origin(), (px(28.), px(28.))),
                    h,
                    true,
                );
            }
            Hit::Reset | Hit::Previous | Hit::Next => {
                icon(scene, theme, r, h, true);
            }
            Hit::Step(_, _) => icon(scene, theme, r, h, true),
            Hit::Toggle(i) => icon(scene, theme, r, h, d.enabled[i]),
            Hit::Hex => {
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    theme.strip_bg,
                    None,
                    &r.to_rounded_rect(px(3.)),
                );
                scene.stroke(
                    &vello::kurbo::Stroke::new(px(1.)),
                    Affine::IDENTITY,
                    if d.editing {
                        theme.accent
                    } else {
                        theme.border
                    },
                    None,
                    &r.to_rounded_rect(px(3.)),
                );
                label(
                    scene,
                    text,
                    &format!("#{}{}", d.hex, if d.editing { "|" } else { "" }),
                    118.,
                    396.,
                    false,
                );
            }
            Hit::Preview => {
                let check = rect(body, 20., 493., 14., 14.);
                scene.stroke(
                    &vello::kurbo::Stroke::new(px(1.)),
                    Affine::IDENTITY,
                    theme.text_dim,
                    None,
                    &check.to_rounded_rect(px(2.)),
                );
                if d.preview {
                    scene.fill(
                        Fill::NonZero,
                        Affine::IDENTITY,
                        theme.accent,
                        None,
                        &check.to_rounded_rect(px(2.)),
                    );
                    line(
                        scene,
                        &[(23., 500.), (26., 503.), (31., 497.)],
                        body,
                        theme.on_accent,
                    );
                }
                label(scene, text, "Preview", 42., 504., false);
            }
            Hit::Ok | Hit::Cancel => crate::widgets::button(
                scene,
                text,
                theme,
                r,
                if h == Hit::Ok { "OK" } else { "Cancel" },
                h == Hit::Ok,
            ),
        }
    }
    line(scene, &[(20., 474.), (500., 474.)], body, theme.border);
    if let Some(error) = &d.error {
        text.draw(
            scene,
            error,
            11.,
            theme.accent,
            body.x0 + px(20.),
            body.y0 + px(467.),
        );
    }
}
