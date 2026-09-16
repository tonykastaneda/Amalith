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
        (rect(body, 356., 14., 68., 28.), Hit::Shuffle),
        (rect(body, 432., 14., 68., 28.), Hit::Reset),
    ];
    for i in d.page * PAGE..((d.page + 1) * PAGE).min(d.source.len()) {
        let y = 80. + (i % PAGE) as f64 * 32.;
        out.extend([
            (rect(body, 20., y, 324., 28.), Hit::Row(i)),
            (rect(body, 352., y, 36., 28.), Hit::Toggle(i)),
            (rect(body, 396., y, 104., 28.), Hit::Picker(i)),
        ]);
    }
    out.extend([
        (rect(body, 420., 342., 36., 24.), Hit::Previous),
        (rect(body, 464., 342., 36., 24.), Hit::Next),
        (rect(body, 20., 386., 126., 28.), Hit::Hex),
    ]);
    for i in 0..3 {
        let x = 160. + i as f64 * 114.;
        out.extend([
            (rect(body, x, 410., 30., 24.), Hit::Step(i, -1.)),
            (rect(body, x + 70., 410., 30., 24.), Hit::Step(i, 1.)),
        ]);
    }
    out.extend([
        (rect(body, 20., 478., 148., 32.), Hit::Preview),
        (rect(body, 304., 478., 92., 32.), Hit::Cancel),
        (rect(body, 408., 478., 92., 32.), Hit::Ok),
    ]);
    out
}
pub fn hit(d: &RecolorDialog, body: Rect, p: Point) -> Option<Hit> {
    controls(d, body)
        .into_iter()
        .find(|(r, _)| r.contains(p))
        .map(|(_, h)| h)
}
pub fn paint(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    body: Rect,
    d: &RecolorDialog,
) {
    let mut label = |s: &str, x: f64, y: f64| {
        text.draw(scene, s, 12., theme.text, body.x0 + px(x), body.y0 + px(y))
    };
    label("Assign artwork colors", 20., 32.);
    label(&format!("Current colors ({})", d.source.len()), 20., 68.);
    label("Recolor", 348., 68.);
    label("New", 396., 68.);
    label(
        &format!(
            "{}–{} of {}",
            d.page * PAGE + 1,
            ((d.page + 1) * PAGE).min(d.source.len()),
            d.source.len()
        ),
        20.,
        360.,
    );
    label("Replacement · RGB", 20., 382.);
    label("Symbols and raster images are not recolored.", 20., 461.);
    for (r, h) in controls(d, body) {
        let button = match h {
            Hit::Row(i) | Hit::Picker(i) => {
                if i == d.selected {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &r);
                }
                let c = if matches!(h, Hit::Row(_)) {
                    d.source[i]
                } else {
                    d.replacement[i]
                };
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    crate::convert::color(c),
                    None,
                    &r.inset(-px(3.)),
                );
                None
            }
            Hit::Toggle(i) => Some(if d.enabled[i] { "→" } else { "—" }.to_string()),
            Hit::Hex => Some(format!("#{}{}", d.hex, if d.editing { "|" } else { "" })),
            Hit::Step(_, delta) => Some(if delta < 0. { "−" } else { "+" }.into()),
            Hit::Previous => Some("‹".into()),
            Hit::Next => Some("›".into()),
            Hit::Reset => Some("Reset".into()),
            Hit::Shuffle => Some("Rotate".into()),
            Hit::Preview => Some(if d.preview { "✓ Preview" } else { "Preview" }.into()),
            Hit::Cancel => Some("Cancel".into()),
            Hit::Ok => Some("OK".into()),
        };
        if let Some(label) = button {
            crate::widgets::button(scene, text, theme, r, &label, matches!(h, Hit::Ok));
        }
    }
    let c = d.replacement[d.selected];
    for (i, (name, value)) in [("R", c.r), ("G", c.g), ("B", c.b)].iter().enumerate() {
        text.draw(
            scene,
            &format!("{}  {}", name, (value * 255.).round() as u8),
            12.,
            theme.text,
            body.x0 + px(164. + i as f64 * 114.),
            body.y0 + px(401.),
        );
    }
    if let Some(error) = &d.error {
        text.draw(
            scene,
            error,
            11.,
            theme.accent,
            body.x0 + px(20.),
            body.y0 + px(445.),
        );
    }
}
