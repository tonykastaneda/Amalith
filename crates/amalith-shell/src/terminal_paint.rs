//! Renders the embedded terminal pane's glyph grid. A sibling of
//! `canvas.rs` — pure, parametric on whatever [`TerminalPaintArgs`] it's
//! given, no dependency on `App` internals beyond the borrowed refs the
//! caller hands in.

use vello::kurbo::{Affine, BezPath, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};

use crate::metrics::px as ui_px;
use crate::text::TextContext;
use crate::theme::Theme;

/// Height of the terminal's tab header — the same metric the document
/// tab strip uses (`app_tab_bar_h`), so the two panes' headers line up
/// pixel-for-pixel instead of merely looking similar.
fn metric_header_h() -> f64 { crate::metrics::with(|m| m.app_tab_bar_h) }

/// Font size (logical px) every terminal pane renders at. Fixed for now —
/// no user-facing zoom control yet. Cell metrics are resolved once at
/// this exact size when the pane opens (see `app/terminal.rs`), so this
/// constant must stay in sync with that resolution, not re-guessed here.
pub const TERMINAL_FONT_SIZE: f32 = 13.0;

/// The single label shown on the terminal's tab. Fixed — this pane never
/// hosts more than one shell, so there's only ever one tab.
const TAB_LABEL: &str = "Terminal";

/// The header strip across the top of the pane — same height as every
/// other flyout-styled header in this app, but styled to read as a tab
/// strip (see `tab_rect`) so the terminal pane visually matches the
/// document pane's own tab strip above the canvas.
pub fn header_rect(rect: Rect) -> Rect {
    Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + metric_header_h())
}

/// The terminal's single "tab" within its header — `(whole rect, close-×
/// rect)`, laid out with the same constants `layout_tabs` (`app/mod.rs`)
/// uses for document tabs, so the two strips read as the same kind of UI.
pub fn tab_rect(text: &mut TextContext, rect: Rect) -> (Rect, Rect) {
    let header = header_rect(rect);
    let tw = text.measure(TAB_LABEL, 12.6);
    let w = tw + ui_px(18.9) /* × */ + ui_px(23.1) /* padding */;
    let x = header.x0 + ui_px(4.0);
    let whole = Rect::new(x, header.y0, (x + w).min(header.x1), header.y1);
    let close = Rect::new(whole.x0 + ui_px(6.0), header.y0 + ui_px(4.0), whole.x0 + ui_px(20.0), header.y1 - ui_px(4.0));
    (whole, close)
}

/// The close (×) on the terminal's tab — hides the pane, same as toggling
/// it from the menu. The underlying shell keeps running until Amalith
/// quits (or the pane is reopened, picking the same session back up).
pub fn close_button_rect(text: &mut TextContext, rect: Rect) -> Rect {
    tab_rect(text, rect).1
}

/// The grid content area — everything below the header.
pub fn content_rect(rect: Rect) -> Rect {
    let h = header_rect(rect);
    Rect::new(rect.x0, h.y1, rect.x1, rect.y1)
}

/// Everything `paint` needs, borrowed from the live `TerminalPane` for the
/// duration of one frame.
pub struct TerminalPaintArgs<'a> {
    pub rect: Rect,
    /// Paint the pane's own tab-styled header (title/× within `rect`
    /// itself)? `true` for the old standalone terminal split (where this
    /// *is* the pane's only header); `false` when embedded as one tab of
    /// a multiplexer pane, which already paints its own tab strip above
    /// every tab it holds — a second header here would be redundant. See
    /// `docs/canvas-panes.md`.
    pub header: bool,
    pub focused: bool,
    pub term: &'a alacritty_terminal::Term<crate::app::PtyReplies>,
    pub font: &'a vello::peniko::FontData,
    pub cell_w: f64,
    pub cell_h: f64,
    pub ascent: f64,
}

fn resolve_color(c: AnsiColor, theme: &Theme) -> Color {
    match c {
        AnsiColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => indexed_rgb(idx),
        AnsiColor::Named(NamedColor::Foreground) => theme.text,
        AnsiColor::Named(NamedColor::Background) => theme.panel_bg,
        AnsiColor::Named(named) => {
            let idx = named as u16;
            if idx < 16 {
                indexed_rgb(idx as u8)
            } else {
                theme.text
            }
        }
    }
}

/// The standard xterm 16/256-color palette — this app has no OSC-4
/// custom-palette support (a script redefining a color slot mid-session
/// isn't tracked), so this is always what a plain indexed color resolves
/// to.
fn indexed_rgb(idx: u8) -> Color {
    const BASE16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 0, 0),
        (0, 205, 0),
        (205, 205, 0),
        (0, 0, 238),
        (205, 0, 205),
        (0, 205, 205),
        (229, 229, 229),
        (127, 127, 127),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (92, 92, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];
    if (idx as usize) < 16 {
        let (r, g, b) = BASE16[idx as usize];
        Color::from_rgb8(r, g, b)
    } else if idx < 232 {
        let i = idx - 16;
        let levels = [0u8, 95, 135, 175, 215, 255];
        let r = levels[(i / 36) as usize];
        let g = levels[((i / 6) % 6) as usize];
        let b = levels[(i % 6) as usize];
        Color::from_rgb8(r, g, b)
    } else {
        let v = (8 + (idx - 232) as u16 * 10).min(255) as u8;
        Color::from_rgb8(v, v, v)
    }
}

/// Paints the terminal pane: header (title / Exit / Close), every visible
/// cell's background/glyph below it, a simple block cursor, and — while
/// focused — an accent-colored border so it's obvious at a glance whether
/// keystrokes are going to the terminal or the document. `viewport`-relative
/// — draws nothing outside `args.rect`.
pub fn paint(scene: &mut Scene, args: &TerminalPaintArgs<'_>, theme: &Theme, text: &mut TextContext) {
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &args.rect);

    let content = if args.header {
        let header = header_rect(args.rect);
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.app_bar, None, &header);
        scene.fill(
            Fill::NonZero,
            Affine::IDENTITY,
            theme.border,
            None,
            &Rect::new(header.x0, header.y1 - 1.0, header.x1, header.y1),
        );
        // A single "tab" styled exactly like a document tab (`layout_tabs`
        // in `app/mod.rs`) — same fill/underline/× treatment — so the
        // terminal pane's header reads as the same kind of UI as the
        // canvas's own tab strip instead of a bespoke toolbar.
        let (whole, close) = tab_rect(text, args.rect);
        if args.focused {
            scene.fill(Fill::NonZero, Affine::IDENTITY, theme.strip_active, None, &whole);
            scene.fill(
                Fill::NonZero,
                Affine::IDENTITY,
                theme.accent,
                None,
                &Rect::new(whole.x0, whole.y1 - ui_px(2.0), whole.x1, whole.y1),
            );
        }
        let ink = if args.focused { theme.text } else { theme.text_dim };
        let xc = close.center();
        let mut xg = BezPath::new();
        xg.move_to((xc.x - ui_px(4.0), xc.y - ui_px(4.0)));
        xg.line_to((xc.x + ui_px(4.0), xc.y + ui_px(4.0)));
        xg.move_to((xc.x + ui_px(4.0), xc.y - ui_px(4.0)));
        xg.line_to((xc.x - ui_px(4.0), xc.y + ui_px(4.0)));
        scene.stroke(&Stroke::new(ui_px(1.3)), Affine::IDENTITY, ink, None, &xg);
        let baseline = header.y0 + header.height() * 0.5 + TERMINAL_FONT_SIZE as f64 * 0.34;
        text.draw(scene, TAB_LABEL, 12.6, ink, close.x1 + ui_px(6.0), baseline);
        content_rect(args.rect)
    } else {
        args.rect
    };

    let Ok(font_ref) = skrifa::FontRef::from_index(args.font.data.as_ref(), args.font.index) else {
        return;
    };
    use skrifa::MetadataProvider;
    let charmap = font_ref.charmap();

    let renderable = args.term.renderable_content();
    let cursor = renderable.cursor;

    for indexed in renderable.display_iter {
        let point = indexed.point;
        let cell = indexed.cell;
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        if point.line.0 < 0 {
            continue;
        }
        let row = point.line.0 as f64;
        let col = point.column.0 as f64;
        let cx = content.x0 + col * args.cell_w;
        let cy = content.y0 + row * args.cell_h;
        if cy >= content.y1 || cx >= content.x1 {
            continue;
        }
        let cell_rect = Rect::new(cx, cy, (cx + args.cell_w).min(content.x1), cy + args.cell_h);

        let (mut fg, mut bg) = (resolve_color(cell.fg, theme), resolve_color(cell.bg, theme));
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if bg != theme.panel_bg {
            scene.fill(Fill::NonZero, Affine::IDENTITY, bg, None, &cell_rect);
        }

        if cell.flags.contains(Flags::HIDDEN) || cell.c == ' ' || cell.c == '\0' {
            continue;
        }
        let Some(gid) = charmap.map(cell.c) else { continue };
        if gid.to_u32() == 0 {
            continue;
        }
        let glyph = vello::Glyph { id: gid.to_u32(), x: cx as f32, y: (cy + args.ascent) as f32 };
        scene
            .draw_glyphs(args.font)
            .brush(&vello::peniko::Brush::Solid(fg))
            .hint(false)
            .font_size(TERMINAL_FONT_SIZE)
            .draw(Fill::NonZero, std::iter::once(glyph));
    }

    if cursor.point.line.0 >= 0 {
        let cx = content.x0 + cursor.point.column.0 as f64 * args.cell_w;
        let cy = content.y0 + cursor.point.line.0 as f64 * args.cell_h;
        let cursor_rect = Rect::new(cx, cy, (cx + args.cell_w).min(content.x1), cy + args.cell_h);
        let alpha = if args.focused { 0.6 } else { 0.3 };
        scene.fill(Fill::NonZero, Affine::IDENTITY, theme.accent.multiply_alpha(alpha), None, &cursor_rect);
    }

    // Focus border — the whole point is to make it obvious at a glance
    // whether keystrokes are going to the terminal or the document.
    if args.focused {
        scene.stroke(&Stroke::new(2.0), Affine::IDENTITY, theme.accent, None, &args.rect.inset(-1.0));
    }
}
