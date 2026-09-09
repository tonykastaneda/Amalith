//! The exact-size shape dialogs — the **SHAPE DIALOG** frame.
//!
//! With a primitive tool active (Rectangle / Rounded Rectangle / Ellipse /
//! Polygon / Star), a plain click on the canvas — a press with no drag —
//! pops one of these instead of rubber-banding a shape. Fill in the
//! numbers, hit OK, and the shape is created at the click point (top-left
//! for rectangles / ellipses, centred for polygons / stars).
//!
//! It is a *panel*, spawned as its own floating window (like the colour
//! picker): draggable by its tab strip, closable by the ×, never dockable,
//! never in the Window menu.
//!
//! Layering — each level ignores the ones below it:
//!
//! ```text
//!   SHAPE DIALOG   this file: window/panel glue, title, OK / Cancel
//!       │
//!     sizing       sizing.rs: the numeric field stack (rows, steppers,
//!       │          link, keyboard, parse/format) — no shape knowledge
//!    ┌──┼──┐
//!  square circle …  one file per shape: its rows + its geometry
//!    │
//!  options        a shape's own extra controls, hanging off that shape
//! ```
//!
//! Adding a control to one shape stays inside that shape's file (or its
//! `options` submodule) — nothing here or in `sizing` changes.

mod arc;
mod ellipse;
mod polygon;
mod rectangle;
mod roundrect;
mod sizing;
mod spiral;
mod star;

use vello::kurbo::{Affine, Point, Rect};
use vello::peniko::Fill;
use vello::Scene;

use crate::text::TextContext;
use crate::theme::Theme;
use crate::tool::Tool;

pub(crate) use sizing::Field;
use sizing::Sizing;

/// Panel body width. The shell adds its tab-strip height for the window.
pub fn metric_w() -> f64 { crate::metrics::with(|m| m.shapedialog_w) }

fn metric_btn_h() -> f64 { crate::metrics::with(|m| m.shapedialog_btn_h) }
fn metric_btn_w() -> f64 { crate::metrics::with(|m| m.shapedialog_btn_w) }
fn metric_btn_gap() -> f64 { crate::metrics::with(|m| m.shapedialog_btn_gap) }
fn metric_gap_before_btns() -> f64 { crate::metrics::with(|m| m.shapedialog_gap_before_btns) }
fn metric_bot_pad() -> f64 { crate::metrics::with(|m| m.shapedialog_bot_pad) }

/// Per-shape behaviour. One implementor per file in this directory.
///
/// A shape supplies its rows (for the `sizing` layer), turns the committed
/// values into geometry, and remembers them. The `options_*` methods are
/// the seam for a shape's own controls below the rows: reserve height,
/// paint into the handed rect, turn a click there into a tag, and react to
/// it. They default to nothing — put real ones in a `<shape>/options.rs`.
pub(crate) trait Shape {
    /// The `sizing` rows, seeded from the remembered [`Params`].
    fn rows(&self, p: &Params) -> Vec<Field>;
    /// Build the shape at `anchor` (document space) from the committed row
    /// values, in row order.
    fn geometry(&self, anchor: Point, v: &[f64]) -> Geometry;
    /// Store the committed row values back into [`Params`].
    fn write_params(&self, v: &[f64], p: &mut Params);
    /// Prime this shape's own (non-row) state from the remembered
    /// [`Params`] — called once, right after construction. Only needed by
    /// a shape whose `Shape` impl isn't a zero-sized unit struct (i.e. one
    /// with its own options controls holding real state, like Arc's Type /
    /// Base Along / Fill Arc).
    fn seed(&mut self, _p: &Params) {}
    /// Show the `sizing` layer's Width/Height constrain-link (rows 0 & 1)?
    fn has_link(&self) -> bool {
        false
    }
    /// Force the created object's fill off regardless of the tool's
    /// current default fill (Arc's "Fill Arc" checkbox, unchecked).
    fn suppress_fill(&self) -> bool {
        false
    }

    /// Extra body height this shape's own controls need, below the rows.
    fn options_height(&self) -> f64 {
        0.0
    }
    /// Draw this shape's own controls into `area` (panel-body coords).
    fn paint_options(
        &self,
        _scene: &mut Scene,
        _area: Rect,
        _theme: &Theme,
        _text: &mut TextContext,
    ) {
    }
    /// Resolve a click at `local` inside `area` into a tag of the shape's
    /// choosing (`None` = not on a control).
    fn hit_options(&self, _area: Rect, _local: Point) -> Option<u32> {
        None
    }
    /// React to one of this shape's own controls. `true` = needs a repaint.
    fn on_option(&mut self, _tag: u32) -> bool {
        false
    }
}

fn shape_for(tool: Tool) -> Box<dyn Shape> {
    match tool {
        Tool::Rectangle => Box::new(rectangle::Rectangle),
        Tool::RoundedRect => Box::new(roundrect::RoundRect),
        Tool::Ellipse => Box::new(ellipse::Ellipse),
        Tool::Polygon => Box::new(polygon::Polygon),
        Tool::Star => Box::new(star::Star),
        Tool::Arc => Box::new(arc::Arc::default()),
        Tool::Spiral => Box::new(spiral::Spiral::default()),
        _ => Box::new(rectangle::Rectangle),
    }
}

/// Body height a dialog for `tool` needs.
pub fn body_height(tool: Tool) -> f64 {
    let shape = shape_for(tool);
    let rows = shape.rows(&Params::default()).len();
    sizing::stack_height(rows) + shape.options_height() + metric_gap_before_btns() + metric_btn_h() + metric_bot_pad()
}

/// Remembered values, so a dialog reopens with what was last entered.
#[derive(Clone, Copy)]
pub struct Params {
    pub rect: (f64, f64),
    pub round: (f64, f64, f64),
    pub ellipse: (f64, f64),
    pub polygon: (f64, f64),
    pub star: (f64, f64, f64),
    /// `(Length X-Axis, Length Y-Axis, Slope -100..100, Type is Open,
    /// Base Along X Axis, Fill Arc)`.
    pub arc: (f64, f64, f64, bool, bool, bool),
    /// `(Radius, Decay %, Segments, winds clockwise)`.
    pub spiral: (f64, f64, f64, bool),
}

impl Default for Params {
    fn default() -> Self {
        Self {
            rect: (100.0, 100.0),
            round: (100.0, 100.0, 20.0),
            ellipse: (100.0, 100.0),
            polygon: (50.0, 6.0),
            star: (19.0983, 50.0, 5.0),
            arc: (100.0, 100.0, 50.0, true, true, false),
            spiral: (50.0, 80.0, 10.0, true),
        }
    }
}

pub enum Geometry {
    Rect(amalith_core::Rect),
    Ellipse(amalith_core::Rect),
    Path(amalith_core::PathData),
}

/// Where a pointer at `local` (panel-body coordinates) landed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    None,
    Field(usize),
    Step(usize, i32),
    Link,
    /// A per-shape control in the options area — tag is the shape's own.
    Option(u32),
    Cancel,
    Ok,
}

pub struct ShapeDialog {
    pub tool: Tool,
    /// Where the user clicked, document space — the created shape's anchor.
    anchor_doc: Point,
    shape: Box<dyn Shape>,
    sizing: Sizing,
}

impl ShapeDialog {
    pub fn open(tool: Tool, anchor_doc: Point, p: &Params) -> Self {
        let mut shape = shape_for(tool);
        shape.seed(p);
        let sizing = Sizing::new(shape.rows(p), shape.has_link());
        Self {
            tool,
            anchor_doc,
            shape,
            sizing,
        }
    }

    pub fn title(&self) -> &'static str {
        match self.tool {
            Tool::Rectangle => "Rectangle",
            Tool::RoundedRect => "Rounded Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon",
            Tool::Star => "Star",
            Tool::Arc => "Arc Segment Tool Options",
            Tool::Spiral => "Spiral",
            _ => "Shape",
        }
    }

    fn options_area(&self, body: Rect) -> Rect {
        let y = body.y0 + self.sizing.height();
        Rect::new(
            body.x0 + sizing::metric_pad_x(),
            y,
            body.x1 - sizing::metric_pad_x(),
            y + self.shape.options_height(),
        )
    }

    fn ok_rect(&self, body: Rect) -> Rect {
        Rect::new(
            body.x1 - sizing::metric_pad_x() - metric_btn_w(),
            body.y1 - metric_bot_pad() - metric_btn_h(),
            body.x1 - sizing::metric_pad_x(),
            body.y1 - metric_bot_pad(),
        )
    }
    fn cancel_rect(&self, body: Rect) -> Rect {
        let ok = self.ok_rect(body);
        Rect::new(ok.x0 - metric_btn_gap() - metric_btn_w(), ok.y0, ok.x0 - metric_btn_gap(), ok.y1)
    }

    pub fn hit(&self, body: Rect, local: Point) -> Hit {
        if self.cancel_rect(body).contains(local) {
            return Hit::Cancel;
        }
        if self.ok_rect(body).contains(local) {
            return Hit::Ok;
        }
        match self.sizing.hit(body, local) {
            sizing::Hit::Field(i) => return Hit::Field(i),
            sizing::Hit::Step(i, d) => return Hit::Step(i, d),
            sizing::Hit::Link => return Hit::Link,
            sizing::Hit::None => {}
        }
        if self.shape.options_height() > 0.0 {
            if let Some(tag) = self.shape.hit_options(self.options_area(body), local) {
                return Hit::Option(tag);
            }
        }
        Hit::None
    }

    // --- editing (delegated to the sizing layer) --------------------

    pub fn focus_field(&mut self, i: usize) {
        self.sizing.focus_field(i);
    }
    pub fn push_char(&mut self, ch: char) {
        self.sizing.push_char(ch);
    }
    pub fn backspace(&mut self) {
        self.sizing.backspace();
    }
    pub fn focus_next(&mut self) {
        self.sizing.focus_next();
    }
    pub fn focus_prev(&mut self) {
        self.sizing.focus_prev();
    }
    pub fn step(&mut self, i: usize, delta: f64) {
        self.sizing.step(i, delta);
    }
    pub fn toggle_link(&mut self) {
        self.sizing.toggle_link();
    }
    pub fn commit_all(&mut self) {
        self.sizing.commit_all();
    }

    /// Apply a per-shape options click. Returns `true` if it changed state.
    pub fn apply_option(&mut self, tag: u32) -> bool {
        self.shape.on_option(tag)
    }

    pub fn write_params(&self, p: &mut Params) {
        self.shape.write_params(&self.sizing.values(), p);
    }

    /// Force the created object's fill off (Arc's "Fill Arc", unchecked).
    pub fn suppress_fill(&self) -> bool {
        self.shape.suppress_fill()
    }

    /// The rect / path data for the shape, in document space. Row values
    /// pass through as typed — clamping to whatever's sane (non-negative,
    /// a minimum size, …) is each shape's own `geometry()`'s job, since a
    /// row isn't always a length: Arc's Slope is a signed -100..100.
    pub fn geometry(&self) -> Geometry {
        let v = self.sizing.values();
        self.shape.geometry(self.anchor_doc, &v)
    }
}

// --- painting: the frame; the layers paint themselves ---------------

pub fn paint(
    scene: &mut Scene,
    dlg: &ShapeDialog,
    body: Rect,
    theme: &Theme,
    text: &mut TextContext,
    caret_on: bool,
) {
    scene.fill(Fill::NonZero, Affine::IDENTITY, theme.panel_bg, None, &body);
    dlg.sizing.paint(scene, body, theme, text, caret_on);
    if dlg.shape.options_height() > 0.0 {
        dlg.shape
            .paint_options(scene, dlg.options_area(body), theme, text);
    }
    crate::widgets::button(scene, text, theme, dlg.cancel_rect(body), "Cancel", false);
    crate::widgets::button(scene, text, theme, dlg.ok_rect(body), "OK", true);
}
