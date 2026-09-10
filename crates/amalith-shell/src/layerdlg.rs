//! The Layer Options dialog — double-click a layer's color swatch in the
//! Layers panel. Name, Color (a fixed named palette, not an arbitrary RGB
//! picker — matching Illustrator's own dropdown), and the panel-style
//! Template / Lock / Show / Print / Preview / Dim Images checkboxes, all
//! committed together on OK as one `Command::SetLayerOptions`. Layout /
//! hit-testing / painting live here; `app/layer_dialog.rs` is the
//! `App`-side glue (spawning the floating window, closing it, keyboard),
//! mirroring `offsetdlg.rs` / `app/offset_dialog.rs`.

use crate::metrics::px as ui_px;

use amalith_commands::LayerOptions;
use amalith_core::{Layer, LayerColor, LayerId};
use vello::kurbo::{Affine, BezPath, Point, Rect, Stroke};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;

pub fn metric_w() -> f64 { crate::metrics::with(|m| m.layerdlg_w) }
fn metric_pad() -> f64 { crate::metrics::with(|m| m.layerdlg_pad) }
fn metric_field_h() -> f64 { crate::metrics::with(|m| m.layerdlg_field_h) }
fn metric_row_gap() -> f64 { crate::metrics::with(|m| m.layerdlg_row_gap) }
fn metric_label_w() -> f64 { crate::metrics::with(|m| m.layerdlg_label_w) }
fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.layerdlg_btn_h) }
fn metric_menu_row_h() -> f64 { crate::metrics::with(|m| m.layerdlg_menu_row_h) }

pub struct LayerOptionsDialog {
    pub id: LayerId,
    pub name: String,
    pub color: LayerColor,
    /// The Color dropdown's open/closed state.
    pub color_menu_open: bool,
    pub visible: bool,
    pub locked: bool,
    pub template: bool,
    pub print: bool,
    pub preview: bool,
    /// The "Dim Images to" checkbox — `Some(pct text)` when checked.
    pub dim_images: Option<String>,
}

impl LayerOptionsDialog {
    pub fn open(layer: &Layer) -> Self {
        Self {
            id: layer.id,
            name: layer.name.clone(),
            color: layer.color,
            color_menu_open: false,
            visible: layer.visible,
            locked: layer.locked,
            template: layer.template,
            print: layer.print,
            preview: layer.preview,
            dim_images: layer.dim_images_to.map(|p| p.to_string()),
        }
    }

    /// The percentage `Command::SetLayerOptions` should commit — clamped
    /// to `0..=100`, defaulting to 50 (Illustrator's own default) for an
    /// empty or unparseable field.
    fn resolved_dim_pct(&self) -> Option<u8> {
        self.dim_images.as_ref().map(|s| {
            s.trim().parse::<u8>().unwrap_or(50).min(100)
        })
    }

    pub fn options(&self) -> LayerOptions {
        LayerOptions {
            name: if self.name.trim().is_empty() { "Layer".to_string() } else { self.name.clone() },
            color: self.color,
            visible: self.visible,
            locked: self.locked,
            template: self.template,
            print: self.print,
            preview: self.preview,
            dim_images_to: self.resolved_dim_pct(),
        }
    }

    pub fn push_char(&mut self, ch: char) {
        if self.name.len() < 64 {
            self.name.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        self.name.pop();
    }
}

struct Layout {
    name_field: Rect,
    color_field: Rect,
    color_swatch: Rect,
    /// Populated only while the dropdown is open — one row per
    /// `LayerColor::ALL` entry, in order.
    color_items: Vec<Rect>,
    template: Rect,
    lock: Rect,
    show: Rect,
    print: Rect,
    preview: Rect,
    dim_images: Rect,
    dim_pct_field: Rect,
    ok: Rect,
    cancel: Rect,
}

fn layout(dlg: &LayerOptionsDialog, body: Rect) -> Layout {
    let x0 = body.x0 + metric_pad();
    let x1 = body.x1 - metric_pad();
    let mut y = body.y0 + metric_pad();
    let name_field = Rect::new(x0 + metric_label_w(), y, x1, y + metric_field_h());
    y += metric_field_h() + metric_row_gap();
    let color_field = Rect::new(x0 + metric_label_w(), y, x1 - ui_px(34.0), y + metric_field_h());
    let color_swatch = Rect::new(x1 - ui_px(28.0), y, x1, y + metric_field_h());
    let color_items = if dlg.color_menu_open {
        LayerColor::ALL
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let iy = color_field.y1 + i as f64 * metric_menu_row_h();
                Rect::new(color_field.x0, iy, x1, iy + metric_menu_row_h())
            })
            .collect()
    } else {
        Vec::new()
    };
    y += metric_field_h() + metric_row_gap();
    let col_w = (x1 - x0) * 0.5;
    let template = Rect::new(x0, y, x0 + col_w - ui_px(4.0), y + ui_px(18.0));
    let lock = Rect::new(x0 + col_w + ui_px(4.0), y, x1, y + ui_px(18.0));
    y += ui_px(28.0);
    let show = Rect::new(x0, y, x0 + col_w - ui_px(4.0), y + ui_px(18.0));
    let print = Rect::new(x0 + col_w + ui_px(4.0), y, x1, y + ui_px(18.0));
    y += ui_px(28.0);
    let preview = Rect::new(x0, y, x0 + col_w - ui_px(4.0), y + ui_px(18.0));
    let dim_images = Rect::new(x0 + col_w + ui_px(4.0), y, x0 + col_w + ui_px(4.0) + ui_px(112.0), y + ui_px(18.0));
    let dim_pct_field = Rect::new(x1 - ui_px(46.0), y - ui_px(3.0), x1, y + ui_px(15.0));
    y += ui_px(28.0) + ui_px(6.0);
    let btn_w = ui_px(72.0);
    let ok = Rect::new(x1 - btn_w, y, x1, y + metric_btn_h());
    let cancel = Rect::new(ok.x0 - ui_px(8.0) - btn_w, y, ok.x0 - ui_px(8.0), y + metric_btn_h());
    Layout {
        name_field,
        color_field,
        color_swatch,
        color_items,
        template,
        lock,
        show,
        print,
        preview,
        dim_images,
        dim_pct_field,
        ok,
        cancel,
    }
}

/// Body height (window-local, excluding the tab strip) — fixed regardless
/// of whether the color dropdown is open, since the open dropdown paints
/// as an overlay rather than pushing the rest of the dialog down.
pub fn body_height() -> f64 {
    metric_pad()
        + 2.0 * (metric_field_h() + metric_row_gap())
        + 3.0 * ui_px(28.0)
        + ui_px(6.0)
        + metric_btn_h()
        + metric_pad()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Name,
    ToggleColorMenu,
    ColorItem(usize),
    ToggleTemplate,
    ToggleLock,
    ToggleShow,
    TogglePrint,
    TogglePreview,
    ToggleDimImages,
    DimPct,
    Ok,
    Cancel,
    None,
}

pub fn hit(dlg: &LayerOptionsDialog, body: Rect, p: Point) -> Hit {
    let lay = layout(dlg, body);
    // Open dropdown items take priority over everything below them.
    if dlg.color_menu_open {
        for (i, r) in lay.color_items.iter().enumerate() {
            if r.contains(p) {
                return Hit::ColorItem(i);
            }
        }
        // Anywhere else while open just closes it (click-outside).
        if !lay.color_field.contains(p) && !lay.color_swatch.contains(p) {
            return Hit::ToggleColorMenu;
        }
    }
    if lay.name_field.contains(p) {
        return Hit::Name;
    }
    if lay.color_field.contains(p) || lay.color_swatch.contains(p) {
        return Hit::ToggleColorMenu;
    }
    if lay.template.contains(p) {
        return Hit::ToggleTemplate;
    }
    if lay.lock.contains(p) {
        return Hit::ToggleLock;
    }
    if lay.show.contains(p) {
        return Hit::ToggleShow;
    }
    if lay.print.contains(p) {
        return Hit::TogglePrint;
    }
    if lay.preview.contains(p) {
        return Hit::TogglePreview;
    }
    if dlg.dim_images.is_some() && lay.dim_pct_field.contains(p) {
        return Hit::DimPct;
    }
    if lay.dim_images.contains(p) {
        return Hit::ToggleDimImages;
    }
    if lay.ok.contains(p) {
        return Hit::Ok;
    }
    if lay.cancel.contains(p) {
        return Hit::Cancel;
    }
    Hit::None
}

const ID: Affine = Affine::IDENTITY;

fn checkbox(scene: &mut Scene, tcx: &mut TextContext, theme: &Theme, r: Rect, label: &str, on: bool) {
    let box_ = Rect::new(r.x0, r.y0, r.x0 + ui_px(16.0), r.y0 + ui_px(16.0));
    scene.fill(Fill::NonZero, ID, if on { theme.accent } else { theme.bg }, None, &box_.to_rounded_rect(ui_px(3.0)));
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &box_.to_rounded_rect(ui_px(3.0)));
    if on {
        let mut tick = BezPath::new();
        tick.move_to((box_.x0 + ui_px(3.5), box_.y0 + ui_px(8.5)));
        tick.line_to((box_.x0 + ui_px(6.5), box_.y0 + ui_px(11.5)));
        tick.line_to((box_.x0 + ui_px(12.5), box_.y0 + ui_px(4.5)));
        scene.stroke(&Stroke::new(ui_px(1.8)), ID, theme.on_accent, None, &tick);
    }
    tcx.draw(scene, label, 12.0, theme.text, box_.x1 + ui_px(8.0), box_.y0 + ui_px(13.0));
}

pub fn paint(
    scene: &mut Scene,
    dlg: &LayerOptionsDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, ID, theme.panel_bg, None, &body);
    let lay = layout(dlg, body);

    // Name.
    text.draw(scene, "Name:", 12.5, theme.text_dim, body.x0 + metric_pad(), lay.name_field.center().y + ui_px(4.5));
    scene.fill(Fill::NonZero, ID, theme.bg, None, &lay.name_field);
    scene.stroke(&Stroke::new(ui_px(1.5)), ID, theme.accent, None, &lay.name_field);
    let shown = if caret_on { format!("{}|", dlg.name) } else { dlg.name.clone() };
    text.draw(scene, &shown, 13.0, theme.text, lay.name_field.x0 + ui_px(10.0), lay.name_field.center().y + ui_px(4.5));

    // Color dropdown + swatch.
    text.draw(scene, "Color:", 12.5, theme.text_dim, body.x0 + metric_pad(), lay.color_field.center().y + ui_px(4.5));
    scene.fill(Fill::NonZero, ID, theme.bg, None, &lay.color_field);
    scene.stroke(
        &Stroke::new(if dlg.color_menu_open { 1.5 } else { 1.0 }),
        ID,
        if dlg.color_menu_open { theme.accent } else { theme.text_dim.with_alpha(0.5) },
        None,
        &lay.color_field,
    );
    let dot_r = ui_px(5.0);
    let dot_c = Point::new(lay.color_field.x0 + ui_px(14.0), lay.color_field.center().y);
    scene.fill(Fill::NonZero, ID, crate::convert::color(dlg.color.rgb()), None, &vello::kurbo::Circle::new(dot_c, dot_r));
    text.draw(scene, dlg.color.label(), 12.5, theme.text, lay.color_field.x0 + ui_px(26.0), lay.color_field.center().y + ui_px(4.5));
    // Chevron.
    let cx = lay.color_field.x1 - ui_px(12.0);
    let cy = lay.color_field.center().y;
    let mut chev = BezPath::new();
    chev.move_to((cx - ui_px(4.0), cy - ui_px(2.0)));
    chev.line_to((cx, cy + ui_px(2.5)));
    chev.line_to((cx + ui_px(4.0), cy - ui_px(2.0)));
    scene.stroke(&Stroke::new(ui_px(1.4)), ID, theme.text_dim, None, &chev);
    // Swatch, to the right of the dropdown.
    scene.fill(Fill::NonZero, ID, crate::convert::color(dlg.color.rgb()), None, &lay.color_swatch);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &lay.color_swatch);

    // Checkbox grid.
    checkbox(scene, text, theme, lay.template, "Template", dlg.template);
    checkbox(scene, text, theme, lay.lock, "Lock", dlg.locked);
    checkbox(scene, text, theme, lay.show, "Show", dlg.visible);
    checkbox(scene, text, theme, lay.print, "Print", dlg.print);
    checkbox(scene, text, theme, lay.preview, "Preview", dlg.preview);
    let dim_on = dlg.dim_images.is_some();
    checkbox(scene, text, theme, lay.dim_images, "Dim Images to:", dim_on);
    scene.fill(Fill::NonZero, ID, if dim_on { theme.bg } else { theme.bg.with_alpha(0.4) }, None, &lay.dim_pct_field);
    scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.text_dim.with_alpha(if dim_on { 0.5 } else { 0.2 }), None, &lay.dim_pct_field);
    if let Some(pct) = &dlg.dim_images {
        let shown = if caret_on { format!("{pct}|") } else { pct.clone() };
        text.draw(scene, &format!("{shown}%"), 12.0, theme.text, lay.dim_pct_field.x0 + ui_px(6.0), lay.dim_pct_field.center().y + ui_px(4.0));
    } else {
        text.draw(scene, "50%", 12.0, theme.text_dim.with_alpha(0.5), lay.dim_pct_field.x0 + ui_px(6.0), lay.dim_pct_field.center().y + ui_px(4.0));
    }

    crate::widgets::button(scene, text, theme, lay.cancel, "Cancel", false);
    crate::widgets::button(scene, text, theme, lay.ok, "OK", true);

    // The open dropdown paints last, on top of everything below it.
    if dlg.color_menu_open {
        let menu_rect = Rect::new(
            lay.color_field.x0,
            lay.color_field.y1,
            body.x1 - metric_pad(),
            lay.color_field.y1 + LayerColor::ALL.len() as f64 * metric_menu_row_h(),
        );
        scene.fill(Fill::NonZero, ID, theme.bg, None, &menu_rect);
        scene.stroke(&Stroke::new(ui_px(1.0)), ID, theme.border, None, &menu_rect);
        for (i, (c, r)) in LayerColor::ALL.iter().zip(lay.color_items.iter()).enumerate() {
            if *c == dlg.color {
                scene.fill(Fill::NonZero, ID, theme.accent.with_alpha(0.22), None, r);
            }
            let dot_c = Point::new(r.x0 + ui_px(14.0), r.center().y);
            scene.fill(Fill::NonZero, ID, crate::convert::color(c.rgb()), None, &vello::kurbo::Circle::new(dot_c, dot_r));
            text.draw(scene, c.label(), 12.0, theme.text, r.x0 + ui_px(26.0), r.center().y + ui_px(4.0));
            let _ = i;
        }
    }
}
