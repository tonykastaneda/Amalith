//! The New Document modal (File ▸ New) — a hand-built form over a dimmed
//! backdrop. No widget toolkit: [`build`] lays out every interactive rect,
//! [`paint`] renders from those rects, and [`hit`] maps a click back to a
//! [`Hit`]. `main.rs` owns the [`NewDocForm`] state and drives it.

use amalith_core::{ColorMode, Length, PreviewMode, RasterEffects, Unit};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;
const FH: f64 = 30.0;

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
    Preview,
}

/// What a click on the modal resolves to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    None,
    Backdrop,
    Field(Field),
    ToggleMenu(Menu),
    MenuItem(Menu, usize),
    Orientation(bool),
    ArtboardMinus,
    ArtboardPlus,
    ToggleLink,
    Create,
    Close,
}

pub struct NewDocForm {
    pub name: String,
    /// Width / height edit buffers, in the current [`unit`](Self::unit).
    pub width: String,
    pub height: String,
    /// Bleed buffers: top, bottom, left, right.
    pub bleed: [String; 4],
    pub unit: Unit,
    pub artboards: usize,
    pub bleed_linked: bool,
    pub color_mode: ColorMode,
    pub raster: RasterEffects,
    pub preview: PreviewMode,
    pub focus: Option<Field>,
    pub open_menu: Option<Menu>,
}

impl Default for NewDocForm {
    fn default() -> Self {
        Self {
            name: "Untitled-1".into(),
            width: "3".into(),
            height: "3".into(),
            bleed: std::array::from_fn(|_| "0".into()),
            unit: Unit::In,
            artboards: 1,
            bleed_linked: true,
            color_mode: ColorMode::Cmyk,
            raster: RasterEffects::High300,
            preview: PreviewMode::Default,
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
    fn buf(&mut self, f: Field) -> &mut String {
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

    fn buf_ref(&self, f: Field) -> &str {
        match f {
            Field::Name => &self.name,
            Field::Width => &self.width,
            Field::Height => &self.height,
            Field::BleedTop => &self.bleed[0],
            Field::BleedBottom => &self.bleed[1],
            Field::BleedLeft => &self.bleed[2],
            Field::BleedRight => &self.bleed[3],
        }
    }

    /// Type a character into the focused field.
    pub fn push_char(&mut self, ch: char) {
        if let Some(f) = self.focus {
            self.buf(f).push(ch);
        }
    }

    /// Backspace the focused field.
    pub fn backspace(&mut self) {
        if let Some(f) = self.focus {
            self.buf(f).pop();
        }
    }

    /// Move focus to the next field (Tab), reformatting the one we leave.
    pub fn focus_next(&mut self) {
        self.commit_focus();
        let cur = self.focus.and_then(|f| FIELDS.iter().position(|x| *x == f));
        let next = cur.map_or(0, |i| (i + 1) % FIELDS.len());
        self.focus = Some(FIELDS[next]);
    }

    /// Normalise the focused numeric field (and propagate linked bleed).
    pub fn commit_focus(&mut self) {
        let Some(f) = self.focus else { return };
        if f == Field::Name {
            return;
        }
        let v = parse(self.buf_ref(f));
        *self.buf(f) = fmt(v);
        if self.bleed_linked && matches!(f, Field::BleedTop | Field::BleedBottom | Field::BleedLeft | Field::BleedRight) {
            let s = fmt(v);
            self.bleed = [s.clone(), s.clone(), s.clone(), s];
        }
    }

    pub fn set_unit(&mut self, unit: Unit) {
        if unit == self.unit {
            return;
        }
        let old = self.unit;
        let conv = |s: &str| fmt(Length::new(parse(s), old).in_unit(unit));
        self.width = conv(&self.width);
        self.height = conv(&self.height);
        self.bleed = std::array::from_fn(|i| conv(&self.bleed[i]));
        self.unit = unit;
    }

    pub fn set_link(&mut self, on: bool) {
        self.bleed_linked = on;
        if on {
            let s = self.bleed[0].clone();
            self.bleed = [s.clone(), s.clone(), s.clone(), s];
        }
    }

    fn portrait(&self) -> bool {
        parse(&self.height) >= parse(&self.width)
    }

    pub fn set_orientation(&mut self, portrait: bool) {
        self.commit_focus();
        if portrait != self.portrait() {
            std::mem::swap(&mut self.width, &mut self.height);
        }
    }

    pub fn width_px(&self) -> f64 {
        Length::new(parse(&self.width), self.unit).px()
    }
    pub fn height_px(&self) -> f64 {
        Length::new(parse(&self.height), self.unit).px()
    }
    /// Bleed in px: top, bottom, left, right.
    pub fn bleed_px(&self) -> [f64; 4] {
        std::array::from_fn(|i| Length::new(parse(&self.bleed[i]), self.unit).px())
    }
}

fn parse(s: &str) -> f64 {
    s.trim().parse::<f64>().unwrap_or(0.0).max(0.0)
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
    match u {
        Unit::Px => "Pixels",
        Unit::Pt => "Points",
        Unit::In => "Inches",
        Unit::Mm => "Millimeters",
        Unit::Cm => "Centimeters",
    }
}
const UNITS: [Unit; 5] = [Unit::Px, Unit::Pt, Unit::In, Unit::Mm, Unit::Cm];
const COLORS: [ColorMode; 2] = [ColorMode::Cmyk, ColorMode::Rgb];
const RASTERS: [RasterEffects; 3] = [
    RasterEffects::Screen72,
    RasterEffects::Medium150,
    RasterEffects::High300,
];
const PREVIEWS: [PreviewMode; 3] = [PreviewMode::Default, PreviewMode::Pixel, PreviewMode::Overprint];

fn color_label(c: ColorMode) -> &'static str {
    match c {
        ColorMode::Cmyk => "CMYK Color",
        ColorMode::Rgb => "RGB Color",
    }
}
fn raster_label(r: RasterEffects) -> &'static str {
    match r {
        RasterEffects::Screen72 => "Screen (72 ppi)",
        RasterEffects::Medium150 => "Medium (150 ppi)",
        RasterEffects::High300 => "High (300 ppi)",
    }
}
fn preview_label(p: PreviewMode) -> &'static str {
    match p {
        PreviewMode::Default => "Default",
        PreviewMode::Pixel => "Pixel",
        PreviewMode::Overprint => "Overprint",
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
pub fn menu_preview(i: usize) -> PreviewMode {
    PREVIEWS[i.min(PREVIEWS.len() - 1)]
}

fn menu_len(m: Menu) -> usize {
    match m {
        Menu::Unit => UNITS.len(),
        Menu::Color => COLORS.len(),
        Menu::Raster => RASTERS.len(),
        Menu::Preview => PREVIEWS.len(),
    }
}

/// Every interactive rect in the modal.
pub struct L {
    pub panel: Rect,
    pub name: Rect,
    pub width: Rect,
    pub unit: Rect,
    pub height: Rect,
    pub orient_p: Rect,
    pub orient_l: Rect,
    pub ab_minus: Rect,
    pub ab_field: Rect,
    pub ab_plus: Rect,
    pub bleed: [Rect; 4],
    pub link: Rect,
    pub color: Rect,
    pub raster: Rect,
    pub preview: Rect,
    pub more: Rect,
    pub close: Rect,
    pub create: Rect,
}

impl L {
    /// The box for `menu`'s trigger.
    fn trigger(&self, m: Menu) -> Rect {
        match m {
            Menu::Unit => self.unit,
            Menu::Color => self.color,
            Menu::Raster => self.raster,
            Menu::Preview => self.preview,
        }
    }
    /// Item rects for an open menu. Opens downward, or upward when the
    /// trigger sits in the lower half of the panel (so a bottom dropdown
    /// never covers the Close / Create buttons).
    fn items(&self, m: Menu) -> Vec<Rect> {
        let t = self.trigger(m);
        let up = t.y0 > self.panel.center().y;
        (0..menu_len(m))
            .map(|i| {
                let y = if up {
                    t.y0 - (i as f64 + 1.0) * FH
                } else {
                    t.y1 + i as f64 * FH
                };
                Rect::new(t.x0, y, t.x1, y + FH)
            })
            .collect()
    }
}

/// Centered modal panel rect for a `w × h` window.
pub fn panel_rect(w: f64, h: f64) -> Rect {
    let pw = 580.0_f64.min(w - 64.0).max(340.0);
    let ph = 792.0_f64.min(h - 48.0).max(420.0);
    Rect::from_center_size(Point::new(w * 0.5, h * 0.5), (pw, ph))
}

pub fn build(panel: Rect) -> L {
    let x = panel.x0 + 30.0;
    let right = panel.x1 - 30.0;
    let fw = right - x;
    let field = |y: f64, x0: f64, x1: f64| Rect::new(x0, y, x1, y + FH);

    let mut y = panel.y0 + 52.0;
    let name = field(y, x, right);
    y += FH + 32.0; // + hairline

    // Width row.
    y += 22.0;
    let col_w = fw * 0.42;
    let width = field(y, x, x + col_w);
    let unit = field(y, x + col_w + 18.0, right);
    y += FH + 28.0;

    // Height / Orientation / Artboards row.
    y += 22.0;
    let height = field(y, x, x + col_w);
    let ox = x + col_w + 18.0;
    let orient_p = Rect::new(ox, y, ox + 30.0, y + 30.0);
    let orient_l = Rect::new(ox + 40.0, y, ox + 70.0, y + 30.0);
    let ab_plus = Rect::new(right - 26.0, y, right, y + 30.0);
    let ab_field = Rect::new(ab_plus.x0 - 10.0 - 56.0, y, ab_plus.x0 - 10.0, y + 30.0);
    let ab_minus = Rect::new(ab_field.x0 - 10.0 - 26.0, y, ab_field.x0 - 10.0, y + 30.0);
    y += 30.0 + 34.0;

    // Bleed: a section header, then two rows of two fields.
    y += 24.0; // "Bleed" header sits at bt.y0 - ~24
    y += 22.0; // "Top" / "Bottom" labels
    let half = (fw - 18.0) * 0.5;
    let bt = field(y, x, x + half);
    let bb = field(y, x + half + 18.0, right);
    y += FH + 28.0;
    y += 22.0; // "Left" / "Right" labels
    let bl = field(y, x, x + half);
    let br = field(y, x + half + 18.0, right);
    y += FH + 20.0;

    // Link bleed.
    let link = Rect::new(x, y, x + 220.0, y + 20.0);
    y += 20.0 + 26.0;

    // Color / Raster / Preview.
    y += 22.0;
    let color = field(y, x, right);
    y += FH + 28.0;
    y += 22.0;
    let raster = field(y, x, right);
    y += FH + 28.0;
    y += 22.0;
    let preview = field(y, x, right);
    y += FH + 26.0;

    let more = Rect::new(x, y, x + 136.0, y + 30.0);

    let create = Rect::new(right - 110.0, panel.y1 - 24.0 - 36.0, right, panel.y1 - 24.0);
    let close = Rect::new(create.x0 - 14.0 - 96.0, create.y0, create.x0 - 14.0, create.y1);

    L {
        panel,
        name,
        width,
        unit,
        height,
        orient_p,
        orient_l,
        ab_minus,
        ab_field,
        ab_plus,
        bleed: [bt, bb, bl, br],
        link,
        color,
        raster,
        preview,
        more,
        close,
        create,
    }
}

pub fn hit(form: &NewDocForm, l: &L, p: Point) -> Hit {
    // An open menu's items take priority; every other click falls through
    // to the normal handling (which also closes the menu).
    if let Some(m) = form.open_menu {
        for (i, r) in l.items(m).iter().enumerate() {
            if r.contains(p) {
                return Hit::MenuItem(m, i);
            }
        }
    }
    if !l.panel.contains(p) {
        return Hit::Backdrop;
    }
    let fields = [
        (l.name, Field::Name),
        (l.width, Field::Width),
        (l.height, Field::Height),
        (l.bleed[0], Field::BleedTop),
        (l.bleed[1], Field::BleedBottom),
        (l.bleed[2], Field::BleedLeft),
        (l.bleed[3], Field::BleedRight),
    ];
    for (r, f) in fields {
        if r.contains(p) {
            return Hit::Field(f);
        }
    }
    if l.unit.contains(p) {
        return Hit::ToggleMenu(Menu::Unit);
    }
    if l.color.contains(p) {
        return Hit::ToggleMenu(Menu::Color);
    }
    if l.raster.contains(p) {
        return Hit::ToggleMenu(Menu::Raster);
    }
    if l.preview.contains(p) {
        return Hit::ToggleMenu(Menu::Preview);
    }
    if l.orient_p.contains(p) {
        return Hit::Orientation(true);
    }
    if l.orient_l.contains(p) {
        return Hit::Orientation(false);
    }
    if l.ab_minus.contains(p) {
        return Hit::ArtboardMinus;
    }
    if l.ab_plus.contains(p) {
        return Hit::ArtboardPlus;
    }
    if l.link.contains(p) {
        return Hit::ToggleLink;
    }
    if l.create.contains(p) {
        return Hit::Create;
    }
    if l.close.contains(p) {
        return Hit::Close;
    }
    Hit::None
}

// ---- painting --------------------------------------------------------

pub fn paint(scene: &mut Scene, text: &mut TextContext, theme: &Theme, win: Rect, form: &NewDocForm) {
    let l = build(panel_rect(win.width(), win.height()));

    // Dim backdrop + panel.
    scene.fill(
        Fill::NonZero,
        ID,
        Color::from_rgb8(0, 0, 0).with_alpha(0.55),
        None,
        &win,
    );
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &l.panel);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &l.panel);

    let dim = theme.text_dim;
    let x = l.panel.x0 + 30.0;

    let caption = |scene: &mut Scene, text: &mut TextContext, s: &str, r: Rect| {
        text.draw(scene, s, 11.5, dim, r.x0, r.y0 - 8.0);
    };

    text.draw(scene, "PRESET DETAILS", 10.5, dim, x, l.panel.y0 + 34.0);
    draw_field(scene, text, theme, l.name, &form.name, form.focus == Some(Field::Name));

    // hairline under name
    let hy = l.name.y1 + 16.0;
    scene.fill(
        Fill::NonZero,
        ID,
        theme.border,
        None,
        &Rect::new(x, hy, l.panel.x1 - 30.0, hy + 1.0),
    );

    caption(scene, text, "Width", l.width);
    draw_field(scene, text, theme, l.width, &form.width, form.focus == Some(Field::Width));
    draw_dropdown(scene, text, theme, l.unit, unit_label(form.unit));

    caption(scene, text, "Height", l.height);
    caption(scene, text, "Orientation", l.orient_p);
    caption(scene, text, "Artboards", l.ab_minus);
    draw_field(scene, text, theme, l.height, &form.height, form.focus == Some(Field::Height));
    draw_orient(scene, theme, l.orient_p, l.orient_l, form.portrait());
    draw_stepper(scene, text, theme, l.ab_minus, l.ab_field, l.ab_plus, form.artboards);

    // "Bleed" section header, well above the Top/Bottom labels.
    text.draw(scene, "Bleed", 12.0, theme.text, x, l.bleed[0].y0 - 28.0);
    let bl = ["Top", "Bottom", "Left", "Right"];
    for i in 0..4 {
        caption(scene, text, bl[i], l.bleed[i]);
        let f = [Field::BleedTop, Field::BleedBottom, Field::BleedLeft, Field::BleedRight][i];
        draw_field(
            scene,
            text,
            theme,
            l.bleed[i],
            &form.bleed[i],
            form.focus == Some(f),
        );
    }

    draw_check(scene, text, theme, l.link, "Link bleed values", form.bleed_linked);

    caption(scene, text, "Color Mode", l.color);
    draw_dropdown(scene, text, theme, l.color, color_label(form.color_mode));
    caption(scene, text, "Raster Effects", l.raster);
    draw_dropdown(scene, text, theme, l.raster, raster_label(form.raster));
    caption(scene, text, "Preview Mode", l.preview);
    draw_dropdown(scene, text, theme, l.preview, preview_label(form.preview));

    draw_button(scene, text, theme, l.more, "More Settings", false);
    draw_button(scene, text, theme, l.close, "Close", false);
    draw_button(scene, text, theme, l.create, "Create", true);

    // Open menu on top.
    if let Some(m) = form.open_menu {
        let items = l.items(m);
        let y0 = items.iter().map(|r| r.y0).fold(f64::MAX, f64::min);
        let y1 = items.iter().map(|r| r.y1).fold(f64::MIN, f64::max);
        let listbox = Rect::new(items[0].x0, y0, items[0].x1, y1);
        scene.fill(Fill::NonZero, ID, theme.strip_bg, None, &listbox);
        scene.stroke(&Stroke::new(1.0), ID, theme.select_blue, None, &listbox);
        let labels: Vec<&str> = match m {
            Menu::Unit => UNITS.iter().map(|u| unit_label(*u)).collect(),
            Menu::Color => COLORS.iter().map(|c| color_label(*c)).collect(),
            Menu::Raster => RASTERS.iter().map(|r| raster_label(*r)).collect(),
            Menu::Preview => PREVIEWS.iter().map(|p| preview_label(*p)).collect(),
        };
        for (r, s) in items.iter().zip(labels) {
            text.draw(scene, s, 12.0, theme.text, r.x0 + 10.0, r.y0 + FH * 0.5 + 4.0);
        }
    }
}

fn draw_field(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    value: &str,
    focused: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.bg, None, &r);
    let border = if focused {
        theme.select_blue
    } else {
        theme.text_dim.with_alpha(0.5)
    };
    scene.stroke(&Stroke::new(1.0), ID, border, None, &r);
    let baseline = r.y0 + r.height() * 0.5 + 4.5;
    text.draw(scene, value, 12.5, theme.text, r.x0 + 8.0, baseline);
    if focused {
        let cx = r.x0 + 8.0 + text.measure(value, 12.5) + 1.5;
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            theme.text,
            None,
            &vello::kurbo::Line::new((cx, r.y0 + 6.0), (cx, r.y1 - 6.0)),
        );
    }
}

fn draw_dropdown(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, value: &str) {
    scene.fill(Fill::NonZero, ID, theme.strip_active, None, &r);
    scene.stroke(&Stroke::new(1.0), ID, theme.text_dim.with_alpha(0.5), None, &r);
    text.draw(
        scene,
        value,
        12.0,
        theme.text,
        r.x0 + 10.0,
        r.y0 + r.height() * 0.5 + 4.0,
    );
    let cx = r.x1 - 16.0;
    let cy = r.y0 + r.height() * 0.5;
    let mut tri = BezPath::new();
    tri.move_to((cx - 4.0, cy - 2.5));
    tri.line_to((cx + 4.0, cy - 2.5));
    tri.line_to((cx, cy + 3.0));
    tri.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &tri);
}

fn draw_orient(scene: &mut Scene, theme: &Theme, p: Rect, land: Rect, portrait: bool) {
    for (r, on) in [(p, portrait), (land, !portrait)] {
        let fill = if on {
            theme.select_blue
        } else {
            theme.strip_active
        };
        scene.fill(Fill::NonZero, ID, fill, None, &r);
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            theme.text_dim.with_alpha(0.6),
            None,
            &r,
        );
        // A little page glyph.
        let g = if r == p {
            Rect::from_center_size(r.center(), (8.0, 11.0))
        } else {
            Rect::from_center_size(r.center(), (12.0, 8.0))
        };
        scene.stroke(
            &Stroke::new(1.0),
            ID,
            if on {
                Color::from_rgb8(0xff, 0xff, 0xff)
            } else {
                theme.text_dim
            },
            None,
            &g,
        );
    }
}

fn draw_stepper(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    minus: Rect,
    field: Rect,
    plus: Rect,
    n: usize,
) {
    for (r, s) in [(minus, "-"), (plus, "+")] {
        scene.fill(Fill::NonZero, ID, theme.strip_active, None, &r);
        scene.stroke(&Stroke::new(1.0), ID, theme.text_dim.with_alpha(0.6), None, &r);
        let w = text.measure(s, 14.0);
        text.draw(
            scene,
            s,
            14.0,
            theme.text,
            r.x0 + (r.width() - w) * 0.5,
            r.y0 + r.height() * 0.5 + 5.0,
        );
    }
    scene.fill(Fill::NonZero, ID, theme.bg, None, &field);
    scene.stroke(&Stroke::new(1.0), ID, theme.text_dim.with_alpha(0.5), None, &field);
    let s = n.to_string();
    let w = text.measure(&s, 12.5);
    text.draw(
        scene,
        &s,
        12.5,
        theme.text,
        field.x0 + (field.width() - w) * 0.5,
        field.y0 + field.height() * 0.5 + 4.5,
    );
}

fn draw_check(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    label: &str,
    on: bool,
) {
    let box_ = Rect::new(r.x0, r.y0 + 2.0, r.x0 + 16.0, r.y0 + 18.0);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &box_);
    scene.stroke(&Stroke::new(1.0), ID, theme.text_dim, None, &box_);
    if on {
        let mut tick = BezPath::new();
        tick.move_to((box_.x0 + 3.0, box_.y0 + 8.0));
        tick.line_to((box_.x0 + 7.0, box_.y0 + 12.0));
        tick.line_to((box_.x0 + 13.0, box_.y0 + 4.0));
        scene.stroke(&Stroke::new(2.0), ID, theme.select_blue, None, &tick);
    }
    text.draw(scene, label, 12.0, theme.text_dim, box_.x1 + 8.0, r.y0 + 14.0);
}

fn draw_button(
    scene: &mut Scene,
    text: &mut TextContext,
    theme: &Theme,
    r: Rect,
    label: &str,
    primary: bool,
) {
    let fill = if primary {
        theme.select_blue
    } else {
        theme.strip_active
    };
    scene.fill(Fill::NonZero, ID, fill, None, &r);
    if !primary {
        scene.stroke(&Stroke::new(1.0), ID, theme.text_dim.with_alpha(0.6), None, &r);
    }
    let col = if primary {
        Color::from_rgb8(0xff, 0xff, 0xff)
    } else {
        theme.text
    };
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
