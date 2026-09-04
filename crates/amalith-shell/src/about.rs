//! The "About Amalith" panel.
//!
//! A card centred over the main window (like Illustrator's About box): a black
//! left panel — wordmark, body copy, and the `Github` / `Credits` links along
//! the bottom — beside the full-bleed illustration on the right. A dimmed
//! backdrop covers the rest of the window. Rendered with vello + parley like
//! the rest of the chrome, so it works the same on every platform.
//!
//! The body text is real, laid-out text: it can be selected with the pointer
//! and copied (⌘/Ctrl+C), and the links are hit-tested rectangles. `Github`
//! opens the repo in the browser; `Credits` flips the panel between the
//! trademark notice and the dedication.

use vello::kurbo::{Affine, Rect, RoundedRect, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

use parley::{Layout, Selection};
use vello::peniko::Brush;

use crate::text::TextContext;

/// Right-side illustration (pre-cropped to the panel's aspect).
const ART_PNG: &[u8] = include_bytes!("../assets/about/art.png");
/// The Amalith wordmark, white on transparent.
const LOGO_PNG: &[u8] = include_bytes!("../assets/about/wordmark.png");

/// Where `Github` points.
const GITHUB_URL: &str = "https://github.com/tonykastaneda/Amalith";

const ABOUT_BODY: &str = "\
Development Build

© 2026 Amalith Contributors

Amalith, the Amalith name, and the Amalith logo are trademarks of their \
respective owners. All other trademarks are the property of their respective \
owners.

Amalith includes open-source software developed by third parties. Those \
components remain subject to their respective copyright notices and license \
terms.

Third-party licenses and notices are available in the application's Legal \
Notices section.

This software is provided under the terms of the Amalith software license.";

const CREDITS_BODY: &str = "\
Amalith is dedicated to, Amanda Isabel Maldonado —my greatest source of love, \
strength, and inspiration. Every part of this app carries something of the \
life we have built together. Without her patience, belief, and unwavering \
support, Amalith would not exist.";

/// Card size in logical points.
pub const WIDTH: f64 = 900.0;
pub const HEIGHT: f64 = 633.0;

const CORNER_RADIUS: f64 = 12.0;
/// Left (black) panel width as a fraction of the card.
const SPLIT: f64 = 0.39;
const PAD_X: f64 = 40.0;
const LOGO_Y: f64 = 40.0;
const LOGO_W: f64 = 196.0;
const BODY_TOP: f64 = 128.0;
const BODY_SIZE: f32 = 13.0;
const LINE_H: f32 = 1.5;
const LINK_SIZE: f32 = 13.0;
const LINK_GAP: f64 = 22.0;
const WRAP_W: f32 = (WIDTH * SPLIT) as f32 - PAD_X as f32 - 24.0;

const SCRIM: Color = Color::from_rgba8(0, 0, 0, 140);
const BG: Color = Color::from_rgb8(10, 10, 12);
const INK: Color = Color::from_rgb8(232, 232, 232);
const LINK: Color = Color::WHITE;
const SEL: Color = Color::from_rgb8(58, 105, 170);

/// What a press landed on.
pub enum Hit {
    /// Outside the card — dismiss.
    Backdrop,
    /// Blank card area — do nothing.
    None,
    /// The `Github` link.
    Github,
    /// The `Credits` / `About` toggle link.
    Toggle,
    /// Inside the body text — a selection drag has begun.
    Text,
}

fn decode(bytes: &[u8]) -> Option<ImageData> {
    let (rgba, w, h) = crate::appicon::decode_png(bytes)?;
    Some(ImageData {
        data: Blob::from(rgba),
        format: ImageFormat::Rgba8,
        alpha_type: ImageAlphaType::Alpha,
        width: w,
        height: h,
    })
}

pub struct About {
    art: ImageData,
    logo: ImageData,
    /// `false` = trademark notice, `true` = dedication.
    credits: bool,
    sel: Option<Selection>,
    dragging: bool,
    // All in window-logical coordinates, refreshed each paint.
    origin: Vec2,
    text_origin: Vec2,
    hit_github: Rect,
    hit_toggle: Rect,
}

impl About {
    /// Load the embedded art. `None` if a PNG is missing or malformed.
    pub fn load() -> Option<Self> {
        Some(Self {
            art: decode(ART_PNG)?,
            logo: decode(LOGO_PNG)?,
            credits: false,
            sel: None,
            dragging: false,
            origin: Vec2::ZERO,
            text_origin: Vec2::ZERO,
            hit_github: Rect::ZERO,
            hit_toggle: Rect::ZERO,
        })
    }

    fn body(&self) -> &'static str {
        if self.credits {
            CREDITS_BODY
        } else {
            ABOUT_BODY
        }
    }

    fn body_layout(&self, tcx: &mut TextContext) -> Layout<Brush> {
        tcx.wrap(self.body(), BODY_SIZE, INK, WRAP_W, LINE_H)
    }

    /// The card rectangle, in window coordinates.
    pub fn card_rect(&self) -> Rect {
        Rect::from_origin_size(self.origin.to_point(), (WIDTH, HEIGHT))
    }

    /// Pointer position in the body text's local space.
    fn text_local(&self, p: Vec2) -> (f32, f32) {
        (
            (p.x - self.text_origin.x) as f32,
            (p.y - self.text_origin.y) as f32,
        )
    }

    /// Handle a left press at `p` (window coordinates). Begins a selection
    /// drag when it lands in the body text.
    pub fn on_press(&mut self, tcx: &mut TextContext, p: Vec2) -> Hit {
        if !self.card_rect().contains(p.to_point()) {
            return Hit::Backdrop;
        }
        if self.hit_github.contains(p.to_point()) {
            return Hit::Github;
        }
        if self.hit_toggle.contains(p.to_point()) {
            return Hit::Toggle;
        }
        let layout = self.body_layout(tcx);
        let (lx, ly) = self.text_local(p);
        if lx >= 0.0 && lx <= layout.width() && ly >= 0.0 && ly <= layout.height() {
            self.sel = Some(Selection::from_point(&layout, lx, ly));
            self.dragging = true;
            return Hit::Text;
        }
        self.sel = None;
        Hit::None
    }

    /// Extend the live selection to `p` (window coordinates).
    pub fn on_drag(&mut self, tcx: &mut TextContext, p: Vec2) {
        if !self.dragging {
            return;
        }
        if let Some(sel) = self.sel {
            let layout = self.body_layout(tcx);
            let (lx, ly) = self.text_local(p);
            self.sel = Some(sel.extend_to_point(&layout, lx, ly));
        }
    }

    pub fn on_release(&mut self) {
        self.dragging = false;
    }

    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Flip between the trademark notice and the dedication.
    pub fn toggle(&mut self) {
        self.credits = !self.credits;
        self.sel = None;
    }

    pub fn github_url(&self) -> &'static str {
        GITHUB_URL
    }

    /// The currently selected text, if any.
    pub fn selected_text(&self) -> Option<String> {
        let sel = self.sel?;
        let range = sel.text_range();
        if range.is_empty() {
            return None;
        }
        self.body().get(range).map(str::to_owned)
    }

    /// Paint the dimmed backdrop and the centred card into `scene`. `wl`/`hl`
    /// are the window's logical size. Refreshes the hit rectangles.
    pub fn paint(&mut self, scene: &mut Scene, tcx: &mut TextContext, wl: f64, hl: f64) {
        // Backdrop over the whole window.
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            SCRIM,
            None,
            &Rect::new(0.0, 0.0, wl, hl),
        );

        let ox = ((wl - WIDTH) / 2.0).round().max(0.0);
        let oy = ((hl - HEIGHT) / 2.0).round().max(0.0);
        self.origin = Vec2::new(ox, oy);
        let o = Affine::translate((ox, oy));

        let card = RoundedRect::new(ox, oy, ox + WIDTH, oy + HEIGHT, CORNER_RADIUS);
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &card);

        // Black card, then the illustration over the right panel.
        scene.fill(Fill::NonZero, o, BG, None, &Rect::new(0.0, 0.0, WIDTH, HEIGHT));
        let px = (WIDTH * SPLIT).round();
        scene.draw_image(
            &self.art,
            o * Affine::translate((px, 0.0))
                * Affine::scale_non_uniform(
                    (WIDTH - px) / self.art.width as f64,
                    HEIGHT / self.art.height as f64,
                ),
        );

        // Wordmark.
        let logo_h = LOGO_W * self.logo.height as f64 / self.logo.width as f64;
        scene.draw_image(
            &self.logo,
            o * Affine::translate((PAD_X, LOGO_Y))
                * Affine::scale_non_uniform(
                    LOGO_W / self.logo.width as f64,
                    logo_h / self.logo.height as f64,
                ),
        );

        // Body text, selection highlight under it.
        let layout = self.body_layout(tcx);
        self.text_origin = self.origin + Vec2::new(PAD_X, BODY_TOP);
        if let Some(sel) = self.sel {
            for (bb, _) in sel.geometry(&layout) {
                scene.fill(
                    Fill::NonZero,
                    Affine::translate((self.text_origin.x, self.text_origin.y)),
                    SEL,
                    None,
                    &Rect::new(bb.x0, bb.y0, bb.x1, bb.y1),
                );
            }
        }
        tcx.draw_layout(scene, &layout, INK, self.text_origin.x, self.text_origin.y);

        // Links along the bottom.
        let base = oy + HEIGHT - 42.0;
        self.hit_github = draw_link(scene, tcx, "Github", ox + PAD_X, base);
        let tx = self.hit_github.x1 + LINK_GAP;
        let label = if self.credits { "About" } else { "Credits" };
        self.hit_toggle = draw_link(scene, tcx, label, tx, base);

        scene.pop_layer();
    }
}

/// Draw an underlined link and return its (padded) hit rectangle.
fn draw_link(scene: &mut Scene, tcx: &mut TextContext, label: &str, x: f64, baseline: f64) -> Rect {
    let w = tcx.measure(label, LINK_SIZE);
    tcx.draw(scene, label, LINK_SIZE, LINK, x, baseline);
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        LINK,
        None,
        &Rect::new(x, baseline + 3.0, x + w, baseline + 4.0),
    );
    Rect::new(x - 4.0, baseline - LINK_SIZE as f64, x + w + 4.0, baseline + 6.0)
}

/// Open `url` in the platform's default browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", "", url]);
        c
    };
    let _ = cmd.spawn();
}
