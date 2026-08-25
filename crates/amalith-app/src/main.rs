mod artboard_tool;
mod camera;

use artboard_tool::{mode_after_keys, next_artboard_name, ArtboardTool, DragKind, Handle};
use camera::Camera;
use eframe::egui::{self, Color32, FontId, Pos2, Rect as EguiRect, Stroke, Vec2};
use amalith_commands::{Command, Editor};
use amalith_core::{
    Bleed, ColorMode, Document, Length, Point, PreviewMode, RasterEffects, Rect, Unit,
};

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 800.0])
            .with_min_inner_size([620.0, 620.0])
            .with_title("Amalith"),
        ..Default::default()
    };
    eframe::run_native(
        "Amalith",
        options,
        Box::new(|cc| Ok(Box::new(AmalithApp::new(cc)))),
    )
}

struct AmalithApp {
    open: Vec<DocumentTab>,
    active: usize,
    creating: Option<NewDocumentForm>,
    error: Option<String>,
}

struct DocumentTab {
    editor: Box<Editor>,
    camera: Camera,
    artboard_tool: ArtboardTool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogAction {
    None,
    Create,
    Close,
}

#[derive(Debug, PartialEq, Eq)]
enum CanvasNavigation {
    ScrubbyZoom,
    HandPan,
    None,
}

fn canvas_navigation(space_down: bool, command_down: bool) -> CanvasNavigation {
    if space_down && command_down {
        CanvasNavigation::ScrubbyZoom
    } else if space_down {
        CanvasNavigation::HandPan
    } else {
        CanvasNavigation::None
    }
}

#[derive(Clone)]
struct NewDocumentForm {
    name: String,
    width: Length,
    height: Length,
    width_text: String,
    height_text: String,
    unit: Unit,
    artboards: usize,
    bleed: [Length; 4],
    bleed_text: [String; 4],
    bleed_linked: bool,
    color_mode: ColorMode,
    raster_effects: RasterEffects,
    preview_mode: PreviewMode,
}

impl Default for NewDocumentForm {
    fn default() -> Self {
        Self::with_name("Untitled-1")
    }
}

impl NewDocumentForm {
    fn with_name(name: impl Into<String>) -> Self {
        let three = Length::new(3.0, Unit::In);
        let zero = Length::from_px(0.0);
        Self {
            name: name.into(),
            width: three,
            height: three,
            width_text: "3".into(),
            height_text: "3".into(),
            unit: Unit::In,
            artboards: 1,
            bleed: [zero; 4],
            bleed_text: std::array::from_fn(|_| "0".into()),
            bleed_linked: true,
            color_mode: ColorMode::Cmyk,
            raster_effects: RasterEffects::High300,
            preview_mode: PreviewMode::Default,
        }
    }
}

impl AmalithApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(49, 49, 49);
        visuals.window_fill = Color32::from_rgb(49, 49, 49);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(39, 39, 39);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(55, 55, 55);
        visuals.selection.bg_fill = Color32::from_rgb(42, 112, 196);
        cc.egui_ctx.set_visuals(visuals);
        Self {
            open: Vec::new(),
            active: 0,
            creating: Some(NewDocumentForm::default()),
            error: None,
        }
    }

    fn create_document(&mut self) {
        let Some(form) = self.creating.as_mut() else {
            return;
        };
        form.commit_dimensions();
        form.commit_all_bleed();
        if form.width.px() <= 0.0 || form.height.px() <= 0.0 {
            self.error = Some("Width and height must be greater than zero.".into());
            return;
        }

        let mut document = Document::new(form.name.trim());
        document.settings.default_unit = form.unit;
        document.settings.color_mode = form.color_mode;
        document.settings.raster_effects = form.raster_effects;
        document.settings.preview_mode = form.preview_mode;
        document.settings.bleed = Bleed {
            top: form.bleed[0].px(),
            bottom: form.bleed[1].px(),
            left: form.bleed[2].px(),
            right: form.bleed[3].px(),
        };
        let mut editor = Editor::new(document);
        let width = form.width.px();
        let height = form.height.px();
        let gap = 48.0;
        for index in 0..form.artboards {
            let x = index as f64 * (width + gap);
            let command = Command::CreateArtboard {
                name: format!("Artboard {}", index + 1),
                rect: Rect::new(x, 0.0, x + width, height),
                index: None,
            };
            if let Err(err) = editor.execute(command) {
                self.error = Some(err.to_string());
                return;
            }
        }
        if let Err(err) = editor.execute(Command::CreateLayer {
            name: "Layer 1".into(),
            index: None,
        }) {
            self.error = Some(err.to_string());
            return;
        }
        self.error = None;
        self.open.push(DocumentTab {
            editor: Box::new(editor),
            camera: Camera::default(),
            artboard_tool: ArtboardTool::default(),
        });
        self.active = self.open.len() - 1;
        self.creating = None;
    }

    fn new_document_ui(
        ctx: &egui::Context,
        form: &mut NewDocumentForm,
        error: Option<&str>,
    ) -> DialogAction {
        let mut action = DialogAction::None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(46, 46, 46)))
            .show(ctx, |ui| {
                let panel_width = 390.0_f32.min(ui.available_width() - 32.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(panel_width, ui.available_height() - 36.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);
                            ui.label(
                                egui::RichText::new("PRESET DETAILS")
                                    .strong()
                                    .color(Color32::from_gray(190)),
                            );
                            ui.add_space(4.0);
                            ui.add_sized(
                                [panel_width, 34.0],
                                egui::TextEdit::singleline(&mut form.name)
                                    .font(FontId::proportional(18.0)),
                            );
                            ui.separator();
                            labeled(ui, "Width", |ui| {
                                ui.horizontal(|ui| {
                                    numeric_field(ui, &mut form.width_text, form.unit, 126.0);
                                    let old = form.unit;
                                    egui::ComboBox::from_id_salt("unit")
                                        .selected_text(unit_name(form.unit))
                                        .width(205.0)
                                        .show_ui(ui, |ui| {
                                            for unit in
                                                [Unit::Px, Unit::Pt, Unit::In, Unit::Mm, Unit::Cm]
                                            {
                                                ui.selectable_value(
                                                    &mut form.unit,
                                                    unit,
                                                    unit_name(unit),
                                                );
                                            }
                                        });
                                    if old != form.unit {
                                        form.change_unit(old);
                                    }
                                });
                            });
                            ui.columns(3, |cols| {
                                labeled(&mut cols[0], "Height", |ui| {
                                    numeric_field(ui, &mut form.height_text, form.unit, 112.0);
                                });
                                labeled(&mut cols[1], "Orientation", |ui| {
                                    ui.horizontal(|ui| {
                                        let portrait = form.height.px() >= form.width.px();
                                        if ui.selectable_label(portrait, "▮").clicked() && !portrait
                                        {
                                            form.commit_dimensions();
                                            form.swap_dimensions();
                                        }
                                        if ui.selectable_label(!portrait, "▬").clicked() && portrait
                                        {
                                            form.commit_dimensions();
                                            form.swap_dimensions();
                                        }
                                    });
                                });
                                labeled(&mut cols[2], "Artboards", |ui| {
                                    ui.horizontal(|ui| {
                                        if ui.small_button("−").clicked() {
                                            form.artboards =
                                                form.artboards.saturating_sub(1).max(1);
                                        }
                                        ui.add(
                                            egui::DragValue::new(&mut form.artboards)
                                                .range(1..=100),
                                        );
                                        if ui.small_button("+").clicked() {
                                            form.artboards = (form.artboards + 1).min(100);
                                        }
                                    });
                                });
                            });
                            ui.label(egui::RichText::new("Bleed").color(Color32::from_gray(190)));
                            egui::Grid::new("bleed_grid")
                                .num_columns(2)
                                .spacing([16.0, 7.0])
                                .show(ui, |ui| {
                                    for row in 0..2 {
                                        for col in 0..2 {
                                            let i = row * 2 + col;
                                            ui.vertical(|ui| {
                                                ui.label(["Top", "Bottom", "Left", "Right"][i]);
                                                let response = numeric_field(
                                                    ui,
                                                    &mut form.bleed_text[i],
                                                    form.unit,
                                                    155.0,
                                                );
                                                if response.lost_focus() {
                                                    form.commit_bleed(i);
                                                }
                                            });
                                        }
                                        ui.end_row();
                                    }
                                });
                            ui.checkbox(&mut form.bleed_linked, "🔗  Link bleed values");
                            combo_row(
                                ui,
                                "Color Mode",
                                "color_mode",
                                color_name(form.color_mode),
                                |ui| {
                                    ui.selectable_value(
                                        &mut form.color_mode,
                                        ColorMode::Cmyk,
                                        "CMYK Color",
                                    );
                                    ui.selectable_value(
                                        &mut form.color_mode,
                                        ColorMode::Rgb,
                                        "RGB Color",
                                    );
                                },
                            );
                            combo_row(
                                ui,
                                "Raster Effects",
                                "raster",
                                raster_name(form.raster_effects),
                                |ui| {
                                    ui.selectable_value(
                                        &mut form.raster_effects,
                                        RasterEffects::Screen72,
                                        "Screen (72 ppi)",
                                    );
                                    ui.selectable_value(
                                        &mut form.raster_effects,
                                        RasterEffects::Medium150,
                                        "Medium (150 ppi)",
                                    );
                                    ui.selectable_value(
                                        &mut form.raster_effects,
                                        RasterEffects::High300,
                                        "High (300 ppi)",
                                    );
                                },
                            );
                            combo_row(
                                ui,
                                "Preview Mode",
                                "preview",
                                preview_name(form.preview_mode),
                                |ui| {
                                    ui.selectable_value(
                                        &mut form.preview_mode,
                                        PreviewMode::Default,
                                        "Default",
                                    );
                                    ui.selectable_value(
                                        &mut form.preview_mode,
                                        PreviewMode::Pixel,
                                        "Pixel",
                                    );
                                    ui.selectable_value(
                                        &mut form.preview_mode,
                                        PreviewMode::Overprint,
                                        "Overprint",
                                    );
                                },
                            );
                            ui.add_space(8.0);
                            let _ = ui.button("More Settings");
                            if let Some(error) = error {
                                ui.colored_label(Color32::from_rgb(245, 110, 110), error);
                            }
                            ui.add_space((ui.available_height() - 58.0).max(8.0));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_sized(
                                            [96.0, 40.0],
                                            egui::Button::new(
                                                egui::RichText::new("Create").strong(),
                                            )
                                            .fill(Color32::from_rgb(48, 112, 201)),
                                        )
                                        .clicked()
                                    {
                                        action = DialogAction::Create;
                                    }
                                    if ui
                                        .add_sized([96.0, 40.0], egui::Button::new("Close"))
                                        .clicked()
                                    {
                                        action = DialogAction::Close;
                                    }
                                },
                            );
                        },
                    );
                });
            });
        action
    }

    fn open_new_document(&mut self) {
        if self.creating.is_none() {
            let name = next_untitled_name(&self.open);
            self.creating = Some(NewDocumentForm::with_name(name));
            self.error = None;
        }
    }

    fn close_tab(&mut self, index: usize) {
        if index >= self.open.len() {
            return;
        }
        self.open.remove(index);
        if self.open.is_empty() {
            self.active = 0;
            self.open_new_document();
        } else if index < self.active {
            self.active -= 1;
        } else if self.active >= self.open.len() {
            self.active = self.open.len() - 1;
        }
    }

    fn workspace_ui(&mut self, ctx: &egui::Context) {
        let mut select = None;
        let mut close = None;
        let mut create = false;
        egui::TopBottomPanel::top("document_tabs")
            .exact_height(34.0)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(39, 39, 39)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                ui.horizontal(|ui| {
                    egui::ScrollArea::horizontal()
                        .max_width((ui.available_width() - 48.0).max(0.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, tab) in self.open.iter().enumerate() {
                                    let active = index == self.active;
                                    let fill = if active {
                                        Color32::from_rgb(58, 58, 58)
                                    } else {
                                        Color32::from_rgb(43, 43, 43)
                                    };
                                    egui::Frame::NONE
                                        .fill(fill)
                                        .inner_margin(egui::Margin::symmetric(7, 3))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .add(egui::Button::new("×").frame(false))
                                                    .clicked()
                                                {
                                                    close = Some(index);
                                                }
                                                let text =
                                                    egui::RichText::new(tab_label(tab, index + 1))
                                                        .strong()
                                                        .color(if active {
                                                            Color32::from_gray(238)
                                                        } else {
                                                            Color32::from_gray(165)
                                                        });
                                                if ui
                                                    .add(egui::Button::new(text).frame(false))
                                                    .clicked()
                                                {
                                                    select = Some(index);
                                                }
                                            });
                                        });
                                }
                            });
                        });
                    ui.add_space(12.0);
                    if ui
                        .add(egui::Button::new("⊕").frame(false))
                        .on_hover_text("New document (⌘N)")
                        .clicked()
                    {
                        create = true;
                    }
                });
            });

        if let Some(index) = select {
            self.active = index.min(self.open.len().saturating_sub(1));
        }
        if let Some(index) = close {
            self.close_tab(index);
        } else if create {
            self.open_new_document();
        }

        if self.creating.is_none() {
            if let Some(tab) = self.open.get_mut(self.active) {
                Self::canvas_ui(
                    ctx,
                    &mut tab.editor,
                    &mut tab.camera,
                    &mut tab.artboard_tool,
                    &mut self.error,
                );
            }
        }
    }

    fn canvas_ui(
        ctx: &egui::Context,
        editor: &mut Editor,
        camera: &mut Camera,
        artboard_tool: &mut ArtboardTool,
        error: &mut Option<String>,
    ) {
        let pasteboard = if artboard_tool.active {
            Color32::from_rgb(115, 115, 115)
        } else {
            Color32::from_rgb(56, 56, 56)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(pasteboard))
            .show(ctx, |ui| {
                let available = ui.max_rect().shrink2(Vec2::new(52.0, 58.0));
                let boards = editor.document().artboards().to_vec();
                if boards.is_empty() {
                    return;
                }
                let min_x = boards
                    .iter()
                    .map(|a| a.rect.x0)
                    .fold(f64::INFINITY, f64::min);
                let min_y = boards
                    .iter()
                    .map(|a| a.rect.y0)
                    .fold(f64::INFINITY, f64::min);
                let max_x = boards
                    .iter()
                    .map(|a| a.rect.x1)
                    .fold(f64::NEG_INFINITY, f64::max);
                let max_y = boards
                    .iter()
                    .map(|a| a.rect.y1)
                    .fold(f64::NEG_INFINITY, f64::max);
                let document_bounds = EguiRect::from_min_max(
                    Pos2::new(min_x as f32, min_y as f32),
                    Pos2::new(max_x as f32, max_y as f32),
                );
                camera.initialize_fit(available, document_bounds);

                let shift_o_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::SHIFT, egui::Key::O) > 0
                });
                let escape_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Escape) > 0
                });
                let navigation_key_down = ctx.input(|input| input.key_down(egui::Key::Space));
                let selection_pressed = !navigation_key_down
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::V) > 0
                    });
                let next_mode = mode_after_keys(
                    artboard_tool.active,
                    shift_o_pressed,
                    escape_pressed,
                    selection_pressed,
                );
                if next_mode != artboard_tool.active {
                    artboard_tool.set_active(next_mode, boards.first().map(|board| board.id));
                }

                let undo_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Z) > 0
                });
                if undo_pressed {
                    let _ = editor.undo();
                    artboard_tool.preview_rect = None;
                }

                let (
                    space_down,
                    command_down,
                    alt_down,
                    primary_down,
                    primary_pressed,
                    primary_released,
                    pointer_position,
                    pointer_delta,
                ) = ctx.input(|input| {
                    (
                        input.key_down(egui::Key::Space),
                        input.modifiers.command,
                        input.modifiers.alt,
                        input.pointer.primary_down(),
                        input.pointer.primary_pressed(),
                        input.pointer.primary_released(),
                        input.pointer.interact_pos(),
                        input.pointer.delta(),
                    )
                });

                let zoom_in_presses = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Equals)
                        + input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Plus)
                });
                let zoom_out_presses = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Minus)
                });
                for _ in 0..zoom_in_presses {
                    camera.zoom_at(available.center(), 2.0, available);
                }
                for _ in 0..zoom_out_presses {
                    camera.zoom_at(available.center(), 0.5, available);
                }

                match canvas_navigation(space_down, command_down) {
                    CanvasNavigation::ScrubbyZoom => {
                        camera.end_pan();
                        if primary_pressed {
                            if let Some(anchor) = pointer_position {
                                camera.begin_scrub(anchor);
                            }
                        } else if primary_down {
                            camera.scrub_zoom(pointer_delta.x, available);
                        } else {
                            camera.end_scrub();
                        }

                        ctx.set_cursor_icon(if primary_down && pointer_delta.x < 0.0 {
                            egui::CursorIcon::ZoomOut
                        } else {
                            egui::CursorIcon::ZoomIn
                        });

                        // Consume Cmd+Space presses and repeats while the canvas owns zoom.
                        ctx.input_mut(|input| {
                            input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Space);
                        });
                    }
                    CanvasNavigation::HandPan => {
                        camera.end_scrub();
                        if primary_pressed {
                            camera.begin_pan();
                        } else if primary_down {
                            camera.drag_pan(pointer_delta);
                        } else {
                            camera.end_pan();
                        }
                        ctx.set_cursor_icon(if primary_down && camera.is_panning() {
                            egui::CursorIcon::Grabbing
                        } else {
                            egui::CursorIcon::Grab
                        });

                        // Prevent Space presses (including key-repeat) from activating UI.
                        ctx.input_mut(|input| {
                            input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Space);
                        });
                    }
                    CanvasNavigation::None => {
                        camera.end_scrub();
                        camera.end_pan();
                        if artboard_tool.active {
                            if let Some(pointer) = pointer_position {
                                let selected = artboard_tool
                                    .selected
                                    .and_then(|id| boards.iter().find(|board| board.id == id));
                                let hovered_handle = selected.and_then(|board| {
                                    let rect = artboard_tool.display_rect(board.id, board.rect);
                                    hit_handle(
                                        pointer,
                                        document_rect_to_screen(rect, camera, available),
                                    )
                                });

                                if primary_pressed && available.contains(pointer) {
                                    let pointer_doc = camera.screen_to_document(pointer, available);
                                    let point =
                                        Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                    if let (Some(board), Some(handle)) = (selected, hovered_handle)
                                    {
                                        let rect = artboard_tool.display_rect(board.id, board.rect);
                                        artboard_tool.begin_drag(
                                            board.id,
                                            rect,
                                            DragKind::Resize(handle),
                                            point,
                                        );
                                    } else if let Some(board) = boards.iter().rev().find(|board| {
                                        document_rect_to_screen(
                                            artboard_tool.display_rect(board.id, board.rect),
                                            camera,
                                            available,
                                        )
                                        .contains(pointer)
                                    }) {
                                        artboard_tool.select(board.id);
                                        artboard_tool.begin_drag(
                                            board.id,
                                            board.rect,
                                            if alt_down {
                                                DragKind::Duplicate
                                            } else {
                                                DragKind::Move
                                            },
                                            point,
                                        );
                                    }
                                } else if primary_down {
                                    let pointer_doc = camera.screen_to_document(pointer, available);
                                    artboard_tool.update_drag(Point::new(
                                        pointer_doc.x as f64,
                                        pointer_doc.y as f64,
                                    ));
                                }

                                let over_artboard = boards.iter().any(|board| {
                                    document_rect_to_screen(
                                        artboard_tool.display_rect(board.id, board.rect),
                                        camera,
                                        available,
                                    )
                                    .contains(pointer)
                                });
                                let cursor = hovered_handle.map(handle_cursor).or_else(|| {
                                    if artboard_tool.is_duplicate_drag()
                                        || (alt_down && over_artboard)
                                    {
                                        return Some(egui::CursorIcon::Copy);
                                    }
                                    over_artboard.then_some(if primary_down {
                                        egui::CursorIcon::Grabbing
                                    } else {
                                        egui::CursorIcon::Grab
                                    })
                                });
                                if let Some(cursor) = cursor {
                                    ctx.set_cursor_icon(cursor);
                                }
                            }
                        }
                    }
                }
                if primary_released && artboard_tool.active {
                    if let Err(err) = artboard_tool.finish_drag(editor) {
                        *error = Some(err.to_string());
                    }
                }

                let painter = ui.painter();
                for (index, artboard) in boards.iter().enumerate() {
                    let document_rect = artboard_tool.display_rect(artboard.id, artboard.rect);
                    let rect = document_rect_to_screen(document_rect, camera, available);
                    let min = rect.min;
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                    let selected = artboard_tool.active
                        && !artboard_tool.is_duplicate_drag()
                        && artboard_tool.selected == Some(artboard.id);
                    if selected {
                        paint_dashed_rect(
                            painter,
                            rect,
                            Stroke::new(1.25_f32, Color32::from_rgb(110, 191, 255)),
                        );
                    } else {
                        painter.rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0_f32, Color32::from_gray(35)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    painter.text(
                        min + Vec2::new(0.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:02} - {}", index + 1, artboard.name),
                        FontId::proportional(12.0),
                        Color32::from_gray(225),
                    );
                    if selected {
                        for handle in Handle::ALL {
                            let center = handle_position(rect, handle);
                            let handle_rect = EguiRect::from_center_size(center, Vec2::splat(8.0));
                            painter.rect_filled(handle_rect, 0.0, Color32::from_rgb(225, 241, 255));
                            painter.rect_stroke(
                                handle_rect,
                                0.0,
                                Stroke::new(1.25_f32, Color32::from_rgb(45, 139, 242)),
                                egui::StrokeKind::Outside,
                            );
                        }
                    }
                }
                if let Some(preview) = artboard_tool.duplicate_preview() {
                    let rect = document_rect_to_screen(preview, camera, available);
                    painter.rect_filled(rect, 0.0, Color32::WHITE);
                    paint_dashed_rect(
                        painter,
                        rect,
                        Stroke::new(1.25_f32, Color32::from_rgb(110, 191, 255)),
                    );
                    painter.text(
                        rect.min + Vec2::new(0.0, -8.0),
                        egui::Align2::LEFT_BOTTOM,
                        format!("{:02} - {}", boards.len() + 1, next_artboard_name(editor)),
                        FontId::proportional(12.0),
                        Color32::from_gray(225),
                    );
                    for handle in Handle::ALL {
                        let center = handle_position(rect, handle);
                        let handle_rect = EguiRect::from_center_size(center, Vec2::splat(8.0));
                        painter.rect_filled(handle_rect, 0.0, Color32::from_rgb(225, 241, 255));
                        painter.rect_stroke(
                            handle_rect,
                            0.0,
                            Stroke::new(1.25_f32, Color32::from_rgb(45, 139, 242)),
                            egui::StrokeKind::Outside,
                        );
                    }
                }
            });
    }
}

impl eframe::App for AmalithApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let new_pressed = ctx.input_mut(|input| {
            input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::N) > 0
        });
        if new_pressed {
            self.open_new_document();
        }
        let close_pressed = ctx.input_mut(|input| {
            input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::W) > 0
        });
        if close_pressed && self.creating.is_none() && !self.open.is_empty() {
            self.close_tab(self.active);
        }

        if self.creating.is_some() {
            let action = {
                let form = self.creating.as_mut().expect("checked above");
                Self::new_document_ui(ctx, form, self.error.as_deref())
            };
            match action {
                DialogAction::Create => self.create_document(),
                DialogAction::Close if self.open.is_empty() => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                DialogAction::Close => {
                    self.creating = None;
                    self.error = None;
                }
                DialogAction::None => {}
            }
        } else {
            self.workspace_ui(ctx);
        }
    }
}

fn document_rect_to_screen(rect: Rect, camera: &Camera, viewport: EguiRect) -> EguiRect {
    EguiRect::from_min_max(
        camera.document_to_screen(Pos2::new(rect.x0 as f32, rect.y0 as f32), viewport),
        camera.document_to_screen(Pos2::new(rect.x1 as f32, rect.y1 as f32), viewport),
    )
}

fn paint_dashed_rect(painter: &egui::Painter, rect: EguiRect, stroke: Stroke) {
    const DASH: f32 = 5.0;
    const GAP: f32 = 4.0;
    for (start, end) in [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ] {
        let edge = end - start;
        let length = edge.length();
        if length <= 0.0 {
            continue;
        }
        let direction = edge / length;
        let mut offset = 0.0;
        while offset < length {
            let dash_end = (offset + DASH).min(length);
            painter.line_segment(
                [start + direction * offset, start + direction * dash_end],
                stroke,
            );
            offset += DASH + GAP;
        }
    }
}

fn handle_position(rect: EguiRect, handle: Handle) -> Pos2 {
    match handle {
        Handle::NorthWest => rect.left_top(),
        Handle::North => Pos2::new(rect.center().x, rect.top()),
        Handle::NorthEast => rect.right_top(),
        Handle::East => Pos2::new(rect.right(), rect.center().y),
        Handle::SouthEast => rect.right_bottom(),
        Handle::South => Pos2::new(rect.center().x, rect.bottom()),
        Handle::SouthWest => rect.left_bottom(),
        Handle::West => Pos2::new(rect.left(), rect.center().y),
    }
}

fn hit_handle(pointer: Pos2, rect: EguiRect) -> Option<Handle> {
    Handle::ALL.into_iter().find(|&handle| {
        EguiRect::from_center_size(handle_position(rect, handle), Vec2::splat(14.0))
            .contains(pointer)
    })
}

fn handle_cursor(handle: Handle) -> egui::CursorIcon {
    match handle {
        Handle::NorthWest | Handle::SouthEast => egui::CursorIcon::ResizeNwSe,
        Handle::NorthEast | Handle::SouthWest => egui::CursorIcon::ResizeNeSw,
        Handle::East | Handle::West => egui::CursorIcon::ResizeHorizontal,
        Handle::North | Handle::South => egui::CursorIcon::ResizeVertical,
    }
}

fn tab_label(tab: &DocumentTab, fallback_number: usize) -> String {
    let document = tab.editor.document();
    let title = document
        .metadata
        .title
        .as_deref()
        .filter(|title| !title.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Untitled-{fallback_number}"));
    let dirty = if tab.editor.can_undo() { "*" } else { "" };
    let mode = match document.settings.color_mode {
        ColorMode::Cmyk => "CMYK",
        ColorMode::Rgb => "RGB",
    };
    let preview = match document.settings.preview_mode {
        PreviewMode::Default => "Preview",
        PreviewMode::Pixel => "Pixel Preview",
        PreviewMode::Overprint => "Overprint Preview",
    };
    format!(
        "{title}{dirty} @ {:.2} % ({mode}/{preview})",
        tab.camera.scale * 100.0
    )
}

fn next_untitled_name(open: &[DocumentTab]) -> String {
    let highest = open
        .iter()
        .filter_map(|tab| tab.editor.document().metadata.title.as_deref())
        .filter_map(|title| title.strip_prefix("Untitled-"))
        .filter_map(|number| number.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Untitled-{}", highest + 1)
}

impl NewDocumentForm {
    fn commit_dimensions(&mut self) {
        if let Ok(value) = self.width_text.trim().parse::<f64>() {
            self.width = Length::new(value, self.unit);
        }
        if let Ok(value) = self.height_text.trim().parse::<f64>() {
            self.height = Length::new(value, self.unit);
        }
    }
    fn change_unit(&mut self, old: Unit) {
        if let Ok(value) = self.width_text.trim().parse::<f64>() {
            self.width = Length::new(value, old);
        }
        if let Ok(value) = self.height_text.trim().parse::<f64>() {
            self.height = Length::new(value, old);
        }
        for i in 0..4 {
            if let Ok(value) = self.bleed_text[i].trim().parse::<f64>() {
                self.bleed[i] = Length::new(value, old);
            }
        }
        self.refresh_text();
    }
    fn swap_dimensions(&mut self) {
        std::mem::swap(&mut self.width, &mut self.height);
        self.refresh_text();
    }
    fn refresh_text(&mut self) {
        self.width_text = format_number(self.width.in_unit(self.unit));
        self.height_text = format_number(self.height.in_unit(self.unit));
        for i in 0..4 {
            self.bleed_text[i] = format_number(self.bleed[i].in_unit(self.unit));
        }
    }
    fn commit_bleed(&mut self, index: usize) {
        if let Ok(value) = self.bleed_text[index].trim().parse::<f64>() {
            let length = Length::new(value.max(0.0), self.unit);
            if self.bleed_linked {
                self.bleed = [length; 4];
            } else {
                self.bleed[index] = length;
            }
            self.refresh_text();
        }
    }
    fn commit_all_bleed(&mut self) {
        if self.bleed_linked {
            let edited = (0..4).find(|&i| {
                self.bleed_text[i]
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .is_some_and(|value| (value - self.bleed[i].in_unit(self.unit)).abs() > 1e-7)
            });
            if let Some(index) = edited {
                self.commit_bleed(index);
            }
        } else {
            for i in 0..4 {
                self.commit_bleed(i);
            }
        }
    }
}

fn labeled(ui: &mut egui::Ui, label: &str, body: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).color(Color32::from_gray(185)));
        body(ui);
    });
}
fn numeric_field(ui: &mut egui::Ui, text: &mut String, unit: Unit, width: f32) -> egui::Response {
    ui.add_sized(
        [width, 34.0],
        egui::TextEdit::singleline(text).hint_text(format!("0 {}", unit_short(unit))),
    )
}
fn combo_row(
    ui: &mut egui::Ui,
    label: &str,
    id: &str,
    selected: &str,
    body: impl FnOnce(&mut egui::Ui),
) {
    labeled(ui, label, |ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(selected)
            .width(342.0)
            .show_ui(ui, body);
    });
}
fn unit_name(unit: Unit) -> &'static str {
    match unit {
        Unit::Px => "Pixels",
        Unit::Pt => "Points",
        Unit::In => "Inches",
        Unit::Mm => "Millimeters",
        Unit::Cm => "Centimeters",
    }
}
fn unit_short(unit: Unit) -> &'static str {
    match unit {
        Unit::Px => "px",
        Unit::Pt => "pt",
        Unit::In => "in",
        Unit::Mm => "mm",
        Unit::Cm => "cm",
    }
}
fn color_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Cmyk => "CMYK Color",
        ColorMode::Rgb => "RGB Color",
    }
}
fn raster_name(value: RasterEffects) -> &'static str {
    match value {
        RasterEffects::Screen72 => "Screen (72 ppi)",
        RasterEffects::Medium150 => "Medium (150 ppi)",
        RasterEffects::High300 => "High (300 ppi)",
    }
}
fn preview_name(value: PreviewMode) -> &'static str {
    match value {
        PreviewMode::Default => "Default",
        PreviewMode::Pixel => "Pixel",
        PreviewMode::Overprint => "Overprint",
    }
}
fn format_number(value: f64) -> String {
    if (value - value.round()).abs() < 1e-7 {
        format!("{:.0}", value)
    } else {
        format!("{:.3}", value).trim_end_matches('0').to_string()
    }
}

#[cfg(test)]
mod canvas_input_tests {
    use super::*;
    use amalith_commands::Command;

    #[test]
    fn command_space_takes_precedence_over_hand_pan() {
        assert_eq!(canvas_navigation(true, true), CanvasNavigation::ScrubbyZoom);
        assert_ne!(canvas_navigation(true, true), CanvasNavigation::HandPan);
    }

    #[test]
    fn tab_label_contains_title_dirty_zoom_and_color_mode() {
        let mut document = Document::new("Untitled-3");
        document.settings.color_mode = ColorMode::Rgb;
        document.settings.preview_mode = PreviewMode::Default;
        let mut editor = Editor::new(document);
        editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap();
        let mut camera = Camera::default();
        camera.scale = 3.4377;
        let tab = DocumentTab {
            editor: Box::new(editor),
            camera,
            artboard_tool: ArtboardTool::default(),
        };

        assert_eq!(tab_label(&tab, 3), "Untitled-3* @ 343.77 % (RGB/Preview)");
    }

    #[test]
    fn tab_label_uses_numbered_fallback_and_clean_state() {
        let mut document = Document::new("ignored");
        document.metadata.title = None;
        let tab = DocumentTab {
            editor: Box::new(Editor::new(document)),
            camera: Camera::default(),
            artboard_tool: ArtboardTool::default(),
        };
        assert_eq!(tab_label(&tab, 4), "Untitled-4 @ 100.00 % (CMYK/Preview)");
    }

    #[test]
    fn untitled_names_increment_from_open_titles() {
        fn tab(title: &str) -> DocumentTab {
            DocumentTab {
                editor: Box::new(Editor::new(Document::new(title))),
                camera: Camera::default(),
                artboard_tool: ArtboardTool::default(),
            }
        }
        assert_eq!(
            next_untitled_name(&[tab("Untitled-2"), tab("Logo")]),
            "Untitled-3"
        );
    }
}
