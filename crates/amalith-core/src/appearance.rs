//! An object's fill and stroke paint.
use crate::ids::GradientId;
use crate::swatch::Color;
use serde::{Deserialize, Serialize};

/// What paints a fill or a stroke: nothing, a flat color, or a gradient.
///
/// [`Paint::Gradient`] carries only a [`GradientId`] into the document's
/// gradient pool — the stops and geometry live there, so `Paint` stays
/// `Copy` and a gradient can be shared between objects (and "saved as a
/// swatch", Illustrator-style).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    None,
    Solid(Color),
    Gradient(GradientId),
}

impl Paint {
    /// The flat color, if this paint is a solid one. `None` for `None` and
    /// for gradients (a gradient has no single color — resolve it against
    /// the pool if you need a representative one).
    pub fn color(self) -> Option<Color> {
        match self {
            Paint::Solid(color) => Some(color),
            Paint::None | Paint::Gradient(_) => None,
        }
    }

    /// The pooled gradient id, if this paint is a gradient.
    pub fn gradient_id(self) -> Option<GradientId> {
        match self {
            Paint::Gradient(id) => Some(id),
            Paint::None | Paint::Solid(_) => None,
        }
    }

    /// `true` for anything that puts pixels down (solid or gradient).
    pub fn is_visible(self) -> bool {
        !matches!(self, Paint::None)
    }
}

/// How a stroke's outline sits relative to the path: centred on it
/// (the classic default), tucked entirely inside a closed shape, or
/// pushed entirely outside it. Illustrator's Stroke panel "Align Stroke".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StrokeAlign {
    #[default]
    Center,
    Inside,
    Outside,
}

/// The shape drawn at an open path's endpoints. Mirrors `kurbo::Cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    /// "Projecting" in Illustrator's panel — a square that overshoots the
    /// endpoint by half the stroke weight.
    Square,
}

/// How a stroke turns a corner. Mirrors `kurbo::Join`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// Everything about a stroke except its paint and weight: the corner /
/// endpoint treatment, the miter cutoff, which side of the path it hugs,
/// and an optional dash pattern. Split out of [`Appearance`] so the whole
/// bundle round-trips through one command.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StrokeStyle {
    #[serde(default)]
    pub cap: LineCap,
    #[serde(default)]
    pub join: LineJoin,
    /// When a mitered corner's spike grows longer than `miter_limit`
    /// times the stroke weight, the join falls back to a bevel. This is
    /// Illustrator's "Limit: N x". Default 10, useful range 1..=500.
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f64,
    #[serde(default)]
    pub align: StrokeAlign,
    /// `false` = solid. `true` = use `dash` below.
    #[serde(default)]
    pub dashed: bool,
    /// Three dash/gap pairs, matching the six boxes in Illustrator's
    /// Stroke panel. A pair of `0.0` is skipped; if every entry is `0.0`
    /// the stroke renders solid even with `dashed` set.
    #[serde(default)]
    pub dash: [f64; 6],
    #[serde(default)]
    pub dash_offset: f64,
}

fn default_miter_limit() -> f64 {
    10.0
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: default_miter_limit(),
            align: StrokeAlign::Center,
            dashed: false,
            dash: [0.0; 6],
            dash_offset: 0.0,
        }
    }
}

impl StrokeStyle {
    /// The dash/gap run with empty pairs dropped, or `None` when the
    /// stroke is effectively solid (not dashed, or every box empty).
    pub fn dash_pattern(&self) -> Option<Vec<f64>> {
        if !self.dashed {
            return None;
        }
        let pat: Vec<f64> = self
            .dash
            .chunks(2)
            .filter(|p| p[0] > 0.0 || p[1] > 0.0)
            .flat_map(|p| [p[0].max(0.0), p[1].max(0.0)])
            .collect();
        if pat.is_empty() || pat.iter().all(|v| *v == 0.0) {
            None
        } else {
            Some(pat)
        }
    }
}

/// One entry in an object's appearance stack — Illustrator's Appearance
/// panel — either a fill or a stroke, each with its own paint and
/// opacity. An object can carry any number of these, in any order.
///
/// [`Appearance::items`] is the *sole* source of truth for what paints
/// and in what order — there is deliberately no second, disconnected
/// place a "current color" lives. Illustrator itself splits this: its
/// toolbar Fill/Stroke and its Appearance panel are two separate things,
/// so picking a color in one doesn't show up in the other, and "Add New
/// Fill" adds an unrelated item instead of building on the color you
/// just picked. Amalith's toolbar/Color-panel swatches edit the topmost
/// matching item directly (see [`Appearance::set_fill`]/`set_stroke`),
/// so a color picked there *is* immediately the Appearance panel's top
/// row.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum AppearanceItem {
    Fill {
        paint: Paint,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_true")]
        visible: bool,
    },
    Stroke {
        paint: Paint,
        width: f64,
        #[serde(default)]
        style: StrokeStyle,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_true")]
        visible: bool,
    },
}

fn default_true() -> bool {
    true
}

impl AppearanceItem {
    pub fn is_fill(&self) -> bool {
        matches!(self, AppearanceItem::Fill { .. })
    }

    pub fn is_stroke(&self) -> bool {
        matches!(self, AppearanceItem::Stroke { .. })
    }

    pub fn paint(&self) -> Paint {
        match *self {
            AppearanceItem::Fill { paint, .. } | AppearanceItem::Stroke { paint, .. } => paint,
        }
    }

    pub fn opacity(&self) -> f32 {
        match *self {
            AppearanceItem::Fill { opacity, .. } | AppearanceItem::Stroke { opacity, .. } => opacity,
        }
    }

    pub fn visible(&self) -> bool {
        match *self {
            AppearanceItem::Fill { visible, .. } | AppearanceItem::Stroke { visible, .. } => visible,
        }
    }
}

/// An object's fill/stroke appearance stack, plus its overall
/// compositing opacity. `items` is the ordered stack (see
/// [`AppearanceItem`]), stored **bottom-to-top**: index 0 paints first
/// (underneath), the last index paints last (on top) — mirroring how
/// the Layers panel already stores sibling order bottom-to-top and
/// reverses it for "frontmost on top" display. The Appearance panel
/// displays `items` reversed for the same reason.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "AppearanceOnDisk")]
pub struct Appearance {
    pub items: Vec<AppearanceItem>,
    /// Per-object compositing multiplier applied after each item's own
    /// opacity.
    pub opacity: f32,
}

fn default_opacity() -> f32 {
    1.0
}

impl Appearance {
    /// 1 pt.
    pub const DEFAULT_STROKE_WIDTH: f64 = 1.0;

    /// The topmost Fill item's paint, or [`Paint::None`] if the stack
    /// has no fill at all.
    pub fn fill(&self) -> Paint {
        self.items
            .iter()
            .rev()
            .find(|i| i.is_fill())
            .map_or(Paint::None, AppearanceItem::paint)
    }

    /// The topmost Stroke item's paint, or [`Paint::None`].
    pub fn stroke(&self) -> Paint {
        self.items
            .iter()
            .rev()
            .find(|i| i.is_stroke())
            .map_or(Paint::None, AppearanceItem::paint)
    }

    /// The topmost Stroke item's width, or [`Self::DEFAULT_STROKE_WIDTH`]
    /// if the stack has no stroke at all.
    pub fn stroke_width(&self) -> f64 {
        self.items.iter().rev().find_map(|i| match i {
            AppearanceItem::Stroke { width, .. } => Some(*width),
            AppearanceItem::Fill { .. } => None,
        }).unwrap_or(Self::DEFAULT_STROKE_WIDTH)
    }

    /// The topmost Stroke item's cap/join/dash/align, or the default.
    pub fn stroke_style(&self) -> StrokeStyle {
        self.items.iter().rev().find_map(|i| match i {
            AppearanceItem::Stroke { style, .. } => Some(*style),
            AppearanceItem::Fill { .. } => None,
        }).unwrap_or_default()
    }

    /// Replaces the topmost Fill item's paint, or pushes a new visible
    /// one on top if the stack has none. Used by the legacy batched
    /// Fill/Stroke commands (Color panel, context-bar swatches,
    /// eyedropper) so multi-select recoloring keeps editing "the"
    /// fill/stroke of possibly many objects at once, unchanged, under
    /// the new stack storage.
    pub fn set_fill(&mut self, paint: Paint) {
        match self.items.iter_mut().rev().find(|i| i.is_fill()) {
            Some(AppearanceItem::Fill { paint: p, .. }) => *p = paint,
            _ => self.items.push(AppearanceItem::Fill { paint, opacity: 1.0, visible: true }),
        }
    }

    pub fn set_stroke(&mut self, paint: Paint) {
        match self.items.iter_mut().rev().find(|i| i.is_stroke()) {
            Some(AppearanceItem::Stroke { paint: p, .. }) => *p = paint,
            _ => self.items.push(AppearanceItem::Stroke {
                paint,
                width: Self::DEFAULT_STROKE_WIDTH,
                style: StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
            }),
        }
    }

    pub fn set_stroke_width(&mut self, width: f64) {
        match self.items.iter_mut().rev().find(|i| i.is_stroke()) {
            Some(AppearanceItem::Stroke { width: w, .. }) => *w = width,
            _ => self.items.push(AppearanceItem::Stroke {
                paint: Paint::None,
                width,
                style: StrokeStyle::default(),
                opacity: 1.0,
                visible: true,
            }),
        }
    }

    pub fn set_stroke_style(&mut self, style: StrokeStyle) {
        match self.items.iter_mut().rev().find(|i| i.is_stroke()) {
            Some(AppearanceItem::Stroke { style: s, .. }) => *s = style,
            _ => self.items.push(AppearanceItem::Stroke {
                paint: Paint::None,
                width: Self::DEFAULT_STROKE_WIDTH,
                style,
                opacity: 1.0,
                visible: true,
            }),
        }
    }
}

impl Default for Appearance {
    /// Every new object's starting appearance: a light fill and a dark
    /// 1pt stroke, both visible immediately — not Illustrator's actual
    /// default (black fill, no stroke), because every primitive tool here
    /// is meant to draw with a visible stroke out of the box for now.
    fn default() -> Self {
        Self {
            items: vec![
                AppearanceItem::Fill {
                    paint: Paint::Solid(Color::rgb(0.87, 0.87, 0.87)),
                    opacity: 1.0,
                    visible: true,
                },
                AppearanceItem::Stroke {
                    paint: Paint::Solid(Color::rgb(0.18, 0.18, 0.18)),
                    width: Self::DEFAULT_STROKE_WIDTH,
                    style: StrokeStyle::default(),
                    opacity: 1.0,
                    visible: true,
                },
            ],
            opacity: default_opacity(),
        }
    }
}

/// On-disk shape for [`Appearance`] before the multi-item stack existed
/// — kept only as a `#[serde(from = ...)]` migration target so an old
/// `.amalith` file (a flat `fill`/`stroke`/`stroke_width`/`stroke_style`,
/// no `items`) still loads with the equivalent single-fill/single-stroke
/// stack. New saves only ever write the new shape (`items` + `opacity`);
/// this is a one-way migration, the same pattern every prior `Appearance`
/// field addition used (see the module's `#[serde(default)]` history).
#[derive(Deserialize)]
struct AppearanceOnDisk {
    #[serde(default = "default_none_paint")]
    fill: Paint,
    #[serde(default = "default_none_paint")]
    stroke: Paint,
    #[serde(default = "default_legacy_stroke_width")]
    stroke_width: f64,
    #[serde(default)]
    stroke_style: StrokeStyle,
    #[serde(default = "default_opacity")]
    opacity: f32,
    #[serde(default)]
    items: Option<Vec<AppearanceItem>>,
}

fn default_none_paint() -> Paint {
    Paint::None
}

fn default_legacy_stroke_width() -> f64 {
    1.0
}

impl From<AppearanceOnDisk> for Appearance {
    fn from(old: AppearanceOnDisk) -> Self {
        let items = old.items.unwrap_or_else(|| {
            vec![
                AppearanceItem::Fill { paint: old.fill, opacity: 1.0, visible: true },
                AppearanceItem::Stroke {
                    paint: old.stroke,
                    width: old.stroke_width,
                    style: old.stroke_style,
                    opacity: 1.0,
                    visible: true,
                },
            ]
        });
        Self { items, opacity: old.opacity }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_appearance_has_a_visible_stroke() {
        let appearance = Appearance::default();
        assert_eq!(appearance.items.len(), 2, "one Fill item, one Stroke item");
        assert_eq!(appearance.stroke().color(), Some(Color::rgb(0.18, 0.18, 0.18)));
        assert_eq!(appearance.stroke_width(), 1.0);
        assert_eq!(appearance.opacity, 1.0);
    }

    #[test]
    fn none_paint_has_no_color() {
        assert_eq!(Paint::None.color(), None);
    }

    #[test]
    fn fill_and_stroke_read_the_topmost_matching_item() {
        let mut a = Appearance {
            items: vec![
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true },
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(0.0, 1.0, 0.0)), opacity: 1.0, visible: true },
            ],
            opacity: 1.0,
        };
        assert_eq!(a.fill(), Paint::Solid(Color::rgb(0.0, 1.0, 0.0)), "the later (topmost) fill wins");
        assert_eq!(a.stroke(), Paint::None, "no stroke item at all");

        a.set_fill(Paint::Solid(Color::rgb(0.0, 0.0, 1.0)));
        assert_eq!(a.items.len(), 2, "set_fill edits the existing topmost fill, doesn't add a new one");
        assert_eq!(a.fill(), Paint::Solid(Color::rgb(0.0, 0.0, 1.0)));

        a.set_stroke(Paint::Solid(Color::rgb(0.0, 0.0, 0.0)));
        assert_eq!(a.items.len(), 3, "set_stroke pushes a new item when the stack has no stroke yet");
        assert_eq!(a.stroke().color(), Some(Color::rgb(0.0, 0.0, 0.0)));
    }

    /// A document saved before the multi-item stack existed has `fill`/
    /// `stroke`/`stroke_width`/`stroke_style` as flat top-level fields
    /// and no `items` at all — this must still load, synthesizing the
    /// equivalent single-fill/single-stroke stack. Built from raw JSON
    /// (not `Appearance::default()`, which always writes the *new*
    /// shape now) to genuinely simulate an old file, including one from
    /// before `stroke_style` itself existed (omitted here too).
    #[test]
    fn old_shape_without_items_synthesizes_a_two_item_stack() {
        let json = serde_json::json!({
            "fill": { "Solid": { "r": 1.0, "g": 0.0, "b": 0.0, "a": 1.0 } },
            "stroke": "None",
            "stroke_width": 3.0,
            "opacity": 0.5,
        });
        let appearance: Appearance = serde_json::from_value(json).unwrap();
        assert_eq!(appearance.opacity, 0.5);
        assert_eq!(appearance.items.len(), 2);
        assert_eq!(appearance.fill(), Paint::Solid(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(appearance.stroke(), Paint::None);
        assert_eq!(appearance.stroke_width(), 3.0);
        assert_eq!(appearance.stroke_style(), StrokeStyle::default(), "stroke_style defaults when the old file predates it too");
    }

    #[test]
    fn new_shape_with_items_round_trips_a_multi_item_stack() {
        let a = Appearance {
            items: vec![
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true },
                AppearanceItem::Stroke {
                    paint: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
                    width: 2.0,
                    style: StrokeStyle::default(),
                    opacity: 0.6,
                    visible: true,
                },
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(0.0, 0.0, 1.0)), opacity: 1.0, visible: false },
            ],
            opacity: 0.8,
        };
        let value = serde_json::to_value(&a).unwrap();
        assert!(value.get("fill").is_none(), "new saves don't write the legacy flat fields");
        let back: Appearance = serde_json::from_value(value).unwrap();
        assert_eq!(back, a);
    }

    #[test]
    fn old_serialized_appearance_defaults_opacity_to_one() {
        let mut value = serde_json::to_value(Appearance::default()).unwrap();
        value.as_object_mut().unwrap().remove("opacity");
        let appearance: Appearance = serde_json::from_value(value).unwrap();
        assert_eq!(appearance.opacity, 1.0);
    }

    #[test]
    fn dash_pattern_drops_empty_pairs_and_solid_stays_none() {
        let solid = StrokeStyle::default();
        assert_eq!(solid.dash_pattern(), None);

        let dashed = StrokeStyle {
            dashed: true,
            dash: [6.0, 3.0, 0.0, 0.0, 0.0, 0.0],
            ..StrokeStyle::default()
        };
        assert_eq!(dashed.dash_pattern(), Some(vec![6.0, 3.0]));

        let empty = StrokeStyle {
            dashed: true,
            ..StrokeStyle::default()
        };
        assert_eq!(empty.dash_pattern(), None);
    }
}
