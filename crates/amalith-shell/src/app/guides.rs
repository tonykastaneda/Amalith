//! Ruler guides and the ruler right-click unit menu — the unit-menu
//! geometry / click, ruler-strip and guide hit-testing, the show / lock
//! toggles, and Clear / Release Guides. The guide *drag* (drawing one out
//! of a ruler, moving one) lives in `app/input`; the ruler strips and the
//! unit-menu flyout are painted in `app/render`. Split out of
//! `app/mod.rs`.

use super::*;

impl App {
    pub(in crate::app) const RM_W: f64 = 168.0;
    pub(in crate::app) const RM_ROW: f64 = 24.0;
    pub(in crate::app) const RM_PAD: f64 = 6.0;

    pub(in crate::app) fn ruler_menu_rect(anchor: Point) -> Rect {
        let n = amalith_core::Unit::ALL.len() as f64;
        let h = Self::RM_PAD * 2.0 + Self::RM_ROW * n;
        Rect::new(anchor.x, anchor.y, anchor.x + Self::RM_W, anchor.y + h)
    }

    /// A press while the ruler unit menu is open. Consumes it.
    pub(in crate::app) fn ruler_menu_click(&mut self, p: Point) {
        let Some(anchor) = self.ruler_menu.take() else {
            return;
        };
        let fly = Self::ruler_menu_rect(anchor);
        if fly.contains(p) {
            let i = ((p.y - fly.y0 - Self::RM_PAD) / Self::RM_ROW).floor();
            if i >= 0.0 {
                if let Some(&unit) = amalith_core::Unit::ALL.get(i as usize) {
                    let _ = self
                        .doc
                        .editor
                        .execute(Command::SetDocumentUnit { unit });
                }
            }
        }
        self.request_main_redraw();
    }

    /// Which guide axis a press over a ruler strip would create: the top
    /// strip drags out horizontal guides, the left strip vertical ones.
    pub(in crate::app) fn ruler_strip_at(&self, p: Point) -> Option<amalith_core::GuideOrient> {
        use amalith_core::GuideOrient;
        if !self.rulers || self.pointer_win != self.main_id {
            return None;
        }
        let r = self.canvas_region();
        if !r.contains(p) {
            return None;
        }
        if p.y < r.y0 + rulers::THICK {
            Some(GuideOrient::Horizontal)
        } else if p.x < r.x0 + rulers::THICK {
            Some(GuideOrient::Vertical)
        } else {
            None
        }
    }

    /// The topmost ruler guide within grab tolerance of `screen`, if guides
    /// are visible, unlocked, and the point is on the canvas.
    pub(in crate::app) fn guide_at(&self, screen: Point) -> Option<amalith_core::GuideId> {
        use amalith_core::GuideOrient;
        if self.guides_hidden || self.guides_locked || !self.canvas_viewport().contains(screen) {
            return None;
        }
        let vt = self.doc.view.to_screen();
        self.doc
            .editor
            .document()
            .guides()
            .iter()
            .rev()
            .find(|g| match g.orient {
                GuideOrient::Horizontal => {
                    ((vt * Point::new(0.0, g.pos)).y - screen.y).abs() <= 4.0
                }
                GuideOrient::Vertical => ((vt * Point::new(g.pos, 0.0)).x - screen.x).abs() <= 4.0,
            })
            .map(|g| g.id)
    }

    /// Flip a guide toggle and persist it.
    pub(in crate::app) fn set_guides_hidden(&mut self, hidden: bool) {
        self.guides_hidden = hidden;
        if hidden {
            self.selected_guides.clear();
        }
        self.sync_guide_menu();
        self.save_layout();
        self.request_main_redraw();
    }
    pub(in crate::app) fn set_guides_locked(&mut self, locked: bool) {
        self.guides_locked = locked;
        self.sync_guide_menu();
        self.save_layout();
        self.request_main_redraw();
    }
    fn sync_guide_menu(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        if let Some(m) = &self.native_menu {
            m.sync_guides(self.guides_hidden, self.guides_locked);
        }
    }

    pub(in crate::app) fn clear_guides(&mut self) {
        if !self.doc.editor.document().guides().is_empty() {
            let _ = self.doc.editor.execute(Command::ClearGuides);
            self.selected_guides.clear();
            self.request_main_redraw();
        }
    }

    /// Convert every ruler guide into an open line path spanning the
    /// current artboard, then drop the guides (Illustrator's Release
    /// Guides, for ruler guides).
    pub(in crate::app) fn release_guides(&mut self) {
        use amalith_core::{Anchor, GuideOrient, HandleMode, PathData, Point as CPoint, Subpath};
        let guides = self.doc.editor.document().guides().to_vec();
        if guides.is_empty() {
            return;
        }
        let span = self
            .current_artboard()
            .and_then(|id| self.doc.editor.document().artboard(id).map(|a| a.rect))
            .unwrap_or_else(|| amalith_core::Rect::new(-10_000.0, -10_000.0, 10_000.0, 10_000.0));
        let layer = self.ensure_layer();
        let corner = |p: CPoint| Anchor {
            point: p,
            handle_in: None,
            handle_out: None,
            mode: HandleMode::Corner,
        };
        for g in &guides {
            let (a, b) = match g.orient {
                GuideOrient::Horizontal => (
                    CPoint::new(span.x0, g.pos),
                    CPoint::new(span.x1, g.pos),
                ),
                GuideOrient::Vertical => (
                    CPoint::new(g.pos, span.y0),
                    CPoint::new(g.pos, span.y1),
                ),
            };
            let path = PathData::from_subpaths(vec![Subpath {
                anchors: vec![corner(a), corner(b)],
                closed: false,
            }]);
            let _ = self.doc.editor.execute(Command::CreatePath {
                layer,
                path,
                name: Some("Guide".into()),
            });
        }
        let _ = self.doc.editor.execute(Command::ClearGuides);
        self.selected_guides.clear();
        self.request_main_redraw();
    }
}
