use amalith_core::Rect as DocumentRect;
use eframe::egui::{Pos2, Rect, Vec2};

pub(crate) const MIN_SCALE: f32 = 0.02;
pub(crate) const MAX_SCALE: f32 = 64.0;

/// Persistent document-to-screen view state.
///
/// `scale` is initialized from the first usable viewport and remains stable;
/// `pan` is a screen-space translation relative to the viewport's top-left.
pub(crate) struct Camera {
    pub(crate) scale: f32,
    pub(crate) pan: Vec2,
    initialized: bool,
    scrub_anchor: Option<Pos2>,
    pan_active: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            scale: 1.0,
            pan: Vec2::ZERO,
            initialized: false,
            scrub_anchor: None,
            pan_active: false,
        }
    }
}

impl Camera {
    pub(crate) fn initialize_fit(&mut self, viewport: Rect, document_bounds: Rect) {
        if self.initialized
            || viewport.width() <= 0.0
            || viewport.height() <= 0.0
            || document_bounds.width() <= 0.0
            || document_bounds.height() <= 0.0
        {
            return;
        }

        self.scale = (viewport.width() / document_bounds.width())
            .min(viewport.height() / document_bounds.height())
            .min(2.15)
            * 0.82;

        let scaled_center = document_bounds.center().to_vec2() * self.scale;
        self.pan = viewport.center() - viewport.min - scaled_center;
        self.initialized = true;
    }

    pub(crate) fn pan_by(&mut self, delta: Vec2) {
        self.pan += delta;
    }

    pub(crate) fn begin_pan(&mut self) {
        self.pan_active = true;
    }

    pub(crate) fn drag_pan(&mut self, delta: Vec2) {
        if self.pan_active {
            self.pan_by(delta);
        }
    }

    pub(crate) fn end_pan(&mut self) {
        self.pan_active = false;
    }

    pub(crate) fn is_panning(&self) -> bool {
        self.pan_active
    }

    pub(crate) fn zoom_at(&mut self, anchor_screen: Pos2, factor: f32, viewport: Rect) {
        let old_scale = self.scale;
        let new_scale = (old_scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        if new_scale == old_scale {
            return;
        }

        let document_point =
            (anchor_screen - viewport.min - self.pan) / old_scale.max(f32::MIN_POSITIVE);
        self.scale = new_scale;
        self.pan = anchor_screen - viewport.min - document_point * new_scale;
    }

    pub(crate) fn begin_scrub(&mut self, anchor_screen: Pos2) {
        self.end_pan();
        self.scrub_anchor = Some(anchor_screen);
    }

    pub(crate) fn scrub_zoom(&mut self, horizontal_delta: f32, viewport: Rect) {
        if let Some(anchor) = self.scrub_anchor {
            self.zoom_at(anchor, 2.0_f32.powf(horizontal_delta / 100.0), viewport);
        }
    }

    pub(crate) fn end_scrub(&mut self) {
        self.scrub_anchor = None;
    }

    pub(crate) fn document_to_screen(&self, document: Pos2, viewport: Rect) -> Pos2 {
        viewport.min + self.pan + document.to_vec2() * self.scale
    }

    pub(crate) fn screen_to_document(&self, screen: Pos2, viewport: Rect) -> Pos2 {
        let point = (screen - viewport.min - self.pan) / self.scale.max(f32::MIN_POSITIVE);
        Pos2::new(point.x, point.y)
    }

    pub(crate) fn visible_document_rect(&self, viewport: Rect) -> DocumentRect {
        let corners = [
            self.screen_to_document(viewport.left_top(), viewport),
            self.screen_to_document(viewport.right_top(), viewport),
            self.screen_to_document(viewport.right_bottom(), viewport),
            self.screen_to_document(viewport.left_bottom(), viewport),
        ];
        let (min_x, max_x) = corners
            .iter()
            .map(|point| point.x as f64)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), x| {
                (min.min(x), max.max(x))
            });
        let (min_y, max_y) = corners
            .iter()
            .map(|point| point.y as f64)
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(min, max), y| {
                (min.min(y), max.max(y))
            });
        DocumentRect::new(min_x, min_y, max_x, max_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_only_initializes_once() {
        let mut camera = Camera::default();
        let document = Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(100.0, 100.0));
        camera.initialize_fit(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 500.0)),
            document,
        );
        let initial_scale = camera.scale;
        let initial_pan = camera.pan;

        camera.initialize_fit(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0)),
            document,
        );

        assert_eq!(camera.scale, initial_scale);
        assert_eq!(camera.pan, initial_pan);
    }

    #[test]
    fn panning_moves_screen_position_one_for_one() {
        let mut camera = Camera::default();
        let viewport = Rect::from_min_size(Pos2::ZERO, Vec2::new(500.0, 500.0));
        let document = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        camera.initialize_fit(viewport, document);
        let before = camera.document_to_screen(Pos2::new(25.0, 25.0), viewport);

        camera.pan_by(Vec2::new(17.0, -9.0));

        assert_eq!(
            camera.document_to_screen(Pos2::new(25.0, 25.0), viewport),
            before + Vec2::new(17.0, -9.0)
        );
    }

    #[test]
    fn zoom_at_keeps_document_point_under_anchor() {
        let mut camera = Camera::default();
        let viewport = Rect::from_min_size(Pos2::new(30.0, 50.0), Vec2::new(500.0, 400.0));
        let document = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        camera.initialize_fit(viewport, document);
        let document_point = Pos2::new(37.0, 62.0);
        let anchor = camera.document_to_screen(document_point, viewport);

        camera.zoom_at(anchor, 2.0, viewport);

        let after = camera.document_to_screen(document_point, viewport);
        assert!((after.x - anchor.x).abs() < 0.001);
        assert!((after.y - anchor.y).abs() < 0.001);
    }

    #[test]
    fn screen_and_document_transforms_are_inverses() {
        let mut camera = Camera::default();
        let viewport = Rect::from_min_size(Pos2::new(30.0, 50.0), Vec2::new(500.0, 400.0));
        camera.initialize_fit(
            viewport,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0)),
        );
        let document = Pos2::new(18.0, 73.0);
        let roundtrip =
            camera.screen_to_document(camera.document_to_screen(document, viewport), viewport);
        assert!((roundtrip - document).length() < 0.001);
    }

    #[test]
    fn visible_document_rect_is_camera_view_aabb() {
        let mut camera = Camera::default();
        camera.scale = 2.0;
        camera.pan = Vec2::new(10.0, -20.0);
        let viewport = Rect::from_min_size(Pos2::new(30.0, 50.0), Vec2::new(100.0, 80.0));

        assert_eq!(
            camera.visible_document_rect(viewport),
            DocumentRect::new(-5.0, 10.0, 45.0, 50.0)
        );
    }
}
