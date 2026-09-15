//! The embedded PTY terminal pane (File ▸ Scripts ▸ Terminal). Split from
//! `impl App` proper the same way `image_trace.rs` splits `TraceState` +
//! its own `impl App` block into one file.
//!
//! Thread model mirrors image tracing exactly: one background thread does
//! blocking I/O (there, tracing a bitmap; here, reading the PTY master)
//! and reports back over an `mpsc::channel`; `about_to_wait` polls it
//! every ~16ms while a pane is open (see the `wake` merge below) and
//! `request_main_redraw`s when something arrived — no new cross-thread
//! wake-up machinery.
//!
//! Rendering lives in `crate::terminal_paint`, a sibling of `canvas.rs`:
//! pure and parametric on an explicit [`crate::terminal_paint::TerminalPaintArgs`],
//! no `App` dependency.

use std::io::{Read, Write};
use std::sync::mpsc;

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::vte::ansi::Processor;
use alacritty_terminal::Term;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::terminal_paint::TERMINAL_FONT_SIZE;

use super::*;

/// One open terminal pane's state. Exists only while the pane is open —
/// `App::terminal` is `None` when closed.
pub(super) struct TerminalPane {
    pub(super) focused: bool,
    /// `true` while the pane is shown; `false` when the user's clicked
    /// its header × ("Close") — the shell keeps running in the
    /// background either way, only `exit_terminal` (the "Exit" button)
    /// or quitting Amalith (⌘Q) actually kills it.
    pub(super) visible: bool,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    rx: mpsc::Receiver<Vec<u8>>,
    reader_thread: Option<std::thread::JoinHandle<()>>,
    // `pub(super)` (visible to the rest of `app`, e.g. `app::render`) —
    // `render/mod.rs` borrows these directly to build one frame's
    // `TerminalPaintArgs`, since going through a method here would make
    // the borrow checker treat the whole of `self` as tied up for the
    // struct's lifetime instead of just this one field (see that call
    // site's own comment).
    pub(super) term: Term<VoidListener>,
    processor: Processor,
    pub(super) font: vello::peniko::FontData,
    pub(super) cell_w: f64,
    pub(super) cell_h: f64,
    pub(super) ascent: f64,
}

/// Minimum width (logical px) either pane is allowed to shrink to when
/// dragging the divider — mirrors `MasterWidth`'s pixel-based clamp, just
/// converted to a ratio bound fresh against the *current* total width on
/// every drag move, so a since-resized window can't leave a stale
/// out-of-range split ratio.
const MIN_DOCUMENT_W: f64 = 300.0;
const MIN_TERMINAL_W: f64 = 200.0;

/// A fixed-size grid for `Term::new` / `alacritty_terminal::grid::Dimensions`
/// — no scrollback beyond the visible viewport (v1 has no scrollback UI).
struct TermDims {
    columns: usize,
    lines: usize,
}

impl Dimensions for TermDims {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.columns
    }
}

/// Shapes a single "M" in the platform's default monospace font to pull a
/// real `peniko::Font` handle plus this pane's fixed cell metrics — the
/// same parley→skrifa hand-off `outline_text_data` uses, just for one
/// glyph instead of a whole paragraph. Returns `None` if no monospace
/// font could be shaped at all (never expected in practice).
fn resolve_terminal_font(text: &mut TextContext) -> Option<(vello::peniko::FontData, f64, f64, f64)> {
    let (fc, lc) = text.parts();
    let sample = "M";
    let mut b = lc.ranged_builder(fc, sample, 1.0, true);
    b.push_default(parley::style::StyleProperty::from(parley::style::GenericFamily::Monospace));
    b.push_default(parley::style::StyleProperty::FontSize(TERMINAL_FONT_SIZE));
    let mut layout = b.build(sample);
    layout.break_all_lines(None);
    layout.align(parley::Alignment::Start, parley::layout::AlignmentOptions::default());

    for line in layout.lines() {
        let lm = line.metrics();
        for item in line.items() {
            let parley::layout::PositionedLayoutItem::GlyphRun(run) = item else { continue };
            let r = run.run();
            let font = r.font().clone();
            let advance = run
                .glyphs()
                .next()
                .map(|g| g.advance as f64)
                .unwrap_or(TERMINAL_FONT_SIZE as f64 * 0.6);
            let cell_h = (lm.ascent + lm.descent + lm.leading).max(1.0) as f64;
            return Some((font, advance.max(1.0), cell_h, lm.ascent as f64));
        }
    }
    None
}

impl App {
    /// File ▸ Scripts ▸ Terminal — a single toggle. Spawns a shell if
    /// File ▸ Scripts ▸ Terminal — starts (or resumes) a terminal tab in
    /// the focused pane. See `App::open_mux_terminal`.
    pub(in crate::app) fn toggle_terminal(&mut self) {
        self.open_mux_terminal();
    }

    /// Whether the pane is currently shown (PTY alive *and* not hidden) —
    /// the one thing layout/painting/hit-testing should check; `terminal
    /// .is_some()` alone only means the shell is alive, which stays true
    /// while hidden.
    pub(in crate::app) fn terminal_visible(&self) -> bool {
        self.terminal.as_ref().is_some_and(|t| t.visible)
    }

    pub(super) fn open_terminal(&mut self) {
        let Some((font, cell_w, cell_h, ascent)) = resolve_terminal_font(&mut self.text) else {
            return;
        };

        let bounds = self.terminal_pane_bounds();
        let header_h = crate::terminal_paint::header_rect(bounds).height();
        let cols = ((bounds.width() / cell_w).floor().max(1.0)) as u16;
        let rows = (((bounds.height() - header_h) / cell_h).floor().max(1.0)) as u16;

        let pty_system = native_pty_system();
        let Ok(pair) = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: cell_w as u16,
            pixel_height: cell_h as u16,
        }) else {
            return;
        };

        let mut cmd = CommandBuilder::new_default_prog();
        // A cheap, read-only bridge to "the document that's open": just
        // enough for a script or an agent's own shell commands (e.g.
        // `amalith-script run "$AMALITH_DOCUMENT_PATH"`) to find it
        // without a live IPC channel into the running `Editor` — that's
        // real future work, not this.
        if let Some(path) = &self.doc.file_path {
            cmd.env("AMALITH_DOCUMENT_PATH", path);
        }
        if let Some(dir) = &self.scripts.dir {
            cmd.env("AMALITH_SCRIPTS_DIR", dir);
            // Auto-cd into the user's scripts folder — the terminal's
            // whole point is running/editing scripts from there, so
            // start off already in it rather than making every session
            // `cd` there by hand.
            cmd.cwd(dir);
        }
        let Ok(child) = pair.slave.spawn_command(cmd) else { return };
        drop(pair.slave);

        let Ok(mut reader) = pair.master.try_clone_reader() else { return };
        let Ok(writer) = pair.master.take_writer() else { return };

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let reader_thread = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        let dims = TermDims { columns: cols as usize, lines: rows as usize };
        let term = Term::new(TermConfig::default(), &dims, VoidListener);

        self.terminal = Some(TerminalPane {
            focused: true,
            visible: true,
            master: pair.master,
            writer,
            child,
            rx,
            reader_thread: Some(reader_thread),
            term,
            processor: Processor::new(),
            font,
            cell_w,
            cell_h,
            ascent,
        });
        self.request_main_redraw();
    }

    /// Header's "Exit" button: actually terminates the shell/child
    /// process — the same teardown quitting Amalith (⌘Q) does, just
    /// triggered early and without closing the app.
    pub(in crate::app) fn exit_terminal(&mut self) {
        if let Some(mut pane) = self.terminal.take() {
            let _ = pane.child.kill();
            let _ = pane.child.wait();
            drop(pane.writer);
            drop(pane.master);
            // Not joined here — a killed child's PTY read can stay
            // blocked briefly on some platforms, and this runs on the
            // main thread. Phase 4 hardens this into a bounded join;
            // for now the thread exits on its own shortly after and is
            // harmless to leave detached until then.
            drop(pane.reader_thread.take());
        }
        self.request_main_redraw();
    }

    /// Drains whatever PTY output arrived since the last tick and feeds
    /// it through the ANSI processor into `Term`'s grid. Called from
    /// `about_to_wait`, parallel to `trace_tick`.
    pub(in crate::app) fn terminal_tick(&mut self) {
        let Some(pane) = &mut self.terminal else { return };
        let mut changed = false;
        while let Ok(bytes) = pane.rx.try_recv() {
            pane.processor.advance(&mut pane.term, &bytes);
            changed = true;
        }
        if changed {
            self.request_main_redraw();
        }
    }

    /// Translates one raw key event into PTY bytes and writes them —
    /// named keys become their VT100/xterm escape sequences, Ctrl+letter
    /// becomes the matching control byte, and everything else falls back
    /// to the event's own (unfiltered — control characters kept, unlike
    /// `rename_key`'s text-editing filter) typed text. Only called while
    /// the pane is focused (see the `on_key` gate in `input/keyboard.rs`).
    pub(in crate::app) fn terminal_key(&mut self, event: &winit::event::KeyEvent) {
        use winit::keyboard::{Key, NamedKey};

        if !event.state.is_pressed() {
            return;
        }
        let ctrl = self.ctrl_down;

        let bytes: Vec<u8> = match &event.logical_key {
            Key::Named(NamedKey::Enter) => vec![b'\r'],
            Key::Named(NamedKey::Backspace) => vec![0x7f],
            Key::Named(NamedKey::Tab) => vec![b'\t'],
            Key::Named(NamedKey::Escape) => vec![0x1b],
            Key::Named(NamedKey::ArrowUp) => b"\x1b[A".to_vec(),
            Key::Named(NamedKey::ArrowDown) => b"\x1b[B".to_vec(),
            Key::Named(NamedKey::ArrowRight) => b"\x1b[C".to_vec(),
            Key::Named(NamedKey::ArrowLeft) => b"\x1b[D".to_vec(),
            Key::Named(NamedKey::Home) => b"\x1b[H".to_vec(),
            Key::Named(NamedKey::End) => b"\x1b[F".to_vec(),
            Key::Named(NamedKey::Delete) => b"\x1b[3~".to_vec(),
            Key::Named(NamedKey::PageUp) => b"\x1b[5~".to_vec(),
            Key::Named(NamedKey::PageDown) => b"\x1b[6~".to_vec(),
            Key::Character(s) if ctrl && s.chars().count() == 1 => {
                let c = s.chars().next().unwrap().to_ascii_uppercase();
                if c.is_ascii_uppercase() {
                    vec![(c as u8) - b'A' + 1]
                } else {
                    Vec::new()
                }
            }
            _ => event.text.as_ref().map(|t| t.as_bytes().to_vec()).unwrap_or_default(),
        };

        if bytes.is_empty() {
            return;
        }
        if let Some(pane) = &mut self.terminal {
            let _ = pane.writer.write_all(&bytes);
            let _ = pane.writer.flush();
        }
    }

    /// Whether the pointer is currently over the terminal/canvas divider's
    /// grab zone — drives the resize cursor in `update_canvas_cursor`, and
    /// mirrors the same `edge_rect` the press handler hit-tests against.
    pub(in crate::app) fn over_terminal_divider(&self) -> bool {
        if self.mux.model.enabled() { return false; }
        let Some(term_rect) = self.terminal_rect() else { return false };
        let edge_rect = Rect::new(term_rect.x0 - metric_rail_edge(), term_rect.y0, term_rect.x0, term_rect.y1);
        edge_rect.inflate(metric_grab_slop() + 1.0, 0.0).contains(self.pointer)
    }

    /// Clamp a candidate split ratio so neither pane drops below its
    /// minimum width, given the current total canvas width (rails
    /// already subtracted).
    pub(in crate::app) fn clamp_terminal_split(&self, ratio: f32, total_w: f64) -> f32 {
        if total_w <= 0.0 {
            return ratio.clamp(0.0, 1.0);
        }
        let min_terminal_ratio = (MIN_TERMINAL_W / total_w) as f32;
        let max_terminal_ratio = (1.0 - MIN_DOCUMENT_W / total_w) as f32;
        if min_terminal_ratio > max_terminal_ratio {
            // Window too narrow for both minimums — just don't move it.
            return self.terminal_split;
        }
        ratio.clamp(min_terminal_ratio, max_terminal_ratio)
    }
}

impl TerminalPane {
    pub(super) fn tick_and_resize(&mut self, bounds: Rect) {
        while let Ok(bytes) = self.rx.try_recv() { self.processor.advance(&mut self.term, &bytes); }
        let columns = (bounds.width() / self.cell_w).floor().max(1.) as usize;
        let lines = ((bounds.height() - crate::terminal_paint::header_rect(bounds).height()) / self.cell_h).floor().max(1.) as usize;
        if columns != self.term.columns() || lines != self.term.screen_lines() {
            self.term.resize(TermDims {columns,lines});
            let _ = self.master.resize(PtySize {rows:lines as u16,cols:columns as u16,pixel_width:0,pixel_height:0});
        }
    }
    pub(super) fn terminate(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}
