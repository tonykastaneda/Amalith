//! Application preferences — the modal opened from Amalith ▸ Preferences…
//! (⌘,). A centred card with a category list on the left and the settings
//! for the selected category on the right, plus Cancel / OK.
//!
//! v1 has one category (General) with a few genuinely-wired settings; more
//! categories slot into [`CATEGORIES`] as they gain real controls.

use std::fmt;

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;
use winit::keyboard::KeyCode;

use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

/// A tool / command shortcut: a letter/digit key, optionally with Shift
/// and/or Cmd (Ctrl on Windows/Linux).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub code: KeyCode,
    pub shift: bool,
    pub cmd: bool,
}

impl KeyChord {
    fn plain(code: KeyCode) -> Self {
        Self {
            code,
            shift: false,
            cmd: false,
        }
    }
    fn with_shift(code: KeyCode) -> Self {
        Self {
            code,
            shift: true,
            cmd: false,
        }
    }
    fn with_cmd_shift(code: KeyCode) -> Self {
        Self {
            code,
            shift: true,
            cmd: true,
        }
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = key_char(self.code).unwrap_or('?');
        match (self.cmd, self.shift) {
            (true, true) => write!(f, "Cmd+Shift+{c}"),
            (true, false) => write!(f, "Cmd+{c}"),
            (false, true) => write!(f, "Shift+{c}"),
            (false, false) => write!(f, "{c}"),
        }
    }
}

impl std::str::FromStr for KeyChord {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let s = s.trim();
        let (cmd, s) = s.strip_prefix("Cmd+").map_or((false, s), |r| (true, r));
        let (shift, rest) = s.strip_prefix("Shift+").map_or((false, s), |r| (true, r));
        let mut ch = rest.chars();
        let (Some(c), None) = (ch.next(), ch.next()) else {
            return Err(());
        };
        key_code(c)
            .map(|code| KeyChord { code, shift, cmd })
            .ok_or(())
    }
}

/// `KeyCode` → its single display character (A–Z, 0–9). `None` for keys we
/// don't allow as tool shortcuts.
pub fn key_char(code: KeyCode) -> Option<char> {
    use KeyCode::*;
    Some(match code {
        KeyA => 'A', KeyB => 'B', KeyC => 'C', KeyD => 'D', KeyE => 'E',
        KeyF => 'F', KeyG => 'G', KeyH => 'H', KeyI => 'I', KeyJ => 'J',
        KeyK => 'K', KeyL => 'L', KeyM => 'M', KeyN => 'N', KeyO => 'O',
        KeyP => 'P', KeyQ => 'Q', KeyR => 'R', KeyS => 'S', KeyT => 'T',
        KeyU => 'U', KeyV => 'V', KeyW => 'W', KeyX => 'X', KeyY => 'Y',
        KeyZ => 'Z',
        Digit0 => '0', Digit1 => '1', Digit2 => '2', Digit3 => '3',
        Digit4 => '4', Digit5 => '5', Digit6 => '6', Digit7 => '7',
        Digit8 => '8', Digit9 => '9',
        Backslash => '\\',
        _ => return None,
    })
}

/// Inverse of [`key_char`], case-insensitive.
pub fn key_code(c: char) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match c.to_ascii_uppercase() {
        'A' => KeyA, 'B' => KeyB, 'C' => KeyC, 'D' => KeyD, 'E' => KeyE,
        'F' => KeyF, 'G' => KeyG, 'H' => KeyH, 'I' => KeyI, 'J' => KeyJ,
        'K' => KeyK, 'L' => KeyL, 'M' => KeyM, 'N' => KeyN, 'O' => KeyO,
        'P' => KeyP, 'Q' => KeyQ, 'R' => KeyR, 'S' => KeyS, 'T' => KeyT,
        'U' => KeyU, 'V' => KeyV, 'W' => KeyW, 'X' => KeyX, 'Y' => KeyY,
        'Z' => KeyZ,
        '0' => Digit0, '1' => Digit1, '2' => Digit2, '3' => Digit3,
        '4' => Digit4, '5' => Digit5, '6' => Digit6, '7' => Digit7,
        '8' => Digit8, '9' => Digit9,
        '\\' => Backslash,
        _ => return None,
    })
}

/// The factory-default shortcut for each tool (`None` = unbound).
pub fn default_tool_key(tool: Tool) -> Option<KeyChord> {
    use KeyCode::*;
    Some(match tool {
        Tool::Select => KeyChord::plain(KeyV),
        Tool::DirectSelect => KeyChord::plain(KeyA),
        Tool::Pen => KeyChord::plain(KeyP),
        Tool::Line => KeyChord::plain(Backslash),
        Tool::Text => KeyChord::plain(KeyT),
        Tool::Rectangle => KeyChord::plain(KeyM),
        Tool::Ellipse => KeyChord::plain(KeyL),
        Tool::Artboard => KeyChord::with_shift(KeyO),
        Tool::RoundedRect | Tool::Polygon | Tool::Star => return None,
    })
}

/// A non-tool command that carries a user-remappable shortcut.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrefAction {
    SwapPaints,
    DefaultPaints,
    Place,
}

impl PrefAction {
    pub const ALL: [PrefAction; 3] = [
        PrefAction::SwapPaints,
        PrefAction::DefaultPaints,
        PrefAction::Place,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrefAction::SwapPaints => "Swap Fill / Stroke",
            PrefAction::DefaultPaints => "Default Fill / Stroke",
            PrefAction::Place => "Place…",
        }
    }

    pub fn default_key(self) -> Option<KeyChord> {
        Some(match self {
            PrefAction::SwapPaints => KeyChord::plain(KeyCode::KeyX),
            PrefAction::DefaultPaints => KeyChord::plain(KeyCode::KeyD),
            PrefAction::Place => KeyChord::with_cmd_shift(KeyCode::KeyP),
        })
    }
}

/// Which binding table a Keyboard-page row edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindTarget {
    Tool(usize),
    Action(usize),
}

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
    /// Tool shortcut per [`Tool::ALL`] position.
    pub tool_keys: [Option<KeyChord>; Tool::ALL.len()],
    /// Command shortcut per [`PrefAction::ALL`] position.
    pub action_keys: [Option<KeyChord>; PrefAction::ALL.len()],
    /// Debug: draw the dashed cull-boundary outline on the canvas.
    pub show_cull_outline: bool,
    /// Debug: inset (logical px) from the viewport where off-screen
    /// objects stop being drawn / decoded. Larger = cull further out.
    pub cull_inset: f64,
}

impl Settings {
    fn default_tool_keys() -> [Option<KeyChord>; Tool::ALL.len()] {
        std::array::from_fn(|i| default_tool_key(Tool::ALL[i]))
    }
    fn default_action_keys() -> [Option<KeyChord>; PrefAction::ALL.len()] {
        std::array::from_fn(|i| PrefAction::ALL[i].default_key())
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            nudge_step: 1.0,
            show_tooltips: true,
            home_on_last_close: true,
            accent: ACCENTS[0].1,
            tool_keys: Settings::default_tool_keys(),
            action_keys: Settings::default_action_keys(),
            show_cull_outline: false,
            cull_inset: crate::canvas::CULL_INSET,
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

pub const CATEGORIES: [&str; 3] = ["General", "Keyboard", "Debug"];

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
    check_cull: Rect,
    cull_up: Rect,
    cull_down: Rect,
    accent_swatches: Vec<(Rect, [u8; 3])>,
    /// Keyboard page: (row rect, which binding it edits).
    bind_rows: Vec<(Rect, BindTarget)>,
    /// The binding currently capturing a keypress, if any.
    pub recording: Option<BindTarget>,
    reset_keys: Rect,
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
    ToggleCullOutline,
    SetCullInset(f64),
    SetAccent([u8; 3]),
    /// Keyboard page: start capturing a key for this binding.
    StartRecording(BindTarget),
    /// Keyboard page: restore the factory shortcuts.
    ResetKeys,
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
            check_cull: Rect::ZERO,
            cull_up: Rect::ZERO,
            cull_down: Rect::ZERO,
            accent_swatches: Vec::new(),
            bind_rows: Vec::new(),
            recording: None,
            reset_keys: Rect::ZERO,
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
        if self.check_cull.contains(p) {
            return Hit::ToggleCullOutline;
        }
        if self.cull_up.contains(p) {
            return Hit::SetCullInset((self.working.cull_inset + 8.0).min(1000.0));
        }
        if self.cull_down.contains(p) {
            return Hit::SetCullInset((self.working.cull_inset - 8.0).max(0.0));
        }
        for (r, rgb) in &self.accent_swatches {
            if r.contains(p) {
                return Hit::SetAccent(*rgb);
            }
        }
        for (r, t) in &self.bind_rows {
            if r.contains(p) {
                return Hit::StartRecording(*t);
            }
        }
        if self.reset_keys.contains(p) {
            return Hit::ResetKeys;
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

        let px = ox + SIDEBAR_W + PAD;

        // Rects from the page that isn't shown must not stay hittable.
        self.accent_swatches.clear();
        self.bind_rows.clear();
        self.inc_up = Rect::ZERO;
        self.inc_down = Rect::ZERO;
        self.check_tips = Rect::ZERO;
        self.check_home = Rect::ZERO;
        self.check_cull = Rect::ZERO;
        self.cull_up = Rect::ZERO;
        self.cull_down = Rect::ZERO;
        self.reset_keys = Rect::ZERO;

        if self.category == 1 {
            self.paint_keyboard(scene, tcx, theme, px, oy);
            let by = oy + H - 40.0;
            self.ok = button(scene, tcx, theme, ox + W - PAD - 76.0, by, "OK", true);
            self.cancel = button(scene, tcx, theme, ox + W - PAD - 174.0, by, "Cancel", false);
            return;
        }

        if self.category == 2 {
            self.paint_debug(scene, tcx, theme, px, oy);
            let by = oy + H - 40.0;
            self.ok = button(scene, tcx, theme, ox + W - PAD - 76.0, by, "OK", true);
            self.cancel = button(scene, tcx, theme, ox + W - PAD - 174.0, by, "Cancel", false);
            return;
        }

        // General page.
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
        self.cancel = button(scene, tcx, theme, ox + W - PAD - 174.0, by, "Cancel", false);
    }

    /// The Keyboard page — one row per tool with its current shortcut.
    fn paint_keyboard(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        px: f64,
        oy: f64,
    ) {
        let mut cy = oy + 60.0;
        tcx.draw(scene, "Tool Shortcuts", 13.0, theme.text, px, cy);
        cy += 12.0;
        tcx.draw(
            scene,
            "Click a shortcut, then press a key. Shift is allowed.",
            11.0,
            theme.text_dim,
            px,
            cy + 12.0,
        );
        cy += 26.0;

        let row_w = W - SIDEBAR_W - PAD * 2.0;
        let recording = self.recording;
        for i in 0..Tool::ALL.len() {
            kb_row(
                scene,
                tcx,
                theme,
                px,
                row_w,
                cy,
                Tool::ALL[i].label(),
                self.working.tool_keys[i],
                recording,
                BindTarget::Tool(i),
                &mut self.bind_rows,
            );
            cy += 27.0;
        }

        cy += 10.0;
        tcx.draw(scene, "Colours", 13.0, theme.text, px, cy + 4.0);
        cy += 16.0;
        for i in 0..PrefAction::ALL.len() {
            kb_row(
                scene,
                tcx,
                theme,
                px,
                row_w,
                cy,
                PrefAction::ALL[i].label(),
                self.working.action_keys[i],
                recording,
                BindTarget::Action(i),
                &mut self.bind_rows,
            );
            cy += 27.0;
        }

        cy += 8.0;
        self.reset_keys = button(scene, tcx, theme, px, cy, "Reset", false);
    }

    /// The Debug page — cull-outline visibility and distance.
    fn paint_debug(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        px: f64,
        oy: f64,
    ) {
        let mut cy = oy + 60.0;
        tcx.draw(scene, "Debug", 13.0, theme.text, px, cy);
        cy += 30.0;

        self.check_cull = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show Cull Outline",
            self.working.show_cull_outline,
        );
        cy += 36.0;

        tcx.draw(scene, "Cull Distance", 12.0, theme.text_dim, px, cy + 14.0);
        let fx = px + 170.0;
        let field = Rect::new(fx, cy, fx + 90.0, cy + 22.0);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(4.0));
        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(4.0));
        tcx.draw(
            scene,
            &format!("{} px", trim(self.working.cull_inset)),
            12.0,
            theme.text,
            fx + 8.0,
            cy + 15.0,
        );
        self.cull_up = Rect::new(field.x1 - 16.0, cy + 1.0, field.x1, cy + 11.0);
        self.cull_down = Rect::new(field.x1 - 16.0, cy + 11.0, field.x1, cy + 21.0);
        tri(scene, self.cull_up.center(), true, theme.text_dim);
        tri(scene, self.cull_down.center(), false, theme.text_dim);
        cy += 42.0;

        tcx.draw(
            scene,
            "How far past the visible canvas an object is kept before it",
            11.0,
            theme.text_dim,
            px,
            cy + 4.0,
        );
        tcx.draw(
            scene,
            "stops drawing. Larger culls further out; the dashed magenta",
            11.0,
            theme.text_dim,
            px,
            cy + 20.0,
        );
        tcx.draw(
            scene,
            "line marks the threshold when Show Cull Outline is on.",
            11.0,
            theme.text_dim,
            px,
            cy + 36.0,
        );
    }
}

/// One Keyboard-page row: `name` on the left, its shortcut chip on the
/// right; the row rect is recorded in `bind_rows` for hit-testing.
#[allow(clippy::too_many_arguments)]
fn kb_row(
    scene: &mut Scene,
    tcx: &mut TextContext,
    theme: &Theme,
    px: f64,
    row_w: f64,
    cy: f64,
    name: &str,
    chord: Option<KeyChord>,
    recording: Option<BindTarget>,
    target: BindTarget,
    bind_rows: &mut Vec<(Rect, BindTarget)>,
) {
    let row = Rect::new(px, cy, px + row_w, cy + 24.0);
    let hot = recording == Some(target);
    if hot {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.accent.with_alpha(0.18),
            None,
            &row.to_rounded_rect(4.0),
        );
    }
    tcx.draw(scene, name, 12.0, theme.text, px + 4.0, cy + 16.0);
    let chip = Rect::new(row.x1 - 120.0, cy + 1.0, row.x1, cy + 23.0);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.bg,
        None,
        &chip.to_rounded_rect(4.0),
    );
    scene.stroke(
        &Stroke::new(1.0),
        Affine::IDENTITY,
        if hot { theme.accent } else { theme.border },
        None,
        &chip.to_rounded_rect(4.0),
    );
    let label = if hot {
        "Press a key…".to_string()
    } else {
        chord.map_or_else(|| "—".to_string(), |c| c.to_string())
    };
    let lw = tcx.measure(&label, 11.5);
    tcx.draw(
        scene,
        &label,
        11.5,
        if hot { theme.accent } else { theme.text },
        chip.center().x - lw / 2.0,
        cy + 16.0,
    );
    bind_rows.push((row, target));
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
