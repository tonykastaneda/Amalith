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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "AppearanceItemOnDisk")]
pub enum AppearanceItem {
    Fill {
        paint: Paint,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_true")]
        visible: bool,
        /// This item's own live, non-destructive effect stack —
        /// Illustrator's Effect menu applied to one Appearance-panel row,
        /// distinct from the *destructive* Object ▸ Path commands (which
        /// insert a whole new sibling object instead). Ordered: each
        /// effect's output feeds the next one's input (see
        /// [`Effect`]'s own doc comment). Empty for most items.
        #[serde(default)]
        effects: Vec<Effect>,
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
        #[serde(default)]
        effects: Vec<Effect>,
    },
}

/// One live effect in an Appearance item's effect stack (see
/// [`AppearanceItem`]'s `effects` field) — Illustrator's own Effect menu,
/// applied to one Fill or Stroke row. Each variant is recomputed fresh
/// from the previous stage's geometry every time this item paints (the
/// first effect starts from the object's own current geometry, or for
/// text, its glyph outline) — nothing here is baked into the document's
/// real path data, the same "live" property the destructive Object ▸
/// Path commands deliberately don't have.
///
/// `Offset` was the first variant (Illustrator's own "Offset Path"), added
/// first because it needed no new rendering machinery — just the
/// polygon-offset primitive already behind the destructive Object ▸ Path
/// ▸ Offset Path command. The rest are Illustrator's Effect ▸ Distort &
/// Transform submenu (Free Distort excepted — it needs an on-canvas
/// corner-drag interaction, not a numeric dialog, so it isn't one of
/// these). The surrounding stack/panel/dialog-retargeting plumbing is
/// written generically over "whichever effect this is," so each variant
/// only ever adds itself here plus its own compute function in
/// `amalith_commands::pathfinder` — never a change to that plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Offset(OffsetEffect),
    ZigZag(ZigZagEffect),
    PuckerBloat(PuckerBloatEffect),
    Roughen(RoughenEffect),
    Transform(TransformEffect),
    Tweak(TweakEffect),
    Twist(TwistEffect),
}

/// [`Effect::Offset`]'s own parameters — negative `amount` insets,
/// positive outsets. Computed via `amalith_commands::pathfinder::offset_path`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct OffsetEffect {
    pub amount: f64,
    #[serde(default)]
    pub join: LineJoin,
    #[serde(default = "default_miter_limit")]
    pub miter_limit: f64,
}

/// [`Effect::ZigZag`]'s parameters — Illustrator's Effect ▸ Distort &
/// Transform ▸ Zig Zag. `size` is the ridge amplitude in document px;
/// `smooth` chooses rounded ridges over sharp corners.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ZigZagEffect {
    pub size: f64,
    #[serde(default = "default_ridges")]
    pub ridges_per_segment: f64,
    #[serde(default)]
    pub smooth: bool,
}

fn default_ridges() -> f64 {
    4.0
}

/// [`Effect::PuckerBloat`]'s parameter — `-100.0..=100.0`; negative
/// pulls each segment's handles toward its anchors (pucker), positive
/// pushes them out (bloat).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PuckerBloatEffect {
    pub amount: f64,
}

/// [`Effect::Roughen`]'s parameters — Illustrator's Effect ▸ Distort &
/// Transform ▸ Roughen. `size` is the jitter amplitude in document px,
/// `detail` the resample density (points per document inch). `seed` is
/// set once when the effect is added (never re-rolled on repaint) so the
/// jagged result stays stable across frames and after reload.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RoughenEffect {
    pub size: f64,
    #[serde(default = "default_detail")]
    pub detail: f64,
    #[serde(default)]
    pub smooth: bool,
    #[serde(default)]
    pub seed: u64,
}

fn default_detail() -> f64 {
    8.0
}

/// [`Effect::Transform`]'s parameters — Illustrator's Effect ▸ Distort &
/// Transform ▸ Transform, applied once around the item's own local-bounds
/// center. No `copies` field — stamping repeated transformed copies would
/// mean one effect step producing more than one output shape, which the
/// current one-shape-in-one-shape-out effect chain doesn't support; a
/// disclosed v1 gap, not silently half-built.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TransformEffect {
    #[serde(default)]
    pub move_x: f64,
    #[serde(default)]
    pub move_y: f64,
    #[serde(default = "default_scale")]
    pub scale_x: f64,
    #[serde(default = "default_scale")]
    pub scale_y: f64,
    #[serde(default)]
    pub rotate: f64,
    #[serde(default)]
    pub reflect_x: bool,
    #[serde(default)]
    pub reflect_y: bool,
}

fn default_scale() -> f64 {
    100.0
}

/// [`Effect::Tweak`]'s parameters — Illustrator's Effect ▸ Distort &
/// Transform ▸ Tweak. `horizontal`/`vertical` are jitter bounds as a
/// percentage of each segment's own length; `modify_*` picks which parts
/// of the path move. `seed` — see [`RoughenEffect`]'s own doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TweakEffect {
    pub horizontal: f64,
    pub vertical: f64,
    #[serde(default = "default_true")]
    pub modify_anchors: bool,
    #[serde(default = "default_true")]
    pub modify_in: bool,
    #[serde(default = "default_true")]
    pub modify_out: bool,
    #[serde(default)]
    pub seed: u64,
}

/// [`Effect::Twist`]'s parameter — rotation in degrees at the item's own
/// local-bounds center, decaying to zero at its bounds' farthest corner.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TwistEffect {
    pub angle: f64,
}

fn default_true() -> bool {
    true
}

/// On-disk shape for [`AppearanceItem`] before the effect *stack* existed
/// — kept only as a `#[serde(from = ...)]` migration target so an item
/// saved with the old single `offset: Option<OffsetEffect>` field still
/// loads with the equivalent one-entry `effects` stack. New saves only
/// ever write `effects`; one-way migration, same pattern as
/// [`AppearanceOnDisk`] one level up.
#[derive(Deserialize)]
enum AppearanceItemOnDisk {
    Fill {
        paint: Paint,
        #[serde(default = "default_opacity")]
        opacity: f32,
        #[serde(default = "default_true")]
        visible: bool,
        #[serde(default)]
        offset: Option<OffsetEffect>,
        #[serde(default)]
        effects: Option<Vec<Effect>>,
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
        #[serde(default)]
        offset: Option<OffsetEffect>,
        #[serde(default)]
        effects: Option<Vec<Effect>>,
    },
}

fn migrate_effects(offset: Option<OffsetEffect>, effects: Option<Vec<Effect>>) -> Vec<Effect> {
    effects.unwrap_or_else(|| offset.into_iter().map(Effect::Offset).collect())
}

impl From<AppearanceItemOnDisk> for AppearanceItem {
    fn from(old: AppearanceItemOnDisk) -> Self {
        match old {
            AppearanceItemOnDisk::Fill { paint, opacity, visible, offset, effects } => AppearanceItem::Fill {
                paint,
                opacity,
                visible,
                effects: migrate_effects(offset, effects),
            },
            AppearanceItemOnDisk::Stroke { paint, width, style, opacity, visible, offset, effects } => AppearanceItem::Stroke {
                paint,
                width,
                style,
                opacity,
                visible,
                effects: migrate_effects(offset, effects),
            },
        }
    }
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

    pub fn effects(&self) -> &[Effect] {
        match self {
            AppearanceItem::Fill { effects, .. } | AppearanceItem::Stroke { effects, .. } => effects,
        }
    }

    pub fn effects_mut(&mut self) -> &mut Vec<Effect> {
        match self {
            AppearanceItem::Fill { effects, .. } | AppearanceItem::Stroke { effects, .. } => effects,
        }
    }

    /// This item's Offset Path effect, if it has (at least) one — every
    /// call site that only ever dealt with "the one offset" keeps
    /// working via this, unaffected by other effect kinds landing later.
    /// Real Illustrator lets the same effect appear more than once in a
    /// stack; this reads the first.
    pub fn offset_effect(&self) -> Option<OffsetEffect> {
        self.effects().iter().find_map(|e| match e {
            Effect::Offset(fx) => Some(*fx),
            _ => None,
        })
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
            _ => self.items.push(AppearanceItem::Fill { paint, opacity: 1.0, visible: true, effects: Vec::new() }),
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
                effects: Vec::new(),
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
                effects: Vec::new(),
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
                effects: Vec::new(),
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
                    effects: Vec::new(),
                },
                AppearanceItem::Stroke {
                    paint: Paint::Solid(Color::rgb(0.18, 0.18, 0.18)),
                    width: Self::DEFAULT_STROKE_WIDTH,
                    style: StrokeStyle::default(),
                    opacity: 1.0,
                    visible: true,
                    effects: Vec::new(),
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
                AppearanceItem::Fill { paint: old.fill, opacity: 1.0, visible: true, effects: Vec::new() },
                AppearanceItem::Stroke {
                    paint: old.stroke,
                    width: old.stroke_width,
                    style: old.stroke_style,
                    opacity: 1.0,
                    visible: true,
                    effects: Vec::new(),
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
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, effects: Vec::new() },
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(0.0, 1.0, 0.0)), opacity: 1.0, visible: true, effects: Vec::new() },
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
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(1.0, 0.0, 0.0)), opacity: 1.0, visible: true, effects: Vec::new() },
                AppearanceItem::Stroke {
                    paint: Paint::Solid(Color::rgb(0.0, 0.0, 0.0)),
                    width: 2.0,
                    style: StrokeStyle::default(),
                    opacity: 0.6,
                    visible: true,
                    effects: vec![Effect::Offset(OffsetEffect { amount: -2.0, join: LineJoin::Round, miter_limit: 4.0 })],
                },
                AppearanceItem::Fill { paint: Paint::Solid(Color::rgb(0.0, 0.0, 1.0)), opacity: 1.0, visible: false, effects: Vec::new() },
            ],
            opacity: 0.8,
        };
        let value = serde_json::to_value(&a).unwrap();
        assert!(value.get("fill").is_none(), "new saves don't write the legacy flat fields");
        let back: Appearance = serde_json::from_value(value).unwrap();
        assert_eq!(back, a);
    }

    /// An item saved before the effect *stack* existed has a single
    /// top-level `offset` field on itself, no `effects` key at all — this
    /// must still load, wrapping that one effect into a one-entry stack.
    /// Built from raw JSON (not by constructing `AppearanceItem` and
    /// re-serializing, which always writes the *new* shape now) to
    /// genuinely simulate an old file.
    #[test]
    fn old_shaped_item_with_a_flat_offset_field_migrates_into_a_one_entry_stack() {
        let json = serde_json::json!({
            "Stroke": {
                "paint": "None",
                "width": 2.0,
                "offset": { "amount": -2.0, "join": "Round", "miter_limit": 4.0 },
            }
        });
        let item: AppearanceItem = serde_json::from_value(json).unwrap();
        assert_eq!(
            item.effects(),
            &[Effect::Offset(OffsetEffect { amount: -2.0, join: LineJoin::Round, miter_limit: 4.0 })],
        );
        assert_eq!(item.offset_effect(), Some(OffsetEffect { amount: -2.0, join: LineJoin::Round, miter_limit: 4.0 }));
    }

    #[test]
    fn old_shaped_item_with_neither_offset_nor_effects_migrates_to_an_empty_stack() {
        let json = serde_json::json!({
            "Fill": { "paint": "None" }
        });
        let item: AppearanceItem = serde_json::from_value(json).unwrap();
        assert_eq!(item.effects(), &[]);
    }

    #[test]
    fn effects_accessors_read_and_write_either_variant() {
        let fx = OffsetEffect { amount: -3.0, join: LineJoin::Bevel, miter_limit: 4.0 };
        let mut fill = AppearanceItem::Fill { paint: Paint::None, opacity: 1.0, visible: true, effects: Vec::new() };
        let mut stroke = AppearanceItem::Stroke {
            paint: Paint::None,
            width: 1.0,
            style: StrokeStyle::default(),
            opacity: 1.0,
            visible: true,
            effects: Vec::new(),
        };
        assert_eq!(fill.offset_effect(), None);
        assert_eq!(stroke.offset_effect(), None);
        fill.effects_mut().push(Effect::Offset(fx));
        stroke.effects_mut().push(Effect::Offset(fx));
        assert_eq!(fill.offset_effect(), Some(fx));
        assert_eq!(stroke.offset_effect(), Some(fx));
        fill.effects_mut().clear();
        assert_eq!(fill.offset_effect(), None, "clearing the stack drops a previously-added effect");
    }

    #[test]
    fn offset_effect_reads_the_first_offset_in_a_multi_effect_stack() {
        let a = OffsetEffect { amount: -1.0, join: LineJoin::Miter, miter_limit: 4.0 };
        let b = OffsetEffect { amount: 5.0, join: LineJoin::Round, miter_limit: 4.0 };
        let item = AppearanceItem::Fill {
            paint: Paint::None,
            opacity: 1.0,
            visible: true,
            effects: vec![Effect::Offset(a), Effect::Offset(b)],
        };
        assert_eq!(item.effects().len(), 2, "stacking the same effect twice is allowed, matching real Illustrator");
        assert_eq!(item.offset_effect(), Some(a), "reads the first Offset entry in the stack");
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
