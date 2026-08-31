//! Colors and metrics for the shell's chrome. One dark theme for now; the
//! struct is the seam for adding more later.

use vello::peniko::Color;

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
    /// drop indicator, focus rings. Amalith gold.
    pub accent: Color,
    /// Faint fill inside a live marquee.
    pub marquee_fill: Color,
    /// Artboard hairline and its name label.
    pub artboard_border: Color,
    pub artboard_label: Color,
    pub text: Color,
    pub text_dim: Color,

    /// Height of a tab strip.
    pub tab_strip_h: f64,
    /// Thickness of the gap between split children.
    pub splitter_thickness: f64,
    /// Horizontal padding inside a tab, per side.
    pub tab_pad_x: f64,
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
            drop_fill: Color::from_rgb8(0xf4, 0xbe, 0x18).with_alpha(0.20),
            drop_line: Color::from_rgb8(0xf4, 0xbe, 0x18),
            accent: Color::from_rgb8(0xf4, 0xbe, 0x18),
            marquee_fill: Color::from_rgb8(0xf4, 0xbe, 0x18).with_alpha(0.12),
            artboard_border: Color::from_rgb8(0x23, 0x23, 0x23),
            artboard_label: Color::from_rgb8(0xe1, 0xe1, 0xe1),
            text: Color::from_rgb8(0xd0, 0xd0, 0xd0),
            text_dim: Color::from_rgb8(0x8a, 0x8a, 0x8a),

            tab_strip_h: 27.3,
            splitter_thickness: 6.0,
            tab_pad_x: 12.6,
        }
    }
}
