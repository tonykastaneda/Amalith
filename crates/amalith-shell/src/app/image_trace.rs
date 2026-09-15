//! Background tracing and non-destructive preview lifecycle.
use super::*;
use crate::image_trace::{Dropdown, Field, Hit, Panel, Preset, View};
use amalith_trace::{CancelToken, Mode, Options, ResultPaths, Tracer};
use std::sync::mpsc::{self, Receiver};

struct Reply {
    key: (ObjectId, u64),
    generation: u64,
    full: bool,
    tracer: Option<Tracer>,
    result: Result<ResultPaths, String>,
}
#[derive(Default)]
pub(super) struct TraceState {
    pub panel: Panel,
    key: Option<(ObjectId, u64)>,
    generation: u64,
    due: Option<Instant>,
    rx: Option<Receiver<Reply>>,
    cancel: CancelToken,
    tracer: Option<Tracer>,
    result: Option<ResultPaths>,
    pub canvas: Option<Document>,
    pub slider: Option<(Field, f64, f64)>,
    expand_when_ready: bool,
    started: bool,
    canvas_revision: u64,
    completed:
        std::collections::HashMap<ObjectId, (amalith_core::ObjectKind, Options, ResultPaths)>,
    pub pick_ignore: bool,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Saved {
    options: Options,
    presets: Vec<Preset>,
}
impl TraceState {
    fn auto_preview(&self) -> bool {
        self.started && self.panel.enabled && self.panel.preview
    }
    fn paint_completed(&self, editor: &mut Editor, active: Option<ObjectId>) {
        for (id, (kind, _, result)) in &self.completed {
            if Some(*id) == active && self.result.is_some() {
                continue;
            }
            if editor
                .document()
                .object(*id)
                .is_some_and(|o| &o.kind == kind)
            {
                let _ = editor.execute(Command::ExpandImageTrace {
                    id: *id,
                    width: result.width,
                    height: result.height,
                    paths: result.paths.clone(),
                });
            }
        }
    }
    pub fn load() -> Self {
        let mut state = Self::default();
        if let Some(saved) = settings::config_dir()
            .and_then(|p| std::fs::read(p.join("image-trace.json")).ok())
            .and_then(|b| serde_json::from_slice::<Saved>(&b).ok())
        {
            state.panel.options = saved.options;
            state.panel.options.normalize();
            state.panel.preset = None;
            state
                .panel
                .presets
                .extend(saved.presets.into_iter().take(16).map(|mut p| {
                    p.options.normalize();
                    p.name = p.name.chars().take(40).collect();
                    p
                }));
        }
        state
    }
    pub fn reset(&mut self) {
        self.invalidate();
        self.key = None;
        self.started = false;
        self.completed.clear();
        self.tracer = None;
        self.due = None;
        self.panel.enabled = false;
        self.panel.dropdown = None;
        self.panel.edit = None;
        self.panel.name_edit = None;
        self.slider = None;
    }
    pub fn pending(&self) -> bool {
        self.rx.is_some() || self.due.is_some()
    }
    fn save(&self) {
        if let Some(dir) = settings::config_dir() {
            let custom = self
                .panel
                .presets
                .iter()
                .skip(Options::presets().len())
                .cloned()
                .collect();
            if let Ok(bytes) = serde_json::to_vec_pretty(&Saved {
                options: self.panel.options.clone(),
                presets: custom,
            }) {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join("image-trace.json"), bytes);
            }
        }
    }
    fn invalidate(&mut self) {
        self.cancel.cancel();
        self.generation += 1;
        self.result = None;
        self.canvas = None;
        self.panel.counts = None;
        self.panel.can_expand = false;
        self.expand_when_ready = false;
        self.pick_ignore = false;
    }
}
impl Drop for TraceState {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
impl App {
    pub(in crate::app) fn trace_changed(&mut self) {
        self.image_trace.invalidate();
        self.trace_rebuild_canvas();
        self.image_trace.panel.preset = None;
        if self.image_trace.auto_preview() {
            self.image_trace.due = Some(Instant::now() + Duration::from_millis(180));
        } else {
            self.image_trace.due = None;
            self.image_trace.panel.status = "Settings changed — click Trace".into();
        }
        self.request_main_redraw();
    }
    pub(in crate::app) fn trace_tick(&mut self) {
        let selected = if self.doc.selection.len() == 1 {
            let id = self.doc.selection[0];
            self.doc
                .editor
                .document()
                .object(id)
                .filter(|o| matches!(o.kind, amalith_core::ObjectKind::Image(_)) && !o.locked)
                .map(|_| (id, self.doc.editor.revision()))
        } else {
            None
        };
        if self.image_trace.key != selected {
            self.image_trace.invalidate();
            self.image_trace.key = selected;
            self.image_trace.tracer = None;
            self.image_trace.panel.enabled = selected.is_some();
            self.image_trace.panel.edit = None;
            self.image_trace.slider = None;
            self.image_trace.due = None;
            self.image_trace.panel.status = if selected.is_some() {
                "Choose a preset or click Trace"
            } else {
                "Select one image to trace"
            }
            .into();
            self.image_trace.started = false;
            if let Some((id, _)) = selected {
                if let Some((kind, options, result)) = self.image_trace.completed.get(&id) {
                    if self
                        .doc
                        .editor
                        .document()
                        .object(id)
                        .is_some_and(|o| &o.kind == kind)
                    {
                        self.image_trace.panel.options = options.clone();
                        self.image_trace.result = Some(result.clone());
                        self.image_trace.panel.counts =
                            Some((result.paths.len(), result.anchors, result.colors));
                        self.image_trace.panel.can_expand = !result.paths.is_empty();
                        self.image_trace.panel.status = "Trace ready".into();
                        self.image_trace.started = true;
                    }
                }
            }
            self.trace_rebuild_canvas();
            self.request_main_redraw();
        }
        if self.image_trace.canvas_revision != self.doc.editor.revision() {
            self.trace_rebuild_canvas();
        }
        let reply = self
            .image_trace
            .rx
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => Some(Reply {
                    key: self.image_trace.key.unwrap_or((ObjectId::new(), 0)),
                    generation: self.image_trace.generation,
                    full: false,
                    tracer: None,
                    result: Err("Tracing worker stopped unexpectedly".into()),
                }),
                Err(_) => None,
            });
        if let Some(reply) = reply {
            self.image_trace.rx = None;
            self.image_trace.panel.busy = false;
            if Some(reply.key) == self.image_trace.key {
                self.image_trace.tracer = reply.tracer;
                if reply.generation == self.image_trace.generation {
                    match reply.result {
                        Ok(result) => {
                            self.image_trace.panel.counts =
                                Some((result.paths.len(), result.anchors, result.colors));
                            self.image_trace.panel.can_expand = !result.paths.is_empty();
                            self.image_trace.panel.status = if result.paths.is_empty() {
                                "No paths — adjust settings"
                            } else if result.reduced {
                                "Preview — Expand traces full resolution"
                            } else {
                                "Trace ready"
                            }
                            .into();
                            if let Some(object) = self.doc.editor.document().object(reply.key.0) {
                                self.image_trace.completed.insert(
                                    reply.key.0,
                                    (
                                        object.kind.clone(),
                                        self.image_trace.panel.options.clone(),
                                        result.clone(),
                                    ),
                                );
                            }
                            self.image_trace.result = Some(result);
                            if reply.full && self.image_trace.expand_when_ready {
                                self.trace_expand();
                            } else {
                                self.trace_rebuild_canvas();
                            }
                        }
                        Err(e) => {
                            self.image_trace.panel.status = e;
                            self.image_trace.expand_when_ready = false;
                        }
                    }
                }
            }
            self.request_main_redraw();
        }
        if self.image_trace.due.is_some_and(|t| Instant::now() >= t)
            && self.image_trace.rx.is_none()
        {
            self.image_trace.due = None;
            self.trace_start(self.image_trace.expand_when_ready);
        }
    }
    fn trace_start(&mut self, full: bool) {
        let Some(key) = self.image_trace.key else {
            return;
        };
        let doc = self.doc.editor.document();
        let Some(object) = doc.object(key.0) else {
            return;
        };
        let amalith_core::ObjectKind::Image(img) = &object.kind else {
            return;
        };
        let Some(asset) = doc.asset(img.asset) else {
            return;
        };
        let source = asset.source.clone();
        let bytes = match &source {
            amalith_core::AssetSource::Embedded { container_path } => {
                self.doc.asset_store.get(container_path).map(|b| b.to_vec())
            }
            _ => None,
        };
        let mut options = self.image_trace.panel.options.clone();
        if options.palette_mode == 2 {
            options.palette = doc
                .swatches()
                .iter()
                .map(|s| {
                    let c = s.color;
                    [
                        (c.r * 255.).round() as u8,
                        (c.g * 255.).round() as u8,
                        (c.b * 255.).round() as u8,
                    ]
                })
                .collect();
        }
        let cached = self.image_trace.tracer.take();
        let generation = self.image_trace.generation;
        let cancel = CancelToken::new();
        self.image_trace.cancel = cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.image_trace.rx = Some(rx);
        self.image_trace.panel.busy = true;
        self.image_trace.panel.status = if full {
            "Tracing full resolution…"
        } else {
            "Tracing…"
        }
        .into();
        std::thread::spawn(move || {
            let mut tracer = cached;
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if tracer.is_none() {
                    let data = match source {
                        amalith_core::AssetSource::Linked { path, .. } => std::fs::read(path)
                            .map_err(|e| format!("Cannot read linked image: {e}"))?,
                        _ => bytes.ok_or("Embedded image is unavailable")?,
                    };
                    if cancel.is_cancelled() {
                        return Err("Trace cancelled".into());
                    }
                    tracer = Some(Tracer::decode(&data)?);
                }
                tracer
                    .as_mut()
                    .unwrap()
                    .trace(&options, full, &cancel, &mut |_| {})
            }))
            .unwrap_or_else(|_| {
                tracer = None;
                Err("The tracing engine could not process this image. Try another preset.".into())
            });
            let _ = tx.send(Reply {
                key,
                generation,
                full,
                tracer,
                result,
            });
        });
        self.request_main_redraw();
    }
    fn trace_command(&self) -> Option<Command> {
        let id = self.image_trace.key?.0;
        let r = self.image_trace.result.as_ref()?;
        Some(Command::ExpandImageTrace {
            id,
            width: r.width,
            height: r.height,
            paths: r.paths.clone(),
        })
    }
    fn trace_expand(&mut self) {
        if self.image_trace.key.is_none_or(|(id, rev)| {
            self.doc.editor.revision() != rev || self.doc.selection.as_slice() != [id]
        }) {
            self.image_trace.invalidate();
            return;
        }
        let Some(cmd) = self.trace_command() else {
            return;
        };
        match self.doc.editor.execute(cmd) {
            Ok(_) => {
                if let Some((id, _)) = self.image_trace.key {
                    self.image_trace.completed.remove(&id);
                }
                self.image_trace.invalidate();
                self.trace_rebuild_canvas();
                self.image_trace.due = None;
                self.image_trace.panel.status = "Expanded to editable paths".into();
            }
            Err(e) => self.image_trace.panel.status = e.to_string(),
        }
        self.request_main_redraw();
    }
    pub(in crate::app) fn trace_rebuild_canvas(&mut self) {
        self.image_trace.canvas_revision = self.doc.editor.revision();
        self.image_trace.canvas = None;
        if self.image_trace.completed.is_empty() && self.image_trace.result.is_none() {
            return;
        }
        let mut editor = Editor::new(self.doc.editor.document().clone());
        let active = self.image_trace.key.map(|k| k.0);
        self.image_trace.paint_completed(&mut editor, active);
        let cmd =
            if self.image_trace.panel.comparing || self.image_trace.panel.view == View::Original {
                None
            } else {
                self.trace_command()
            };
        let Some(cmd) = cmd else {
            self.image_trace.canvas = Some(editor.into_document());
            return;
        };
        let source = self
            .doc
            .editor
            .document()
            .object(self.image_trace.key.unwrap().0)
            .cloned();
        if editor.execute(cmd).is_err() {
            return;
        }
        let mut doc = editor.into_document();
        let id = self.image_trace.key.unwrap().0;
        let children = match doc.object(id).map(|o| &o.kind) {
            Some(amalith_core::ObjectKind::Group(g)) => g.children.clone(),
            _ => return,
        };
        let view = self.image_trace.panel.view;
        if view != View::Result {
            for child in children {
                if let Some(o) = doc.object_mut(child) {
                    if matches!(view, View::Outline | View::SourceOutline) {
                        o.appearance.set_fill(amalith_core::Paint::None);
                    }
                    o.appearance
                        .set_stroke(amalith_core::Paint::Solid(amalith_core::Color::rgb(
                            0.2, 0.6, 1.,
                        )));
                    o.appearance.set_stroke_width(0.7);
                }
            }
        }
        if view == View::SourceOutline {
            if let Some(mut original) = source {
                original.id = ObjectId::new();
                original.parent = amalith_core::ObjectParent::Group(id);
                original.transform = amalith_core::Affine::IDENTITY;
                original.appearance = Default::default();
                let _ = doc.insert_object(original, 0);
            }
        }
        self.image_trace.canvas = Some(doc);
    }
    pub(in crate::app) fn trace_hit(&mut self, hit: Hit) {
        match hit {
            Hit::PickIgnore => {
                if self.image_trace.tracer.is_some() {
                    self.image_trace.pick_ignore = true;
                    self.image_trace.panel.status =
                        "Click the source image to sample a color".into();
                } else {
                    self.image_trace.panel.status =
                        "Trace the image before sampling a color".into();
                }
            }
            Hit::None => {}
            Hit::Dropdown(d) => {
                self.trace_commit_edit();
                self.image_trace.panel.dropdown = if self.image_trace.panel.dropdown == Some(d) {
                    None
                } else {
                    Some(d)
                };
            }
            Hit::Choose(d, i) => {
                self.image_trace.panel.dropdown = None;
                match d {
                    Dropdown::View => {
                        if let Some(v) = View::ALL.get(i) {
                            self.image_trace.panel.view = *v;
                            self.trace_rebuild_canvas();
                        }
                    }
                    Dropdown::Preset => {
                        if let Some(p) = self.image_trace.panel.presets.get(i) {
                            self.image_trace.panel.options = p.options.clone();
                            self.trace_changed();
                            self.image_trace.panel.preset = Some(i);
                        }
                    }
                    Dropdown::Mode => {
                        self.image_trace.panel.options.mode = match i {
                            0 => Mode::BlackWhite,
                            1 => Mode::Grayscale,
                            _ => Mode::Color,
                        };
                        self.trace_changed();
                    }
                    Dropdown::Palette => {
                        self.image_trace.panel.options.palette_mode = i as u8;
                        self.trace_changed();
                    }
                }
                self.image_trace.save();
            }
            Hit::Field(f) => {
                self.trace_commit_edit();
                let p = &mut self.image_trace.panel;
                let s = if f == Field::IgnoreColor {
                    let c = p.options.ignore_color;
                    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
                } else {
                    format!("{:.0}", p.value(f))
                };
                p.edit = Some((f, s, true));
            }
            Hit::Slider(f, x, x0, x1) => {
                self.trace_commit_edit();
                self.image_trace.slider = Some((f, x0, x1));
                self.trace_slider_at(x);
            }
            Hit::Advanced => self.image_trace.panel.advanced = !self.image_trace.panel.advanced,
            Hit::Transparency => {
                self.image_trace.panel.options.transparency =
                    !self.image_trace.panel.options.transparency;
                self.trace_changed();
                self.image_trace.save();
            }
            Hit::Ignore => {
                self.image_trace.panel.options.ignore = !self.image_trace.panel.options.ignore;
                self.trace_changed();
                self.image_trace.save();
            }
            Hit::Preview => {
                self.image_trace.panel.preview = !self.image_trace.panel.preview;
                if self.image_trace.panel.preview {
                    self.trace_changed();
                }
            }
            Hit::Compare => {
                self.image_trace.panel.comparing = true;
                self.trace_rebuild_canvas();
            }
            Hit::Trace => {
                self.trace_commit_edit();
                if self.image_trace.panel.busy {
                    self.image_trace.invalidate();
                    self.image_trace.due = None;
                    self.image_trace.panel.status = "Trace cancelled".into();
                } else if self.image_trace.panel.enabled {
                    self.image_trace.started = true;
                    self.image_trace.due = Some(Instant::now());
                }
            }
            Hit::Expand => {
                self.trace_commit_edit();
                if self.image_trace.panel.can_expand && !self.image_trace.panel.busy {
                    if self.image_trace.result.as_ref().is_some_and(|r| r.reduced) {
                        self.image_trace.expand_when_ready = true;
                        self.image_trace.due = Some(Instant::now());
                    } else {
                        self.trace_expand();
                    }
                }
            }
        }
        self.request_main_redraw();
    }
    pub(in crate::app) fn trace_slider_at(&mut self, x: f64) {
        if let Some((f, x0, x1)) = self.image_trace.slider {
            let (min, max) = self.image_trace.panel.range(f);
            self.image_trace
                .panel
                .set(f, min + (max - min) * ((x - x0) / (x1 - x0)).clamp(0., 1.));
            self.trace_changed();
        }
    }
    pub(in crate::app) fn trace_release(&mut self) -> bool {
        let handled = self.image_trace.slider.take().is_some() || self.image_trace.panel.comparing;
        self.image_trace.panel.comparing = false;
        if handled {
            self.image_trace.save();
            self.trace_rebuild_canvas();
            self.request_main_redraw();
        }
        handled
    }
    fn trace_commit_edit(&mut self) {
        if let Some((f, s, _)) = self.image_trace.panel.edit.take() {
            if f == Field::IgnoreColor {
                if let Ok(v) = u32::from_str_radix(s.trim().trim_start_matches('#'), 16) {
                    if s.trim().trim_start_matches('#').len() == 6 {
                        self.image_trace.panel.options.ignore_color =
                            [(v >> 16) as u8, (v >> 8) as u8, v as u8];
                        self.trace_changed();
                    }
                }
            } else if let Ok(v) = s
                .trim()
                .trim_end_matches('%')
                .trim_end_matches("px")
                .trim()
                .parse()
            {
                self.image_trace.panel.set(f, v);
                self.trace_changed();
            }
            self.image_trace.save();
        }
    }
    pub(in crate::app) fn trace_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
            && (self.image_trace.pick_ignore || self.image_trace.panel.dropdown.is_some())
        {
            self.image_trace.pick_ignore = false;
            self.image_trace.panel.dropdown = None;
            self.request_main_redraw();
            return true;
        }
        if self.image_trace.panel.edit.is_none() && self.image_trace.panel.name_edit.is_none() {
            return false;
        }
        if !event.state.is_pressed() {
            return true;
        }
        match event.physical_key {
            PhysicalKey::Code(KeyCode::Escape) => {
                self.image_trace.panel.edit = None;
                self.image_trace.panel.name_edit = None;
            }
            PhysicalKey::Code(KeyCode::Enter | KeyCode::NumpadEnter) => {
                self.trace_commit_edit();
                if let Some((name, _, rename)) = self.image_trace.panel.name_edit.take() {
                    let name = name.trim();
                    if !name.is_empty() {
                        if rename {
                            if let Some(i) = self
                                .image_trace
                                .panel
                                .preset
                                .filter(|i| *i >= Options::presets().len())
                            {
                                self.image_trace.panel.presets[i].name = name.into();
                            }
                        } else if self.image_trace.panel.presets.len()
                            < Options::presets().len() + 16
                        {
                            self.image_trace.panel.presets.push(Preset {
                                name: name.into(),
                                options: self.image_trace.panel.options.clone(),
                            });
                            self.image_trace.panel.preset =
                                Some(self.image_trace.panel.presets.len() - 1);
                        }
                        self.image_trace.save();
                    }
                }
            }
            _ => {
                let target = if let Some((_, s, fresh)) = &mut self.image_trace.panel.edit {
                    Some((s, fresh))
                } else {
                    self.image_trace
                        .panel
                        .name_edit
                        .as_mut()
                        .map(|(s, fresh, _)| (s, fresh))
                };
                if let Some((s, fresh)) = target {
                    if event.physical_key == PhysicalKey::Code(KeyCode::Backspace) {
                        if *fresh {
                            s.clear();
                        } else {
                            s.pop();
                        }
                        *fresh = false;
                    } else if let Some(txt) = &event.text {
                        for ch in txt.chars().filter(|c| !c.is_control()) {
                            if *fresh {
                                s.clear();
                                *fresh = false;
                            }
                            if s.chars().count() < 40 {
                                s.push(ch);
                            }
                        }
                    }
                }
            }
        }
        self.request_main_redraw();
        true
    }
    pub(in crate::app) fn trace_pick_at(&mut self) -> bool {
        if !self.image_trace.pick_ignore {
            return false;
        }
        let Some((id, _)) = self.image_trace.key else {
            return false;
        };
        let doc = self.doc.editor.document();
        let Some(object) = doc.object(id) else {
            return false;
        };
        let amalith_core::ObjectKind::Image(img) = &object.kind else {
            return false;
        };
        let local = doc.world_transform(id).inverse()
            * crate::convert::point_to_core(self.doc_point(self.pointer));
        let b = img.local_bounds;
        if b.contains(local) {
            if let Some(t) = &self.image_trace.tracer {
                let (w, h) = t.dimensions();
                let x = ((local.x - b.x0) / b.width() * w as f64).floor() as u32;
                let y = ((local.y - b.y0) / b.height() * h as f64).floor() as u32;
                if let Some(c) = t.sample(x, y) {
                    self.image_trace.panel.options.ignore_color = c;
                    self.image_trace.panel.options.ignore = true;
                    self.trace_changed();
                    self.image_trace.save();
                }
            }
        }
        self.image_trace.pick_ignore = false;
        self.request_main_redraw();
        true
    }
    pub(in crate::app) fn trace_preset_action(&mut self, action: &str) {
        match action {
            "trace-save" => {
                self.image_trace.panel.name_edit = Some(("New Preset".into(), true, false))
            }
            "trace-rename" => {
                if let Some(i) = self
                    .image_trace
                    .panel
                    .preset
                    .filter(|i| *i >= Options::presets().len())
                {
                    self.image_trace.panel.name_edit =
                        Some((self.image_trace.panel.presets[i].name.clone(), true, true));
                }
            }
            "trace-delete" => {
                if let Some(i) = self
                    .image_trace
                    .panel
                    .preset
                    .filter(|i| *i >= Options::presets().len())
                {
                    self.image_trace.panel.presets.remove(i);
                    self.image_trace.panel.preset = None;
                    self.image_trace.save();
                }
            }
            _ => {}
        }
        self.request_main_redraw();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_requires_explicit_start() {
        let mut state = TraceState::default();
        state.panel.enabled = true;
        state.panel.preview = true;
        assert!(!state.auto_preview());
        state.started = true;
        assert!(state.auto_preview());
        state.panel.enabled = false;
        assert!(!state.auto_preview());
    }
    #[test]
    fn completed_trace_survives_deselection_and_unrelated_edits() {
        use amalith_core::*;
        let mut doc = Document::new("Trace");
        let layer = LayerId::new();
        doc.insert_layer(Layer::new(layer, "Layer"), 0);
        let id = ObjectId::new();
        let kind = ObjectKind::Image(ImageData {
            asset: AssetId::new(),
            local_bounds: Rect::new(0., 0., 10., 10.),
        });
        doc.insert_object(Object::new(id, ObjectParent::Layer(layer), kind.clone()), 0)
            .unwrap();
        let mut state = TraceState::default();
        state.completed.insert(
            id,
            (
                kind,
                Options::default(),
                ResultPaths {
                    width: 10,
                    height: 10,
                    anchors: 4,
                    colors: 1,
                    reduced: false,
                    paths: vec![(
                        PathData::from_bezpath(kurbo::Shape::to_path(
                            &Rect::new(0., 0., 5., 5.),
                            0.1,
                        )),
                        Color::rgb(1., 0., 0.),
                    )],
                },
            ),
        );
        state.invalidate(); // Selection changes invalidate only the active job.
        let mut preview = Editor::new(doc.clone());
        preview
            .execute(Command::RenameObject {
                id,
                name: Some("Renamed".into()),
            })
            .unwrap();
        state.paint_completed(&mut preview, None);
        assert!(matches!(
            preview.document().object(id).unwrap().kind,
            ObjectKind::Group(_)
        ));
        assert!(matches!(doc.object(id).unwrap().kind, ObjectKind::Image(_)));
        state.reset();
        assert!(state.completed.is_empty());
    }
    #[test]
    fn changing_settings_cancels_old_job_and_disables_expand() {
        let mut state = TraceState::default();
        let token = state.cancel.clone();
        state.panel.can_expand = true;
        state.expand_when_ready = true;
        state.invalidate();
        assert!(token.is_cancelled());
        assert_eq!(state.generation, 1);
        assert!(!state.panel.can_expand);
        assert!(!state.expand_when_ready);
    }
    #[test]
    fn custom_presets_round_trip_without_backend_session_state() {
        let saved = Saved {
            options: Options::default(),
            presets: vec![Preset {
                name: "My logo".into(),
                options: Options {
                    threshold: 160,
                    ..Default::default()
                },
            }],
        };
        let bytes = serde_json::to_vec(&saved).unwrap();
        let result: Saved = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.presets[0].name, "My logo");
        assert_eq!(result.presets[0].options.threshold, 160);
        assert_eq!(result.options, saved.options);
    }
}
