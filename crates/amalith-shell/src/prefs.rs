//! Application preferences — the modal opened from Amalith ▸ Preferences…
//! (⌘,). A centred card with a category list on the left and the settings
//! for the selected category on the right, plus Cancel / OK.
//!
//! v1 has one category (General) with a few genuinely-wired settings; more
//! categories slot into [`CATEGORIES`] as they gain real controls.

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

/// The settings the app actually reads. Cheap to copy; the modal edits a
/// working copy and only writes back on OK.
#[derive(Clone, Copy, PartialEq)]
pub struct Settings {
    /// Arrow-key nudge distance in px (Shift ×10).
    pub nudge_step: f64,
    /// Whether hover tooltips are shown.
    pub show_tooltips: bool,
    /// Show the Home screen when the last document tab closes.
    pub home_on_last_close: bool,
    /// App accent colour, sRGB. Feeds [`crate::theme::Theme::set_accent`].
    pub accent: [u8; 3],
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            nudge_step: 1.0,
            show_tooltips: true,
            home_on_last_close: true,
            accent: ACCENTS[0].1,
        }
    }
}

/// Selectable accent presets (label, sRGB). The first is the default.
pub const ACCENTS: [(&str, [u8; 3]); 6] = [
    ("Blue", [0x3b, 0x9b, 0xff]),
    ("Gold", [0xf4, 0xbe, 0x18]),
    ("Green", [0x4c, 0xb7, 0x6b]),
    ("Red", [0xe0, 0x50, 0x50]),
    ("Violet", [0x9b, 0x6c, 0xf0]),
    ("Graphite", [0x9a, 0x9a, 0x9a]),
];

pub const CATEGORIES: [&str; 1] = ["General"];

const W: f64 = 660.0;
const H: f64 = 440.0;
const SIDEBAR_W: f64 = 168.0;
const PAD: f64 = 22.0;

pub struct Prefs {
    pub working: Settings,
    pub category: usize,
    // Hit rects, window coords, refreshed each paint.
    origin: Point,
    cat_rows: Vec<Rect>,
    inc_up: Rect,
    inc_down: Rect,
    check_tips: Rect,
    check_home: Rect,
    accent_swatches: Vec<(Rect, [u8; 3])>,
    cancel: Rect,
    ok: Rect,
}

pub enum Hit {
    None,
    Backdrop,
    Category(usize),
    IncStep(f64),
    ToggleTips,
    ToggleHome,
    SetAccent([u8; 3]),
    Cancel,
    Ok,
}

impl Prefs {
    pub fn new(current: Settings) -> Self {
        Self {
            working: current,
            category: 0,
            origin: Point::ZERO,
            cat_rows: Vec::new(),
            inc_up: Rect::ZERO,
            inc_down: Rect::ZERO,
            check_tips: Rect::ZERO,
            check_home: Rect::ZERO,
            accent_swatches: Vec::new(),
            cancel: Rect::ZERO,
            ok: Rect::ZERO,
        }
    }

    fn card(&self) -> Rect {
        Rect::from_origin_size(self.origin, (W, H))
    }

    pub fn on_press(&mut self, p: Point) -> Hit {
        if !self.card().contains(p) {
            return Hit::Backdrop;
        }
        for (i, r) in self.cat_rows.iter().enumerate() {
            if r.contains(p) {
                return Hit::Category(i);
            }
        }
        if self.inc_up.contains(p) {
            return Hit::IncStep((self.working.nudge_step + 0.5).min(100.0));
        }
        if self.inc_down.contains(p) {
            return Hit::IncStep((self.working.nudge_step - 0.5).max(0.5));
        }
        if self.check_tips.contains(p) {
            return Hit::ToggleTips;
        }
        if self.check_home.contains(p) {
            return Hit::ToggleHome;
        }
        for (r, rgb) in &self.accent_swatches {
            if r.contains(p) {
                return Hit::SetAccent(*rgb);
            }
        }
        if self.cancel.contains(p) {
            return Hit::Cancel;
        }
        if self.ok.contains(p) {
            return Hit::Ok;
        }
        Hit::None
    }

    pub fn paint(&mut self, scene: &mut Scene, tcx: &mut TextContext, theme: &Theme, wl: f64, hl: f64) {
        // Scrim.
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            Color::from_rgba8(0, 0, 0, 150),
            None,
            &Rect::new(0.0, 0.0, wl, hl),
        );
        let ox = ((wl - W) / 2.0).round().max(0.0);
        let oy = ((hl - H) / 2.0).round().max(0.0);
        self.origin = Point::new(ox, oy);
        let card = self.card();
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &card.to_rounded_rect(8.0));
        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &card.to_rounded_rect(8.0));

        // Title bar.
        let tw = tcx.measure("Preferences", 13.0);
        tcx.draw(scene, "Preferences", 13.0, theme.text, card.center().x - tw / 2.0, oy + 22.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.border,
            None,
            &Rect::new(ox, oy + 36.0, ox + W, oy + 37.0),
        );

        // Category sidebar.
        self.cat_rows.clear();
        let mut y = oy + 48.0;
        for (i, name) in CATEGORIES.iter().enumerate() {
            let row = Rect::new(ox + 8.0, y, ox + SIDEBAR_W - 8.0, y + 26.0);
            if i == self.category {
                scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &row.to_rounded_rect(4.0));
            } else if row.contains(Point::ZERO) {
                // no-op
            }
            let col = if i == self.category {
                theme.on_accent
            } else {
                theme.text
            };
            tcx.draw(scene, name, 12.5, col, row.x0 + 10.0, row.y0 + 17.0);
            self.cat_rows.push(row);
            y += 28.0;
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.border,
            None,
            &Rect::new(ox + SIDEBAR_W, oy + 37.0, ox + SIDEBAR_W + 1.0, oy + H - 52.0),
        );

        // General page.
        let px = ox + SIDEBAR_W + PAD;
        let mut cy = oy + 60.0;
        tcx.draw(scene, "General", 13.0, theme.text, px, cy);
        cy += 30.0;

        tcx.draw(scene, "Keyboard Increment", 12.0, theme.text_dim, px, cy + 14.0);
        let fx = px + 170.0;
        let field = Rect::new(fx, cy, fx + 90.0, cy + 22.0);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(4.0));
        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(4.0));
        tcx.draw(
            scene,
            &format!("{} pt", trim(self.working.nudge_step)),
            12.0,
            theme.text,
            fx + 8.0,
            cy + 15.0,
        );
        self.inc_up = Rect::new(field.x1 - 16.0, cy + 1.0, field.x1, cy + 11.0);
        self.inc_down = Rect::new(field.x1 - 16.0, cy + 11.0, field.x1, cy + 21.0);
        tri(scene, self.inc_up.center(), true, theme.text_dim);
        tri(scene, self.inc_down.center(), false, theme.text_dim);
        cy += 44.0;

        self.check_tips = checkbox(scene, tcx, theme, px, cy, "Show Tool Tips", self.working.show_tooltips);
        cy += 30.0;
        self.check_home = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show the Home Screen when the last document closes",
            self.working.home_on_last_close,
        );
        cy += 40.0;

        // Accent colour swatches.
        tcx.draw(scene, "Accent Color", 12.0, theme.text_dim, px, cy + 12.0);
        self.accent_swatches.clear();
        let mut sx = px + 170.0;
        for (_, rgb) in ACCENTS {
            let sw = Rect::new(sx, cy, sx + 20.0, cy + 20.0);
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgb8(rgb[0], rgb[1], rgb[2]),
                None,
                &sw.to_rounded_rect(4.0),
            );
            if rgb == self.working.accent {
                scene.stroke(
                    &Stroke::new(2.0),
                    Affine::IDENTITY,
                    theme.text,
                    None,
                    &sw.inflate(2.5, 2.5).to_rounded_rect(6.0),
                );
            }
            self.accent_swatches.push((sw, rgb));
            sx += 30.0;
        }

        // Footer buttons.
        let by = oy + H - 40.0;
        self.ok = button(scene, tcx, theme, ox + W - PAD - 76.0, by, "OK", true);
        self.cancel = button(scene, tcx, theme, ox + W - PAD - 76.0 - 12.0 - 86.0, by, "Cancel", false);
    }
}

fn trim(v: f64) -> String {
    if v.fract().abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

fn tri(scene: &mut Scene, c: Point, up: bool, color: Color) {
    let d = 3.0;
    let mut p = vello::kurbo::BezPath::new();
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
    scene.fill(Fill::NonZero, Affine::IDENTITY, color, None, &p);
}

fn checkbox(
    scene: &mut Scene,
    tcx: &mut TextContext,
    theme: &Theme,
    x: f64,
    y: f64,
    label: &str,
    on: bool,
) -> Rect {
    let box_ = Rect::new(x, y, x + 16.0, y + 16.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        if on { theme.accent } else { theme.bg },
        None,
        &box_.to_rounded_rect(3.0),
    );
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &box_.to_rounded_rect(3.0));
    if on {
        let mut tick = vello::kurbo::BezPath::new();
        tick.move_to((x + 3.5, y + 8.5));
        tick.line_to((x + 6.5, y + 11.5));
        tick.line_to((x + 12.5, y + 4.5));
        scene.stroke(
            &Stroke::new(1.8),
            Affine::IDENTITY,
            theme.on_accent,
            None,
            &tick,
        );
    }
    tcx.draw(scene, label, 12.0, theme.text, x + 24.0, y + 13.0);
    // Whole-row hit rect.
    Rect::new(x, y - 2.0, x + 24.0 + tcx.measure(label, 12.0), y + 18.0)
}

fn button(
    scene: &mut Scene,
    tcx: &mut TextContext,
    theme: &Theme,
    x: f64,
    y: f64,
    label: &str,
    primary: bool,
) -> Rect {
    let r = Rect::new(x, y, x + 86.0, y + 26.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        if primary { theme.accent } else { theme.bg },
        None,
        &r.to_rounded_rect(5.0),
    );
    scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &r.to_rounded_rect(5.0));
    let col = if primary {
        theme.on_accent
    } else {
        theme.text
    };
    let w = tcx.measure(label, 12.5);
    tcx.draw(scene, label, 12.5, col, r.center().x - w / 2.0, r.center().y + 4.0);
    r
}
