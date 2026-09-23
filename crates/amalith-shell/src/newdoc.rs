//! New Document form state and data-entry logic — the fields, units,
//! bleed, and layer-kind settings a document starts with. The only UI
//! that presents this now is the compact "New Document" overlay
//! (`app/multiplexer.rs`'s `QuickNewDoc`/`paint_quick_newdoc`/
//! `quick_newdoc_press`), which owns layout, painting and hit-testing
//! itself; this module just holds [`NewDocForm`]'s state and the pure
//! data-entry logic (`amalith_core::Document` and back).

use amalith_core::{ColorMode, LayerKind, Length, PreviewMode, RasterEffects, Unit};

use crate::text::TextContext;
use crate::text_field::TextField;

/// A text-editable field.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Name,
    Width,
    Height,
    BleedTop,
    BleedBottom,
    BleedLeft,
    BleedRight,
}

/// A dropdown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Menu {
    Unit,
    Color,
    Raster,
    /// Which kind of layer the document starts with — Vector (the
    /// long-standing default) or Raster, for a document that's going to
    /// be pixel work from the first stroke.
    StartLayer,
}

pub struct NewDocForm {
    pub name: TextField,
    /// Width / height fields, in the current [`unit`](Self::unit).
    pub width: TextField,
    pub height: TextField,
    /// Bleed fields: top, bottom, left, right.
    pub bleed: [TextField; 4],
    pub unit: Unit,
    pub artboards: usize,
    pub bleed_linked: bool,
    pub color_mode: ColorMode,
    pub raster: RasterEffects,
    pub preview: PreviewMode,
    /// The new document's first layer's kind (Vector or Raster).
    pub start_layer: LayerKind,
    pub focus: Option<Field>,
    pub open_menu: Option<Menu>,
}

impl Default for NewDocForm {
    fn default() -> Self {
        Self {
            name: TextField::new("Untitled-1"),
            width: TextField::new("3"),
            height: TextField::new("3"),
            bleed: std::array::from_fn(|_| TextField::new("0")),
            unit: Unit::In,
            artboards: 1,
            bleed_linked: true,
            color_mode: ColorMode::Cmyk,
            raster: RasterEffects::High300,
            preview: PreviewMode::Default,
            start_layer: LayerKind::Vector,
            focus: Some(Field::Name),
            open_menu: None,
        }
    }
}

const FIELDS: [Field; 7] = [
    Field::Name,
    Field::Width,
    Field::Height,
    Field::BleedTop,
    Field::BleedBottom,
    Field::BleedLeft,
    Field::BleedRight,
];

impl NewDocForm {
    pub fn field(&mut self, f: Field) -> &mut TextField {
        match f {
            Field::Name => &mut self.name,
            Field::Width => &mut self.width,
            Field::Height => &mut self.height,
            Field::BleedTop => &mut self.bleed[0],
            Field::BleedBottom => &mut self.bleed[1],
            Field::BleedLeft => &mut self.bleed[2],
            Field::BleedRight => &mut self.bleed[3],
        }
    }

    fn text_of(&self, f: Field) -> String {
        match f {
            Field::Name => self.name.text(),
            Field::Width => self.width.text(),
            Field::Height => self.height.text(),
            Field::BleedTop => self.bleed[0].text(),
            Field::BleedBottom => self.bleed[1].text(),
            Field::BleedLeft => self.bleed[2].text(),
            Field::BleedRight => self.bleed[3].text(),
        }
    }

    /// The focused field, if any.
    pub fn focused(&self) -> Option<Field> {
        self.focus
    }

    /// Move focus to the next / previous field (Tab), reformatting the one
    /// we leave and selecting all of the one we land on.
    pub fn focus_next(&mut self, back: bool, tcx: &mut TextContext) {
        self.commit_focus();
        let cur = self.focus.and_then(|f| FIELDS.iter().position(|x| *x == f));
        let n = FIELDS.len();
        let next = cur.map_or(0, |i| if back { (i + n - 1) % n } else { (i + 1) % n });
        self.focus = Some(FIELDS[next]);
        self.field(FIELDS[next]).select_all(tcx);
    }

    /// Nudge the focused numeric field by `delta` (the Up / Down arrows),
    /// re-select it, and propagate to the linked bleed set.
    pub fn step_focused(&mut self, delta: f64, tcx: &mut TextContext) {
        let Some(f) = self.focus else { return };
        if f == Field::Name {
            return;
        }
        let v = (parse(&self.text_of(f), self.unit) + delta).max(0.0);
        let s = fmt(v);
        self.field(f).set_text(&s);
        self.field(f).select_all(tcx);
        if self.bleed_linked
            && matches!(
                f,
                Field::BleedTop | Field::BleedBottom | Field::BleedLeft | Field::BleedRight
            )
        {
            for i in 0..4 {
                self.bleed[i].set_text(&s);
            }
        }
    }

    /// Normalise the focused numeric field (and propagate linked bleed).
    pub fn commit_focus(&mut self) {
        let Some(f) = self.focus else { return };
        if f == Field::Name {
            return;
        }
        let v = parse(&self.text_of(f), self.unit);
        self.field(f).set_text(&fmt(v));
        if self.bleed_linked
            && matches!(
                f,
                Field::BleedTop | Field::BleedBottom | Field::BleedLeft | Field::BleedRight
            )
        {
            let s = fmt(v);
            for i in 0..4 {
                self.bleed[i].set_text(&s);
            }
        }
    }

    pub fn set_unit(&mut self, unit: Unit) {
        if unit == self.unit {
            return;
        }
        let old = self.unit;
        let conv = |tf: &mut TextField| {
            let v = Length::new(parse(&tf.text(), old), old).in_unit(unit);
            tf.set_text(&fmt(v));
        };
        conv(&mut self.width);
        conv(&mut self.height);
        for b in &mut self.bleed {
            conv(b);
        }
        self.unit = unit;
    }

    pub fn set_link(&mut self, on: bool) {
        self.bleed_linked = on;
        if on {
            let s = self.bleed[0].text();
            for i in 0..4 {
                self.bleed[i].set_text(&s);
            }
        }
    }

    pub(crate) fn portrait(&self) -> bool {
        parse(&self.height.text(), self.unit) >= parse(&self.width.text(), self.unit)
    }

    pub fn set_orientation(&mut self, portrait: bool) {
        self.commit_focus();
        if portrait != self.portrait() {
            std::mem::swap(&mut self.width, &mut self.height);
        }
    }

    pub fn width_px(&self) -> f64 {
        Length::new(parse(&self.width.text(), self.unit), self.unit).px()
    }
    pub fn height_px(&self) -> f64 {
        Length::new(parse(&self.height.text(), self.unit), self.unit).px()
    }
    /// Bleed in px: top, bottom, left, right.
    pub fn bleed_px(&self) -> [f64; 4] {
        std::array::from_fn(|i| Length::new(parse(&self.bleed[i].text(), self.unit), self.unit).px())
    }
}

/// Parses a Width/Height/Bleed buffer — arithmetic, plus any per-literal
/// unit suffix (`5in`, `3cm`) converted into `unit` — so typing e.g. `5in`
/// while the dialog's Units dropdown reads `px` converts it to px.
fn parse(s: &str, unit: Unit) -> f64 {
    amalith_core::parse_measurement(s, amalith_core::MeasureKind::Length(unit))
        .unwrap_or(0.0)
        .max(0.0)
}

/// Format a number with up to 3 decimals, trailing zeros trimmed.
fn fmt(v: f64) -> String {
    let mut s = format!("{v:.3}");
    while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
        s.pop();
    }
    s
}

pub fn unit_label(u: Unit) -> &'static str {
    u.label()
}
pub(crate) const UNITS: [Unit; 5] = [Unit::Px, Unit::Pt, Unit::In, Unit::Mm, Unit::Cm];
pub(crate) const COLORS: [ColorMode; 2] = [ColorMode::Cmyk, ColorMode::Rgb];
pub(crate) const RASTERS: [RasterEffects; 3] = [
    RasterEffects::Screen72,
    RasterEffects::Medium150,
    RasterEffects::High300,
];
pub(crate) const START_LAYERS: [LayerKind; 2] = [LayerKind::Vector, LayerKind::Raster];

pub(crate) fn color_label(c: ColorMode) -> &'static str {
    match c {
        ColorMode::Cmyk => "CMYK Color",
        ColorMode::Rgb => "RGB Color",
    }
}
pub(crate) fn raster_label(r: RasterEffects) -> &'static str {
    match r {
        RasterEffects::Screen72 => "Screen (72 ppi)",
        RasterEffects::Medium150 => "Medium (150 ppi)",
        RasterEffects::High300 => "High (300 ppi)",
    }
}
pub(crate) fn start_layer_label(k: LayerKind) -> &'static str {
    match k {
        LayerKind::Vector => "Vector Layer",
        LayerKind::Raster => "Raster Layer",
    }
}

/// The value at index `i` of a menu (clamped).
pub fn menu_unit(i: usize) -> Unit {
    UNITS[i.min(UNITS.len() - 1)]
}
pub fn menu_color(i: usize) -> ColorMode {
    COLORS[i.min(COLORS.len() - 1)]
}
pub fn menu_raster(i: usize) -> RasterEffects {
    RASTERS[i.min(RASTERS.len() - 1)]
}
pub fn menu_start_layer(i: usize) -> LayerKind {
    START_LAYERS[i.min(START_LAYERS.len() - 1)]
}
