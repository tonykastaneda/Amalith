//! Colors and metrics for the shell's chrome, plus a handful of named
//! [`ColorScheme`]s (built from famous terminal palettes) that reskin the
//! whole app at once — everything that draws from a `Theme` picks it up
//! automatically. Two things are deliberately *not* theme tokens and so
//! never change with the scheme: the artboard's own paper fill (always
//! literal white, drawn straight from `vello::peniko::Color::WHITE` in
//! `app/render/main_view.rs`, since it represents the actual document
//! page) and per-layer contour/selection-edge colors (`LayerColor`,
//! `amalith-core/src/layer.rs`), which are keyed to the layer, not the
//! chrome.

use vello::peniko::Color;

/// A named app-wide color scheme — either one of Amalith's own accent
/// recolors of the base dark chrome, or a full palette borrowed from a
/// named terminal theme. `Theme::for_scheme` maps each one onto every
/// `Theme` token; see the module doc comment for what's deliberately
/// excluded from that mapping. These used to be a separate "Accent
/// Color" swatch picker, independent of the chrome theme — folded in
/// here instead, so accent is just one more thing a theme determines
/// rather than a second, overlapping customization.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorScheme {
    #[default]
    BasicBlue,
    Amalith,
    Emerald,
    RoseRed,
    PerfectPurple,
    Graphite,
    /// https://draculatheme.com
    Dracula,
    /// https://terminalcolors.com/themes/catppuccin/mocha/
    CatppuccinMocha,
    /// https://terminalcolors.com/themes/panda/default/
    Panda,
    /// https://terminalcolors.com/themes/tokyo-night/default/
    TokyoNight,
    /// https://terminalcolors.com/themes/solarized/dark/
    SolarizedDark,
    /// https://terminalcolors.com/themes/cobalt2/default/
    Cobalt2,
    /// https://terminalcolors.com/themes/gruvbox/dark/
    GruvboxDark,
    /// https://terminalcolors.com/themes/gotham/default/
    Gotham,
    /// https://terminalcolors.com/themes/nordic/default/
    Nordic,
    /// https://github.com/mbadolato/iTerm2-Color-Schemes (C64)
    C64,
    /// https://github.com/mbadolato/iTerm2-Color-Schemes (Batman)
    Batman,
    /// https://github.com/mbadolato/iTerm2-Color-Schemes (Acid Lime)
    AcidLime,
}

impl ColorScheme {
    pub const ALL: [ColorScheme; 18] = [
        ColorScheme::BasicBlue,
        ColorScheme::Amalith,
        ColorScheme::Emerald,
        ColorScheme::RoseRed,
        ColorScheme::PerfectPurple,
        ColorScheme::Graphite,
        ColorScheme::Dracula,
        ColorScheme::CatppuccinMocha,
        ColorScheme::Panda,
        ColorScheme::TokyoNight,
        ColorScheme::SolarizedDark,
        ColorScheme::Cobalt2,
        ColorScheme::GruvboxDark,
        ColorScheme::Gotham,
        ColorScheme::Nordic,
        ColorScheme::C64,
        ColorScheme::Batman,
        ColorScheme::AcidLime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ColorScheme::BasicBlue => "Basic Blue",
            ColorScheme::Amalith => "Amalith",
            ColorScheme::Emerald => "Emerald",
            ColorScheme::RoseRed => "Rose Red",
            ColorScheme::PerfectPurple => "Perfect Purple",
            ColorScheme::Graphite => "Graphite",
            ColorScheme::Dracula => "Dracula",
            ColorScheme::CatppuccinMocha => "Catppuccin Mocha",
            ColorScheme::Panda => "Panda",
            ColorScheme::TokyoNight => "Tokyo Night",
            ColorScheme::SolarizedDark => "Solarized Dark",
            ColorScheme::Cobalt2 => "Cobalt2",
            ColorScheme::GruvboxDark => "Gruvbox Dark",
            ColorScheme::Gotham => "Gotham",
            ColorScheme::Nordic => "Nordic",
            ColorScheme::C64 => "C64",
            ColorScheme::Batman => "Batman",
            ColorScheme::AcidLime => "Acid Lime",
        }
    }

    /// Stable on-disk key, independent of `label()`.
    pub fn id_str(self) -> &'static str {
        match self {
            ColorScheme::BasicBlue => "basic_blue",
            ColorScheme::Amalith => "amalith",
            ColorScheme::Emerald => "emerald",
            ColorScheme::RoseRed => "rose_red",
            ColorScheme::PerfectPurple => "perfect_purple",
            ColorScheme::Graphite => "graphite",
            ColorScheme::Dracula => "dracula",
            ColorScheme::CatppuccinMocha => "catppuccin_mocha",
            ColorScheme::Panda => "panda",
            ColorScheme::TokyoNight => "tokyo_night",
            ColorScheme::SolarizedDark => "solarized_dark",
            ColorScheme::Cobalt2 => "cobalt2",
            ColorScheme::GruvboxDark => "gruvbox_dark",
            ColorScheme::Gotham => "gotham",
            ColorScheme::Nordic => "nordic",
            ColorScheme::C64 => "c64",
            ColorScheme::Batman => "batman",
            ColorScheme::AcidLime => "acid_lime",
        }
    }

    pub fn from_id_str(s: &str) -> Option<ColorScheme> {
        Self::ALL.into_iter().find(|c| c.id_str() == s)
    }
}

/// A concrete palette a full-repaint [`ColorScheme`] maps onto every
/// `Theme` token via [`Theme::from_palette`]. Only the colors that
/// actually vary between schemes are listed here — `on_accent` and the
/// three other accent-derived tokens (`drop_fill`/`drop_line`/
/// `marquee_fill`) aren't, since `Theme::set_accent` derives them, and
/// several `Theme` fields intentionally reuse one of these (e.g.
/// `panel_bg`/`strip_active` both just take `bg`) rather than needing
/// their own slot.
struct Palette {
    bg: Color,
    app_bar: Color,
    canvas_bg: Color,
    pasteboard: Color,
    border: Color,
    accent: Color,
    text: Color,
    text_dim: Color,
    symbol_accent: Color,
    raster_accent: Color,
}

#[derive(Clone, Debug)]
pub struct Theme {
    /// Window ground (behind the canvas and rails).
    pub bg: Color,
    /// The top app bar.
    pub app_bar: Color,
    /// The canvas area behind the artboards.
    pub canvas_bg: Color,
    /// The lighter canvas fill shown while the Artboard tool is active.
    pub pasteboard: Color,
    /// A panel body.
    pub panel_bg: Color,
    /// Tab strip background (inactive).
    pub strip_bg: Color,
    /// The active tab's background — reads as continuous with the body.
    pub strip_active: Color,
    /// Hairline around a group.
    pub border: Color,
    /// Splitter handle fill.
    pub splitter: Color,
    /// Translucent wash over the region a drop would occupy.
    pub drop_fill: Color,
    /// The solid Illustrator-style insertion line.
    pub drop_line: Color,
    /// App accent — selection box, transform handles, active-tab underline,
    /// drop indicator, focus rings. User-settable in Preferences; blue by
    /// default. Set via [`Theme::set_accent`] so the derived tokens below
    /// stay in step.
    pub accent: Color,
    /// Legible ink for text / glyphs drawn *on* `accent` — near-black on a
    /// light accent, white on a dark one.
    pub on_accent: Color,
    /// Faint fill inside a live marquee.
    pub marquee_fill: Color,
    /// Artboard hairline and its name label.
    pub artboard_border: Color,
    pub artboard_label: Color,
    pub text: Color,
    pub text_dim: Color,
    /// Distinguishes "editing a Symbol's shared definition" from ordinary
    /// group isolation — the isolation bar's underline while any
    /// breadcrumb in the current isolation stack is a Symbol instance
    /// (see `App::is_editing_symbol`). Deliberately not derived from
    /// `accent` (which the user can repoint) — this needs to stay a
    /// stable, recognizable "you're inside a Symbol" color regardless of
    /// the user's chosen selection-UI accent.
    pub symbol_accent: Color,
    /// The "Raster Layer" mode badge/tab color, paired with `accent` for
    /// "Vector Layer" — see `panels::layers::LayerKind`. Same reasoning
    /// as `symbol_accent`: deliberately not derived from `accent`, so a
    /// user who repoints their selection-UI accent doesn't also lose the
    /// vector/raster mode distinction (hand-picked per scheme below so it
    /// stays legible and non-clashing against that scheme's own accent).
    pub raster_accent: Color,

    /// Height of a tab strip.
    pub tab_strip_h: f64,
    /// Height of a group's title bar — a separate row above its tab
    /// strip, matching Illustrator: the close (×) and collapse-to-icons
    /// («/») controls, and the whole-group drag/detach handle, live here,
    /// fully decoupled from the tab strip (which only ever handles
    /// individual tabs).
    pub group_title_h: f64,
    /// Thickness of the gap between split children.
    pub splitter_thickness: f64,
    /// Horizontal padding inside a tab, per side.
    pub tab_pad_x: f64,
    /// Width reserved on the right of a tab strip for the panel hamburger.
    pub panel_menu_w: f64,
    /// Width of the close (×) button on a group's title bar (left end).
    pub group_close_w: f64,
    /// Width of the collapse-to-icons («/») button on a group's title bar
    /// (right end) — shown only on the group that owns the control (a
    /// column's leader when attached; always, when floating).
    pub panel_collapse_w: f64,
}

impl Theme {
    pub fn set_ui_scale(&mut self, scale: f64) {
        let base = Self::default();
        self.tab_strip_h = base.tab_strip_h * scale;
        self.group_title_h = base.group_title_h * scale;
        self.splitter_thickness = base.splitter_thickness * scale;
        self.tab_pad_x = base.tab_pad_x * scale;
        self.panel_menu_w = base.panel_menu_w * scale;
        self.group_close_w = base.group_close_w * scale;
        self.panel_collapse_w = base.panel_collapse_w * scale;
    }

    /// Point the accent at `c` and refresh every token derived from it
    /// (drop indicator wash + line, marquee fill, and the on-accent ink).
    pub fn set_accent(&mut self, c: Color) {
        self.accent = c;
        self.drop_line = c;
        self.drop_fill = c.with_alpha(0.20);
        self.marquee_fill = c.with_alpha(0.12);
        // Perceptual-ish luma in sRGB space; light accent → dark ink.
        let [r, g, b, _] = c.components;
        let luma = 0.299 * r + 0.587 * g + 0.114 * b;
        self.on_accent = if luma > 0.6 {
            Color::from_rgb8(0x14, 0x14, 0x16)
        } else {
            Color::WHITE
        };
    }

    /// A fresh base `Theme` for `scheme`, own accent baked in via
    /// `set_accent`. Callers that also want the current UI scale
    /// reflected (this returns unscaled base metrics, same as
    /// `Theme::default()`) should call `set_ui_scale` right after — see
    /// `App::apply_color_scheme`.
    pub fn for_scheme(scheme: ColorScheme) -> Self {
        // The six plain recolors are just the base dark chrome with a
        // different accent (these used to be the standalone "Accent
        // Color" swatches); everything else brings its own full palette.
        let rgb = match scheme {
            ColorScheme::BasicBlue => [0x3b, 0x9b, 0xff],
            ColorScheme::Amalith => [0xf4, 0xbe, 0x18],
            ColorScheme::Emerald => [0x4c, 0xb7, 0x6b],
            ColorScheme::RoseRed => [0xe0, 0x50, 0x50],
            ColorScheme::PerfectPurple => [0x9b, 0x6c, 0xf0],
            ColorScheme::Graphite => [0x9a, 0x9a, 0x9a],
            _ => return Self::from_palette(scheme.palette()),
        };
        let mut t = Self::default();
        t.set_accent(Color::from_rgb8(rgb[0], rgb[1], rgb[2]));
        t
    }

    /// Maps `p` onto every `Theme` color token, then derives the
    /// accent-dependent ones via `set_accent`. Metric fields come from
    /// `Theme::default()`, same as every other constructor here.
    fn from_palette(p: Palette) -> Self {
        let mut t = Self {
            bg: p.bg,
            app_bar: p.app_bar,
            canvas_bg: p.canvas_bg,
            pasteboard: p.pasteboard,
            panel_bg: p.bg,
            strip_bg: p.app_bar,
            strip_active: p.bg,
            border: p.border,
            splitter: p.pasteboard,
            // Overwritten by `set_accent` below.
            drop_fill: Color::TRANSPARENT,
            drop_line: Color::TRANSPARENT,
            accent: p.accent,
            on_accent: Color::WHITE,
            marquee_fill: Color::TRANSPARENT,
            artboard_border: p.border,
            artboard_label: p.text,
            text: p.text,
            text_dim: p.text_dim,
            symbol_accent: p.symbol_accent,
            raster_accent: p.raster_accent,
            ..Self::default()
        };
        t.set_accent(t.accent);
        t
    }
}

impl ColorScheme {
    /// The full-repaint schemes' real palettes, pulled from each
    /// theme's actual background/foreground/ANSI colors (via
    /// terminalcolors.com's Alacritty export) rather than approximated.
    /// Not called for the six plain accent recolors — see
    /// `Theme::for_scheme`.
    fn palette(self) -> Palette {
        match self {
            // https://draculatheme.com
            ColorScheme::Dracula => Palette {
                bg: Color::from_rgb8(0x28, 0x2a, 0x36),
                app_bar: Color::from_rgb8(0x21, 0x22, 0x2c),
                canvas_bg: Color::from_rgb8(0x1e, 0x1f, 0x29),
                pasteboard: Color::from_rgb8(0x44, 0x47, 0x5a), // Current Line
                border: Color::from_rgb8(0x1b, 0x1c, 0x25),
                accent: Color::from_rgb8(0xbd, 0x93, 0xf9), // Purple
                text: Color::from_rgb8(0xf8, 0xf8, 0xf2), // Foreground
                text_dim: Color::from_rgb8(0x62, 0x72, 0xa4), // Comment
                symbol_accent: Color::from_rgb8(0xff, 0x79, 0xc6), // Pink
                raster_accent: Color::from_rgb8(0xff, 0xb8, 0x6c), // Orange
            },
            ColorScheme::CatppuccinMocha => Palette {
                bg: Color::from_rgb8(0x1e, 0x1e, 0x2e),
                app_bar: Color::from_rgb8(0x17, 0x17, 0x27),
                canvas_bg: Color::from_rgb8(0x14, 0x14, 0x24),
                pasteboard: Color::from_rgb8(0x35, 0x37, 0x48), // selection bg
                border: Color::from_rgb8(0x11, 0x11, 0x21),
                accent: Color::from_rgb8(0x89, 0xb4, 0xfa), // blue
                text: Color::from_rgb8(0xcd, 0xd6, 0xf4),
                text_dim: Color::from_rgb8(0x58, 0x5b, 0x70), // bright black
                symbol_accent: Color::from_rgb8(0xf5, 0xc2, 0xe7), // pink
                raster_accent: Color::from_rgb8(0xfa, 0xb3, 0x87), // peach
            },
            ColorScheme::Panda => Palette {
                bg: Color::from_rgb8(0x29, 0x2a, 0x2b),
                app_bar: Color::from_rgb8(0x22, 0x23, 0x24),
                canvas_bg: Color::from_rgb8(0x1f, 0x20, 0x21),
                pasteboard: Color::from_rgb8(0x5f, 0x4e, 0x3b), // selection bg
                border: Color::from_rgb8(0x1c, 0x1d, 0x1e),
                accent: Color::from_rgb8(0xff, 0x75, 0xb5), // pink
                text: Color::from_rgb8(0xcc, 0xcc, 0xcc),
                text_dim: Color::from_rgb8(0x75, 0x75, 0x75), // bright black
                symbol_accent: Color::from_rgb8(0x19, 0xf9, 0xd8), // cyan/green
                raster_accent: Color::from_rgb8(0xff, 0xb8, 0x6c), // orange
            },
            ColorScheme::TokyoNight => Palette {
                bg: Color::from_rgb8(0x1a, 0x1b, 0x26),
                app_bar: Color::from_rgb8(0x13, 0x14, 0x1f),
                canvas_bg: Color::from_rgb8(0x10, 0x11, 0x1c),
                pasteboard: Color::from_rgb8(0x28, 0x34, 0x57), // selection bg
                border: Color::from_rgb8(0x0d, 0x0e, 0x19),
                accent: Color::from_rgb8(0x7a, 0xa2, 0xf7), // blue
                text: Color::from_rgb8(0xc0, 0xca, 0xf5),
                text_dim: Color::from_rgb8(0x41, 0x48, 0x68), // bright black
                symbol_accent: Color::from_rgb8(0xbb, 0x9a, 0xf7), // magenta
                raster_accent: Color::from_rgb8(0xff, 0x9e, 0x64), // orange
            },
            ColorScheme::SolarizedDark => Palette {
                bg: Color::from_rgb8(0x00, 0x2b, 0x36),
                app_bar: Color::from_rgb8(0x00, 0x24, 0x2f),
                canvas_bg: Color::from_rgb8(0x00, 0x21, 0x2c),
                pasteboard: Color::from_rgb8(0x07, 0x36, 0x42), // base02
                border: Color::from_rgb8(0x00, 0x1e, 0x29),
                accent: Color::from_rgb8(0x26, 0x8b, 0xd2), // blue
                text: Color::from_rgb8(0x83, 0x94, 0x96),
                text_dim: Color::from_rgb8(0x58, 0x6e, 0x75), // base01
                symbol_accent: Color::from_rgb8(0xd3, 0x36, 0x82), // magenta
                raster_accent: Color::from_rgb8(0xcb, 0x4b, 0x16), // orange
            },
            ColorScheme::Cobalt2 => Palette {
                bg: Color::from_rgb8(0x12, 0x27, 0x38),
                app_bar: Color::from_rgb8(0x0b, 0x20, 0x31),
                canvas_bg: Color::from_rgb8(0x08, 0x1d, 0x2e),
                pasteboard: Color::from_rgb8(0x00, 0x50, 0xa4), // selection bg
                border: Color::from_rgb8(0x05, 0x1a, 0x2b),
                accent: Color::from_rgb8(0x00, 0x88, 0xff), // blue
                text: Color::from_rgb8(0xff, 0xff, 0xff),
                text_dim: Color::from_rgb8(0x7d, 0x88, 0x92), // muted blue-gray
                symbol_accent: Color::from_rgb8(0xfb, 0x94, 0xff), // magenta
                raster_accent: Color::from_rgb8(0xff, 0xc6, 0x00), // gold
            },
            ColorScheme::GruvboxDark => Palette {
                bg: Color::from_rgb8(0x28, 0x28, 0x28),
                app_bar: Color::from_rgb8(0x21, 0x21, 0x21),
                canvas_bg: Color::from_rgb8(0x1e, 0x1e, 0x1e),
                pasteboard: Color::from_rgb8(0x92, 0x83, 0x74), // bright black / gray
                border: Color::from_rgb8(0x1b, 0x1b, 0x1b),
                accent: Color::from_rgb8(0xfa, 0xbd, 0x2f), // bright yellow
                text: Color::from_rgb8(0xeb, 0xdb, 0xb2),
                text_dim: Color::from_rgb8(0x92, 0x83, 0x74), // bright black / gray
                symbol_accent: Color::from_rgb8(0xfb, 0x49, 0x34), // bright red
                raster_accent: Color::from_rgb8(0xfe, 0x80, 0x19), // bright orange
            },
            ColorScheme::Gotham => Palette {
                bg: Color::from_rgb8(0x0c, 0x10, 0x14),
                app_bar: Color::from_rgb8(0x05, 0x09, 0x0d),
                canvas_bg: Color::from_rgb8(0x02, 0x06, 0x0a),
                pasteboard: Color::from_rgb8(0x0a, 0x37, 0x49), // selection bg
                border: Color::from_rgb8(0x00, 0x03, 0x07),
                accent: Color::from_rgb8(0x33, 0x85, 0x9e), // cyan
                text: Color::from_rgb8(0x99, 0xd1, 0xce),
                text_dim: Color::from_rgb8(0x4e, 0x51, 0x66), // slate
                symbol_accent: Color::from_rgb8(0xc2, 0x31, 0x27), // red
                raster_accent: Color::from_rgb8(0xd9, 0x82, 0x2b), // orange
            },
            ColorScheme::Nordic => Palette {
                bg: Color::from_rgb8(0x24, 0x29, 0x33),
                app_bar: Color::from_rgb8(0x1d, 0x22, 0x2c),
                canvas_bg: Color::from_rgb8(0x1a, 0x1f, 0x29),
                pasteboard: Color::from_rgb8(0x3b, 0x42, 0x52), // Polar Night
                border: Color::from_rgb8(0x17, 0x1c, 0x26),
                accent: Color::from_rgb8(0x88, 0xc0, 0xd0), // frost cyan
                text: Color::from_rgb8(0xbb, 0xc3, 0xd4),
                text_dim: Color::from_rgb8(0x3b, 0x42, 0x52), // Polar Night
                symbol_accent: Color::from_rgb8(0xb4, 0x8e, 0xad), // magenta
                raster_accent: Color::from_rgb8(0xd0, 0x87, 0x70), // Aurora orange
            },
            ColorScheme::C64 => Palette {
                bg: Color::from_rgb8(0x40, 0x31, 0x8d),
                app_bar: Color::from_rgb8(0x39, 0x2a, 0x86),
                canvas_bg: Color::from_rgb8(0x36, 0x27, 0x83),
                pasteboard: Color::from_rgb8(0x78, 0x69, 0xc4), // selection
                border: Color::from_rgb8(0x33, 0x24, 0x80),
                accent: Color::from_rgb8(0x67, 0xb6, 0xbd), // cyan
                // Terminal fg (#7869c4) is the C64 light-blue — too close
                // to bg for chrome labels, so ink uses bright white and
                // the real fg becomes dim text.
                text: Color::from_rgb8(0xf7, 0xf7, 0xf7),
                text_dim: Color::from_rgb8(0x78, 0x69, 0xc4),
                symbol_accent: Color::from_rgb8(0x98, 0x4c, 0xa3), // purple
                raster_accent: Color::from_rgb8(0xe0, 0xa6, 0x3a), // gold
            },
            ColorScheme::Batman => Palette {
                bg: Color::from_rgb8(0x1b, 0x1d, 0x1e),
                app_bar: Color::from_rgb8(0x14, 0x16, 0x17),
                canvas_bg: Color::from_rgb8(0x11, 0x13, 0x14),
                pasteboard: Color::from_rgb8(0x4d, 0x50, 0x4c), // selection
                border: Color::from_rgb8(0x0e, 0x10, 0x11),
                accent: Color::from_rgb8(0xfc, 0xef, 0x0c), // cursor yellow
                // Terminal fg (#6f6f6f) is a mid-gray — fine as dim
                // chrome, too dim as primary ink, so labels use ANSI white.
                text: Color::from_rgb8(0xc6, 0xc5, 0xbf),
                text_dim: Color::from_rgb8(0x6f, 0x6f, 0x6f),
                symbol_accent: Color::from_rgb8(0xe6, 0xdc, 0x44), // gold
                raster_accent: Color::from_rgb8(0xd9, 0x79, 0x04), // orange
            },
            ColorScheme::AcidLime => Palette {
                bg: Color::from_rgb8(0x08, 0x0c, 0x05),
                app_bar: Color::from_rgb8(0x01, 0x05, 0x00),
                canvas_bg: Color::from_rgb8(0x00, 0x02, 0x00),
                pasteboard: Color::from_rgb8(0x1b, 0x2a, 0x10), // selection
                border: Color::from_rgb8(0x00, 0x00, 0x00),
                accent: Color::from_rgb8(0xc2, 0xff, 0x33), // cursor lime
                text: Color::from_rgb8(0xd4, 0xef, 0xbc),
                text_dim: Color::from_rgb8(0x4a, 0x6b, 0x36), // bright black
                symbol_accent: Color::from_rgb8(0xff, 0x33, 0x44), // red
                raster_accent: Color::from_rgb8(0xff, 0x9f, 0x1c), // orange
            },
            // The six plain recolors never reach here — see `Theme::for_scheme`.
            ColorScheme::BasicBlue
            | ColorScheme::Amalith
            | ColorScheme::Emerald
            | ColorScheme::RoseRed
            | ColorScheme::PerfectPurple
            | ColorScheme::Graphite => unreachable!("plain recolors don't use Theme::from_palette"),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            bg: Color::from_rgb8(0x1e, 0x1e, 0x1e),
            app_bar: Color::from_rgb8(0x30, 0x30, 0x30),
            canvas_bg: Color::from_rgb8(0x33, 0x33, 0x33),
            pasteboard: Color::from_rgb8(0x5b, 0x5b, 0x5b),
            panel_bg: Color::from_rgb8(0x2b, 0x2b, 0x2b),
            strip_bg: Color::from_rgb8(0x24, 0x24, 0x24),
            strip_active: Color::from_rgb8(0x2b, 0x2b, 0x2b),
            border: Color::from_rgb8(0x15, 0x15, 0x15),
            // Light enough to read as a grabbable groove against panel_bg.
            splitter: Color::from_rgb8(0x3d, 0x3d, 0x3d),
            drop_fill: Color::from_rgb8(0x1d, 0x7a, 0xf0).with_alpha(0.20),
            drop_line: Color::from_rgb8(0x1d, 0x7a, 0xf0),
            accent: Color::from_rgb8(0x3b, 0x9b, 0xff),
            on_accent: Color::WHITE,
            marquee_fill: Color::from_rgb8(0x3b, 0x9b, 0xff).with_alpha(0.12),
            artboard_border: Color::from_rgb8(0x23, 0x23, 0x23),
            artboard_label: Color::from_rgb8(0xe1, 0xe1, 0xe1),
            text: Color::from_rgb8(0xd0, 0xd0, 0xd0),
            text_dim: Color::from_rgb8(0x8a, 0x8a, 0x8a),
            symbol_accent: Color::from_rgb8(0xa0, 0x6c, 0xf5),
            raster_accent: Color::from_rgb8(0xff, 0x9f, 0x1c),

            tab_strip_h: 27.3,
            group_title_h: 20.0,
            splitter_thickness: 6.0,
            tab_pad_x: 12.6,
            panel_menu_w: 26.0,
            group_close_w: 22.0,
            panel_collapse_w: 22.0,
        }
    }
}
