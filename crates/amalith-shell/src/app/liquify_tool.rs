//! The liquify brushes — Warp (⇧R), Twirl, Pucker, Bloat, Scallop,
//! Crystallize and Wrinkle, in the Width flyout. A drag paints the brush
//! along the pointer's path over the selected paths (every editable path
//! in scope when nothing is selected); the stroke previews live and
//! commits as one `Command::Liquify` on release. Alt-drag sizes the
//! brush from its centre (Shift keeps it round); `[` / `]` shrink and
//! grow it. The brush is measured in document units, like Illustrator's.

use super::*;
use amalith_core::liquify::{self, LiquifyParams};
use vello::kurbo::Shape as _;

/// Brush size bounds (document units) and the `[` / `]` step factor.
pub(in crate::app) const LIQUIFY_MIN_SIZE: f64 = 2.0;
pub(in crate::app) const LIQUIFY_MAX_SIZE: f64 = 2000.0;
const LIQUIFY_STEP: f64 = 1.25;

impl App {
    /// The brush for the active tool: the shared size and strength, with
    /// the tool's own effect.
    fn liquify_params(&self) -> Option<LiquifyParams> {
        let kind = self.active_tool.liquify_kind()?;
        Some(LiquifyParams { kind, ..self.liquify })
    }

    /// Paths the brush bends: every editable path inside the selection
    /// (reaching into selected groups), or every editable path in scope
    /// when nothing is selected — the same scoping as Illustrator's.
    fn liquify_targets(&self) -> Vec<ObjectId> {
        let doc = self.doc.editor.document();
        let mut all = match self.isolation_root() {
            Some(root) => anchors::path_leaves_in(doc, root),
            None => anchors::path_leaves(doc),
        };
        all.retain(|&id| {
            if !matches!(doc.object(id).map(|o| &o.kind), Some(amalith_core::ObjectKind::Path(_))) {
                return false;
            }
            let mut cur = id;
            let mut in_selection = self.doc.selection.is_empty();
            loop {
                let Some(o) = doc.object(cur) else { return false };
                if o.locked || !o.visible {
                    return false;
                }
                in_selection |= self.doc.selection.contains(&cur);
                match o.parent {
                    amalith_core::ObjectParent::Layer(l) => {
                        return in_selection && doc.layer(l).is_some_and(|l| l.visible && !l.locked);
                    }
                    amalith_core::ObjectParent::Group(g) => cur = g,
                    amalith_core::ObjectParent::Symbol(_) => return in_selection,
                }
            }
        });
        all
    }

    /// Press: Alt starts sizing the brush, anything else starts a stroke.
    pub(in crate::app) fn liquify_press(&mut self) -> bool {
        if self.active_tool.liquify_kind().is_none() {
            return false;
        }
        let dp = self.doc_point(self.pointer);
        self.drag = if self.alt_down {
            Drag::LiquifyResize { center: dp }
        } else {
            let targets = self.liquify_targets();
            let mut drag = Drag::Liquify { stroke: vec![convert::point_to_core(dp)], targets, preview: Vec::new() };
            self.liquify_refresh(&mut drag);
            drag
        };
        self.request_main_redraw();
        true
    }

    /// Recomputes a live stroke's preview from scratch — the very same
    /// computation the command will run, so the release matches it.
    fn liquify_refresh(&self, drag: &mut Drag) {
        let (Drag::Liquify { stroke, targets, preview }, Some(prm)) = (drag, self.liquify_params()) else { return };
        let doc = self.doc.editor.document();
        let dab_pts = liquify::dabs(stroke, prm.dab_spacing());
        *preview = targets
            .iter()
            .enumerate()
            .filter_map(|(i, &id)| {
                let pd = doc.object(id)?.kind.path_data()?;
                let live = liquify::liquify_path(pd, doc.world_transform(id), &dab_pts, &prm, i as u64)?;
                Some((id, live))
            })
            .collect();
    }

    /// Pointer move during a stroke or a brush resize.
    pub(in crate::app) fn liquify_move(&mut self) {
        let dp = self.doc_point(self.pointer);
        let zoom = self.doc.view.zoom.max(1e-6);
        let mut drag = std::mem::take(&mut self.drag);
        match &mut drag {
            Drag::Liquify { stroke, .. } => {
                let p = convert::point_to_core(dp);
                if stroke.last().is_none_or(|&last| (last - p).hypot() * zoom > 1.5) {
                    stroke.push(p);
                    self.liquify_refresh(&mut drag);
                }
            }
            Drag::LiquifyResize { center } => {
                let w = ((dp.x - center.x).abs() * 2.0).clamp(LIQUIFY_MIN_SIZE, LIQUIFY_MAX_SIZE);
                let h = ((dp.y - center.y).abs() * 2.0).clamp(LIQUIFY_MIN_SIZE, LIQUIFY_MAX_SIZE);
                self.liquify.width = w;
                self.liquify.height = if self.shift_down { w } else { h };
            }
            _ => {}
        }
        self.drag = drag;
        self.request_main_redraw();
    }

    /// Release: a stroke commits as one command; a resize just ends.
    pub(in crate::app) fn liquify_release(&mut self, drag: Drag) {
        if let (Drag::Liquify { stroke, targets, preview }, Some(params)) = (drag, self.liquify_params()) {
            if !preview.is_empty() {
                let _ = self.doc.editor.execute(Command::Liquify { objects: targets, stroke, params });
                self.doc.anchor_sel.clear();
            }
        }
        self.request_main_redraw();
    }

    /// `[` / `]`: shrink or grow the brush, keeping its proportions.
    pub(in crate::app) fn liquify_resize_step(&mut self, grow: bool) {
        let k = if grow { LIQUIFY_STEP } else { 1.0 / LIQUIFY_STEP };
        let longest = self.liquify.width.max(self.liquify.height);
        let k = (longest * k).clamp(LIQUIFY_MIN_SIZE, LIQUIFY_MAX_SIZE) / longest;
        self.liquify.width *= k;
        self.liquify.height *= k;
        self.request_main_redraw();
    }

    /// The brush outline, always shown while a liquify tool is active —
    /// at the pointer, or pinned to its centre while Alt-sizing.
    pub(in crate::app) fn paint_liquify_brush(&mut self) {
        if self.active_tool.liquify_kind().is_none() {
            return;
        }
        let to_screen = self.doc.view.to_screen();
        let c = match self.drag {
            Drag::LiquifyResize { center } => center,
            _ => self.doc_point(self.pointer),
        };
        let (rx, ry) = (self.liquify.width / 2.0, self.liquify.height / 2.0);
        let brush = to_screen
            * Affine::translate(c.to_vec2())
            * Affine::rotate(self.liquify.angle.to_radians())
            * (vello::kurbo::Ellipse::new(Point::ORIGIN, (rx, ry), 0.0).to_path(0.1));
        let ink = self.theme.text_dim;
        self.content.stroke(&Stroke::new(1.0), ID, ink, None, &brush);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use amalith_core::{Anchor, PathData, Subpath};

    fn square_app() -> (App, ObjectId) {
        let mut document = Document::new("Liquify");
        let layer = LayerId::new();
        document.insert_layer(amalith_core::Layer::new(layer, "Layer"), 0);
        let mut app = App::new();
        app.doc = Doc::new(Editor::new(document));
        app.doc.view.pan = Vec2::new(400.0, 300.0);
        let pts = [(0., 0.), (200., 0.), (200., 200.), (0., 200.)];
        let path = PathData::from_subpaths(vec![Subpath {
            anchors: pts.iter().map(|&(x, y)| Anchor::corner(amalith_core::Point::new(x, y))).collect(),
            closed: true,
        }]);
        let parent = amalith_core::ObjectParent::Layer(layer);
        let Ok(CommandOutcome::Object(id)) = app.doc.editor.execute(Command::CreatePath { parent, path, name: None })
        else {
            panic!("create path")
        };
        (app, id)
    }

    fn at(app: &mut App, x: f64, y: f64) {
        app.pointer = app.doc.view.to_screen() * Point::new(x, y);
    }

    #[test]
    fn a_stroke_previews_live_and_commits_what_it_previewed() {
        let (mut app, id) = square_app();
        app.set_tool(Tool::Warp);
        app.liquify.width = 100.0;
        app.liquify.height = 100.0;
        at(&mut app, 200.0, 100.0);
        assert!(app.liquify_press());
        for x in [210.0, 220.0, 230.0, 240.0] {
            at(&mut app, x, 100.0);
            app.liquify_move();
        }
        let Drag::Liquify { preview, .. } = &app.drag else { panic!("a live stroke") };
        let previewed = preview.iter().find(|(o, _)| *o == id).expect("the square is bent").1.clone();
        let drag = std::mem::take(&mut app.drag);
        app.liquify_release(drag);
        let committed = app.doc.editor.document().object(id).unwrap().kind.path_data().unwrap().clone();
        assert_eq!(committed.subpaths(), previewed.subpaths(), "release commits exactly the preview");
        assert!(committed.local_bounds().x1 > 205.0);
        app.doc.editor.undo().unwrap();
        assert_eq!(app.doc.editor.document().object(id).unwrap().kind.path_data().unwrap().subpaths().len(), 1);
    }

    #[test]
    fn alt_drag_and_brackets_size_the_brush() {
        let (mut app, _) = square_app();
        app.set_tool(Tool::Bloat);
        app.alt_down = true;
        at(&mut app, 0.0, 0.0);
        app.liquify_press();
        at(&mut app, 40.0, 10.0);
        app.liquify_move();
        assert_eq!((app.liquify.width, app.liquify.height), (80.0, 20.0));
        app.liquify_resize_step(true);
        assert!((app.liquify.width - 100.0).abs() < 1e-9 && (app.liquify.height - 25.0).abs() < 1e-9);
    }

    #[test]
    fn only_the_selection_is_bent_when_there_is_one() {
        let (mut app, id) = square_app();
        app.set_tool(Tool::Pucker);
        assert_eq!(app.liquify_targets(), vec![id]);
        app.doc.selection = vec![ObjectId::new()];
        assert!(app.liquify_targets().is_empty());
    }
}
