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
        Tool::Hand => KeyChord::plain(KeyH),
        Tool::Zoom => KeyChord::plain(KeyZ),
        Tool::Eyedropper => KeyChord::plain(KeyI),
        Tool::Rotate => KeyChord::plain(KeyR),
        Tool::RoundedRect | Tool::Polygon | Tool::Star => return None,
    })
}

/// A non-tool command that carries a user-remappable shortcut.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PrefAction {
    SwapPaints,
    DefaultPaints,
    Place,
    CommandPalette,
}

impl PrefAction {
    pub const ALL: [PrefAction; 4] = [
        PrefAction::SwapPaints,
        PrefAction::DefaultPaints,
        PrefAction::Place,
        PrefAction::CommandPalette,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrefAction::SwapPaints => "Swap Fill / Stroke",
            PrefAction::DefaultPaints => "Default Fill / Stroke",
            PrefAction::Place => "Place…",
            PrefAction::CommandPalette => "Command Palette",
        }
    }

    pub fn default_key(self) -> Option<KeyChord> {
        Some(match self {
            PrefAction::SwapPaints => KeyChord::plain(KeyCode::KeyX),
            PrefAction::DefaultPaints => KeyChord::plain(KeyCode::KeyD),
            PrefAction::Place => KeyChord::with_cmd_shift(KeyCode::KeyP),
            PrefAction::CommandPalette => KeyChord {
                code: KeyCode::KeyK,
                shift: false,
                cmd: true,
            },
        })
    }
}

/// Which binding table a Keyboard-page row edits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BindTarget {
    Tool(usize),
    Action(usize),
    /// A user script, by index into `Prefs::script_paths`.
    Script(usize),
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
    /// Debug: show the bottom-centre FPS counter.
    pub show_fps: bool,
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
            show_fps: true,
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

pub const CATEGORIES: [&str; 4] = ["General", "Keyboard", "Scripts", "Debug"];

const W: f64 = 660.0;
const H: f64 = 440.0;
const SIDEBAR_W: f64 = 168.0;
const PAD: f64 = 22.0;

pub struct Prefs {
    pub working: Settings,
    /// Working copy of the scripts config (folder + key bindings).
    pub working_scripts: crate::scripts::ScriptsConfig,
    /// Scripts discovered in the working folder — index space for
    /// [`BindTarget::Script`].
    pub script_paths: Vec<std::path::PathBuf>,
    pub category: usize,
    // Hit rects, window coords, refreshed each paint.
    origin: Point,
    cat_rows: Vec<Rect>,
    inc_up: Rect,
    inc_down: Rect,
    check_tips: Rect,
    check_home: Rect,
    check_fps: Rect,
    check_cull: Rect,
    cull_up: Rect,
    cull_down: Rect,
    accent_swatches: Vec<(Rect, [u8; 3])>,
    /// Keyboard page: (row rect, which binding it edits).
    bind_rows: Vec<(Rect, BindTarget)>,
    /// The binding currently capturing a keypress, if any.
    pub recording: Option<BindTarget>,
    /// Scroll offset of the Keyboard / Scripts binding list, in px.
    pub page_scroll: crate::scroll_view::ScrollView,
    /// Working copy of the shortcut presets.
    pub working_keymaps: crate::keymap::Keymaps,
    /// Keyboard page: the preset dropdown is expanded.
    pub preset_menu_open: bool,
    /// Keyboard page: typing a name for a preset about to be saved.
    pub naming: Option<crate::text_field::TextField>,
    preset_trigger: Rect,
    preset_add: Rect,
    preset_items: Vec<Rect>,
    reset_keys: Rect,
    scripts_choose: Rect,
    scripts_clear: Rect,
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
    ToggleFps,
    ToggleCullOutline,
    SetCullInset(f64),
    SetAccent([u8; 3]),
    /// Keyboard page: start capturing a key for this binding.
    StartRecording(BindTarget),
    /// Keyboard page: restore the factory shortcuts.
    ResetKeys,
    /// Keyboard page: preset dropdown — open/close, pick one, or start a
    /// new one from the current edits.
    TogglePresetMenu,
    PickPreset(usize),
    AddPreset,
    /// Scripts page: open a folder picker / clear the chosen folder.
    ChooseScriptsFolder,
    ClearScriptsFolder,
    Cancel,
    Ok,
}

impl Prefs {
    pub fn new(
        current: Settings,
        scripts: crate::scripts::ScriptsConfig,
        keymaps: crate::keymap::Keymaps,
    ) -> Self {
        let script_paths = scripts
            .dir
            .as_deref()
            .map(crate::scripts::list)
            .unwrap_or_default();
        Self {
            working: current,
            working_scripts: scripts,
            script_paths,
            working_keymaps: keymaps,
            preset_menu_open: false,
            naming: None,
            preset_trigger: Rect::ZERO,
            preset_add: Rect::ZERO,
            preset_items: Vec::new(),
            category: 0,
            origin: Point::ZERO,
            cat_rows: Vec::new(),
            inc_up: Rect::ZERO,
            inc_down: Rect::ZERO,
            check_tips: Rect::ZERO,
            check_home: Rect::ZERO,
            check_fps: Rect::ZERO,
            check_cull: Rect::ZERO,
            cull_up: Rect::ZERO,
            cull_down: Rect::ZERO,
            accent_swatches: Vec::new(),
            bind_rows: Vec::new(),
            recording: None,
            page_scroll: crate::scroll_view::ScrollView::new(),
            reset_keys: Rect::ZERO,
            scripts_choose: Rect::ZERO,
            scripts_clear: Rect::ZERO,
            cancel: Rect::ZERO,
            ok: Rect::ZERO,
        }
    }

    /// Save the current shortcut edits as a preset named by `self.naming`
    /// and make it active. A blank / built-in name just cancels.
    pub fn commit_naming(&mut self) {
        if let Some(field) = self.naming.take() {
            let name = field.text().trim().to_string();
            if !name.is_empty() && name != crate::keymap::BUILTIN {
                self.working_keymaps.upsert(
                    name,
                    self.working.tool_keys,
                    self.working.action_keys,
                );
            }
        }
    }

    /// Re-scan the working folder after it changes.
    pub fn refresh_scripts(&mut self) {
        self.script_paths = self
            .working_scripts
            .dir
            .as_deref()
            .map(crate::scripts::list)
            .unwrap_or_default();
        self.recording = None;
    }

    fn card(&self) -> Rect {
        Rect::from_origin_size(self.origin, (W, H))
    }

    pub fn on_press(&mut self, p: Point) -> Hit {
        if !self.card().contains(p) {
            return Hit::Backdrop;
        }
        // Open preset dropdown: its items sit over everything else.
        for (i, r) in self.preset_items.iter().enumerate() {
            if r.contains(p) {
                return Hit::PickPreset(i);
            }
        }
        for (i, r) in self.cat_rows.iter().enumerate() {
            if r.contains(p) {
                return Hit::Category(i);
            }
        }
        if self.preset_trigger.contains(p) {
            return Hit::TogglePresetMenu;
        }
        if self.preset_add.contains(p) {
            return Hit::AddPreset;
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
        if self.check_fps.contains(p) {
            return Hit::ToggleFps;
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
        if self.scripts_choose.contains(p) {
            return Hit::ChooseScriptsFolder;
        }
        if self.scripts_clear.contains(p) {
            return Hit::ClearScriptsFolder;
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
        self.check_fps = Rect::ZERO;
        self.check_cull = Rect::ZERO;
        self.cull_up = Rect::ZERO;
        self.cull_down = Rect::ZERO;
        self.reset_keys = Rect::ZERO;
        self.scripts_choose = Rect::ZERO;
        self.scripts_clear = Rect::ZERO;
        self.preset_trigger = Rect::ZERO;
        self.preset_add = Rect::ZERO;
        self.preset_items.clear();

        let footer = |s: &mut Self, scene: &mut Scene, tcx: &mut TextContext| {
            let by = oy + H - 40.0;
            s.ok = button(scene, tcx, theme, ox + W - PAD - 76.0, by, "OK", true);
            s.cancel = button(scene, tcx, theme, ox + W - PAD - 174.0, by, "Cancel", false);
        };

        if self.category == 1 {
            self.paint_keyboard(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        if self.category == 2 {
            self.paint_scripts(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        if self.category == 3 {
            self.paint_debug(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
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
        let row_w = W - SIDEBAR_W - PAD * 2.0;

        // Preset row.
        tcx.draw(scene, "Preset", 12.0, theme.text_dim, px, oy + 62.0);
        let chip = Rect::new(px + 52.0, oy + 46.0, px + 52.0 + 210.0, oy + 70.0);
        let naming = self.naming.is_some();
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &chip.to_rounded_rect(4.0));
        scene.stroke(
            &Stroke::new(1.0),
            Affine::IDENTITY,
            if naming { theme.accent } else { theme.border },
            None,
            &chip.to_rounded_rect(4.0),
        );
        if let Some(field) = &mut self.naming {
            field.paint(scene, tcx, theme, chip, "name preset", true);
        } else {
            tcx.draw(
                scene,
                &self.working_keymaps.active,
                12.0,
                theme.text,
                chip.x0 + 10.0,
                chip.y0 + 16.0,
            );
            tri(scene, Point::new(chip.x1 - 14.0, chip.center().y), false, theme.text_dim);
            self.preset_trigger = chip;
        }
        // "+" — save the current edits as a new preset.
        let add = Rect::new(chip.x1 + 8.0, chip.y0, chip.x1 + 8.0 + 26.0, chip.y1);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_active, None, &add.to_rounded_rect(4.0));
        scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.border, None, &add.to_rounded_rect(4.0));
        let plus = if naming { "OK" } else { "+" };
        let pw = tcx.measure(plus, 13.0);
        tcx.draw(scene, plus, 13.0, theme.text, add.center().x - pw / 2.0, add.center().y + 4.5);
        self.preset_add = add;

        tcx.draw(scene, "Tool Shortcuts", 13.0, theme.text, px, oy + 96.0);
        tcx.draw(
            scene,
            "Click a shortcut, then press a key. Shift is allowed.",
            11.0,
            theme.text_dim,
            px,
            oy + 116.0,
        );

        let recording = self.recording;
        // Reset sits in the fixed footer row so the list never hides it.
        self.reset_keys = button(scene, tcx, theme, px, oy + H - 40.0, "Reset", false);

        let n_tools = Tool::ALL.len();
        let n_acts = PrefAction::ALL.len();
        let content_h = n_tools as f64 * 27.0 + 26.0 + n_acts as f64 * 27.0 + 6.0;
        let view = Rect::new(px - 6.0, oy + 128.0, px + row_w + 14.0, oy + H - 52.0);
        let sc = self.begin_scroll_list(scene, theme, view, content_h);

        let mut y = view.y0 + 2.0 - sc;
        for i in 0..n_tools {
            kb_row(
                scene, tcx, theme, px, row_w, y,
                Tool::ALL[i].label(),
                self.working.tool_keys[i],
                recording,
                BindTarget::Tool(i),
                view,
                &mut self.bind_rows,
            );
            y += 27.0;
        }
        y += 6.0;
        tcx.draw(scene, "Colours", 13.0, theme.text, px, y + 4.0);
        y += 20.0;
        for i in 0..n_acts {
            kb_row(
                scene, tcx, theme, px, row_w, y,
                PrefAction::ALL[i].label(),
                self.working.action_keys[i],
                recording,
                BindTarget::Action(i),
                view,
                &mut self.bind_rows,
            );
            y += 27.0;
        }
        scene.pop_layer();

        // Preset dropdown, painted last so it sits over the list.
        if self.preset_menu_open && self.naming.is_none() {
            let names = self.working_keymaps.names();
            let t = self.preset_trigger;
            let box_ = Rect::new(t.x0, t.y1 + 2.0, t.x1, t.y1 + 2.0 + names.len() as f64 * 24.0);
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_bg, None, &box_.to_rounded_rect(4.0));
            scene.stroke(&Stroke::new(1.0), Affine::IDENTITY, theme.accent, None, &box_.to_rounded_rect(4.0));
            for (i, name) in names.iter().enumerate() {
                let r = Rect::new(box_.x0, box_.y0 + i as f64 * 24.0, box_.x1, box_.y0 + (i as f64 + 1.0) * 24.0);
                if name == &self.working_keymaps.active {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent.with_alpha(0.18), None, &r);
                }
                tcx.draw(scene, name, 12.0, theme.text, r.x0 + 10.0, r.y0 + 16.0);
                self.preset_items.push(r);
            }
        }
    }

    /// Clip to `view`, draw a scrollbar for `content_h`, and return the
    /// clamped scroll offset. The caller must `scene.pop_layer()` when done
    /// drawing the list. Shared by the Keyboard and Scripts pages.
    fn begin_scroll_list(
        &mut self,
        scene: &mut Scene,
        theme: &Theme,
        view: Rect,
        content_h: f64,
    ) -> f64 {
        self.page_scroll.begin(scene, theme, view, content_h)
    }

    /// The Scripts page — pick the user's script folder and bind keys to
    /// the scripts in it.
    fn paint_scripts(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        px: f64,
        oy: f64,
    ) {
        let mut cy = oy + 60.0;
        tcx.draw(scene, "Scripts", 13.0, theme.text, px, cy);
        cy += 12.0;
        tcx.draw(
            scene,
            "Point Amalith at a folder of scripts you keep yourself — updates can't touch it.",
            11.0,
            theme.text_dim,
            px,
            cy + 12.0,
        );
        cy += 30.0;

        tcx.draw(scene, "Folder", 12.0, theme.text_dim, px, cy + 14.0);
        let path_str = self
            .working_scripts
            .dir
            .as_ref()
            .map(|d| elide_left(&d.display().to_string(), 52))
            .unwrap_or_else(|| "None chosen".to_string());
        tcx.draw(scene, &path_str, 11.5, theme.text, px + 58.0, cy + 14.0);
        cy += 26.0;
        self.scripts_choose = button(scene, tcx, theme, px, cy, "Choose…", true);
        self.scripts_clear = if self.working_scripts.dir.is_some() {
            button(scene, tcx, theme, px + 100.0, cy, "Clear", false)
        } else {
            Rect::ZERO
        };
        cy += 42.0;

        if self.working_scripts.dir.is_none() {
            return;
        }
        if self.script_paths.is_empty() {
            tcx.draw(
                scene,
                "No scripts (.sh, .py, .js, …) found in that folder.",
                11.5,
                theme.text_dim,
                px,
                cy + 4.0,
            );
            return;
        }

        tcx.draw(scene, "Shortcuts", 13.0, theme.text, px, cy + 4.0);
        tcx.draw(
            scene,
            "Click a shortcut, then press a key (Cmd / Shift allowed).",
            11.0,
            theme.text_dim,
            px,
            cy + 22.0,
        );
        let list_top = cy + 34.0;

        let row_w = W - SIDEBAR_W - PAD * 2.0;
        let recording = self.recording;
        let names: Vec<String> = self
            .script_paths
            .iter()
            .map(|p| crate::scripts::label(p))
            .collect();
        let content_h = names.len() as f64 * 27.0 + 4.0;
        let view = Rect::new(px - 6.0, list_top, px + row_w + 14.0, oy + H - 52.0);
        let sc = self.begin_scroll_list(scene, theme, view, content_h);

        let mut y = view.y0 + 2.0 - sc;
        for (i, name) in names.iter().enumerate() {
            let chord = self.working_scripts.chord_for(name);
            kb_row(
                scene, tcx, theme, px, row_w, y,
                name, chord, recording,
                BindTarget::Script(i),
                view,
                &mut self.bind_rows,
            );
            y += 27.0;
        }
        scene.pop_layer();
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

        self.check_fps = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show FPS Counter",
            self.working.show_fps,
        );
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
    viewport: Rect,
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
    // Only rows fully inside the scroll viewport are clickable, so a row
    // peeking under the header / footer can't be hit.
    if row.y0 >= viewport.y0 - 0.5 && row.y1 <= viewport.y1 + 0.5 {
        bind_rows.push((row, target));
    }
}

fn trim(v: f64) -> String {
    if v.fract().abs() < 0.05 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// Keep the last `max` characters of `s`, prefixing `…` when clipped — so
/// a long path shows its most-specific tail.
fn elide_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - max).collect();
    format!("…{tail}")
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
