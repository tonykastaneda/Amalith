//! **Export for Screens** (File ▸ Export ▸ Export for Screens, ⌘⌥E).
//!
//! A float-only panel — same machinery as the colour picker and the shape
//! dialogs: its own window, movable by the tab strip, never dockable,
//! never in the Window menu. This module is the pure UI (state, layout,
//! hit-testing, painting); `app/export.rs` owns the window glue and the
//! actual render-and-write.
//!
//! Layout: a left pane of artboard thumbnails with tick-boxes, a right
//! column of options (what to export, where to, the Formats table), and a
//! bottom bar with the counts and the Export button.

mod formats;

pub use formats::{scale_label, Format, Row};

use std::path::PathBuf;

use vello::kurbo::{Affine, BezPath, Circle, Point, Rect, Stroke};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

/// Panel body width. The shell adds its tab-strip height for the window.
pub const W: f64 = 900.0;
/// Panel body height.
pub const H: f64 = 600.0;

const PAD: f64 = 24.0;
/// x where the right-hand options column starts.
const RIGHT_X: f64 = 540.0;
const ROW_H: f64 = 26.0;
const BTN_H: f64 = 30.0;
const FIELD_H: f64 = 24.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Artboards,
    Assets,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectMode {
    All,
    Range,
    FullDocument,
}

/// What the "Create Sub-folders" grouping is by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SubBy {
    Scale,
    Format,
}

/// Which text field holds the caret.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Focus {
    None,
    Range,
    Prefix,
    Suffix(usize),
}

/// An open Scale / Format dropdown on a Formats-table row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpenMenu {
    Scale(usize),
    Format(usize),
}

/// One artboard as far as the dialog cares.
pub struct Item {
    pub name: String,
    /// Document-space rect, for the thumbnail transform and aspect.
    pub rect: amalith_core::Rect,
    pub checked: bool,
}

pub struct ExportForScreens {
    pub tab: Tab,
    pub mode: SelectMode,
    pub range: String,
    pub include_bleed: bool,
    pub dest: PathBuf,
    pub open_after: bool,
    pub subfolders: bool,
    pub sub_by: SubBy,
    /// Export PDFs as multiple files (else a single combined file).
    pub pdf_multi: bool,
    pub prefix: String,
    pub rows: Vec<Row>,
    pub items: Vec<Item>,
    focus: Focus,
    menu: Option<OpenMenu>,
}

/// Where a pointer at `local` (panel-body coords) landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Tab(Tab),
    Mode(SelectMode),
    ToggleBleed,
    PickFolder,
    ToggleOpenAfter,
    ToggleSubfolders,
    SubBy(SubBy),
    PdfMulti(bool),
    FocusRange,
    FocusPrefix,
    FocusSuffix(usize),
    /// Open a row's Scale / Format dropdown.
    OpenScale(usize),
    OpenFormat(usize),
    /// Pick item `k` from whichever dropdown is open.
    MenuPick(usize),
    /// A click that just dismisses an open dropdown.
    MenuClose,
    RemoveRow(usize),
    AddScale,
    ToggleItem(usize),
    ClearSelection,
    Cancel,
    Export,
}

/// What the shell should do after [`ExportForScreens::apply`].
pub enum Outcome {
    None,
    PickFolder,
    Run,
    Cancel,
}

impl ExportForScreens {
    pub fn new(items: Vec<Item>, dest: PathBuf) -> Self {
        Self {
            tab: Tab::Artboards,
            mode: SelectMode::All,
            range: default_range(&items),
            include_bleed: true,
            dest,
            open_after: true,
            subfolders: false,
            sub_by: SubBy::Format,
            pdf_multi: true,
            prefix: String::new(),
            rows: formats::defaults(),
            items,
            focus: Focus::None,
            menu: None,
        }
    }

    pub fn title(&self) -> &'static str {
        "Export for Screens"
    }

    /// Indices of the artboards that will actually export, honouring the
    /// Select mode and the per-item tick-boxes.
    pub fn selected(&self) -> Vec<usize> {
        match self.mode {
            SelectMode::All | SelectMode::FullDocument => {
                (0..self.items.len()).filter(|&i| self.items[i].checked).collect()
            }
            SelectMode::Range => {
                let want = parse_range(&self.range, self.items.len());
                (0..self.items.len())
                    .filter(|&i| self.items[i].checked && want.contains(&(i + 1)))
                    .collect()
            }
        }
    }

    pub fn total_exports(&self) -> usize {
        self.selected().len() * self.rows.len().max(1)
    }

    // --- interaction ------------------------------------------------

    pub fn apply(&mut self, hit: Hit) -> Outcome {
        match hit {
            Hit::Tab(t) => self.tab = t,
            Hit::Mode(m) => {
                self.mode = m;
                if m != SelectMode::Range && self.focus == Focus::Range {
                    self.focus = Focus::None;
                }
            }
            Hit::ToggleBleed => self.include_bleed = !self.include_bleed,
            Hit::PickFolder => return Outcome::PickFolder,
            Hit::ToggleOpenAfter => self.open_after = !self.open_after,
            Hit::ToggleSubfolders => self.subfolders = !self.subfolders,
            Hit::SubBy(b) => self.sub_by = b,
            Hit::PdfMulti(v) => self.pdf_multi = v,
            Hit::FocusRange => {
                self.mode = SelectMode::Range;
                self.focus = Focus::Range;
            }
            Hit::FocusPrefix => self.focus = Focus::Prefix,
            Hit::FocusSuffix(i) => self.focus = Focus::Suffix(i),
            Hit::OpenScale(i) => {
                self.menu = if self.menu == Some(OpenMenu::Scale(i)) {
                    None
                } else {
                    Some(OpenMenu::Scale(i))
                };
            }
            Hit::OpenFormat(i) => {
                self.menu = if self.menu == Some(OpenMenu::Format(i)) {
                    None
                } else {
                    Some(OpenMenu::Format(i))
                };
            }
            Hit::MenuPick(k) => {
                match self.menu {
                    Some(OpenMenu::Scale(i)) => {
                        if let (Some(row), Some(&s)) = (self.rows.get_mut(i), formats::SCALES.get(k)) {
                            row.scale = s;
                        }
                    }
                    Some(OpenMenu::Format(i)) => {
                        if let (Some(row), Some(&f)) = (self.rows.get_mut(i), Format::ALL.get(k)) {
                            row.format = f;
                        }
                    }
                    None => {}
                }
                self.menu = None;
            }
            Hit::MenuClose => self.menu = None,
            Hit::RemoveRow(i) => {
                if self.rows.len() > 1 && i < self.rows.len() {
                    self.rows.remove(i);
                    if let Focus::Suffix(f) = self.focus {
                        if f == i {
                            self.focus = Focus::None;
                        }
                    }
                }
            }
            Hit::AddScale => {
                let next = self.rows.last().map(|r| r.scale).unwrap_or(1.0);
                self.rows.push(Row {
                    scale: next,
                    suffix: String::new(),
                    format: self.rows.last().map(|r| r.format).unwrap_or(Format::Png),
                });
            }
            Hit::ToggleItem(i) => {
                if let Some(it) = self.items.get_mut(i) {
                    it.checked = !it.checked;
                }
            }
            Hit::ClearSelection => {
                for it in &mut self.items {
                    it.checked = false;
                }
            }
            Hit::Cancel => return Outcome::Cancel,
            Hit::Export => return Outcome::Run,
            Hit::None => {}
        }
        Outcome::None
    }

    /// Type into whichever field has the caret.
    pub fn push_char(&mut self, ch: char) {
        match self.focus {
            Focus::Range if !ch.is_control() => self.range.push(ch),
            Focus::Prefix if !ch.is_control() => self.prefix.push(ch),
            Focus::Suffix(i) if !ch.is_control() => {
                if let Some(r) = self.rows.get_mut(i) {
                    r.suffix.push(ch);
                }
            }
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.focus {
            Focus::Range => {
                self.range.pop();
            }
            Focus::Prefix => {
                self.prefix.pop();
            }
            Focus::Suffix(i) => {
                if let Some(r) = self.rows.get_mut(i) {
                    r.suffix.pop();
                }
            }
            Focus::None => {}
        }
    }

    pub fn defocus(&mut self) {
        self.focus = Focus::None;
        self.menu = None;
    }

    fn any_pdf(&self) -> bool {
        formats::any_pdf(&self.rows)
    }
}

/// A dropdown popup: the item rects, and whether it drops up or down.
struct MenuLayout {
    items: Vec<Rect>,
    frame: Rect,
}

fn menu_layout(anchor: Rect, n: usize, body: Rect) -> MenuLayout {
    let item_h = 22.0;
    let w = anchor.width().max(74.0);
    let h = n as f64 * item_h + 6.0;
    let up = anchor.y1 + h > body.y1 - 8.0;
    let (top, x0) = if up {
        (anchor.y0 - h, anchor.x0)
    } else {
        (anchor.y1 + 2.0, anchor.x0)
    };
    let frame = Rect::new(x0, top, x0 + w, top + h);
    let items = (0..n)
        .map(|i| {
            let y = frame.y0 + 3.0 + i as f64 * item_h;
            Rect::new(frame.x0 + 2.0, y, frame.x1 - 2.0, y + item_h)
        })
        .collect();
    MenuLayout { items, frame }
}

// --- layout --------------------------------------------------------

struct L {
    tab_artboards: Rect,
    tab_assets: Rect,
    grid: Rect,
    cells: Vec<Rect>,   // thumbnail rects, item order
    boxes: Vec<Rect>,   // tick-box rects
    select_box: Rect,
    r_all: Rect,
    r_range: Rect,
    range_field: Rect,
    r_full: Rect,
    dest_field: Rect,
    folder_btn: Rect,
    open_after: Rect,
    subfolders: Rect,
    r_sub_scale: Rect,
    r_sub_format: Rect,
    r_pdf_single: Rect,
    r_pdf_multi: Rect,
    fmt_rows: Vec<FmtRow>,
    add_scale: Rect,
    clear_sel: Rect,
    prefix_field: Rect,
    cancel: Rect,
    export: Rect,
}

#[derive(Clone, Copy)]
struct FmtRow {
    scale: Rect,
    suffix: Rect,
    format: Rect,
    remove: Rect,
}

fn layout(body: Rect, n_items: usize, n_rows: usize) -> L {
    let x0 = body.x0;
    let y0 = body.y0;
    let tab_y = y0 + 12.0;
    let tab_artboards = Rect::new(x0 + PAD + 60.0, tab_y, x0 + PAD + 168.0, tab_y + 28.0);
    let tab_assets = Rect::new(tab_artboards.x1 + 6.0, tab_y, tab_artboards.x1 + 96.0, tab_y + 28.0);

    let grid = Rect::new(
        x0 + PAD,
        tab_artboards.y1 + 12.0,
        x0 + RIGHT_X - 18.0,
        body.y1 - 60.0,
    );
    // 4 columns, square-ish cells.
    let cols = 4usize;
    let gap = 22.0;
    let cell = ((grid.width() - gap * (cols as f64 - 1.0)) / cols as f64).clamp(60.0, 140.0);
    let stride = cell + gap + 22.0; // + label row
    let mut cells = Vec::with_capacity(n_items);
    let mut boxes = Vec::with_capacity(n_items);
    for i in 0..n_items {
        let cx = grid.x0 + (i % cols) as f64 * (cell + gap);
        let cy = grid.y0 + (i / cols) as f64 * stride;
        let r = Rect::new(cx, cy, cx + cell, cy + cell);
        boxes.push(Rect::new(r.x0 + 4.0, r.y1 - 18.0, r.x0 + 18.0, r.y1 - 4.0));
        cells.push(r);
    }

    let rx = x0 + RIGHT_X;
    let rr = body.x1 - PAD;
    let mut y = grid.y0;

    // Select box.
    let select_box = Rect::new(rx, y + 18.0, rr, y + 130.0);
    let r_all = Rect::new(select_box.x0 + 12.0, select_box.y0 + 14.0, select_box.x0 + 30.0, select_box.y0 + 32.0);
    let r_range = Rect::new(select_box.x0 + 96.0, r_all.y0, select_box.x0 + 114.0, r_all.y1);
    let range_field = Rect::new(r_range.x1 + 18.0, r_range.y0 - 3.0, select_box.x1 - 14.0, r_range.y0 + 21.0);
    let r_full = Rect::new(select_box.x0 + 12.0, select_box.y0 + 62.0, select_box.x0 + 30.0, select_box.y0 + 80.0);
    y = select_box.y1 + 22.0;

    // Export to.
    y += 20.0; // label
    let folder_btn = Rect::new(rr - 34.0, y, rr, y + 30.0);
    let dest_field = Rect::new(rx, y, folder_btn.x0 - 10.0, y + 30.0);
    y = dest_field.y1 + 14.0;

    let open_after = Rect::new(rx, y, rx + 18.0, y + 18.0);
    y += ROW_H;
    let subfolders = Rect::new(rx, y, rx + 18.0, y + 18.0);
    y += ROW_H;
    let r_sub_scale = Rect::new(rx + 22.0, y, rx + 40.0, y + 18.0);
    let r_sub_format = Rect::new(rx + 108.0, y, rx + 126.0, y + 18.0);
    y += ROW_H + 8.0;

    // Export PDFs as.
    y += 20.0; // label
    let r_pdf_single = Rect::new(rx + 128.0, y, rx + 146.0, y + 18.0);
    let r_pdf_multi = Rect::new(rx + 236.0, y, rx + 254.0, y + 18.0);
    y += ROW_H + 8.0;

    // Formats table.
    y += 20.0; // "Formats:" label
    y += 22.0; // column headers
    let col_scale_x = rx + 6.0;
    let col_suffix_x = rx + 84.0;
    let col_format_x = rr - 158.0;
    let mut fmt_rows = Vec::with_capacity(n_rows);
    for _ in 0..n_rows {
        fmt_rows.push(FmtRow {
            scale: Rect::new(col_scale_x, y, col_scale_x + 70.0, y + FIELD_H),
            suffix: Rect::new(col_suffix_x, y, col_format_x - 12.0, y + FIELD_H),
            format: Rect::new(col_format_x, y, rr - 24.0, y + FIELD_H),
            remove: Rect::new(rr - 18.0, y + 4.0, rr - 2.0, y + 20.0),
        });
        y += FIELD_H + 8.0;
    }
    let add_scale = Rect::new(rx, y, rr, y + BTN_H);

    // Bottom bar.
    let by = body.y1 - PAD - BTN_H;
    let clear_sel = Rect::new(x0 + PAD + 180.0, by, x0 + PAD + 360.0, by + BTN_H);
    let prefix_field = Rect::new(clear_sel.x1 + 60.0, by + 3.0, clear_sel.x1 + 210.0, by + BTN_H - 3.0);
    let export = Rect::new(rr - 150.0, by, rr, by + BTN_H);
    let cancel = Rect::new(export.x0 - 12.0 - 100.0, by, export.x0 - 12.0, by + BTN_H);

    L {
        tab_artboards,
        tab_assets,
        grid,
        cells,
        boxes,
        select_box,
        r_all,
        r_range,
        range_field,
        r_full,
        dest_field,
        folder_btn,
        open_after,
        subfolders,
        r_sub_scale,
        r_sub_format,
        r_pdf_single,
        r_pdf_multi,
        fmt_rows,
        add_scale,
        clear_sel,
        prefix_field,
        cancel,
        export,
    }
}

// --- hit ----------------------------------------------------------

pub fn hit(dlg: &ExportForScreens, body: Rect, p: Point) -> Hit {
    let l = layout(body, dlg.items.len(), dlg.rows.len());

    // An open dropdown captures the next click.
    if let Some(m) = dlg.menu {
        let (anchor, n) = match m {
            OpenMenu::Scale(i) => (l.fmt_rows[i].scale, formats::SCALES.len()),
            OpenMenu::Format(i) => (l.fmt_rows[i].format, Format::ALL.len()),
        };
        let ml = menu_layout(anchor, n, body);
        for (k, r) in ml.items.iter().enumerate() {
            if r.contains(p) {
                return Hit::MenuPick(k);
            }
        }
        return Hit::MenuClose;
    }

    if l.tab_artboards.contains(p) {
        return Hit::Tab(Tab::Artboards);
    }
    if l.tab_assets.contains(p) {
        return Hit::Tab(Tab::Assets);
    }
    if l.cancel.contains(p) {
        return Hit::Cancel;
    }
    if l.export.contains(p) {
        return Hit::Export;
    }
    if l.clear_sel.contains(p) {
        return Hit::ClearSelection;
    }
    if l.prefix_field.contains(p) {
        return Hit::FocusPrefix;
    }

    if dlg.tab == Tab::Artboards {
        for (i, b) in l.boxes.iter().enumerate() {
            if b.contains(p) || l.cells[i].contains(p) {
                return Hit::ToggleItem(i);
            }
        }
    }

    if hit_radio(l.r_all, p) {
        return Hit::Mode(SelectMode::All);
    }
    if hit_radio(l.r_range, p) {
        return Hit::Mode(SelectMode::Range);
    }
    if l.range_field.contains(p) {
        return Hit::FocusRange;
    }
    if hit_radio(l.r_full, p) {
        return Hit::Mode(SelectMode::FullDocument);
    }
    // "Include Bleed" — its checkbox sits between the All/Range row and Full.
    let bleed_box = Rect::new(
        l.select_box.x0 + 12.0,
        l.r_all.y1 + 8.0,
        l.select_box.x0 + 30.0,
        l.r_all.y1 + 26.0,
    );
    if bleed_box.contains(p) {
        return Hit::ToggleBleed;
    }

    if l.folder_btn.contains(p) || l.dest_field.contains(p) {
        return Hit::PickFolder;
    }
    if hit_check(l.open_after, p) {
        return Hit::ToggleOpenAfter;
    }
    if hit_check(l.subfolders, p) {
        return Hit::ToggleSubfolders;
    }
    if dlg.subfolders {
        if hit_radio(l.r_sub_scale, p) {
            return Hit::SubBy(SubBy::Scale);
        }
        if hit_radio(l.r_sub_format, p) {
            return Hit::SubBy(SubBy::Format);
        }
    }
    if dlg.any_pdf() {
        if hit_radio(l.r_pdf_single, p) {
            return Hit::PdfMulti(false);
        }
        if hit_radio(l.r_pdf_multi, p) {
            return Hit::PdfMulti(true);
        }
    }

    for (i, fr) in l.fmt_rows.iter().enumerate() {
        if fr.remove.contains(p) {
            return Hit::RemoveRow(i);
        }
        if fr.scale.contains(p) {
            return Hit::OpenScale(i);
        }
        if fr.suffix.contains(p) {
            return Hit::FocusSuffix(i);
        }
        if fr.format.contains(p) {
            return Hit::OpenFormat(i);
        }
    }
    if l.add_scale.contains(p) {
        return Hit::AddScale;
    }
    Hit::None
}

fn hit_radio(r: Rect, p: Point) -> bool {
    r.inflate(2.0, 2.0).contains(p)
}
fn hit_check(r: Rect, p: Point) -> bool {
    // The label to the right is part of the target.
    Rect::new(r.x0, r.y0, r.x0 + 220.0, r.y1).contains(p)
}

// --- paint -------------------------------------------------------

pub fn paint(
    scene: &mut Scene,
    dlg: &ExportForScreens,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
    doc: &amalith_core::Document,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let l = layout(body, dlg.items.len(), dlg.rows.len());

    // Tabs.
    tab(scene, text, theme, l.tab_artboards, "Artboards", dlg.tab == Tab::Artboards);
    tab(scene, text, theme, l.tab_assets, "Assets", dlg.tab == Tab::Assets);

    // Left pane frame.
    let pane = Rect::new(l.grid.x0 - 10.0, l.grid.y0 - 10.0, l.grid.x1 + 10.0, l.grid.y1 + 10.0);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &pane);

    if dlg.tab == Tab::Assets {
        text.draw(
            scene,
            "No assets yet — flag objects with Asset Export.",
            13.0,
            theme.text_dim,
            l.grid.x0 + 8.0,
            l.grid.y0 + 30.0,
        );
    } else {
        scene.push_clip_layer(Fill::NonZero, ID, &pane);
        for (i, cell) in l.cells.iter().enumerate() {
            let it = &dlg.items[i];
            thumb(scene, doc, it.rect, *cell, theme);
            let border = if it.checked { theme.accent } else { theme.border };
            scene.stroke(&Stroke::new(if it.checked { 2.0 } else { 1.0 }), ID, border, None, cell);
            check(scene, l.boxes[i], it.checked, theme);
            text.draw(
                scene,
                &format!("{}", i + 1),
                11.0,
                theme.text_dim,
                cell.x0,
                cell.y1 + 15.0,
            );
            text.draw(scene, &it.name, 12.0, theme.text, cell.x0 + 16.0, cell.y1 + 15.0);
        }
        scene.pop_layer();
    }

    // ----- right column -----
    text.draw(scene, "Select:", 12.0, theme.text_dim, l.select_box.x0, l.select_box.y0 - 8.0);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &l.select_box);
    radio(scene, l.r_all, dlg.mode == SelectMode::All, theme);
    text.draw(scene, "All", 12.0, theme.text, l.r_all.x1 + 8.0, l.r_all.y0 + 14.0);
    radio(scene, l.r_range, dlg.mode == SelectMode::Range, theme);
    text.draw(scene, "Range:", 12.0, theme.text, l.r_range.x1 + 8.0, l.r_range.y0 + 14.0);
    field(scene, text, theme, l.range_field, &dlg.range, dlg.focus == Focus::Range && caret_on);
    let bleed_box = Rect::new(
        l.select_box.x0 + 12.0,
        l.r_all.y1 + 8.0,
        l.select_box.x0 + 30.0,
        l.r_all.y1 + 26.0,
    );
    check(scene, bleed_box, dlg.include_bleed, theme);
    text.draw(scene, "Include Bleed", 12.0, theme.text, bleed_box.x1 + 8.0, bleed_box.y0 + 14.0);
    radio(scene, l.r_full, dlg.mode == SelectMode::FullDocument, theme);
    text.draw(scene, "Full Document", 12.0, theme.text, l.r_full.x1 + 8.0, l.r_full.y0 + 14.0);

    text.draw(scene, "Export to:", 12.0, theme.text_dim, l.dest_field.x0, l.dest_field.y0 - 8.0);
    field(scene, text, theme, l.dest_field, &dlg.dest.display().to_string(), false);
    scene.fill(Fill::NonZero, ID, theme.strip_active, None, &l.folder_btn.to_rounded_rect(3.0));
    folder_glyph(scene, l.folder_btn, theme.text);

    check(scene, l.open_after, dlg.open_after, theme);
    text.draw(scene, "Open Location after Export", 12.0, theme.text, l.open_after.x1 + 8.0, l.open_after.y0 + 14.0);
    check(scene, l.subfolders, dlg.subfolders, theme);
    text.draw(scene, "Create Sub-folders", 12.0, theme.text, l.subfolders.x1 + 8.0, l.subfolders.y0 + 14.0);
    let sub_col = if dlg.subfolders { theme.text } else { theme.text_dim };
    radio(scene, l.r_sub_scale, dlg.sub_by == SubBy::Scale, theme);
    text.draw(scene, "Scale", 12.0, sub_col, l.r_sub_scale.x1 + 8.0, l.r_sub_scale.y0 + 14.0);
    radio(scene, l.r_sub_format, dlg.sub_by == SubBy::Format, theme);
    text.draw(scene, "Format", 12.0, sub_col, l.r_sub_format.x1 + 8.0, l.r_sub_format.y0 + 14.0);

    let pdf_col = if dlg.any_pdf() { theme.text } else { theme.text_dim };
    text.draw(scene, "Export PDFs as:", 12.0, theme.text_dim, l.select_box.x0, l.r_pdf_single.y0 - 8.0);
    radio(scene, l.r_pdf_single, !dlg.pdf_multi, theme);
    text.draw(scene, "Single File", 12.0, pdf_col, l.r_pdf_single.x1 + 8.0, l.r_pdf_single.y0 + 14.0);
    radio(scene, l.r_pdf_multi, dlg.pdf_multi, theme);
    text.draw(scene, "Multiple Files", 12.0, pdf_col, l.r_pdf_multi.x1 + 8.0, l.r_pdf_multi.y0 + 14.0);

    // Formats table.
    let ftitle_y = l.fmt_rows.first().map(|r| r.scale.y0 - 30.0).unwrap_or(l.add_scale.y0 - 60.0);
    text.draw(scene, "Formats:", 12.0, theme.text_dim, l.select_box.x0, ftitle_y);
    if let Some(fr) = l.fmt_rows.first() {
        let hy = fr.scale.y0 - 8.0;
        text.draw(scene, "Scale", 11.0, theme.text_dim, fr.scale.x0 + 4.0, hy);
        text.draw(scene, "Suffix", 11.0, theme.text_dim, fr.suffix.x0 + 4.0, hy);
        text.draw(scene, "Format", 11.0, theme.text_dim, fr.format.x0 + 4.0, hy);
    }
    for (i, fr) in l.fmt_rows.iter().enumerate() {
        let row = &dlg.rows[i];
        dropdown(scene, text, theme, fr.scale, &formats::scale_label(row.scale), !row.format.is_vector());
        field(scene, text, theme, fr.suffix, &row.suffix, dlg.focus == Focus::Suffix(i) && caret_on);
        dropdown(scene, text, theme, fr.format, row.format.label(), true);
        if dlg.rows.len() > 1 {
            x_glyph(scene, fr.remove, theme.text_dim);
        }
    }
    button(scene, text, theme, l.add_scale, "+  Add Scale", false);

    // Bottom bar.
    button(scene, text, theme, l.clear_sel, "Clear Selection", false);
    text.draw(scene, "Prefix:", 12.0, theme.text_dim, l.clear_sel.x1 + 8.0, l.prefix_field.y0 + 16.0);
    field(scene, text, theme, l.prefix_field, &dlg.prefix, dlg.focus == Focus::Prefix && caret_on);

    let n_sel = dlg.selected().len();
    let counts = format!("Selected: {},  Total Export: {}", n_sel, dlg.total_exports());
    let cw = text.measure(&counts, 12.0);
    text.draw(
        scene,
        &counts,
        12.0,
        theme.text_dim,
        body.x0 + (W - cw) * 0.5,
        l.cancel.y0 - 12.0,
    );
    button(scene, text, theme, l.cancel, "Cancel", false);
    button(scene, text, theme, l.export, "Export Artboard", true);

    // Open dropdown — drawn last so it sits over everything.
    if let Some(m) = dlg.menu {
        let (anchor, items): (Rect, Vec<String>) = match m {
            OpenMenu::Scale(i) => (
                l.fmt_rows[i].scale,
                formats::SCALES.iter().map(|s| formats::scale_label(*s)).collect(),
            ),
            OpenMenu::Format(i) => (
                l.fmt_rows[i].format,
                Format::ALL.iter().map(|f| f.label().to_string()).collect(),
            ),
        };
        let cur = match m {
            OpenMenu::Scale(i) => formats::SCALES
                .iter()
                .position(|s| (s - dlg.rows[i].scale).abs() < 1e-6),
            OpenMenu::Format(i) => Format::ALL.iter().position(|f| *f == dlg.rows[i].format),
        };
        let ml = menu_layout(anchor, items.len(), body);
        scene.fill(Fill::NonZero, ID, theme.bg, None, &ml.frame.to_rounded_rect(4.0));
        scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &ml.frame.to_rounded_rect(4.0));
        for (k, r) in ml.items.iter().enumerate() {
            if Some(k) == cur {
                scene.fill(Fill::NonZero, ID, theme.strip_active, None, r);
            }
            text.draw(scene, &items[k], 12.0, theme.text, r.x0 + 8.0, r.y0 + r.height() * 0.5 + 4.0);
        }
    }
}

const ID: Affine = Affine::IDENTITY;

/// A tiny vector preview of whatever sits under `ab` (document space),
/// mapped into `cell`.
fn thumb(scene: &mut Scene, doc: &amalith_core::Document, ab: amalith_core::Rect, cell: Rect, theme: &Theme) {
    scene.fill(Fill::NonZero, ID, theme.canvas_bg, None, &cell);
    let (aw, ah) = ((ab.x1 - ab.x0).max(1.0), (ab.y1 - ab.y0).max(1.0));
    let s = (cell.width() / aw).min(cell.height() / ah);
    let (dw, dh) = (aw * s, ah * s);
    let m = Affine::translate((
        cell.x0 + (cell.width() - dw) * 0.5,
        cell.y0 + (cell.height() - dh) * 0.5,
    )) * Affine::scale(s)
        * Affine::translate((-ab.x0, -ab.y0));
    scene.push_clip_layer(Fill::NonZero, ID, &cell);
    for layer in doc.layers().iter().filter(|l| l.visible) {
        for &id in doc.children_of(amalith_core::ObjectParent::Layer(layer.id)) {
            thumb_object(scene, doc, id, m);
        }
    }
    scene.pop_layer();
}

fn thumb_object(scene: &mut Scene, doc: &amalith_core::Document, id: amalith_core::ObjectId, m: Affine) {
    use amalith_core::ObjectKind;
    let Some(obj) = doc.object(id) else { return };
    if !obj.visible {
        return;
    }
    let om = m * crate::convert::affine(doc.world_transform(id));
    match &obj.kind {
        ObjectKind::Path(pd) => {
            let bez = crate::convert::bez_path(&pd.geometry);
            let col = obj
                .appearance
                .fill
                .color()
                .map(crate::convert::color)
                .unwrap_or(Color::from_rgb8(0x88, 0x88, 0x88));
            scene.fill(Fill::NonZero, om, col, None, &bez);
        }
        ObjectKind::CompoundPath(cp) => {
            let mut bez = BezPath::new();
            for sub in &cp.subpaths {
                bez.extend(crate::convert::bez_path(sub));
            }
            let col = obj
                .appearance
                .fill
                .color()
                .map(crate::convert::color)
                .unwrap_or(Color::from_rgb8(0x88, 0x88, 0x88));
            scene.fill(Fill::NonZero, om, col, None, &bez);
        }
        ObjectKind::Group(g) => {
            for &child in &g.children {
                thumb_object(scene, doc, child, m);
            }
        }
        _ => {
            if let Some(b) = doc.bounds_of(id) {
                scene.fill(
                    Fill::NonZero,
                    m,
                    Color::from_rgb8(0x55, 0x55, 0x55),
                    None,
                    &crate::convert::rect(b),
                );
            }
        }
    }
}

fn tab(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, on: bool) {
    if on {
        scene.fill(Fill::NonZero, ID, theme.strip_active, None, &r.to_rounded_rect(4.0));
    }
    let col = if on { theme.accent } else { theme.text_dim };
    let w = text.measure(label, 13.0);
    text.draw(scene, label, 13.0, col, r.x0 + (r.width() - w) * 0.5, r.y0 + r.height() * 0.5 + 4.5);
}

fn radio(scene: &mut Scene, r: Rect, on: bool, theme: &Theme) {
    let c = Circle::new((r.x0 + r.width() * 0.5, r.y0 + r.height() * 0.5), r.width() * 0.5);
    scene.stroke(&Stroke::new(1.2), ID, theme.text_dim, None, &c);
    if on {
        scene.fill(
            Fill::NonZero,
            ID,
            theme.accent,
            None,
            &Circle::new(c.center, r.width() * 0.28),
        );
    }
}

fn check(scene: &mut Scene, r: Rect, on: bool, theme: &Theme) {
    let rr = r.to_rounded_rect(3.0);
    scene.fill(Fill::NonZero, ID, if on { theme.accent } else { theme.bg }, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    if on {
        let mut p = BezPath::new();
        p.move_to((r.x0 + 3.5, r.y0 + r.height() * 0.55));
        p.line_to((r.x0 + r.width() * 0.42, r.y1 - 3.5));
        p.line_to((r.x1 - 3.0, r.y0 + 3.0));
        scene.stroke(&Stroke::new(1.8), ID, theme.on_accent, None, &p);
    }
}

fn field(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, val: &str, caret: bool) {
    let rr = r.to_rounded_rect(3.0);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    // Trim from the left so a long path stays readable at the tail.
    let mut shown = val.to_string();
    while text.measure(&shown, 12.0) > r.width() - 14.0 && shown.chars().count() > 3 {
        shown = format!("…{}", &shown[shown.char_indices().nth(2).map(|(i, _)| i).unwrap_or(0)..]);
    }
    text.draw(scene, &shown, 12.0, theme.text, r.x0 + 7.0, r.y0 + r.height() * 0.5 + 4.0);
    if caret {
        let cx = r.x0 + 7.0 + text.measure(&shown, 12.0) + 1.0;
        scene.fill(Fill::NonZero, ID, theme.text, None, &Rect::new(cx, r.y0 + 4.0, cx + 1.4, r.y1 - 4.0));
    }
}

fn dropdown(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, val: &str, enabled: bool) {
    let rr = r.to_rounded_rect(3.0);
    scene.fill(Fill::NonZero, ID, theme.bg, None, &rr);
    scene.stroke(&Stroke::new(1.0), ID, theme.border, None, &rr);
    let col = if enabled { theme.text } else { theme.text_dim };
    text.draw(scene, val, 12.0, col, r.x0 + 7.0, r.y0 + r.height() * 0.5 + 4.0);
    let cx = r.x1 - 12.0;
    let cy = r.y0 + r.height() * 0.5;
    let mut p = BezPath::new();
    p.move_to((cx - 3.5, cy - 2.0));
    p.line_to((cx + 3.5, cy - 2.0));
    p.line_to((cx, cy + 2.5));
    p.close_path();
    scene.fill(Fill::NonZero, ID, theme.text_dim, None, &p);
}

fn button(scene: &mut Scene, text: &mut TextContext, theme: &Theme, r: Rect, label: &str, primary: bool) {
    let rr = r.to_rounded_rect(5.0);
    scene.fill(
        Fill::NonZero,
        ID,
        if primary { theme.accent } else { theme.strip_active },
        None,
        &rr,
    );
    if !primary {
        scene.stroke(&Stroke::new(1.0), ID, theme.text_dim.with_alpha(0.6), None, &rr);
    }
    let col = if primary {
        Color::from_rgb8(0xff, 0xff, 0xff)
    } else {
        theme.text
    };
    let w = text.measure(label, 12.5);
    text.draw(scene, label, 12.5, col, r.x0 + (r.width() - w) * 0.5, r.y0 + r.height() * 0.5 + 4.5);
}

fn folder_glyph(scene: &mut Scene, r: Rect, col: Color) {
    let g = Rect::new(r.x0 + 8.0, r.y0 + 9.0, r.x1 - 8.0, r.y1 - 9.0);
    let mut p = BezPath::new();
    p.move_to((g.x0, g.y0 + 3.0));
    p.line_to((g.x0 + 5.0, g.y0 + 3.0));
    p.line_to((g.x0 + 7.0, g.y0));
    p.line_to((g.x1, g.y0));
    p.line_to((g.x1, g.y1));
    p.line_to((g.x0, g.y1));
    p.close_path();
    scene.stroke(&Stroke::new(1.3), ID, col, None, &p);
}

fn x_glyph(scene: &mut Scene, r: Rect, col: Color) {
    let mut p = BezPath::new();
    p.move_to((r.x0 + 3.0, r.y0 + 3.0));
    p.line_to((r.x1 - 3.0, r.y1 - 3.0));
    p.move_to((r.x1 - 3.0, r.y0 + 3.0));
    p.line_to((r.x0 + 3.0, r.y1 - 3.0));
    scene.stroke(&Stroke::new(1.4), ID, col, None, &p);
}

// --- helpers ----------------------------------------------------

fn default_range(items: &[Item]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        format!("1-{}", items.len())
    }
}

/// Parse `"1-3, 5, 7-8"` into the 1-based set it names, clamped to `n`.
fn parse_range(s: &str, n: usize) -> std::collections::HashSet<usize> {
    let mut out = std::collections::HashSet::new();
    for part in s.split([',', ' ']).filter(|p| !p.trim().is_empty()) {
        let part = part.trim();
        if let Some((a, b)) = part.split_once('-') {
            if let (Ok(a), Ok(b)) = (a.trim().parse::<usize>(), b.trim().parse::<usize>()) {
                for v in a.min(b)..=a.max(b) {
                    if (1..=n).contains(&v) {
                        out.insert(v);
                    }
                }
            }
        } else if let Ok(v) = part.parse::<usize>() {
            if (1..=n).contains(&v) {
                out.insert(v);
            }
        }
    }
    out
}
