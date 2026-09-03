//! A reusable vertical scroll region — the widget the ad-hoc
//! `scroll: f64` + hand-rolled clamp + hand-drawn scrollbar copies around
//! the app (Preferences lists, the New Document form, the font dropdown,
//! panels) should migrate to.
//!
//! The offset is **always re-clamped to the live content range** at
//! `begin` and on every wheel / drag, so a scroller can't accumulate an
//! out-of-range value and feel frozen while it unwinds — the bug the New
//! Document form hit.
//!
//! Usage:
//! ```ignore
//! let off = sv.begin(scene, theme, view_rect, content_h);
//! // draw content translated by -off, clipped to view_rect …
//! ScrollView::end(scene);
//! ```

use vello::kurbo::{Affine, Point, Rect};
use vello::peniko::{Color, Fill};
use vello::Scene;

use crate::theme::Theme;

const ID: Affine = Affine::IDENTITY;
/// Scrollbar gutter width and minimum thumb length, px.
const BAR_W: f64 = 4.0;
const MIN_THUMB: f64 = 24.0;
/// How far past the visible bar a press still counts as grabbing it.
const GRAB_SLOP: f64 = 6.0;

#[derive(Default)]
pub struct ScrollView {
    /// Requested offset. The effective value is this clamped to
    /// `[0, content_h - view_h]`, recomputed live.
    off: f64,
    /// `(content_h, view_h)` from the last `begin`, so wheel / drag can
    /// clamp without the caller repeating them.
    range: (f64, f64),
    view: Rect,
    /// Thumb rect from the last `begin` (`None` when content fits).
    thumb: Option<Rect>,
    /// Active thumb drag: `(pointer y at grab, offset at grab)`.
    drag: Option<(f64, f64)>,
}

impl ScrollView {
    pub fn new() -> Self {
        Self::default()
    }

    fn max(&self) -> f64 {
        (self.range.0 - self.range.1).max(0.0)
    }

    /// Clip to `view`, draw the scrollbar for `content_h`, and return the
    /// clamped offset. Caller draws content shifted up by the return
    /// value, then calls [`ScrollView::end`].
    pub fn begin(&mut self, scene: &mut Scene, theme: &Theme, view: Rect, content_h: f64) -> f64 {
        self.view = view;
        self.range = (content_h, view.height());
        self.off = self.off.clamp(0.0, self.max());
        self.thumb = None;

        let max = self.max();
        if max > 0.0 {
            let vh = view.height();
            let th = (vh * (vh / content_h)).clamp(MIN_THUMB, vh);
            let ty = view.y0 + (vh - th) * (self.off / max);
            let bar = Rect::new(view.x1 - BAR_W - 1.0, ty, view.x1 - 1.0, ty + th);
            let col = if self.drag.is_some() {
                theme.text_dim
            } else {
                Color::from_rgba8(0x9a, 0x9a, 0x9a, 0x88)
            };
            scene.fill(Fill::NonZero, ID, col, None, &bar.to_rounded_rect(BAR_W * 0.5));
            self.thumb = Some(bar);
        }

        scene.push_clip_layer(Fill::NonZero, ID, &view);
        self.off
    }

    pub fn end(scene: &mut Scene) {
        scene.pop_layer();
    }

    /// The clamped offset (valid after a `begin`).
    pub fn offset(&self) -> f64 {
        self.off.clamp(0.0, self.max())
    }

    pub fn set_offset(&mut self, off: f64) {
        self.off = off.clamp(0.0, self.max());
    }

    /// Feed a wheel delta (`dy` from `on_wheel`: positive scrolls up).
    pub fn wheel(&mut self, dy: f64) {
        self.off = (self.off - dy).clamp(0.0, self.max());
    }

    /// A press at `p` (screen px). Grabs the thumb, or pages the track.
    /// Returns whether it consumed the press.
    pub fn press(&mut self, p: Point) -> bool {
        let Some(thumb) = self.thumb else {
            return false;
        };
        if thumb.inflate(GRAB_SLOP, GRAB_SLOP).contains(p) {
            self.drag = Some((p.y, self.off));
            return true;
        }
        // A click in the gutter above / below the thumb pages toward it.
        let gutter = Rect::new(self.view.x1 - BAR_W - GRAB_SLOP, self.view.y0, self.view.x1, self.view.y1);
        if gutter.contains(p) {
            let page = self.range.1 * 0.9 * if p.y < thumb.y0 { -1.0 } else { 1.0 };
            self.off = (self.off + page).clamp(0.0, self.max());
            return true;
        }
        false
    }

    /// Continue a thumb drag; returns whether one is active.
    pub fn drag_to(&mut self, p: Point) -> bool {
        let Some((y0, off0)) = self.drag else {
            return false;
        };
        let vh = self.range.1;
        let th = (vh * (vh / self.range.0)).clamp(MIN_THUMB, vh);
        let travel = (vh - th).max(1.0);
        self.off = (off0 + (p.y - y0) / travel * self.max()).clamp(0.0, self.max());
        true
    }

    pub fn end_drag(&mut self) {
        self.drag = None;
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }
}
