//! The Home / Welcome screen.
//!
//! Shown on launch and whenever the last document tab is closed: a left panel
//! with the app mark, the welcome wordmark, and a short stack of external
//! links (News, Docs, GitHub), plus a three-column grid on the right — a
//! "New Document" tile, recent files, and dark-grey placeholders, with a
//! scrollbar when the grid overflows.
//!
//! The YouTube tutorials link is kept in the code but hidden for now (see
//! `Badge::Youtube` / `hit_youtube`) — bring it back once the app is closer
//! to feature-complete.
//!
//! Rendered with vello + parley like the rest of the chrome. It's a full-window
//! surface: while it's up, the canvas underneath takes no input. Artwork comes
//! from `branding/NewDocument/` (SVGs rasterised to PNG at build prep time).

use std::path::{Path, PathBuf};

use vello::kurbo::{Affine, BezPath, Rect, RoundedRect, Stroke, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const MARK_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/mark.png");
const WELCOME_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/welcome.png");
const YOUTUBE_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/youtube.png");
const GITHUB_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/github.png");
const TILE_PNG: &[u8] = include_bytes!("../../../branding/NewDocument/newdoc-tile.png");

/// A blanket YouTube search, per the brief. Hidden for now — kept so the
/// link can be restored without redoing the wiring.
#[allow(dead_code)]
pub const YOUTUBE_URL: &str =
    "https://www.youtube.com/results?search_query=Illustrator+Tutorial";
pub const GITHUB_URL: &str = "https://github.com/tonykastaneda/Amalith";
pub const NEWS_URL: &str = "https://amalith.app/news";
pub const DOCS_URL: &str = "https://amalith.app/docs";

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
const TILE_PLACEHOLDER: Color = Color::from_rgb8(38, 38, 40);
const SCROLL_THUMB: Color = Color::from_rgb8(90, 90, 94);
/// Always fill at least this many cells (New Document + recents + blanks).
const MIN_SLOTS: usize = 9;
const COLS: usize = 3;

/// What a press on the Home screen landed on.
pub enum Hit {
    None,
    NewDocument,
    Recent(usize),
    /// Hidden for now; kept so the arm in `press.rs` still compiles.
    #[allow(dead_code)]
    Youtube,
    News,
    Docs,
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
    /// Decoded but not painted right now — see the module docs.
    #[allow(dead_code)]
    youtube: ImageData,
    github: ImageData,
    tile: ImageData,
    /// (path, display name), most-recent first.
    recents: Vec<(PathBuf, String)>,
    /// Vertical scroll of the document grid, in px.
    scroll: f64,
    /// Last paint's max scroll; wheel clamping uses this.
    max_scroll: f64,
    // Hit rectangles in window coordinates, refreshed each paint.
    hit_new: Rect,
    hit_recents: Vec<Rect>,
    /// Stays `Rect::ZERO` while the YouTube row is hidden.
    hit_youtube: Rect,
    hit_news: Rect,
    hit_docs: Rect,
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
            scroll: 0.0,
            max_scroll: 0.0,
            hit_new: Rect::ZERO,
            hit_recents: Vec::new(),
            hit_youtube: Rect::ZERO,
            hit_news: Rect::ZERO,
            hit_docs: Rect::ZERO,
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
        if self.hit_news.contains(pt) {
            return Hit::News;
        }
        if self.hit_docs.contains(pt) {
            return Hit::Docs;
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

    /// Wheel over the document grid. `dy` is the same sign as the rest of
    /// the app (positive = scroll content down / reveal items above).
    pub fn on_scroll(&mut self, dy: f64) {
        self.scroll = (self.scroll - dy).clamp(0.0, self.max_scroll);
    }

    pub fn paint(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        wl: f64,
        hl: f64,
    ) {
        let split = (wl * SPLIT).round();
        scene.fill(Fill::NonZero, Affine::IDENTITY, BG_RIGHT, None, &Rect::new(0.0, 0.0, wl, hl));
        scene.fill(Fill::NonZero, Affine::IDENTITY, BG_LEFT, None, &Rect::new(0.0, 0.0, split, hl));
        self.paint_left(scene, tcx, split);
        self.paint_grid(scene, tcx, theme, split, wl, hl);
    }

    fn paint_left(&mut self, scene: &mut Scene, tcx: &mut TextContext, split: f64) {
        // The header block (mark, wordmark, version) is centred in the panel.
        let cx = split / 2.0;

        let mark_y = 104.0;
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

        // Divider sits a fixed gap below the header.
        let dy = (ver_baseline + 8.0 + 56.0).round();
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            DIVIDER,
            None,
            &Rect::new(PAD, dy, split - PAD, dy + 1.0),
        );

        // Link rows. The YouTube tutorials row is hidden for now; its hit
        // rect stays empty so `on_press` can never resolve to it.
        self.hit_youtube = Rect::ZERO;
        let y0 = dy + 46.0;
        let stride = BADGE + 32.0;
        self.hit_news = link_row(
            scene,
            tcx,
            Badge::News,
            y0,
            split,
            "News",
            "Latest from Amalith",
        );
        self.hit_docs = link_row(
            scene,
            tcx,
            Badge::Docs,
            y0 + stride,
            split,
            "Docs",
            "Guides and Reference",
        );
        self.hit_github = link_row(
            scene,
            tcx,
            Badge::Img(&self.github),
            y0 + stride * 2.0,
            split,
            "Github",
            "Changelogs and New Releases",
        );
    }

    fn paint_grid(
        &mut self,
        scene: &mut Scene,
        tcx: &mut TextContext,
        theme: &Theme,
        split: f64,
        wl: f64,
        hl: f64,
    ) {
        let area_x = split + 92.0;
        let area_top = 92.0;
        let area_bottom = hl - 40.0;
        let scroll_w = 10.0;
        let area_w = (wl - area_x - 56.0 - scroll_w).max(240.0);

        let gap = 30.0;
        let tile = ((area_w - gap * (COLS as f64 - 1.0)) / COLS as f64).clamp(130.0, 215.0);
        let label_gap = 12.0;
        let cell_h = tile + label_gap + 24.0;
        let row_stride = cell_h + gap;

        let filled = 1 + self.recents.len();
        let total = filled.max(MIN_SLOTS);
        let rows = total.div_ceil(COLS);
        let content_h = rows as f64 * row_stride - gap;
        let viewport_h = (area_bottom - area_top).max(1.0);
        self.max_scroll = (content_h - viewport_h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);

        let clip = Rect::new(split, 0.0, wl, hl);
        scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &clip);

        self.hit_new = Rect::ZERO;
        self.hit_recents.clear();
        self.hit_recents.resize(self.recents.len(), Rect::ZERO);
        for idx in 0..total {
            let col = idx % COLS;
            let row = idx / COLS;
            let x = area_x + col as f64 * (tile + gap);
            let y = area_top + row as f64 * row_stride - self.scroll;
            if y + tile < area_top - 8.0 || y > area_bottom + 8.0 {
                continue;
            }
            let tile_rect = Rect::from_origin_size((x, y), (tile, tile));

            if idx == 0 {
                image_into(scene, &self.tile, tile_rect);
                self.hit_new = tile_rect;
                label(scene, tcx, theme, "New Document", tile_rect, label_gap, true);
            } else if idx - 1 < self.recents.len() {
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    TILE_RECENT,
                    None,
                    &RoundedRect::from_rect(tile_rect, 18.0),
                );
                let name = self.recents[idx - 1].1.clone();
                label(scene, tcx, theme, &name, tile_rect, label_gap, false);
                self.hit_recents[idx - 1] = tile_rect;
            } else {
                scene.fill(
                    Fill::NonZero,
                    Affine::IDENTITY,
                    TILE_PLACEHOLDER,
                    None,
                    &RoundedRect::from_rect(tile_rect, 18.0),
                );
            }
        }

        if self.max_scroll > 0.0 {
            let track = Rect::new(
                wl - 22.0,
                area_top,
                wl - 16.0,
                area_bottom,
            );
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                Color::from_rgb8(28, 28, 30),
                None,
                &track.to_rounded_rect(3.0),
            );
            let frac = (viewport_h / content_h).clamp(0.12, 1.0);
            let th = (track.height() * frac).max(28.0);
            let ty = track.y0 + (track.height() - th) * (self.scroll / self.max_scroll);
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                SCROLL_THUMB,
                None,
                &Rect::new(track.x0, ty, track.x1, ty + th).to_rounded_rect(3.0),
            );
        }

        scene.pop_layer();
    }
}

/// The little square at the head of a link row: an art PNG, or a drawn
/// monoline glyph for the rows that don't have dedicated art yet.
enum Badge<'a> {
    Img(&'a ImageData),
    News,
    Docs,
}

fn link_row(
    scene: &mut Scene,
    tcx: &mut TextContext,
    badge: Badge<'_>,
    y: f64,
    split: f64,
    title: &str,
    sub: &str,
) -> Rect {
    let box_ = Rect::from_origin_size((PAD, y), (BADGE, BADGE));
    match badge {
        Badge::Img(img) => image_into(scene, img, box_),
        Badge::News => draw_news_badge(scene, box_),
        Badge::Docs => draw_docs_badge(scene, box_),
    }
    let tx = PAD + BADGE + 18.0;
    tcx.draw(scene, title, 15.0, INK, tx, y + 20.0);
    tcx.draw(scene, sub, 12.0, DIM, tx, y + 39.0);
    Rect::new(PAD - 6.0, y - 6.0, split - PAD, y + BADGE + 6.0)
}

/// Rounded-grey badge background shared by the drawn glyphs.
fn badge_bg(scene: &mut Scene, box_: Rect) {
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        TILE_RECENT,
        None,
        &RoundedRect::from_rect(box_, 12.0),
    );
}

/// A newspaper: framed page, masthead bar, three text lines.
fn draw_news_badge(scene: &mut Scene, box_: Rect) {
    badge_bg(scene, box_);
    let g = box_.inset(-12.0);
    let stroke = Stroke::new(1.6);
    scene.stroke(
        &stroke,
        Affine::IDENTITY,
        INK,
        None,
        &RoundedRect::from_rect(g, 2.0),
    );
    // Masthead.
    scene.fill(
        Fill::NonZero,
        Affine::IDENTITY,
        INK,
        None,
        &Rect::new(g.x0 + 3.0, g.y0 + 3.0, g.x1 - 3.0, g.y0 + 7.0),
    );
    // Text lines.
    for i in 0..3 {
        let ly = g.y0 + 12.0 + i as f64 * 4.5;
        let x1 = if i == 2 { g.x1 - 7.0 } else { g.x1 - 3.0 };
        scene.stroke(
            &Stroke::new(1.4),
            Affine::IDENTITY,
            INK,
            None,
            &line_path((g.x0 + 3.0, ly), (x1, ly)),
        );
    }
}

/// A document page with a folded top-right corner and three text lines.
fn draw_docs_badge(scene: &mut Scene, box_: Rect) {
    badge_bg(scene, box_);
    let g = box_.inset(-13.0);
    let fold = 7.0;
    let mut page = BezPath::new();
    page.move_to((g.x0, g.y0));
    page.line_to((g.x1 - fold, g.y0));
    page.line_to((g.x1, g.y0 + fold));
    page.line_to((g.x1, g.y1));
    page.line_to((g.x0, g.y1));
    page.close_path();
    scene.stroke(&Stroke::new(1.6), Affine::IDENTITY, INK, None, &page);
    // Folded corner.
    let mut corner = BezPath::new();
    corner.move_to((g.x1 - fold, g.y0));
    corner.line_to((g.x1 - fold, g.y0 + fold));
    corner.line_to((g.x1, g.y0 + fold));
    scene.stroke(&Stroke::new(1.4), Affine::IDENTITY, INK, None, &corner);
    // Text lines.
    for i in 0..3 {
        let ly = g.y0 + 13.0 + i as f64 * 5.0;
        let x1 = if i == 2 { g.x1 - 6.0 } else { g.x1 - 4.0 };
        scene.stroke(
            &Stroke::new(1.4),
            Affine::IDENTITY,
            INK,
            None,
            &line_path((g.x0 + 4.0, ly), (x1, ly)),
        );
    }
}

fn line_path(a: (f64, f64), b: (f64, f64)) -> BezPath {
    let mut p = BezPath::new();
    p.move_to(a);
    p.line_to(b);
    p
}

/// Centred caption under a tile. `selected` draws the accent highlight pill.
fn label(
    scene: &mut Scene,
    tcx: &mut TextContext,
    theme: &Theme,
    s: &str,
    tile: Rect,
    gap: f64,
    selected: bool,
) {
    let w = tcx.measure(s, 14.0);
    let cx = tile.x0 + tile.width() / 2.0;
    let baseline = tile.y1 + gap + 14.0;
    if selected {
        let pill = Rect::new(cx - w / 2.0 - 8.0, baseline - 15.0, cx + w / 2.0 + 8.0, baseline + 5.0);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.accent,
            None,
            &RoundedRect::from_rect(pill, 4.0),
        );
    }
    let col = if selected { theme.on_accent } else { INK };
    tcx.draw(scene, s, 14.0, col, cx - w / 2.0, baseline);
}
