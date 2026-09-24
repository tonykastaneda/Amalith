//! Adjustment layers on screen and in raster export.
//!
//! An adjustment changes everything beneath it in its own layer. Vello can't
//! run a color transform mid-scene, so the canvas works in two halves:
//!
//! 1. While a scene is built (`canvas::paint`, `canvas::export_scene`), a
//!    layer that holds active adjustments paints the content beneath each
//!    one into its own [`Scene`] instead, records a [`LayerJob`], and draws
//!    one placeholder image where that content goes. The placeholder is a
//!    stable [`ImageData`] with an empty blob, handed out by the
//!    [`AdjustCollector`] so it keeps the same identity frame to frame.
//! 2. Before the scene is rendered, [`engine::AdjustEngine`] renders each
//!    job's content offscreen, runs the color pass (`color.wgsl`), and swaps
//!    the result in for the placeholder with vello's `override_image`.
//!
//! A layer's steps chain: each adjustment's input is the previous one's
//! output plus whatever sits between them.

pub mod engine;

use std::collections::HashMap;

use amalith_core::appearance::BlendMode;
use amalith_core::{AdjustmentOp, Document, LayerId, ObjectId, ObjectKind};
use vello::kurbo::{Affine, Rect};
use vello::peniko::{Blob, ImageAlphaType, ImageData, ImageFormat};
use vello::Scene;

/// The largest side of an offscreen adjustment render, in pixels; beyond
/// it the job renders at a lower resolution and is drawn scaled up, which
/// keeps vello's image atlas (8192²) from overflowing.
const MAX_SIDE: f64 = 8192.0;

/// One adjustment's work within a layer.
pub struct Step {
    /// The adjustment's input, in the job's pixel space: the previous
    /// step's output (drawn first) plus the objects between the two.
    pub content: Scene,
    pub op: AdjustmentOp,
    pub blend: BlendMode,
    pub opacity: f32,
    /// Where the result is published: a later step's input, or (for the
    /// last step) the placeholder the canvas drew.
    pub output: ImageData,
}

/// The adjustments of one layer in one render.
pub struct LayerJob {
    pub width: u32,
    pub height: u32,
    pub steps: Vec<Step>,
}

/// Which of a layer's top-level children are adjustments that change
/// anything: visible, not fully transparent, a color op (blurs and noise
/// come later), and not an identity. Returns their indices in paint order.
pub fn active_adjustments(doc: &Document, children: &[ObjectId]) -> Vec<usize> {
    children
        .iter()
        .enumerate()
        .filter_map(|(i, &id)| {
            let obj = doc.object(id)?;
            let ObjectKind::Adjustment(data) = &obj.kind else { return None };
            let active = obj.visible
                && obj.appearance.opacity > 0.0
                && !data.op.is_spatial()
                && !data.op.is_identity();
            active.then_some(i)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct SlotKey {
    pane: u64,
    layer: LayerId,
    step: usize,
    width: u32,
    height: u32,
}

/// Collects adjustment jobs while a scene is built, and owns the stable
/// placeholder images they publish to.
pub struct AdjustCollector {
    enabled: bool,
    /// Physical pixels per scene unit (the window's DPI factor; 1 for export).
    dpi: f64,
    pane: u64,
    frame: u64,
    jobs: Vec<LayerJob>,
    slots: HashMap<SlotKey, (ImageData, u64)>,
    expired: Vec<ImageData>,
}

impl AdjustCollector {
    /// A collector that records nothing: every layer paints as it did
    /// before adjustments existed. For thumbnails, and for any frame the GPU
    /// engine isn't available to.
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            dpi: 1.0,
            pane: 0,
            frame: 0,
            jobs: Vec::new(),
            slots: HashMap::new(),
            expired: Vec::new(),
        }
    }

    /// Starts a frame. `enabled` must be false unless the engine will run
    /// this frame's jobs: a placeholder that never gets its texture panics
    /// in vello.
    pub fn begin_frame(&mut self, enabled: bool, dpi: f64) {
        self.enabled = enabled;
        self.dpi = dpi;
        self.pane = 0;
        self.frame += 1;
        self.jobs.clear();
    }

    /// Which canvas the following paints belong to (0 = the main view), so
    /// split panes keep their own placeholders.
    pub fn set_pane(&mut self, pane: u64) {
        self.pane = pane;
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Takes this frame's jobs, plus placeholders unused for a few frames
    /// (their textures should be released).
    pub fn finish(&mut self) -> (Vec<LayerJob>, Vec<ImageData>) {
        let frame = self.frame;
        let mut expired = std::mem::take(&mut self.expired);
        self.slots.retain(|_, (image, last)| {
            let keep = frame.saturating_sub(*last) < 3;
            if !keep {
                expired.push(image.clone());
            }
            keep
        });
        (std::mem::take(&mut self.jobs), expired)
    }

    fn slot(&mut self, layer: LayerId, step: usize, width: u32, height: u32) -> ImageData {
        let key = SlotKey { pane: self.pane, layer, step, width, height };
        let frame = self.frame;
        let entry = self.slots.entry(key).or_insert_with(|| {
            let image = ImageData {
                // Never read: the engine overrides it with a GPU texture.
                data: Blob::new(std::sync::Arc::new([])),
                format: ImageFormat::Rgba8,
                alpha_type: ImageAlphaType::Alpha,
                width,
                height,
            };
            (image, frame)
        });
        entry.1 = frame;
        entry.0.clone()
    }

    /// Paints one layer's children into `scene`, routing everything beneath
    /// its active adjustments through the engine. `region` is the part of
    /// scene space that's rendered (the canvas viewport, or an export's
    /// pixel rect); `paint` paints one child into a scene.
    pub fn paint_layer(
        &mut self,
        scene: &mut Scene,
        doc: &Document,
        layer: LayerId,
        children: &[ObjectId],
        region: Rect,
        mut paint: impl FnMut(&mut Scene, ObjectId),
    ) {
        let active = if self.enabled { active_adjustments(doc, children) } else { Vec::new() };
        let Some(&last) = active.last() else {
            for &id in children {
                paint(scene, id);
            }
            return;
        };
        let Some((width, height, to_pixels)) = job_space(region, self.dpi) else {
            for &id in children {
                paint(scene, id);
            }
            return;
        };
        let from_pixels = to_pixels.inverse();

        let mut steps = Vec::with_capacity(active.len());
        let mut start = 0;
        let mut previous: Option<ImageData> = None;
        for (i, &k) in active.iter().enumerate() {
            let Some(obj) = doc.object(children[k]) else { continue };
            let ObjectKind::Adjustment(data) = &obj.kind else { continue };
            let mut local = Scene::new();
            if let Some(input) = &previous {
                local.draw_image(input, from_pixels);
            }
            for &id in &children[start..k] {
                paint(&mut local, id);
            }
            let mut content = Scene::new();
            content.append(&local, Some(to_pixels));
            let output = self.slot(layer, i, width, height);
            steps.push(Step {
                content,
                op: data.op.clone(),
                blend: data.blend_mode,
                opacity: obj.appearance.opacity,
                output: output.clone(),
            });
            previous = Some(output);
            start = k + 1;
        }
        if let Some(result) = previous {
            scene.draw_image(&result, from_pixels);
            self.jobs.push(LayerJob { width, height, steps });
        }
        for &id in &children[last + 1..] {
            paint(scene, id);
        }
    }
}

/// The pixel grid a job renders into: `region` at `dpi` pixels per unit,
/// scaled down if a side would pass [`MAX_SIDE`]. Returns its size and the
/// map from scene space into it.
fn job_space(region: Rect, dpi: f64) -> Option<(u32, u32, Affine)> {
    if !(region.width() > 0.0 && region.height() > 0.0 && dpi > 0.0) {
        return None;
    }
    let longest = region.width().max(region.height()) * dpi;
    let scale = dpi * (MAX_SIDE / longest).min(1.0);
    let width = (region.width() * scale).ceil().max(1.0) as u32;
    let height = (region.height() * scale).ceil().max(1.0) as u32;
    Some((width, height, Affine::scale(scale) * Affine::translate(-region.origin().to_vec2())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{AdjustmentData, AdjustmentKind, Layer, LayerKind, Object, ObjectParent};

    fn doc_with(kinds: &[Option<AdjustmentOp>]) -> (Document, LayerId, Vec<ObjectId>) {
        let mut doc = Document::new("t");
        let mut layer = Layer::new(LayerId::new(), "L");
        layer.kind = LayerKind::Raster;
        let lid = layer.id;
        doc.insert_layer(layer, 0);
        let mut ids = Vec::new();
        for kind in kinds {
            let body = match kind {
                Some(op) => ObjectKind::Adjustment(AdjustmentData::new(op.clone())),
                None => ObjectKind::Image(amalith_core::ImageData {
                    asset: amalith_core::AssetId::new(),
                    local_bounds: amalith_core::Rect::ZERO,
                    mask: None,
                }),
            };
            let obj = Object::new(ObjectId::new(), ObjectParent::Layer(lid), body);
            ids.push(obj.id);
            doc.insert_object(obj, ids.len() - 1).unwrap();
        }
        (doc, lid, ids)
    }

    fn invert() -> Option<AdjustmentOp> {
        Some(AdjustmentOp::Invert)
    }

    #[test]
    fn identity_hidden_and_spatial_adjustments_are_skipped() {
        let (mut doc, _, ids) = doc_with(&[
            None,
            invert(),
            Some(AdjustmentKind::Levels.default_op()),     // identity
            Some(AdjustmentKind::GaussianBlur.default_op()), // spatial: later phase
            invert(),
        ]);
        assert_eq!(active_adjustments(&doc, &ids), vec![1, 4]);
        doc.object_mut(ids[4]).unwrap().visible = false;
        assert_eq!(active_adjustments(&doc, &ids), vec![1]);
        doc.object_mut(ids[1]).unwrap().appearance.opacity = 0.0;
        assert!(active_adjustments(&doc, &ids).is_empty());
    }

    #[test]
    fn a_layer_splits_into_chained_steps_and_paints_the_rest_on_top() {
        let (doc, layer, ids) = doc_with(&[None, None, invert(), None, invert(), None]);
        let mut c = AdjustCollector::disabled();
        c.begin_frame(true, 2.0);
        let mut painted_total = Vec::new();
        c.paint_layer(&mut Scene::new(), &doc, layer, &ids, Rect::new(10., 20., 110., 70.), |_, id| {
            painted_total.push(id);
        });
        // Every non-adjustment child is painted exactly once, in order.
        assert_eq!(painted_total, vec![ids[0], ids[1], ids[3], ids[5]]);
        let (jobs, _) = c.finish();
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!((job.width, job.height), (200, 100), "region at 2× DPI");
        assert_eq!(job.steps.len(), 2);
        assert_ne!(job.steps[0].output.data.id(), job.steps[1].output.data.id());
    }

    #[test]
    fn placeholders_are_stable_across_frames_and_expire_when_unused() {
        let (doc, layer, ids) = doc_with(&[None, invert()]);
        let region = Rect::new(0., 0., 50., 50.);
        let mut c = AdjustCollector::disabled();
        let mut run = |c: &mut AdjustCollector| {
            c.begin_frame(true, 1.0);
            c.paint_layer(&mut Scene::new(), &doc, layer, &ids, region, |_, _| {});
            c.finish()
        };
        let (a, _) = run(&mut c);
        let (b, _) = run(&mut c);
        assert_eq!(a[0].steps[0].output.data.id(), b[0].steps[0].output.data.id());

        let mut expired = Vec::new();
        for _ in 0..4 {
            c.begin_frame(true, 1.0);
            expired.extend(c.finish().1);
        }
        assert_eq!(expired.len(), 1, "the unused placeholder is released once");
    }

    #[test]
    fn a_disabled_collector_paints_every_child_directly() {
        let (doc, layer, ids) = doc_with(&[None, invert(), None]);
        let mut c = AdjustCollector::disabled();
        let mut painted = Vec::new();
        c.paint_layer(&mut Scene::new(), &doc, layer, &ids, Rect::new(0., 0., 10., 10.), |_, id| painted.push(id));
        assert_eq!(painted, vec![ids[0], ids[1], ids[2]]);
        assert!(c.finish().0.is_empty());
    }

    #[test]
    fn huge_regions_are_scaled_to_fit_the_atlas() {
        let (w, h, _) = job_space(Rect::new(0., 0., 10_000., 5_000.), 2.0).unwrap();
        assert_eq!((w, h), (8192, 4096));
    }
}
