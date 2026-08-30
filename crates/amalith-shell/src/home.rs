//! The Home / Welcome screen.
//!
//! Shown on launch and whenever the last document tab is closed: a left panel
//! with the app mark, the welcome wordmark, and two external links (tutorials
//! on YouTube, the repo on GitHub), plus a grid on the right — a "New
//! Document" tile followed by one tile per recently-opened file.
//!
//! Rendered with vello + parley like the rest of the chrome. It's a full-window
//! surface: while it's up, the canvas underneath takes no input. Artwork comes
//! from `branding/NewDocument/` (SVGs rasterised to PNG at build prep time).

use std::path::{Path, PathBuf};

use vello::kurbo::{Affine, Rect, RoundedRect, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

use crate::text::TextContext;

const MARK_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/mark.png");
const WELCOME_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/welcome.png");
const YOUTUBE_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/youtube.png");
const GITHUB_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/github.png");
const TILE_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/newdoc-tile.png");

/// A blanket YouTube search, per the brief.
pub const YOUTUBE_URL: &str =
    "https://www.youtube.com/results?search_query=Illustrator+Tutorial";
pub const GITHUB_URL: &str = "https://github.com/tonykastaneda/Amalith";

const SPLIT: f64 = 0.39;
const PAD: f64 = 56.0;
const MARK_SIZE: f64 = 148.0;
const BADGE: f64 = 46.0;

const BG_LEFT: Color = Color::from_rgb8(27, 27, 29);
const BG_RIGHT: Color = Color::from_rgb8(17, 17, 19);
const INK: Color = Color::from_rgb8(238, 238, 240);
const DIM: Color = Color::from_rgb8(138, 138, 144);
const DIVIDER: Color = Color::from_rgb8(48, 48, 51);
const TILE_RECENT: Color = Color::from_rgb8(46, 46, 48);
const SEL: Color = Color::from_rgb8(59, 111, 214);

/// What a press on the Home screen landed on.
pub enum Hit {
    None,
    NewDocument,
    Recent(usize),
    Youtube,
    Github,
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

/// Scale + place `img` so it fills `dst` (dst must match the image's aspect
/// closely, or it will stretch).
fn image_into(scene: &mut Scene, img: &ImageData, dst: Rect) {
    let xf = Affine::translate((dst.x0, dst.y0))
        * Affine::scale_non_uniform(
            dst.width() / img.width as f64,
            dst.height() / img.height as f64,
        );
    scene.draw_image(img, xf);
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".into())
}

pub struct Home {
    mark: ImageData,
    welcome: ImageData,
    youtube: ImageData,
    github: ImageData,
    tile: ImageData,
    /// (path, display name), most-recent first.
    recents: Vec<(PathBuf, String)>,
    // Hit rectangles in window coordinates, refreshed each paint.
    hit_new: Rect,
    hit_recents: Vec<Rect>,
    hit_youtube: Rect,
    hit_github: Rect,
}

impl Home {
    pub fn new(recents: Vec<PathBuf>) -> Option<Self> {
        Some(Self {
            mark: decode(MARK_PNG)?,
            welcome: decode(WELCOME_PNG)?,
            youtube: decode(YOUTUBE_PNG)?,
            github: decode(GITHUB_PNG)?,
            tile: decode(TILE_PNG)?,
            recents: recents
                .into_iter()
                .map(|p| {
                    let n = display_name(&p);
                    (p, n)
                })
                .collect(),
            hit_new: Rect::ZERO,
            hit_recents: Vec::new(),
            hit_youtube: Rect::ZERO,
            hit_github: Rect::ZERO,
        })
    }

    pub fn recent_path(&self, i: usize) -> Option<&Path> {
        self.recents.get(i).map(|(p, _)| p.as_path())
    }

    pub fn on_press(&self, p: Vec2) -> Hit {
        let pt = p.to_point();
        if self.hit_new.contains(pt) {
            return Hit::NewDocument;
        }
        if self.hit_youtube.contains(pt) {
            return Hit::Youtube;
        }
        if self.hit_github.contains(pt) {
            return Hit::Github;
        }
        for (i, r) in self.hit_recents.iter().enumerate() {
            if r.contains(pt) {
                return Hit::Recent(i);
            }
        }
        Hit::None
    }

    pub fn paint(&mut self, scene: &mut Scene, tcx: &mut TextContext, wl: f64, hl: f64) {
        let split = (wl * SPLIT).round();
        scene.fill(Fill::NonZero, Affine::IDENTITY, BG_RIGHT, None, &Rect::new(0.0, 0.0, wl, hl));
        scene.fill(Fill::NonZero, Affine::IDENTITY, BG_LEFT, None, &Rect::new(0.0, 0.0, split, hl));
        self.paint_left(scene, tcx, split);
        self.paint_grid(scene, tcx, split, wl, hl);
    }

    fn paint_left(&mut self, scene: &mut Scene, tcx: &mut TextContext, split: f64) {
        // The header block (mark, wordmark, version) is centred in the panel.
        let cx = split / 2.0;

        let mark_y = 52.0;
        image_into(
            scene,
            &self.mark,
            Rect::from_origin_size((cx - MARK_SIZE / 2.0, mark_y), (MARK_SIZE, MARK_SIZE)),
        );

        // "Welcome to Amalith" wordmark.
        let wm_top = mark_y + MARK_SIZE + 46.0;
        let wm_w = (split - PAD * 2.0).clamp(220.0, 430.0);
        let wm_h = wm_w * self.welcome.height as f64 / self.welcome.width as f64;
        image_into(
            scene,
            &self.welcome,
            Rect::from_origin_size((cx - wm_w / 2.0, wm_top), (wm_w, wm_h)),
        );

        let ver = "Ver. Alpha";
        let ver_w = tcx.measure(ver, 15.0);
        let ver_baseline = wm_top + wm_h + 30.0;
        tcx.draw(scene, ver, 15.0, DIM, cx - ver_w / 2.0, ver_baseline);

        // Divider sits a gap below the header equal to the gap above the mark,
        // so the header block reads as evenly inset.
        let dy = (ver_baseline + 8.0 + mark_y).round();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            DIVIDER,
            None,
            &Rect::new(PAD, dy, split - PAD, dy + 1.0),
        );

        // Link rows.
        let y0 = dy + 46.0;
        self.hit_youtube = link_row(
            scene,
            tcx,
            &self.youtube,
            y0,
            split,
            "New to Amalith?",
            "20+ Years of Tutorials Ready at Launch",
        );
        self.hit_github = link_row(
            scene,
            tcx,
            &self.github,
            y0 + BADGE + 32.0,
            split,
            "Github",
            "Changelogs and New Releases",
        );
    }

    fn paint_grid(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        split: f64,
        wl: f64,
        hl: f64,
    ) {
        let area_x = split + 92.0;
        let area_top = 92.0;
        let area_w = (wl - area_x - 56.0).max(240.0);

        let cols = 3usize;
        let gap = 30.0;
        let tile = ((area_w - gap * (cols as f64 - 1.0)) / cols as f64).clamp(130.0, 215.0);
        let label_gap = 12.0;
        let cell_h = tile + label_gap + 24.0;

        self.hit_recents.clear();
        let total = 1 + self.recents.len();
        for idx in 0..total {
            let col = idx % cols;
            let row = idx / cols;
            let x = area_x + col as f64 * (tile + gap);
            let y = area_top + row as f64 * (cell_h + gap);
            if y + tile > hl - 20.0 {
                break;
            }
            let tile_rect = Rect::from_origin_size((x, y), (tile, tile));

            if idx == 0 {
                image_into(scene, &self.tile, tile_rect);
                self.hit_new = tile_rect;
                label(scene, tcx, "New Document", tile_rect, label_gap, true);
            } else {
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    TILE_RECENT,
                    None,
                    &RoundedRect::from_rect(tile_rect, 18.0),
                );
                let name = self.recents[idx - 1].1.clone();
                label(scene, tcx, &name, tile_rect, label_gap, false);
                self.hit_recents.push(tile_rect);
            }
        }
    }
}

fn link_row(
    scene: &mut Scene,
    tcx: &mut TextContext,
    badge: &ImageData,
    y: f64,
    split: f64,
    title: &str,
    sub: &str,
) -> Rect {
    image_into(
        scene,
        badge,
        Rect::from_origin_size((PAD, y), (BADGE, BADGE)),
    );
    let tx = PAD + BADGE + 18.0;
    tcx.draw(scene, title, 15.0, INK, tx, y + 20.0);
    tcx.draw(scene, sub, 12.0, DIM, tx, y + 39.0);
    Rect::new(PAD - 6.0, y - 6.0, split - PAD, y + BADGE + 6.0)
}

/// Centred caption under a tile. `selected` draws the blue highlight pill.
fn label(scene: &mut Scene, tcx: &mut TextContext, s: &str, tile: Rect, gap: f64, selected: bool) {
    let w = tcx.measure(s, 14.0);
    let cx = tile.x0 + tile.width() / 2.0;
    let baseline = tile.y1 + gap + 14.0;
    if selected {
        let pill = Rect::new(cx - w / 2.0 - 8.0, baseline - 15.0, cx + w / 2.0 + 8.0, baseline + 5.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            SEL,
            None,
            &RoundedRect::from_rect(pill, 4.0),
        );
    }
    tcx.draw(scene, s, 14.0, INK, cx - w / 2.0, baseline);
}
