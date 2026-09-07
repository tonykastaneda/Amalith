//! The Home / Welcome screen.
//!
//! Shown on launch and whenever the last document tab is closed: a left panel
//! with the app mark, the welcome wordmark, and a short stack of external
//! links (News, Docs, GitHub), plus a responsive grid on the right — a
//! "New Document" tile, recent files with rendered previews, and dark-grey
//! placeholders, with a scrollbar when the grid overflows and an Open /
//! Import bar pinned below it.
//!
//! The YouTube tutorials link is kept in the code but hidden for now (see
//! `Badge::Youtube` / `hit_youtube`) — bring it back once the app is closer
//! to feature-complete.
//!
//! Rendered with vello + parley like the rest of the chrome. It's a full-window
//! surface: while it's up, the canvas underneath takes no input. Artwork comes
//! from `assets/home/` (SVGs rasterised to PNG at build prep time). Recent-file
//! previews are rendered headlessly and cached to disk — see `app/thumbnails.rs`;
//! this module only paints whatever preview it's handed via `set_thumbnail`.

use std::path::{Path, PathBuf};

use vello::kurbo::{Affine, BezPath, Rect, RoundedRect, Stroke, Vec2};
use vello::peniko::{Blob, Color, Fill, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

const MARK_PNG: &[u8] = include_bytes!("../assets/home/mark.png");
const WELCOME_PNG: &[u8] = include_bytes!("../assets/home/welcome.png");
const YOUTUBE_PNG: &[u8] = include_bytes!("../assets/home/youtube.png");
const GITHUB_PNG: &[u8] = include_bytes!("../assets/home/github.png");
const TILE_PNG: &[u8] = include_bytes!("../assets/home/newdoc-tile.png");

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
const TILE_RECENT_HOVER: Color = Color::from_rgb8(56, 56, 59);
const TILE_PLACEHOLDER: Color = Color::from_rgb8(38, 38, 40);
const SCROLL_THUMB: Color = Color::from_rgb8(90, 90, 94);
const BAR_BG: Color = Color::from_rgb8(24, 24, 26);
/// Always fill at least this many cells (New Document + recents + blanks).
const MIN_SLOTS: usize = 9;
/// Column count settles around this tile width as the window resizes.
const TARGET_TILE: f64 = 172.0;
const MIN_TILE: f64 = 128.0;
const MAX_TILE: f64 = 224.0;
/// Height of the solid Open / Import bar along the bottom of the panel.
const TOOLBAR_H: f64 = 76.0;

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
    Import,
}

/// Which tile the pointer is currently over — drives the highlight that
/// used to be permanently glued to New Document. Open / Import use the
/// New Document dialog's plain (non-hover) button styling, so they don't
/// need a hover state of their own.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hover {
    NewDoc,
    Recent(usize),
}

/// A recent file's rendered preview, generated headlessly by
/// `App::recent_thumbnail` and handed back in via `set_thumbnail`.
enum ThumbState {
    /// Not requested yet, or `App` hasn't gotten to it this frame.
    Pending,
    /// Tried and failed (missing file, decode error, empty document, …) —
    /// don't keep retrying every frame.
    Failed,
    Ready(ImageData),
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

/// Scale + place `img` so it fits inside `dst` without cropping, centred.
fn image_contain(scene: &mut Scene, img: &ImageData, dst: Rect) {
    let (iw, ih) = (img.width as f64, img.height as f64);
    if iw <= 0.0 || ih <= 0.0 {
        return;
    }
    let s = (dst.width() / iw).min(dst.height() / ih);
    let (dw, dh) = (iw * s, ih * s);
    let cx = dst.x0 + (dst.width() - dw) / 2.0;
    let cy = dst.y0 + (dst.height() - dh) / 2.0;
    image_into(scene, img, Rect::from_origin_size((cx, cy), (dw, dh)));
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
    /// (path, display name, preview), most-recent first.
    recents: Vec<(PathBuf, String, ThumbState)>,
    /// Vertical scroll of the document grid, in px.
    scroll: f64,
    /// Last paint's max scroll; wheel clamping uses this.
    max_scroll: f64,
    hover: Option<Hover>,
    /// The recent file a single click landed on. Open (and double-click)
    /// act on this — a click here only marks the tile, it doesn't open it.
    selected: Option<usize>,
    // Hit rectangles in window coordinates, refreshed each paint.
    hit_new: Rect,
    hit_recents: Vec<Rect>,
    /// Stays `Rect::ZERO` while the YouTube row is hidden.
    hit_youtube: Rect,
    hit_news: Rect,
    hit_docs: Rect,
    hit_github: Rect,
    hit_open: Rect,
    hit_import: Rect,
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
                    (p, n, ThumbState::Pending)
                })
                .collect(),
            scroll: 0.0,
            max_scroll: 0.0,
            hover: None,
            selected: None,
            hit_new: Rect::ZERO,
            hit_recents: Vec::new(),
            hit_youtube: Rect::ZERO,
            hit_news: Rect::ZERO,
            hit_docs: Rect::ZERO,
            hit_github: Rect::ZERO,
            hit_open: Rect::ZERO,
            hit_import: Rect::ZERO,
        })
    }

    pub fn recent_path(&self, i: usize) -> Option<&Path> {
        self.recents.get(i).map(|(p, ..)| p.as_path())
    }

    /// Index of the first recent file that still needs its preview
    /// rendered, if any. `App` drives `recent_thumbnail` off this, one per
    /// frame, until the whole list is settled.
    pub fn next_missing_thumbnail(&self) -> Option<usize> {
        self.recents
            .iter()
            .position(|(.., t)| matches!(t, ThumbState::Pending))
    }

    pub fn set_thumbnail(&mut self, i: usize, img: Option<ImageData>) {
        if let Some(entry) = self.recents.get_mut(i) {
            entry.2 = match img {
                Some(img) => ThumbState::Ready(img),
                None => ThumbState::Failed,
            };
        }
    }

    /// `double` is whether this press is the second half of a double-click
    /// (see `App::click_streak`). A single click on a recent file only
    /// selects it — Open (or a double-click) is what actually opens it.
    pub fn on_press(&mut self, p: Vec2, double: bool) -> Hit {
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
        if self.hit_open.contains(pt) {
            return match self.selected {
                Some(i) => Hit::Recent(i),
                None => Hit::None,
            };
        }
        if self.hit_import.contains(pt) {
            return Hit::Import;
        }
        for (i, r) in self.hit_recents.iter().enumerate() {
            if r.contains(pt) {
                if double {
                    return Hit::Recent(i);
                }
                self.selected = Some(i);
                return Hit::None;
            }
        }
        Hit::None
    }

    /// Pointer moved. Returns whether the hover state changed (and so the
    /// screen needs a repaint) — mirrors `CommandPalette::hover`.
    pub fn on_move(&mut self, p: Vec2) -> bool {
        let pt = p.to_point();
        let next = if self.hit_new.contains(pt) {
            Some(Hover::NewDoc)
        } else {
            self.hit_recents
                .iter()
                .position(|r| r.contains(pt))
                .map(Hover::Recent)
        };
        if next != self.hover {
            self.hover = next;
            true
        } else {
            false
        }
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
        let area_right = wl - 56.0;
        let scroll_w = 10.0;
        let area_w = (area_right - area_x - scroll_w).max(240.0);
        let area_bottom = hl - TOOLBAR_H;

        // Column count settles around `TARGET_TILE`, so the grid actually
        // reflows (more/fewer columns, not just resized ones) as the
        // window is resized, instead of being pinned at a fixed count.
        let gap = 30.0;
        let cols = (((area_w + gap) / (TARGET_TILE + gap)).round() as usize).max(1);
        let tile = ((area_w - gap * (cols as f64 - 1.0)) / cols as f64).clamp(MIN_TILE, MAX_TILE);
        let label_gap = 12.0;
        let cell_h = tile + label_gap + 24.0;
        let row_stride = cell_h + gap;

        let filled = 1 + self.recents.len();
        let total = filled.max(MIN_SLOTS.max(cols));
        let rows = total.div_ceil(cols);
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
            let col = idx % cols;
            let row = idx / cols;
            let x = area_x + col as f64 * (tile + gap);
            let y = area_top + row as f64 * row_stride - self.scroll;
            if y + tile < area_top - 8.0 || y > area_bottom + 8.0 {
                continue;
            }
            let tile_rect = Rect::from_origin_size((x, y), (tile, tile));

            if idx == 0 {
                image_into(scene, &self.tile, tile_rect);
                self.hit_new = tile_rect;
                let sel = self.hover == Some(Hover::NewDoc);
                label(scene, tcx, theme, "New Document", tile_rect, label_gap, sel);
            } else if idx - 1 < self.recents.len() {
                let is_sel = self.selected == Some(idx - 1);
                let is_hov = self.hover == Some(Hover::Recent(idx - 1));
                let rr = RoundedRect::from_rect(tile_rect, 18.0);
                let bg = if is_hov { TILE_RECENT_HOVER } else { TILE_RECENT };
                scene.fill(Fill::NonZero, Affine::IDENTITY, bg, None, &rr);
                if let ThumbState::Ready(img) = &self.recents[idx - 1].2 {
                    scene.push_clip_layer(Fill::NonZero, Affine::IDENTITY, &rr);
                    image_contain(scene, img, tile_rect.inset(-12.0));
                    scene.pop_layer();
                }
                if is_sel {
                    scene.stroke(&Stroke::new(2.0), Affine::IDENTITY, theme.accent, None, &rr);
                }
                let name = self.recents[idx - 1].1.clone();
                label(scene, tcx, theme, &name, tile_rect, label_gap, is_sel);
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
            let track = Rect::new(wl - 22.0, area_top, wl - 16.0, area_bottom);
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

        // A solid bar along the bottom, like a file picker's footer: a flat
        // panel with a hairline top border, holding Open (primary — acts on
        // whichever tile is selected) and Import (secondary).
        let bar = Rect::new(split, area_bottom, wl, hl);
        scene.fill(Fill::NonZero, Affine::IDENTITY, BAR_BG, None, &bar);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            DIVIDER,
            None,
            &Rect::new(split, area_bottom, wl, area_bottom + 1.0),
        );

        // Same button dimensions as the New Document dialog's Create /
        // Cancel pair (`newdoc::layout`).
        let btn_h = 34.0;
        let btn_gap = 12.0;
        let btn_y = area_bottom + (TOOLBAR_H - btn_h) / 2.0;
        let open_rect = Rect::new(area_right - 104.0, btn_y, area_right, btn_y + btn_h);
        let import_rect = Rect::new(
            open_rect.x0 - btn_gap - 92.0,
            btn_y,
            open_rect.x0 - btn_gap,
            btn_y + btn_h,
        );
        self.hit_open = open_rect;
        self.hit_import = import_rect;
        // Exactly the New Document dialog's Cancel / Create pair, so the
        // two bars actually match — Open only turns "primary" (solid
        // accent) once a file is selected; until then it reads as a
        // second Cancel-style outline, same as Import.
        crate::widgets::button(scene, tcx, theme, import_rect, "Import…", false);
        crate::widgets::button(scene, tcx, theme, open_rect, "Open", self.selected.is_some());
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

/// Centred caption under a tile. `selected` draws the accent highlight pill
/// — hover, for New Document; click-selection, for a recent file.
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
