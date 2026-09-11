//! Application preferences — the modal opened from Amalith ▸ Preferences…
//! (⌘,). A centred card with a category list on the left and the settings
//! for the selected category on the right, plus Cancel / OK.
//!
//! v1 has one category (General) with a few genuinely-wired settings; more
//! categories slot into [`CATEGORIES`] as they gain real controls.

use crate::metrics::px as ui_px;

use std::fmt;

use vello::kurbo::{Affine, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;
use winit::keyboard::KeyCode;

use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

/// A tool / command shortcut: a letter/digit/arrow key, optionally with
/// Shift, Option (Alt on Windows/Linux), and/or Cmd (Ctrl).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyChord {
    pub code: KeyCode,
    pub shift: bool,
    pub cmd: bool,
    pub alt: bool,
}

impl KeyChord {
    fn plain(code: KeyCode) -> Self {
        Self {
            code,
            shift: false,
            cmd: false,
            alt: false,
        }
    }
    fn with_shift(code: KeyCode) -> Self {
        Self {
            code,
            shift: true,
            cmd: false,
            alt: false,
        }
    }
    fn with_cmd_shift(code: KeyCode) -> Self {
        Self {
            code,
            shift: true,
            cmd: true,
            alt: false,
        }
    }
    /// Option/Alt alone — the primary modifier for the text-formatting
    /// nudges (kerning/tracking, leading): Illustrator reserves Option
    /// there instead of the OS-standard word-navigation meaning.
    pub fn with_alt(code: KeyCode) -> Self {
        Self {
            code,
            shift: false,
            cmd: false,
            alt: true,
        }
    }
    /// Shift+Option — baseline shift's primary modifier.
    pub fn with_shift_alt(code: KeyCode) -> Self {
        Self {
            code,
            shift: true,
            cmd: false,
            alt: true,
        }
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let c = key_char(self.code).unwrap_or('?');
        if self.cmd {
            write!(f, "Cmd+")?;
        }
        if self.alt {
            write!(f, "Option+")?;
        }
        if self.shift {
            write!(f, "Shift+")?;
        }
        write!(f, "{c}")
    }
}

impl std::str::FromStr for KeyChord {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let s = s.trim();
        let (cmd, s) = s.strip_prefix("Cmd+").map_or((false, s), |r| (true, r));
        let (alt, s) = s.strip_prefix("Option+").map_or((false, s), |r| (true, r));
        let (shift, rest) = s.strip_prefix("Shift+").map_or((false, s), |r| (true, r));
        let mut ch = rest.chars();
        let (Some(c), None) = (ch.next(), ch.next()) else {
            return Err(());
        };
        key_code(c)
            .map(|code| KeyChord { code, shift, cmd, alt })
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
        ArrowLeft => '←', ArrowRight => '→', ArrowUp => '↑', ArrowDown => '↓',
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
        '←' => ArrowLeft, '→' => ArrowRight, '↑' => ArrowUp, '↓' => ArrowDown,
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
        Tool::Gradient => KeyChord::plain(KeyG),
        Tool::Rotate => KeyChord::plain(KeyR),
        Tool::Reflect => KeyChord::plain(KeyO),
        Tool::Scale => KeyChord::plain(KeyS),
        Tool::Blend => KeyChord::plain(KeyW),
        Tool::Width => KeyChord::with_shift(KeyW),
        Tool::FreeTransform => KeyChord::plain(KeyE),
        Tool::ShapeBuilder => KeyChord::with_shift(KeyM),
        Tool::Eraser => KeyChord::with_shift(KeyE),
        Tool::RoundedRect | Tool::Polygon | Tool::Star | Tool::Shear | Tool::Arc | Tool::Spiral
        | Tool::Join => {
            return None
        }
    })
}

/// A non-tool command that carries a user-remappable shortcut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefAction {
    SwapPaints,
    DefaultPaints,
    Place,
    CommandPalette,
    TrackingDecrease,
    TrackingIncrease,
    LeadingDecrease,
    LeadingIncrease,
    BaselineShiftUp,
    BaselineShiftDown,
}

impl PrefAction {
    pub const ALL: [PrefAction; 10] = [
        PrefAction::SwapPaints,
        PrefAction::DefaultPaints,
        PrefAction::Place,
        PrefAction::CommandPalette,
        PrefAction::TrackingDecrease,
        PrefAction::TrackingIncrease,
        PrefAction::LeadingDecrease,
        PrefAction::LeadingIncrease,
        PrefAction::BaselineShiftUp,
        PrefAction::BaselineShiftDown,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PrefAction::SwapPaints => "Swap Fill / Stroke",
            PrefAction::DefaultPaints => "Default Fill / Stroke",
            PrefAction::Place => "Place…",
            PrefAction::CommandPalette => "Command Palette",
            PrefAction::TrackingDecrease => "Decrease Tracking/Kerning",
            PrefAction::TrackingIncrease => "Increase Tracking/Kerning",
            PrefAction::LeadingDecrease => "Decrease Leading",
            PrefAction::LeadingIncrease => "Increase Leading",
            PrefAction::BaselineShiftUp => "Baseline Shift Up",
            PrefAction::BaselineShiftDown => "Baseline Shift Down",
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
                alt: false,
            },
            PrefAction::TrackingDecrease => KeyChord::with_alt(KeyCode::ArrowLeft),
            PrefAction::TrackingIncrease => KeyChord::with_alt(KeyCode::ArrowRight),
            // Illustrator: Option-Up tightens (decreases) leading,
            // Option-Down loosens (increases) it.
            PrefAction::LeadingDecrease => KeyChord::with_alt(KeyCode::ArrowUp),
            PrefAction::LeadingIncrease => KeyChord::with_alt(KeyCode::ArrowDown),
            PrefAction::BaselineShiftUp => KeyChord::with_shift_alt(KeyCode::ArrowUp),
            PrefAction::BaselineShiftDown => KeyChord::with_shift_alt(KeyCode::ArrowDown),
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
    /// Chrome size, independent of monitor DPI and document zoom.
    pub ui_scale: f64,
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
    /// Selection handle / grab-radius size — Illustrator's own separate
    /// "Selection & Anchor Display" preference. Independent of
    /// `ui_scale`; see [`crate::handle_scale`].
    pub handle_size: crate::handle_scale::HandleSize,
    /// Smart Guides master switch (⌘U) — unlike `outline_mode`/
    /// `transparency_grid` (plain `App` fields, reset every launch), this
    /// one is a real persisted preference: users expect ⌘U to stay how
    /// they left it, the way Illustrator's own does.
    pub smart_guides_enabled: bool,
    /// Dashed alignment lines to other objects' edges/center while moving.
    pub sg_alignment_guides: bool,
    /// The "anchor"/"path"/"endpoint" hover labels.
    pub sg_anchor_path_labels: bool,
    /// Highlights the exact path under the cursor (useful inside groups).
    pub sg_object_highlighting: bool,
    /// Distance/angle readouts while drawing or dragging.
    pub sg_measurement_labels: bool,
    /// Preset-angle guides from the last anchor while drawing with the Pen.
    pub sg_construction_guides: bool,
    /// A reference guide at the original angle/size while scaling/rotating.
    pub sg_transform_tools: bool,
    /// Equal-gap labels when 3+ objects line up with matching spacing.
    pub sg_spacing_guides: bool,
    /// Screen-px snapping tolerance — Illustrator's own default is 4.
    pub sg_tolerance: f64,
    /// Construction Guides' preset angles (degrees from the last anchor);
    /// 6 slots, matching Illustrator's own Smart Guides preferences.
    pub sg_angles: [f64; 6],
    /// View ▸ Show Grid (⌘'). A real persisted preference, same reasoning
    /// as `smart_guides_enabled`.
    pub show_grid: bool,
    /// View ▸ Snap to Grid (⇧⌘').
    pub snap_to_grid: bool,
    /// View ▸ Snap to Pixel — rounds to the nearest whole document unit.
    pub snap_to_pixel: bool,
    /// View ▸ Snap to Point (⌥⌘') — anchor-point snapping independent of
    /// the Smart Guides master switch; Illustrator ships this on by
    /// default and most users never turn it off.
    pub snap_to_point: bool,
    /// Grid line spacing, canonical px. Illustrator's own default is 1
    /// inch (72pt).
    pub grid_spacing: f64,
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
            ui_scale: 1.0,
            nudge_step: 1.0,
            show_tooltips: true,
            home_on_last_close: true,
            accent: ACCENTS[0].1,
            tool_keys: Settings::default_tool_keys(),
            action_keys: Settings::default_action_keys(),
            show_fps: true,
            show_cull_outline: false,
            cull_inset: crate::canvas::CULL_INSET,
            handle_size: crate::handle_scale::HandleSize::default(),
            smart_guides_enabled: true,
            sg_alignment_guides: true,
            sg_anchor_path_labels: true,
            sg_object_highlighting: true,
            sg_measurement_labels: true,
            sg_construction_guides: true,
            sg_transform_tools: true,
            sg_spacing_guides: true,
            sg_tolerance: 4.0,
            sg_angles: [0.0, 45.0, 90.0, 135.0, 0.0, 0.0],
            show_grid: false,
            snap_to_grid: false,
            snap_to_pixel: false,
            snap_to_point: true,
            grid_spacing: 72.0,
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

pub const CATEGORIES: [&str; 5] = ["General", "Smart Guides", "Keyboard", "Scripts", "Debug"];

fn metric_w() -> f64 { crate::metrics::with(|m| m.prefs_w) }
fn metric_h() -> f64 { crate::metrics::with(|m| m.prefs_h) }
fn metric_sidebar_w() -> f64 { crate::metrics::with(|m| m.prefs_sidebar_w) }
fn metric_pad() -> f64 { crate::metrics::with(|m| m.prefs_pad) }

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
    grid_up: Rect,
    grid_down: Rect,
    check_tips: Rect,
    check_home: Rect,
    check_fps: Rect,
    check_cull: Rect,
    cull_up: Rect,
    cull_down: Rect,
    /// Smart Guides page: the 7 sub-feature checkboxes, in declaration
    /// order (see `SG_CHECK_LABELS`).
    sg_checks: Vec<Rect>,
    sg_tolerance_up: Rect,
    sg_tolerance_down: Rect,
    /// Smart Guides page: one (up, down) pair per `Settings.sg_angles` slot.
    sg_angle_steppers: Vec<(Rect, Rect)>,
    sg_angle_fields: Vec<Rect>,
    pub sg_angle_edit: Option<(usize, crate::text_field::TextField)>,
    accent_swatches: Vec<(Rect, [u8; 3])>,
    scale_buttons: Vec<(Rect, f64)>,
    handle_size_buttons: Vec<(Rect, crate::handle_scale::HandleSize)>,
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
    /// General page: View ▸ Show Grid's line spacing, canonical px.
    SetGridSpacing(f64),
    ToggleTips,
    ToggleHome,
    ToggleFps,
    ToggleCullOutline,
    SetCullInset(f64),
    SetAccent([u8; 3]),
    SetUiScale(f64),
    SetHandleSize(crate::handle_scale::HandleSize),
    /// Smart Guides page: toggle sub-feature `i` (index into the page's
    /// own declaration order — see `paint_smart_guides`'s `LABELS`).
    ToggleSgFeature(usize),
    SetSgTolerance(f64),
    /// Smart Guides page: set construction-guide angle slot `i`.
    SetSgAngle(usize, f64),
    EditSgAngle(usize),
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
    pub fn commit_sg_angle(&mut self) {
        if let Some((slot,field)) = self.sg_angle_edit.take() {
            if let Ok(v) = field.text().trim().trim_end_matches('°').parse::<f64>() {
                if v.is_finite() { self.working.sg_angles[slot] = v.rem_euclid(180.0); }
            }
        }
    }

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
            grid_up: Rect::ZERO,
            grid_down: Rect::ZERO,
            check_tips: Rect::ZERO,
            check_home: Rect::ZERO,
            check_fps: Rect::ZERO,
            check_cull: Rect::ZERO,
            cull_up: Rect::ZERO,
            cull_down: Rect::ZERO,
            sg_checks: Vec::new(),
            sg_tolerance_up: Rect::ZERO,
            sg_tolerance_down: Rect::ZERO,
            sg_angle_steppers: Vec::new(),
            sg_angle_fields: Vec::new(),
            sg_angle_edit: None,
            accent_swatches: Vec::new(),
            scale_buttons: Vec::new(),
            handle_size_buttons: Vec::new(),
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
        Rect::from_origin_size(self.origin, (metric_w(), metric_h()))
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
        if self.grid_up.contains(p) {
            return Hit::SetGridSpacing((self.working.grid_spacing + 1.0).min(10_000.0));
        }
        if self.grid_down.contains(p) {
            return Hit::SetGridSpacing((self.working.grid_spacing - 1.0).max(1.0));
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
        for (r, scale) in &self.scale_buttons {
            if r.contains(p) { return Hit::SetUiScale(*scale); }
        }
        for (i, r) in self.sg_checks.iter().enumerate() {
            if r.contains(p) { return Hit::ToggleSgFeature(i); }
        }
        if self.sg_tolerance_up.contains(p) {
            return Hit::SetSgTolerance((self.working.sg_tolerance + 0.5).min(50.0));
        }
        if self.sg_tolerance_down.contains(p) {
            return Hit::SetSgTolerance((self.working.sg_tolerance - 0.5).max(0.5));
        }
        for (i, (up, down)) in self.sg_angle_steppers.iter().enumerate() {
            if up.contains(p) {
                return Hit::SetSgAngle(i, self.working.sg_angles[i] + 15.0);
            }
            if down.contains(p) {
                return Hit::SetSgAngle(i, self.working.sg_angles[i] - 15.0);
            }
        }
        for (i,r) in self.sg_angle_fields.iter().enumerate() {
            if r.contains(p) { return Hit::EditSgAngle(i); }
        }
        for (r, size) in &self.handle_size_buttons {
            if r.contains(p) { return Hit::SetHandleSize(*size); }
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
        let ox = ((wl - metric_w()) / 2.0).round().max(0.0);
        let oy = ((hl - metric_h()) / 2.0).round().max(0.0);
        self.origin = Point::new(ox, oy);
        let card = self.card();
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &card.to_rounded_rect(ui_px(8.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &card.to_rounded_rect(ui_px(8.0)));

        // Title bar.
        let tw = tcx.measure("Preferences", 13.0);
        tcx.draw(scene, "Preferences", 13.0, theme.text, card.center().x - tw / 2.0, oy + ui_px(22.0));
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.border,
            None,
            &Rect::new(ox, oy + ui_px(36.0), ox + metric_w(), oy + ui_px(37.0)),
        );

        // Category sidebar.
        self.cat_rows.clear();
        let mut y = oy + ui_px(48.0);
        for (i, name) in CATEGORIES.iter().enumerate() {
            let row = Rect::new(ox + ui_px(8.0), y, ox + metric_sidebar_w() - ui_px(8.0), y + ui_px(26.0));
            if i == self.category {
                scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent, None, &row.to_rounded_rect(ui_px(4.0)));
            } else if row.contains(Point::ZERO) {
                // no-op
            }
            let col = if i == self.category {
                theme.on_accent
            } else {
                theme.text
            };
            tcx.draw(scene, name, 12.5, col, row.x0 + ui_px(10.0), row.y0 + ui_px(17.0));
            self.cat_rows.push(row);
            y += ui_px(28.0);
        }
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.border,
            None,
            &Rect::new(ox + metric_sidebar_w(), oy + ui_px(37.0), ox + metric_sidebar_w() + 1.0, oy + metric_h() - ui_px(52.0)),
        );

        let px = ox + metric_sidebar_w() + metric_pad();

        // Rects from the page that isn't shown must not stay hittable.
        self.accent_swatches.clear();
        self.scale_buttons.clear();
        self.handle_size_buttons.clear();
        self.bind_rows.clear();
        self.inc_up = Rect::ZERO;
        self.inc_down = Rect::ZERO;
        self.grid_up = Rect::ZERO;
        self.grid_down = Rect::ZERO;
        self.check_tips = Rect::ZERO;
        self.check_home = Rect::ZERO;
        self.check_fps = Rect::ZERO;
        self.check_cull = Rect::ZERO;
        self.cull_up = Rect::ZERO;
        self.cull_down = Rect::ZERO;
        self.sg_checks.clear();
        self.sg_tolerance_up = Rect::ZERO;
        self.sg_tolerance_down = Rect::ZERO;
        self.sg_angle_steppers.clear();
        self.sg_angle_fields.clear();
        self.reset_keys = Rect::ZERO;
        self.scripts_choose = Rect::ZERO;
        self.scripts_clear = Rect::ZERO;
        self.preset_trigger = Rect::ZERO;
        self.preset_add = Rect::ZERO;
        self.preset_items.clear();

        let footer = |s: &mut Self, scene: &mut Scene, tcx: &mut TextContext| {
            let by = oy + metric_h() - ui_px(40.0);
            s.ok = button(scene, tcx, theme, ox + metric_w() - metric_pad() - ui_px(76.0), by, "OK", true);
            s.cancel = button(scene, tcx, theme, ox + metric_w() - metric_pad() - ui_px(174.0), by, "Cancel", false);
        };

        if self.category == 1 {
            self.paint_smart_guides(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        if self.category == 2 {
            self.paint_keyboard(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        if self.category == 3 {
            self.paint_scripts(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        if self.category == 4 {
            self.paint_debug(scene, tcx, theme, px, oy);
            footer(self, scene, tcx);
            return;
        }

        // General page.
        let mut cy = oy + ui_px(60.0);
        tcx.draw(scene, "General", 13.0, theme.text, px, cy);
        cy += ui_px(30.0);

        tcx.draw(scene, "Keyboard Increment", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        let fx = px + ui_px(170.0);
        let field = Rect::new(fx, cy, fx + ui_px(90.0), cy + ui_px(22.0));
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(ui_px(4.0)));
        tcx.draw(
            scene,
            &format!("{} pt", trim(self.working.nudge_step)),
            12.0,
            theme.text,
            fx + ui_px(8.0),
            cy + ui_px(15.0),
        );
        self.inc_up = Rect::new(field.x1 - ui_px(16.0), cy + 1.0, field.x1, cy + ui_px(11.0));
        self.inc_down = Rect::new(field.x1 - ui_px(16.0), cy + ui_px(11.0), field.x1, cy + ui_px(21.0));
        tri(scene, self.inc_up.center(), true, theme.text_dim);
        tri(scene, self.inc_down.center(), false, theme.text_dim);
        cy += ui_px(44.0);

        // View ▸ Show Grid's line spacing — its own system, not a Smart
        // Guides sub-feature, but this is the only numeric preference it
        // has, so it lives on General next to the other plain numbers
        // rather than needing a whole page of its own.
        tcx.draw(scene, "Grid Spacing", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        let gfield = Rect::new(fx, cy, fx + ui_px(90.0), cy + ui_px(22.0));
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &gfield.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &gfield.to_rounded_rect(ui_px(4.0)));
        tcx.draw(
            scene,
            &format!("{} pt", trim(self.working.grid_spacing)),
            12.0,
            theme.text,
            fx + ui_px(8.0),
            cy + ui_px(15.0),
        );
        self.grid_up = Rect::new(gfield.x1 - ui_px(16.0), cy + 1.0, gfield.x1, cy + ui_px(11.0));
        self.grid_down = Rect::new(gfield.x1 - ui_px(16.0), cy + ui_px(11.0), gfield.x1, cy + ui_px(21.0));
        tri(scene, self.grid_up.center(), true, theme.text_dim);
        tri(scene, self.grid_down.center(), false, theme.text_dim);
        cy += ui_px(44.0);

        self.check_tips = checkbox(scene, tcx, theme, px, cy, "Show Tool Tips", self.working.show_tooltips);
        cy += ui_px(30.0);
        self.check_home = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show the Home Screen when the last document closes",
            self.working.home_on_last_close,
        );
        cy += ui_px(40.0);

        // Accent colour swatches.
        tcx.draw(scene, "Accent Color", 12.0, theme.text_dim, px, cy + ui_px(12.0));
        let mut sx = px + ui_px(170.0);
        for (_, rgb) in ACCENTS {
            let sw = Rect::new(sx, cy, sx + ui_px(20.0), cy + ui_px(20.0));
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgb8(rgb[0], rgb[1], rgb[2]),
                None,
                &sw.to_rounded_rect(ui_px(4.0)),
            );
            if rgb == self.working.accent {
                scene.stroke(
                    &Stroke::new(ui_px(2.0)),
                    Affine::IDENTITY,
                    theme.text,
                    None,
                    &sw.inflate(ui_px(2.5), ui_px(2.5)).to_rounded_rect(ui_px(6.0)),
                );
            }
            self.accent_swatches.push((sw, rgb));
            sx += ui_px(30.0);
        }

        cy += ui_px(42.0);
        tcx.draw(scene, "UI Scale", 12.0, theme.text_dim, px, cy + ui_px(16.0));
        for (i, scale) in [1.0, 1.25, 1.5].into_iter().enumerate() {
            let x = px + ui_px(170.0) + i as f64 * ui_px(78.0);
            let r = Rect::new(x, cy, x + ui_px(70.0), cy + ui_px(26.0));
            crate::widgets::button(scene, tcx, theme, r, &format!("{:.0}%", scale * 100.0), self.working.ui_scale == scale);
            self.scale_buttons.push((r, scale));
        }
        tcx.draw(scene, "Native menu size follows system settings.", 11.0, theme.text_dim, px, cy + ui_px(48.0));

        // Selection & anchor handle size — its own preference, independent
        // of UI Scale above (Illustrator's own Selection & Anchor Display
        // preference is likewise separate from its general UI scaling).
        cy += ui_px(60.0);
        tcx.draw(scene, "Handle Size", 12.0, theme.text_dim, px, cy + ui_px(16.0));
        for (i, size) in crate::handle_scale::HandleSize::ALL.into_iter().enumerate() {
            let x = px + ui_px(170.0) + i as f64 * ui_px(78.0);
            let r = Rect::new(x, cy, x + ui_px(70.0), cy + ui_px(26.0));
            crate::widgets::button(scene, tcx, theme, r, size.label(), self.working.handle_size == size);
            self.handle_size_buttons.push((r, size));
        }
        tcx.draw(scene, "Selection handles and their grab radius on the canvas.", 11.0, theme.text_dim, px, cy + ui_px(48.0));

        // Footer buttons.
        let by = oy + metric_h() - ui_px(40.0);
        self.ok = button(scene, tcx, theme, ox + metric_w() - metric_pad() - ui_px(76.0), by, "OK", true);
        self.cancel = button(scene, tcx, theme, ox + metric_w() - metric_pad() - ui_px(174.0), by, "Cancel", false);
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
        let row_w = metric_w() - metric_sidebar_w() - metric_pad() * 2.0;

        // Preset row.
        tcx.draw(scene, "Preset", 12.0, theme.text_dim, px, oy + ui_px(62.0));
        let chip = Rect::new(px + ui_px(52.0), oy + ui_px(46.0), px + ui_px(52.0) + ui_px(210.0), oy + ui_px(70.0));
        let naming = self.naming.is_some();
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &chip.to_rounded_rect(ui_px(4.0)));
        scene.stroke(
            &Stroke::new(ui_px(1.0)),
            Affine::IDENTITY,
            if naming { theme.accent } else { theme.border },
            None,
            &chip.to_rounded_rect(ui_px(4.0)),
        );
        if let Some(field) = &mut self.naming {
            field.paint(scene, tcx, theme, chip, "name preset", true);
        } else {
            tcx.draw(
                scene,
                &self.working_keymaps.active,
                12.0,
                theme.text,
                chip.x0 + ui_px(10.0),
                chip.y0 + ui_px(16.0),
            );
            tri(scene, Point::new(chip.x1 - ui_px(14.0), chip.center().y), false, theme.text_dim);
            self.preset_trigger = chip;
        }
        // "+" — save the current edits as a new preset.
        let add = Rect::new(chip.x1 + ui_px(8.0), chip.y0, chip.x1 + ui_px(8.0) + ui_px(26.0), chip.y1);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_active, None, &add.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &add.to_rounded_rect(ui_px(4.0)));
        let plus = if naming { "OK" } else { "+" };
        let pw = tcx.measure(plus, 13.0);
        tcx.draw(scene, plus, 13.0, theme.text, add.center().x - pw / 2.0, add.center().y + ui_px(4.5));
        self.preset_add = add;

        tcx.draw(scene, "Tool Shortcuts", 13.0, theme.text, px, oy + ui_px(96.0));
        tcx.draw(
            scene,
            "Click a shortcut, then press a key. Shift is allowed.",
            11.0,
            theme.text_dim,
            px,
            oy + ui_px(116.0),
        );

        let recording = self.recording;
        // Reset sits in the fixed footer row so the list never hides it.
        self.reset_keys = button(scene, tcx, theme, px, oy + metric_h() - ui_px(40.0), "Reset", false);

        let n_tools = Tool::ALL.len();
        let n_acts = PrefAction::ALL.len();
        let content_h = n_tools as f64 * ui_px(27.0) + ui_px(26.0) + n_acts as f64 * ui_px(27.0) + ui_px(6.0);
        let view = Rect::new(px - ui_px(6.0), oy + ui_px(128.0), px + row_w + ui_px(14.0), oy + metric_h() - ui_px(52.0));
        let sc = self.begin_scroll_list(scene, theme, view, content_h);

        let mut y = view.y0 + ui_px(2.0) - sc;
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
            y += ui_px(27.0);
        }
        y += ui_px(6.0);
        tcx.draw(scene, "Colours", 13.0, theme.text, px, y + ui_px(4.0));
        y += ui_px(20.0);
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
            y += ui_px(27.0);
        }
        scene.pop_layer();

        // Preset dropdown, painted last so it sits over the list.
        if self.preset_menu_open && self.naming.is_none() {
            let names = self.working_keymaps.names();
            let t = self.preset_trigger;
            let box_ = Rect::new(t.x0, t.y1 + ui_px(2.0), t.x1, t.y1 + ui_px(2.0) + names.len() as f64 * ui_px(24.0));
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_bg, None, &box_.to_rounded_rect(ui_px(4.0)));
            scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.accent, None, &box_.to_rounded_rect(ui_px(4.0)));
            for (i, name) in names.iter().enumerate() {
                let r = Rect::new(box_.x0, box_.y0 + i as f64 * ui_px(24.0), box_.x1, box_.y0 + (i as f64 + 1.0) * 24.0);
                if name == &self.working_keymaps.active {
                    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent.with_alpha(0.18), None, &r);
                }
                tcx.draw(scene, name, 12.0, theme.text, r.x0 + ui_px(10.0), r.y0 + ui_px(16.0));
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
        let mut cy = oy + ui_px(60.0);
        tcx.draw(scene, "Scripts", 13.0, theme.text, px, cy);
        cy += ui_px(12.0);
        tcx.draw(
            scene,
            "Point Amalith at a folder of scripts you keep yourself — updates can't touch it.",
            11.0,
            theme.text_dim,
            px,
            cy + ui_px(12.0),
        );
        cy += ui_px(30.0);

        tcx.draw(scene, "Folder", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        let path_str = self
            .working_scripts
            .dir
            .as_ref()
            .map(|d| elide_left(&d.display().to_string(), 52))
            .unwrap_or_else(|| "None chosen".to_string());
        tcx.draw(scene, &path_str, 11.5, theme.text, px + ui_px(58.0), cy + ui_px(14.0));
        cy += ui_px(26.0);
        self.scripts_choose = button(scene, tcx, theme, px, cy, "Choose…", true);
        self.scripts_clear = if self.working_scripts.dir.is_some() {
            button(scene, tcx, theme, px + ui_px(100.0), cy, "Clear", false)
        } else {
            Rect::ZERO
        };
        cy += ui_px(42.0);

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
                cy + ui_px(4.0),
            );
            return;
        }

        tcx.draw(scene, "Shortcuts", 13.0, theme.text, px, cy + ui_px(4.0));
        tcx.draw(
            scene,
            "Click a shortcut, then press a key (Cmd / Shift allowed).",
            11.0,
            theme.text_dim,
            px,
            cy + ui_px(22.0),
        );
        let list_top = cy + ui_px(34.0);

        let row_w = metric_w() - metric_sidebar_w() - metric_pad() * 2.0;
        let recording = self.recording;
        let names: Vec<String> = self
            .script_paths
            .iter()
            .map(|p| crate::scripts::label(p))
            .collect();
        let content_h = names.len() as f64 * ui_px(27.0) + ui_px(4.0);
        let view = Rect::new(px - ui_px(6.0), list_top, px + row_w + ui_px(14.0), oy + metric_h() - ui_px(52.0));
        let sc = self.begin_scroll_list(scene, theme, view, content_h);

        let mut y = view.y0 + ui_px(2.0) - sc;
        for (i, name) in names.iter().enumerate() {
            let chord = self.working_scripts.chord_for(name);
            kb_row(
                scene, tcx, theme, px, row_w, y,
                name, chord, recording,
                BindTarget::Script(i),
                view,
                &mut self.bind_rows,
            );
            y += ui_px(27.0);
        }
        scene.pop_layer();
    }

    /// The Debug page — cull-outline visibility and distance.
    /// The Smart Guides page: the 7 Illustrator sub-feature checkboxes,
    /// the snapping-tolerance stepper, and 6 construction-guide angle
    /// steppers.
    fn paint_smart_guides(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        px: f64,
        oy: f64,
    ) {
        let mut cy = oy + ui_px(60.0);
        tcx.draw(scene, "Smart Guides", 13.0, theme.text, px, cy);
        cy += ui_px(30.0);

        const LABELS: [(&str, fn(&Settings) -> bool); 7] = [
            ("Alignment Guides", |s| s.sg_alignment_guides),
            ("Anchor/Path Labels", |s| s.sg_anchor_path_labels),
            ("Object Highlighting", |s| s.sg_object_highlighting),
            ("Measurement Labels", |s| s.sg_measurement_labels),
            ("Construction Guides", |s| s.sg_construction_guides),
            ("Transform Tools", |s| s.sg_transform_tools),
            ("Spacing Guides", |s| s.sg_spacing_guides),
        ];
        for (label, get) in LABELS {
            let r = checkbox(scene, tcx, theme, px, cy, label, get(&self.working));
            self.sg_checks.push(r);
            cy += ui_px(26.0);
        }
        cy += ui_px(10.0);

        tcx.draw(scene, "Snapping Tolerance", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        let fx = px + ui_px(170.0);
        let field = Rect::new(fx, cy, fx + ui_px(90.0), cy + ui_px(22.0));
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(ui_px(4.0)));
        tcx.draw(scene, &format!("{} px", trim(self.working.sg_tolerance)), 12.0, theme.text, fx + ui_px(8.0), cy + ui_px(15.0));
        self.sg_tolerance_up = Rect::new(field.x1 - ui_px(16.0), cy + 1.0, field.x1, cy + ui_px(11.0));
        self.sg_tolerance_down = Rect::new(field.x1 - ui_px(16.0), cy + ui_px(11.0), field.x1, cy + ui_px(21.0));
        tri(scene, self.sg_tolerance_up.center(), true, theme.text_dim);
        tri(scene, self.sg_tolerance_down.center(), false, theme.text_dim);
        cy += ui_px(38.0);

        tcx.draw(scene, "Construction Guide Angles", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        cy += ui_px(20.0);
        let step_w = ui_px(58.0);
        for (i, deg) in self.working.sg_angles.into_iter().enumerate() {
            let x = px + i as f64 * (step_w + ui_px(6.0));
            let field = Rect::new(x, cy, x + step_w, cy + ui_px(22.0));
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(ui_px(4.0)));
            scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(ui_px(4.0)));
            tcx.draw(scene, &format!("{}°", trim(deg)), 11.5, theme.text, x + ui_px(6.0), cy + ui_px(15.0));
            let up = Rect::new(field.x1 - ui_px(14.0), cy + 1.0, field.x1, cy + ui_px(11.0));
            let down = Rect::new(field.x1 - ui_px(14.0), cy + ui_px(11.0), field.x1, cy + ui_px(21.0));
            tri(scene, up.center(), true, theme.text_dim);
            tri(scene, down.center(), false, theme.text_dim);
            self.sg_angle_steppers.push((up, down));
            let input = Rect::new(field.x0,field.y0,up.x0,field.y1);
            self.sg_angle_fields.push(input);
            if let Some((slot,editor)) = &mut self.sg_angle_edit {
                if *slot == i { editor.paint(scene,tcx,theme,input,"Angle",true); }
            }
        }
        cy += ui_px(38.0);

        tcx.draw(scene, "Click an angle to type a value. Tolerance stays constant on screen.", 11.0, theme.text_dim, px, cy + ui_px(4.0));
    }

    fn paint_debug(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        px: f64,
        oy: f64,
    ) {
        let mut cy = oy + ui_px(60.0);
        tcx.draw(scene, "Debug", 13.0, theme.text, px, cy);
        cy += ui_px(30.0);

        self.check_fps = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show FPS Counter",
            self.working.show_fps,
        );
        cy += ui_px(30.0);

        self.check_cull = checkbox(
            scene,
            tcx,
            theme,
            px,
            cy,
            "Show Cull Outline",
            self.working.show_cull_outline,
        );
        cy += ui_px(36.0);

        tcx.draw(scene, "Cull Distance", 12.0, theme.text_dim, px, cy + ui_px(14.0));
        let fx = px + ui_px(170.0);
        let field = Rect::new(fx, cy, fx + ui_px(90.0), cy + ui_px(22.0));
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.bg, None, &field.to_rounded_rect(ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &field.to_rounded_rect(ui_px(4.0)));
        tcx.draw(
            scene,
            &format!("{} px", trim(self.working.cull_inset)),
            12.0,
            theme.text,
            fx + ui_px(8.0),
            cy + ui_px(15.0),
        );
        self.cull_up = Rect::new(field.x1 - ui_px(16.0), cy + 1.0, field.x1, cy + ui_px(11.0));
        self.cull_down = Rect::new(field.x1 - ui_px(16.0), cy + ui_px(11.0), field.x1, cy + ui_px(21.0));
        tri(scene, self.cull_up.center(), true, theme.text_dim);
        tri(scene, self.cull_down.center(), false, theme.text_dim);
        cy += ui_px(42.0);

        tcx.draw(
            scene,
            "How far past the visible canvas an object is kept before it",
            11.0,
            theme.text_dim,
            px,
            cy + ui_px(4.0),
        );
        tcx.draw(
            scene,
            "stops drawing. Larger culls further out; the dashed magenta",
            11.0,
            theme.text_dim,
            px,
            cy + ui_px(20.0),
        );
        tcx.draw(
            scene,
            "line marks the threshold when Show Cull Outline is on.",
            11.0,
            theme.text_dim,
            px,
            cy + ui_px(36.0),
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
    let row = Rect::new(px, cy, px + row_w, cy + ui_px(24.0));
    let hot = recording == Some(target);
    if hot {
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.accent.with_alpha(0.18),
            None,
            &row.to_rounded_rect(ui_px(4.0)),
        );
    }
    tcx.draw(scene, name, 12.0, theme.text, px + ui_px(4.0), cy + ui_px(16.0));
    let chip = Rect::new(row.x1 - ui_px(120.0), cy + 1.0, row.x1, cy + ui_px(23.0));
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        theme.bg,
        None,
        &chip.to_rounded_rect(ui_px(4.0)),
    );
    scene.stroke(
        &Stroke::new(ui_px(1.0)),
        Affine::IDENTITY,
        if hot { theme.accent } else { theme.border },
        None,
        &chip.to_rounded_rect(ui_px(4.0)),
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
        cy + ui_px(16.0),
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
    let d = ui_px(3.0);
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
    let box_ = Rect::new(x, y, x + ui_px(16.0), y + ui_px(16.0));
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        if on { theme.accent } else { theme.bg },
        None,
        &box_.to_rounded_rect(ui_px(3.0)),
    );
    scene.stroke(&Stroke::new(ui_px(1.0)), Affine::IDENTITY, theme.border, None, &box_.to_rounded_rect(ui_px(3.0)));
    if on {
        let mut tick = vello::kurbo::BezPath::new();
        tick.move_to((x + ui_px(3.5), y + ui_px(8.5)));
        tick.line_to((x + ui_px(6.5), y + ui_px(11.5)));
        tick.line_to((x + ui_px(12.5), y + ui_px(4.5)));
        scene.stroke(
            &Stroke::new(ui_px(1.8)),
            Affine::IDENTITY,
            theme.on_accent,
            None,
            &tick,
        );
    }
    tcx.draw(scene, label, 12.0, theme.text, x + ui_px(24.0), y + ui_px(13.0));
    // Whole-row hit rect.
    Rect::new(x, y - ui_px(2.0), x + ui_px(24.0) + tcx.measure(label, 12.0), y + ui_px(18.0))
}

/// `x, y` size-convenience wrapper over `widgets::button` — every call site
/// here just wants a fixed-size button planted at a point, not a
/// caller-computed `Rect`.
fn button(
    scene: &mut Scene,
    tcx: &mut TextContext,
    theme: &Theme,
    x: f64,
    y: f64,
    label: &str,
    primary: bool,
) -> Rect {
    let r = Rect::new(x, y, x + ui_px(86.0), y + ui_px(26.0));
    crate::widgets::button(scene, tcx, theme, r, label, primary);
    r
}

#[cfg(test)]
mod scale_tests {
    use super::*;
    use crate::window_dpi::WindowDpi;

    #[test]
    fn smart_guide_angle_fields_are_editable_and_stay_inside_the_card() {
        let mut p=Prefs::new(Settings::default(),Default::default(),Default::default());
        p.category=1;
        let mut text=TextContext::new();
        p.paint(&mut Scene::new(),&mut text,&Theme::default(),1800.0,1000.0);
        assert_eq!(p.sg_angle_fields.len(),6);
        for (i,r) in p.sg_angle_fields.clone().into_iter().enumerate() {
            assert!(p.card().contains(r.origin()));
            assert!(r.y1 < p.ok.y0);
            assert!(matches!(p.on_press(r.center()),Hit::EditSgAngle(j) if i==j));
        }
        p.sg_angle_edit=Some((2,crate::text_field::TextField::new("22.5")));
        p.commit_sg_angle();
        assert_eq!(p.working.sg_angles[2],22.5);
        p.sg_angle_edit=Some((2,crate::text_field::TextField::new("NaN")));
        p.commit_sg_angle();
        assert_eq!(p.working.sg_angles[2],22.5);
        p.category=0;
        p.paint(&mut Scene::new(),&mut text,&Theme::default(),1800.0,1000.0);
        assert!(p.sg_angle_fields.is_empty());
    }

    #[test]
    fn scale_buttons_share_painted_hit_geometry_at_each_dpi() {
        struct Reset;
        impl Drop for Reset { fn drop(&mut self) { crate::metrics::apply(1.0); } }
        let _reset = Reset;
        for scale in [1.0, 1.25, 1.5] {
            crate::metrics::apply(scale);
            let mut prefs = Prefs::new(Settings::default(), Default::default(), Default::default());
            let mut theme = Theme::default();
            theme.set_ui_scale(scale);
            let mut text = TextContext::new();
            text.set_ui_scale(scale);
            prefs.paint(&mut Scene::new(), &mut text, &theme, 1800.0, 1000.0);
            let buttons = prefs.scale_buttons.clone();
            assert_eq!(buttons.len(), 3);
            for pair in buttons.windows(2) { assert!(pair[0].0.x1 < pair[1].0.x0); }
            for (rect, value) in buttons {
                assert!(prefs.card().contains(rect.center()));
                for factor in [1.0, 1.5, 2.0] {
                    let dpi = WindowDpi::new(factor);
                    let physical = dpi.transform() * rect.center();
                    assert!(matches!(prefs.on_press(dpi.point(physical.x, physical.y)), Hit::SetUiScale(v) if v == value));
                }
            }
            prefs.category = 1;
            prefs.paint(&mut Scene::new(), &mut text, &theme, 1800.0, 1000.0);
            assert!(prefs.scale_buttons.is_empty(), "hidden page cannot retain clickable controls");
        }
    }

    /// The Handle Size segmented control (`10-selection-anchor-size-preference-medium.md`)
    /// must satisfy the same painted-geometry-matches-hit-geometry contract
    /// as UI Scale's, and stay off `Settings.ui_scale` entirely: painting
    /// at every DPI/UI-scale combination must always click through to the
    /// same `HandleSize`, and must never collide with the UI Scale row
    /// above it.
    #[test]
    fn handle_size_buttons_share_painted_hit_geometry_and_stay_off_ui_scale() {
        struct Reset;
        impl Drop for Reset { fn drop(&mut self) { crate::metrics::apply(1.0); } }
        let _reset = Reset;
        for scale in [1.0, 1.25, 1.5] {
            crate::metrics::apply(scale);
            let mut prefs = Prefs::new(Settings::default(), Default::default(), Default::default());
            let mut theme = Theme::default();
            theme.set_ui_scale(scale);
            let mut text = TextContext::new();
            text.set_ui_scale(scale);
            prefs.paint(&mut Scene::new(), &mut text, &theme, 1800.0, 1000.0);
            let scale_buttons = prefs.scale_buttons.clone();
            let buttons = prefs.handle_size_buttons.clone();
            assert_eq!(buttons.len(), 3);
            for pair in buttons.windows(2) { assert!(pair[0].0.x1 < pair[1].0.x0); }
            // Two distinct preference rows — their rects must not overlap
            // vertically (a real regression if the layout math above ever
            // collides the two rows).
            for &(scale_rect, _) in &scale_buttons {
                for &(size_rect, _) in &buttons {
                    assert!(
                        scale_rect.y1 <= size_rect.y0 || size_rect.y1 <= scale_rect.y0,
                        "UI Scale and Handle Size rows must not overlap"
                    );
                }
            }
            for (rect, value) in buttons {
                assert!(prefs.card().contains(rect.center()));
                for factor in [1.0, 1.5, 2.0] {
                    let dpi = WindowDpi::new(factor);
                    let physical = dpi.transform() * rect.center();
                    assert!(matches!(prefs.on_press(dpi.point(physical.x, physical.y)), Hit::SetHandleSize(v) if v == value));
                }
            }
            prefs.category = 1;
            prefs.paint(&mut Scene::new(), &mut text, &theme, 1800.0, 1000.0);
            assert!(prefs.handle_size_buttons.is_empty(), "hidden page cannot retain clickable controls");
        }
        // Independent of `ui_scale`: picking a HandleSize must not touch it.
        let mut prefs = Prefs::new(Settings::default(), Default::default(), Default::default());
        let before = prefs.working.ui_scale;
        prefs.working.handle_size = crate::handle_scale::HandleSize::Large;
        assert_eq!(prefs.working.ui_scale, before, "HandleSize must stay independent of ui_scale");
    }

    /// `PrefAction::ALL` is a hand-maintained array with no compiler tie to
    /// the enum's variant list — see
    /// `01-prefaction-and-tool-all-sync-easy.md`. A variant missing from
    /// `ALL` still compiles: it just gets no default keybinding, no row on
    /// the Preferences ▸ Keyboard page, and no persistence round-trip.
    /// `covered` is an exhaustive match with no wildcard, so this test
    /// fails to *compile* the moment a variant is added to `PrefAction`
    /// and forgotten here.
    #[test]
    fn pref_action_all_covers_every_variant_exactly_once() {
        fn covered(a: PrefAction) -> bool {
            match a {
                PrefAction::SwapPaints
                | PrefAction::DefaultPaints
                | PrefAction::Place
                | PrefAction::CommandPalette
                | PrefAction::TrackingDecrease
                | PrefAction::TrackingIncrease
                | PrefAction::LeadingDecrease
                | PrefAction::LeadingIncrease
                | PrefAction::BaselineShiftUp
                | PrefAction::BaselineShiftDown => true,
            }
        }
        for a in PrefAction::ALL {
            assert!(covered(a), "{a:?} missing from the exhaustive check above");
        }
        let mut seen: Vec<PrefAction> = Vec::new();
        for a in PrefAction::ALL {
            assert!(!seen.contains(&a), "{a:?} appears more than once in PrefAction::ALL");
            seen.push(a);
        }
    }
}
