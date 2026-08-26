mod artboard_tool;
mod camera;
mod ellipse_tool;
mod rectangle_tool;
mod selection;

use amalith_commands::{Command, CommandOutcome, Editor};
use amalith_core::{
    ArtboardId, Bleed, ColorMode, Document, LayerId, Length, ObjectKind, ObjectParent, PathData,
    Point, PreviewMode, RasterEffects, Rect, Unit,
};
use artboard_tool::{next_artboard_name, ArtboardTool, DragKind, Handle};
use camera::Camera;
use eframe::egui::{self, Color32, FontId, Pos2, Rect as EguiRect, Stroke, Vec2};
use ellipse_tool::EllipseTool;
use rectangle_tool::RectangleTool;
use selection::SelectionTool;

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
    artboards_panel: ArtboardsPanelState,
    layers_panel: LayersPanelState,
}

struct DocumentTab {
    editor: Box<Editor>,
    camera: Camera,
    artboard_tool: ArtboardTool,
    rectangle_tool: RectangleTool,
    ellipse_tool: EllipseTool,
    selection_tool: SelectionTool,
}

struct ArtboardsPanelState {
    renaming: Option<ArtboardId>,
    rename_text: String,
    focus_rename: bool,
    chrome: PanelChromeState,
}

struct LayersPanelState {
    selected_layer: Option<LayerId>,
    chrome: PanelChromeState,
}

struct PanelChromeState {
    dock: PanelDock,
    hidden: bool,
    drag_offset: Vec2,
    dragging: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum PanelDock {
    Left,
    Right,
    Floating { pos: Pos2 },
}

impl Default for ArtboardsPanelState {
    fn default() -> Self {
        Self {
            renaming: None,
            rename_text: String::new(),
            focus_rename: false,
            chrome: PanelChromeState::default(),
        }
    }
}

impl Default for LayersPanelState {
    fn default() -> Self {
        Self {
            selected_layer: None,
            chrome: PanelChromeState::default(),
        }
    }
}

impl Default for PanelChromeState {
    fn default() -> Self {
        Self {
            dock: PanelDock::Right,
            hidden: false,
            drag_offset: Vec2::ZERO,
            dragging: false,
        }
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolKind {
    Selection,
    Rectangle,
    Ellipse,
    Artboard,
}

fn activate_tool(
    tool: ToolKind,
    artboard_tool: &mut ArtboardTool,
    rectangle_tool: &mut RectangleTool,
    ellipse_tool: &mut EllipseTool,
    selection_tool: &mut SelectionTool,
    first_artboard: Option<ArtboardId>,
) {
    artboard_tool.set_active(tool == ToolKind::Artboard, first_artboard);
    rectangle_tool.set_active(tool == ToolKind::Rectangle);
    ellipse_tool.set_active(tool == ToolKind::Ellipse);
    selection_tool.set_active(tool == ToolKind::Selection);
}

fn delete_shortcut_allowed(canvas_shortcuts_enabled: bool, navigation_key_down: bool) -> bool {
    canvas_shortcuts_enabled && !navigation_key_down
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

fn apply_canvas_gesture(
    camera: &mut Camera,
    scroll_delta: Vec2,
    zoom_delta: f32,
    anchor: Pos2,
    viewport: EguiRect,
) {
    if (zoom_delta - 1.0).abs() > f32::EPSILON {
        camera.zoom_at(anchor, zoom_delta, viewport);
    } else {
        camera.pan_by(scroll_delta);
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
            artboards_panel: ArtboardsPanelState::default(),
            layers_panel: LayersPanelState::default(),
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
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
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
                    &mut tab.rectangle_tool,
                    &mut tab.ellipse_tool,
                    &mut tab.selection_tool,
                    &mut self.artboards_panel,
                    &mut self.layers_panel,
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
        rectangle_tool: &mut RectangleTool,
        ellipse_tool: &mut EllipseTool,
        selection_tool: &mut SelectionTool,
        artboards_panel: &mut ArtboardsPanelState,
        layers_panel: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        Self::panels_ui(
            ctx,
            editor,
            artboard_tool,
            selection_tool,
            artboards_panel,
            layers_panel,
            error,
        );
        Self::tools_bar_ui(
            ctx,
            editor,
            artboard_tool,
            rectangle_tool,
            ellipse_tool,
            selection_tool,
        );
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
                let visible_document = camera.visible_document_rect(available);

                let shift_o_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::SHIFT, egui::Key::O) > 0
                });
                let escape_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Escape) > 0
                });
                let navigation_key_down = ctx.input(|input| input.key_down(egui::Key::Space));
                let canvas_shortcuts_enabled = !ctx.wants_keyboard_input();
                let selection_pressed = !navigation_key_down
                    && canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::V) > 0
                    });
                let rectangle_pressed = !navigation_key_down
                    && canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::M) > 0
                    });
                let ellipse_pressed = !navigation_key_down
                    && canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::L) > 0
                    });
                let first_artboard = boards.first().map(|board| board.id);
                if shift_o_pressed {
                    if artboard_tool.active {
                        artboard_tool.set_active(false, first_artboard);
                        rectangle_tool.set_active(false);
                        ellipse_tool.set_active(false);
                        selection_tool.set_active(false);
                    } else {
                        activate_tool(
                            ToolKind::Artboard,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            first_artboard,
                        );
                    }
                } else if selection_pressed {
                    activate_tool(
                        ToolKind::Selection,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        first_artboard,
                    );
                } else if rectangle_pressed {
                    activate_tool(
                        ToolKind::Rectangle,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        first_artboard,
                    );
                } else if ellipse_pressed {
                    activate_tool(
                        ToolKind::Ellipse,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        first_artboard,
                    );
                } else if escape_pressed {
                    artboard_tool.set_active(false, first_artboard);
                    rectangle_tool.set_active(false);
                    ellipse_tool.set_active(false);
                    selection_tool.set_active(false);
                }

                let delete_pressed =
                    delete_shortcut_allowed(canvas_shortcuts_enabled, navigation_key_down)
                        && ctx.input_mut(|input| {
                            input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
                                + input
                                    .count_and_consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                                > 0
                        });
                if delete_pressed {
                    if artboard_tool.active && !artboard_tool.is_dragging() {
                        if let Some(id) = artboard_tool.selected {
                            if let Err(err) = editor.execute(Command::DeleteArtboard { id }) {
                                *error = Some(err.to_string());
                            } else {
                                artboard_tool.preview_rect = None;
                                artboard_tool.selected = editor
                                    .document()
                                    .artboards()
                                    .last()
                                    .map(|artboard| artboard.id);
                            }
                        }
                    } else if selection_tool.active && !selection_tool.is_dragging() {
                        if !selection_tool.selected.is_empty() {
                            let ids = selection_tool.selected.iter().copied().collect();
                            if let Err(err) = editor.execute(Command::DeleteObjects { ids }) {
                                *error = Some(err.to_string());
                            } else {
                                selection_tool.selected.clear();
                                selection_tool.cancel_drag();
                            }
                        }
                    }
                }

                if canvas_shortcuts_enabled
                    && !artboard_tool.active
                    && selection_tool.active
                    && !selection_tool.selected.is_empty()
                {
                    let stack_step = ctx.input_mut(|input| {
                        if input
                            .count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::OpenBracket)
                            > 0
                        {
                            -1
                        } else if input.count_and_consume_key(
                            egui::Modifiers::COMMAND,
                            egui::Key::CloseBracket,
                        ) > 0
                        {
                            1
                        } else {
                            0
                        }
                    });
                    if stack_step != 0 {
                        let ids = selection_tool.selected.iter().copied().collect();
                        if let Err(err) = editor.execute(Command::NudgeStack {
                            ids,
                            steps: stack_step,
                        }) {
                            *error = Some(err.to_string());
                        }
                    }
                }

                let undo_pressed = ctx.input_mut(|input| {
                    input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::Z) > 0
                });
                if undo_pressed {
                    let _ = editor.undo();
                    artboard_tool.preview_rect = None;
                    if artboard_tool
                        .selected
                        .is_some_and(|id| editor.document().artboard(id).is_none())
                    {
                        artboard_tool.selected = editor
                            .document()
                            .artboards()
                            .last()
                            .map(|artboard| artboard.id);
                    }
                    selection_tool.retain_existing(editor.document());
                }

                let (
                    space_down,
                    command_down,
                    shift_down,
                    alt_down,
                    primary_down,
                    primary_pressed,
                    primary_released,
                    pointer_position,
                    pointer_delta,
                    smooth_scroll_delta,
                    zoom_delta,
                ) = ctx.input(|input| {
                    (
                        input.key_down(egui::Key::Space),
                        input.modifiers.command,
                        input.modifiers.shift,
                        input.modifiers.alt,
                        input.pointer.primary_down(),
                        input.pointer.primary_pressed(),
                        input.pointer.primary_released(),
                        input.pointer.interact_pos(),
                        input.pointer.delta(),
                        input.smooth_scroll_delta,
                        input.zoom_delta(),
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
                        rectangle_tool.cancel_drag();
                        ellipse_tool.cancel_drag();
                        selection_tool.cancel_drag();
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
                        rectangle_tool.cancel_drag();
                        ellipse_tool.cancel_drag();
                        selection_tool.cancel_drag();
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
                        let gesture_on_canvas = (smooth_scroll_delta != Vec2::ZERO
                            || (zoom_delta - 1.0).abs() > f32::EPSILON)
                            && pointer_position.is_some_and(|pointer| {
                                available.contains(pointer) && ui.rect_contains_pointer(available)
                            });
                        if let Some(pointer) = pointer_position.filter(|pointer| {
                            available.contains(*pointer) && ui.rect_contains_pointer(available)
                        }) {
                            apply_canvas_gesture(
                                camera,
                                smooth_scroll_delta,
                                zoom_delta,
                                pointer,
                                available,
                            );
                        }
                        if gesture_on_canvas {
                            rectangle_tool.cancel_drag();
                            ellipse_tool.cancel_drag();
                            selection_tool.cancel_drag();
                        } else if rectangle_tool.active {
                            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                            if let Some(pointer) = pointer_position {
                                let pointer_doc = camera.screen_to_document(pointer, available);
                                let point = Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                if primary_pressed
                                    && available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    rectangle_tool.begin_drag(point);
                                } else if primary_down {
                                    rectangle_tool.update_drag(point, shift_down);
                                }
                            }
                            if primary_released {
                                if let Err(err) = rectangle_tool.finish_drag(editor) {
                                    *error = Some(err.to_string());
                                }
                            }
                        } else if ellipse_tool.active {
                            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                            if let Some(pointer) = pointer_position {
                                let pointer_doc = camera.screen_to_document(pointer, available);
                                let point = Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                if primary_pressed
                                    && available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    ellipse_tool.begin_drag(point);
                                } else if primary_down {
                                    ellipse_tool.update_drag(point, shift_down);
                                }
                            }
                            if primary_released {
                                if let Err(err) = ellipse_tool.finish_drag(editor) {
                                    *error = Some(err.to_string());
                                }
                            }
                        } else if artboard_tool.active {
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
                        } else if selection_tool.active {
                            if let Some(pointer) = pointer_position {
                                let selected_quad = selection_tool
                                    .selected_quad(editor.document())
                                    .map(|quad| document_quad_to_screen(quad, camera, available))
                                    .or_else(|| {
                                        selection_tool.selected_union_bounds(editor.document()).map(
                                            |bounds| {
                                                document_quad_to_screen(
                                                    [
                                                        Point::new(bounds.x0, bounds.y0),
                                                        Point::new(bounds.x1, bounds.y0),
                                                        Point::new(bounds.x1, bounds.y1),
                                                        Point::new(bounds.x0, bounds.y1),
                                                    ],
                                                    camera,
                                                    available,
                                                )
                                            },
                                        )
                                    });
                                let hovered_handle = selected_quad
                                    .and_then(|quad| hit_oriented_handle(pointer, quad));
                                let hovered_rotate = hovered_handle.is_none()
                                    && selected_quad
                                        .is_some_and(|quad| hit_rotation_halo(pointer, quad));
                                let pointer_doc = camera.screen_to_document(pointer, available);
                                let point = Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                if primary_pressed
                                    && available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    if let Some(handle) = hovered_handle {
                                        selection_tool.begin_scale(editor.document(), handle);
                                    } else if hovered_rotate {
                                        selection_tool.begin_rotate(editor.document(), point);
                                    } else {
                                        selection_tool.press(
                                            editor.document(),
                                            point,
                                            visible_document,
                                            alt_down,
                                            shift_down,
                                        );
                                    }
                                } else if primary_down {
                                    selection_tool.drag(point, shift_down, alt_down);
                                }
                                if selection_tool.is_duplicate_drag() {
                                    ctx.set_cursor_icon(egui::CursorIcon::Copy);
                                } else if let Some(handle) = hovered_handle {
                                    ctx.set_cursor_icon(handle_cursor(handle));
                                } else if hovered_rotate {
                                    ctx.set_cursor_icon(egui::CursorIcon::Alias);
                                }
                            }
                            if primary_released {
                                if let Err(err) = selection_tool.finish_drag(editor) {
                                    *error = Some(err.to_string());
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
                    if !rects_intersect(document_rect, visible_document) {
                        continue;
                    }
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
                for layer in editor.document().layers() {
                    for &id in editor.document().children_of(ObjectParent::Layer(layer.id)) {
                        let Some(object) = editor.document().object(id) else {
                            continue;
                        };
                        if !object.visible || !matches!(object.kind, ObjectKind::Path(_)) {
                            continue;
                        }
                        let Some(bounds) = editor.document().bounds_of(id) else {
                            continue;
                        };
                        if !rects_intersect(bounds, visible_document) {
                            continue;
                        }
                        let move_delta = (!selection_tool.active)
                            .then(|| artboard_tool.move_preview())
                            .flatten()
                            .filter(|(source, _)| rects_intersect(bounds, *source))
                            .map(|(_, delta)| delta)
                            .unwrap_or_default();
                        if let (Some(object), Some(transform)) = (
                            editor.document().object(id),
                            selection_tool.display_transform(editor.document(), id),
                        ) {
                            if let ObjectKind::Path(path) = &object.kind {
                                for local_points in path.flattened_points(0.5) {
                                    let points = local_points
                                        .into_iter()
                                        .map(|point| {
                                            let point = transform * point + move_delta;
                                            let screen = camera.document_to_screen(
                                                Pos2::new(point.x as f32, point.y as f32),
                                                available,
                                            );
                                            screen
                                        })
                                        .collect::<Vec<_>>();
                                    if points.len() >= 3 {
                                        painter.add(egui::Shape::convex_polygon(
                                            points,
                                            Color32::from_gray(225),
                                            Stroke::new(1.0_f32, Color32::from_gray(45)),
                                        ));
                                    }
                                }
                            }
                        }
                        if let Some((_, delta)) = artboard_tool
                            .duplicate_artwork_preview()
                            .filter(|(source, _)| rects_intersect(bounds, *source))
                        {
                            let copy_rect =
                                document_rect_to_screen(bounds + delta, camera, available);
                            painter.rect_filled(copy_rect, 0.0, Color32::from_gray(225));
                            painter.rect_stroke(
                                copy_rect,
                                0.0,
                                Stroke::new(1.0_f32, Color32::from_gray(45)),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
                if let Some(bounds) = selection_tool.duplicate_preview_bounds(editor.document()) {
                    if rects_intersect(bounds, visible_document) {
                        if let Some(quad) = selection_tool.duplicate_preview_quad(editor.document())
                        {
                            let points = document_quad_to_screen(quad, camera, available).to_vec();
                            painter.add(egui::Shape::convex_polygon(
                                points,
                                Color32::from_gray(225),
                                Stroke::new(1.0_f32, Color32::from_gray(45)),
                            ));
                        }
                    }
                }
                if let Some(marquee) = selection_tool.marquee_rect() {
                    let rect = document_rect_to_screen(marquee, camera, available);
                    painter.rect_filled(
                        rect,
                        0.0,
                        Color32::from_rgba_unmultiplied(90, 165, 255, 26),
                    );
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_rgb(59, 155, 255)),
                        egui::StrokeKind::Outside,
                    );
                }
                if selection_tool.active {
                    if selection_tool.selected_intersects(editor.document(), visible_document) {
                        let quad = selection_tool.selected_quad(editor.document()).or_else(|| {
                            selection_tool
                                .selected_union_bounds(editor.document())
                                .map(|bounds| {
                                    [
                                        Point::new(bounds.x0, bounds.y0),
                                        Point::new(bounds.x1, bounds.y0),
                                        Point::new(bounds.x1, bounds.y1),
                                        Point::new(bounds.x0, bounds.y1),
                                    ]
                                })
                        });
                        if let Some(quad) = quad {
                            let quad = document_quad_to_screen(quad, camera, available);
                            let blue = Color32::from_rgb(59, 155, 255);
                            painter.add(egui::Shape::closed_line(
                                quad.to_vec(),
                                Stroke::new(1.25_f32, blue),
                            ));
                            for handle in Handle::ALL {
                                let handle_rect = EguiRect::from_center_size(
                                    oriented_handle_position(quad, handle),
                                    Vec2::splat(8.0),
                                );
                                painter.rect_filled(handle_rect, 0.0, Color32::WHITE);
                                painter.rect_stroke(
                                    handle_rect,
                                    0.0,
                                    Stroke::new(1.25_f32, blue),
                                    egui::StrokeKind::Outside,
                                );
                            }
                            let center =
                                EguiRect::from_center_size(quad_center(quad), Vec2::splat(6.0));
                            painter.rect_filled(center, 0.0, blue);
                        }
                    }
                }
                if let Some(preview) = rectangle_tool.preview_rect {
                    let rect = document_rect_to_screen(preview, camera, available);
                    painter.rect_filled(rect, 0.0, Color32::from_white_alpha(80));
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_gray(35)),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(preview) = ellipse_tool.preview_rect {
                    let path = PathData::ellipse(preview);
                    for local_points in path.flattened_points(0.5) {
                        let points = local_points
                            .into_iter()
                            .map(|point| {
                                camera.document_to_screen(
                                    Pos2::new(point.x as f32, point.y as f32),
                                    available,
                                )
                            })
                            .collect::<Vec<_>>();
                        if points.len() >= 3 {
                            painter.add(egui::Shape::convex_polygon(
                                points,
                                Color32::from_white_alpha(80),
                                Stroke::new(1.0_f32, Color32::from_gray(35)),
                            ));
                        }
                    }
                }
                if let Some(preview) = artboard_tool.duplicate_preview() {
                    if !rects_intersect(preview, visible_document) {
                        return;
                    }
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

    fn tools_bar_ui(
        ctx: &egui::Context,
        editor: &Editor,
        artboard_tool: &mut ArtboardTool,
        rectangle_tool: &mut RectangleTool,
        ellipse_tool: &mut EllipseTool,
        selection_tool: &mut SelectionTool,
    ) {
        let first_artboard = editor.document().artboards().first().map(|board| board.id);
        egui::SidePanel::left("tools_bar")
            .exact_width(46.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(42, 42, 42)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(8.0);
                    let button =
                        |ui: &mut egui::Ui, label: &str, tooltip: &str, active: bool| -> bool {
                            ui.add_sized(
                                [34.0, 34.0],
                                egui::Button::new(label)
                                    .fill(if active {
                                        Color32::from_rgb(68, 100, 132)
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .frame(false),
                            )
                            .on_hover_text(tooltip)
                            .clicked()
                        };

                    if button(ui, "V", "Selection (V)", selection_tool.active) {
                        activate_tool(
                            ToolKind::Selection,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            first_artboard,
                        );
                    }
                    if button(ui, "M", "Rectangle (M)", rectangle_tool.active) {
                        activate_tool(
                            ToolKind::Rectangle,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            first_artboard,
                        );
                    }
                    if button(ui, "L", "Ellipse (L)", ellipse_tool.active) {
                        activate_tool(
                            ToolKind::Ellipse,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            first_artboard,
                        );
                    }
                    if button(ui, "Art", "Artboard (Shift+O)", artboard_tool.active) {
                        activate_tool(
                            ToolKind::Artboard,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            first_artboard,
                        );
                    }
                });
            });
    }

    fn artboards_panel_body(
        ui: &mut egui::Ui,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        state: &mut ArtboardsPanelState,
        error: &mut Option<String>,
    ) {
        let boards = editor.document().artboards().to_vec();
        let rows_height = (ui.available_height() - 34.0).max(0.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(rows_height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, board) in boards.iter().enumerate() {
                    let selected = artboard_tool.selected == Some(board.id);
                    let fill = if selected {
                        Color32::from_rgb(62, 82, 103)
                    } else {
                        Color32::TRANSPARENT
                    };
                    egui::Frame::NONE
                        .fill(fill)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let index_response = ui.add_sized(
                                    [25.0, 22.0],
                                    egui::Label::new(
                                        egui::RichText::new((index + 1).to_string())
                                            .color(Color32::from_gray(155)),
                                    )
                                    .sense(egui::Sense::click()),
                                );

                                if state.renaming == Some(board.id) {
                                    let response = ui.add_sized(
                                        [ui.available_width(), 22.0],
                                        egui::TextEdit::singleline(&mut state.rename_text)
                                            .id_salt(("artboard_rename", board.id)),
                                    );
                                    if state.focus_rename {
                                        response.request_focus();
                                        state.focus_rename = false;
                                    }
                                    let enter = response.has_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                                    if enter || response.lost_focus() {
                                        let name = state.rename_text.trim();
                                        if !name.is_empty() && name != board.name {
                                            if let Err(err) =
                                                editor.execute(Command::RenameArtboard {
                                                    id: board.id,
                                                    name: name.to_owned(),
                                                })
                                            {
                                                *error = Some(err.to_string());
                                            }
                                        }
                                        state.renaming = None;
                                    }
                                } else {
                                    let name_response = ui.add_sized(
                                        [ui.available_width(), 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(&board.name)
                                                .color(Color32::from_gray(225)),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if name_response.double_clicked() {
                                        state.renaming = Some(board.id);
                                        state.rename_text = board.name.clone();
                                        state.focus_rename = true;
                                    }
                                    if name_response.clicked() {
                                        artboard_tool.select(board.id);
                                    }
                                }
                                if index_response.clicked() {
                                    artboard_tool.select(board.id);
                                }
                            });
                        });
                }
            });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.separator();
            if ui
                .add_sized(
                    [32.0, 28.0],
                    egui::Button::new(egui::RichText::new("+").size(20.0)).frame(false),
                )
                .on_hover_text("New artboard")
                .clicked()
            {
                match artboard_tool.create_to_right(editor) {
                    Ok(CommandOutcome::Artboard(id)) => artboard_tool.select(id),
                    Ok(_) => {}
                    Err(err) => *error = Some(err.to_string()),
                }
            }
        });
    }

    fn layers_panel_body(
        ui: &mut egui::Ui,
        editor: &mut Editor,
        selection_tool: &mut SelectionTool,
        state: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let layers = editor.document().layers().to_vec();
        let rows_height = (ui.available_height() - 34.0).max(0.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .max_height(rows_height)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for layer in &layers {
                    let layer_selected = state.selected_layer == Some(layer.id);
                    let layer_response = egui::Frame::NONE
                        .fill(if layer_selected {
                            Color32::from_rgb(62, 82, 103)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&layer.name)
                                        .strong()
                                        .color(Color32::from_gray(225)),
                                )
                                .sense(egui::Sense::click()),
                            )
                        })
                        .inner;
                    if layer_response.clicked() {
                        state.selected_layer = Some(layer.id);
                    }

                    for &id in layer.children.iter().rev() {
                        let Some(object) = editor.document().object(id) else {
                            continue;
                        };
                        let selected = selection_tool.selected.contains(&id);
                        let response = egui::Frame::NONE
                            .fill(if selected {
                                Color32::from_rgb(62, 82, 103)
                            } else {
                                Color32::TRANSPARENT
                            })
                            .inner_margin(egui::Margin {
                                left: 28,
                                right: 10,
                                top: 4,
                                bottom: 4,
                            })
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(
                                            object.name.as_deref().unwrap_or("Object"),
                                        )
                                        .color(Color32::from_gray(210)),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                            })
                            .inner;
                        if response.clicked() {
                            let add = ui.input(|input| input.modifiers.shift);
                            if add {
                                if !selection_tool.selected.insert(id) {
                                    selection_tool.selected.remove(&id);
                                }
                            } else {
                                selection_tool.selected.clear();
                                selection_tool.selected.insert(id);
                            }
                            state.selected_layer = Some(layer.id);
                        }
                    }
                }
            });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.separator();
            if ui
                .add_sized(
                    [32.0, 28.0],
                    egui::Button::new(egui::RichText::new("+").size(20.0)).frame(false),
                )
                .on_hover_text("New layer")
                .clicked()
            {
                let next = editor
                    .document()
                    .layers()
                    .iter()
                    .filter_map(|layer| layer.name.strip_prefix("Layer "))
                    .filter_map(|suffix| suffix.parse::<usize>().ok())
                    .max()
                    .unwrap_or(0)
                    + 1;
                match editor.execute(Command::CreateLayer {
                    name: format!("Layer {next}"),
                    index: None,
                }) {
                    Ok(CommandOutcome::Layer(id)) => state.selected_layer = Some(id),
                    Ok(_) => {}
                    Err(err) => *error = Some(err.to_string()),
                }
            }
        });
    }

    fn panels_ui(
        ctx: &egui::Context,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        selection_tool: &mut SelectionTool,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let artboards_left = !artboards.chrome.hidden && artboards.chrome.dock == PanelDock::Left;
        let layers_left = !layers.chrome.hidden && layers.chrome.dock == PanelDock::Left;
        if artboards_left || layers_left {
            egui::SidePanel::left("left_panel_dock")
                .exact_width(220.0)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(Color32::from_rgb(42, 42, 42)))
                .show(ctx, |ui| {
                    Self::dock_column_body(
                        ui,
                        ctx,
                        PanelDock::Left,
                        editor,
                        artboard_tool,
                        selection_tool,
                        artboards,
                        layers,
                        error,
                    );
                });
        }

        let artboards_right = !artboards.chrome.hidden && artboards.chrome.dock == PanelDock::Right;
        let layers_right = !layers.chrome.hidden && layers.chrome.dock == PanelDock::Right;
        if artboards_right || layers_right {
            egui::SidePanel::right("right_panel_dock")
                .exact_width(220.0)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(Color32::from_rgb(42, 42, 42)))
                .show(ctx, |ui| {
                    Self::dock_column_body(
                        ui,
                        ctx,
                        PanelDock::Right,
                        editor,
                        artboard_tool,
                        selection_tool,
                        artboards,
                        layers,
                        error,
                    );
                });
        }

        Self::floating_artboards_panel(ctx, editor, artboard_tool, artboards, error);
        Self::floating_layers_panel(ctx, editor, selection_tool, layers, error);
        Self::paint_panel_drop_target(ctx, &artboards.chrome, "artboards_drop_target");
        Self::paint_panel_drop_target(ctx, &layers.chrome, "layers_drop_target");
    }

    #[allow(clippy::too_many_arguments)]
    fn dock_column_body(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        side: PanelDock,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        selection_tool: &mut SelectionTool,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let show_artboards = !artboards.chrome.hidden && artboards.chrome.dock == side;
        let show_layers = !layers.chrome.hidden && layers.chrome.dock == side;
        let panel_count = usize::from(show_artboards) + usize::from(show_layers);
        let panel_height = ui.available_height() / panel_count.max(1) as f32;

        if show_artboards {
            ui.push_id("artboards_panel", |ui| {
                ui.allocate_ui(Vec2::new(ui.available_width(), panel_height), |ui| {
                    let header = Self::panel_title_bar(ui, "Artboards");
                    Self::handle_panel_drag(ctx, header, &mut artboards.chrome);
                    ui.separator();
                    Self::artboards_panel_body(ui, editor, artboard_tool, artboards, error);
                });
            });
        }
        if show_layers {
            ui.push_id("layers_panel", |ui| {
                ui.allocate_ui(Vec2::new(ui.available_width(), panel_height), |ui| {
                    let header = Self::panel_title_bar(ui, "Layers");
                    Self::handle_panel_drag(ctx, header, &mut layers.chrome);
                    ui.separator();
                    Self::layers_panel_body(ui, editor, selection_tool, layers, error);
                });
            });
        }
    }

    fn panel_title_bar(ui: &mut egui::Ui, title: &str) -> egui::Response {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .strong()
                        .color(Color32::from_gray(226)),
                )
                .sense(egui::Sense::drag()),
            )
        })
        .inner
        .on_hover_cursor(egui::CursorIcon::Grab)
    }

    fn handle_panel_drag(
        ctx: &egui::Context,
        response: egui::Response,
        chrome: &mut PanelChromeState,
    ) {
        let Some(pointer) = response.interact_pointer_pos() else {
            return;
        };
        if response.drag_started() {
            chrome.drag_offset = pointer - response.rect.min;
            chrome.dragging = true;
        }
        if response.dragged() || response.drag_stopped() {
            chrome.dock = Self::dock_target(pointer, ctx).unwrap_or(PanelDock::Floating {
                pos: pointer - chrome.drag_offset,
            });
            ctx.request_repaint();
        }
        if response.drag_stopped() && !matches!(chrome.dock, PanelDock::Floating { .. }) {
            chrome.dragging = false;
        }
    }

    fn floating_artboards_panel(
        ctx: &egui::Context,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        state: &mut ArtboardsPanelState,
        error: &mut Option<String>,
    ) {
        let PanelDock::Floating { pos } = state.chrome.dock else {
            return;
        };
        if state.chrome.hidden {
            return;
        }
        let mut open = true;
        let response = egui::Window::new("Artboards")
            .id(egui::Id::new("artboards_panel"))
            .default_width(220.0)
            .min_width(180.0)
            .min_height(180.0)
            .current_pos(pos)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(42, 42, 42)))
            .show(ctx, |ui| {
                Self::artboards_panel_body(ui, editor, artboard_tool, state, error)
            });
        state.chrome.hidden = !open;
        Self::handle_floating_response(
            ctx,
            response.map(|window| window.response),
            &mut state.chrome,
            pos,
        );
    }

    fn floating_layers_panel(
        ctx: &egui::Context,
        editor: &mut Editor,
        selection_tool: &mut SelectionTool,
        state: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let PanelDock::Floating { pos } = state.chrome.dock else {
            return;
        };
        if state.chrome.hidden {
            return;
        }
        let mut open = true;
        let response = egui::Window::new("Layers")
            .id(egui::Id::new("layers_panel"))
            .default_width(220.0)
            .min_width(180.0)
            .min_height(180.0)
            .current_pos(pos)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.style()).fill(Color32::from_rgb(42, 42, 42)))
            .show(ctx, |ui| {
                Self::layers_panel_body(ui, editor, selection_tool, state, error)
            });
        state.chrome.hidden = !open;
        Self::handle_floating_response(
            ctx,
            response.map(|window| window.response),
            &mut state.chrome,
            pos,
        );
    }

    fn handle_floating_response(
        ctx: &egui::Context,
        response: Option<egui::Response>,
        chrome: &mut PanelChromeState,
        previous_pos: Pos2,
    ) {
        let Some(response) = response else { return };
        let actual_pos = response.rect.min;
        let pointer_down = ctx.input(|input| input.pointer.primary_down());
        let window_moved = actual_pos.distance(previous_pos) > 0.1;
        if pointer_down && window_moved {
            chrome.dragging = true;
        }
        let pos = if chrome.dragging && pointer_down && !window_moved {
            ctx.input(|input| input.pointer.interact_pos())
                .map_or(actual_pos, |pointer| pointer - chrome.drag_offset)
        } else {
            actual_pos
        };
        chrome.dock = PanelDock::Floating { pos };
        if chrome.dragging && ctx.input(|input| input.pointer.primary_released()) {
            if let Some(pointer) = ctx.input(|input| input.pointer.interact_pos()) {
                if let Some(target) = Self::dock_target(pointer, ctx) {
                    chrome.dock = target;
                }
            }
            chrome.dragging = false;
        }
    }

    fn dock_target(pointer: Pos2, ctx: &egui::Context) -> Option<PanelDock> {
        let rect = ctx.input(|input| input.screen_rect());
        if pointer.x <= rect.left() + 40.0 {
            Some(PanelDock::Left)
        } else if pointer.x >= rect.right() - 40.0 {
            Some(PanelDock::Right)
        } else {
            None
        }
    }

    fn paint_panel_drop_target(ctx: &egui::Context, state: &PanelChromeState, id: &'static str) {
        if !state.dragging || !ctx.input(|input| input.pointer.primary_down()) {
            return;
        }
        let Some(pointer) = ctx.input(|input| input.pointer.interact_pos()) else {
            return;
        };
        let rect = ctx.input(|input| input.screen_rect());
        let target = if pointer.x <= rect.left() + 40.0 {
            Some(EguiRect::from_min_max(
                rect.min,
                Pos2::new(rect.left() + 4.0, rect.bottom()),
            ))
        } else if pointer.x >= rect.right() - 40.0 {
            Some(EguiRect::from_min_max(
                Pos2::new(rect.right() - 4.0, rect.top()),
                rect.max,
            ))
        } else {
            None
        };
        if let Some(target) = target {
            ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new(id),
            ))
            .rect_filled(target, 0.0, Color32::from_rgb(45, 139, 242));
        }
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

fn document_quad_to_screen(quad: [Point; 4], camera: &Camera, viewport: EguiRect) -> [Pos2; 4] {
    quad.map(|point| camera.document_to_screen(Pos2::new(point.x as f32, point.y as f32), viewport))
}

fn quad_center(quad: [Pos2; 4]) -> Pos2 {
    Pos2::new(
        quad.iter().map(|point| point.x).sum::<f32>() / 4.0,
        quad.iter().map(|point| point.y).sum::<f32>() / 4.0,
    )
}

fn oriented_handle_position(quad: [Pos2; 4], handle: Handle) -> Pos2 {
    let midpoint = |a: Pos2, b: Pos2| a + (b - a) * 0.5;
    match handle {
        Handle::NorthWest => quad[0],
        Handle::North => midpoint(quad[0], quad[1]),
        Handle::NorthEast => quad[1],
        Handle::East => midpoint(quad[1], quad[2]),
        Handle::SouthEast => quad[2],
        Handle::South => midpoint(quad[2], quad[3]),
        Handle::SouthWest => quad[3],
        Handle::West => midpoint(quad[3], quad[0]),
    }
}

fn hit_oriented_handle(pointer: Pos2, quad: [Pos2; 4]) -> Option<Handle> {
    Handle::ALL.into_iter().find(|&handle| {
        EguiRect::from_center_size(oriented_handle_position(quad, handle), Vec2::splat(14.0))
            .contains(pointer)
    })
}

fn hit_rotation_halo(pointer: Pos2, quad: [Pos2; 4]) -> bool {
    if point_in_convex_quad(pointer, quad) {
        return false;
    }
    let center = quad_center(quad);
    [quad[0], quad[1], quad[2], quad[3]]
        .into_iter()
        .any(|corner| {
            let outward = corner - center;
            let length = outward.length();
            if length <= f32::EPSILON {
                return false;
            }
            let offset = pointer - corner;
            let distance = offset.length();
            let points_outward = offset.dot(outward / length) > 0.0;
            points_outward && (8.0..=32.0).contains(&distance)
        })
}

fn point_in_convex_quad(pointer: Pos2, quad: [Pos2; 4]) -> bool {
    let mut has_positive = false;
    let mut has_negative = false;
    for index in 0..4 {
        let edge = quad[(index + 1) % 4] - quad[index];
        let to_pointer = pointer - quad[index];
        let cross = edge.x * to_pointer.y - edge.y * to_pointer.x;
        has_positive |= cross > 0.0;
        has_negative |= cross < 0.0;
    }
    !(has_positive && has_negative)
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

fn rects_intersect(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && a.x1 > b.x0 && a.y0 < b.y1 && a.y1 > b.y0
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
    fn tool_switch_is_exclusive() {
        let mut artboard = ArtboardTool::default();
        let mut rectangle = RectangleTool::default();
        let mut ellipse = EllipseTool::default();
        let mut selection = SelectionTool::default();

        activate_tool(
            ToolKind::Rectangle,
            &mut artboard,
            &mut rectangle,
            &mut ellipse,
            &mut selection,
            None,
        );
        assert!(rectangle.active);
        assert!(!selection.active);
        assert!(!artboard.active);

        activate_tool(
            ToolKind::Artboard,
            &mut artboard,
            &mut rectangle,
            &mut ellipse,
            &mut selection,
            None,
        );
        assert!(artboard.active);
        assert!(!rectangle.active);
        assert!(!selection.active);
    }

    #[test]
    fn delete_shortcut_is_blocked_while_text_field_has_focus() {
        assert!(!delete_shortcut_allowed(false, false));
        assert!(!delete_shortcut_allowed(true, true));
        assert!(delete_shortcut_allowed(true, false));
    }

    #[test]
    fn corner_square_is_scale_not_rotate() {
        let quad = [
            Pos2::new(100.0, 100.0),
            Pos2::new(300.0, 100.0),
            Pos2::new(300.0, 250.0),
            Pos2::new(100.0, 250.0),
        ];
        let pointer = quad[0];

        assert_eq!(hit_oriented_handle(pointer, quad), Some(Handle::NorthWest));
        assert!(!hit_rotation_halo(pointer, quad));
    }

    #[test]
    fn outside_corner_diagonal_has_generous_rotate_target() {
        let quad = [
            Pos2::new(100.0, 100.0),
            Pos2::new(300.0, 100.0),
            Pos2::new(300.0, 250.0),
            Pos2::new(100.0, 250.0),
        ];
        let outward = (quad[0] - quad_center(quad)).normalized();
        let pointer = quad[0] + outward * 20.0;

        assert_eq!(hit_oriented_handle(pointer, quad), None);
        assert!(hit_rotation_halo(pointer, quad));
    }

    #[test]
    fn scroll_delta_pans_canvas_one_for_one() {
        let mut camera = Camera::default();
        let viewport = EguiRect::from_min_size(Pos2::new(30.0, 50.0), Vec2::new(500.0, 400.0));
        let delta = Vec2::new(17.0, -9.0);

        apply_canvas_gesture(&mut camera, delta, 1.0, viewport.center(), viewport);

        assert_eq!(camera.pan, delta);
    }

    #[test]
    fn pinch_keeps_document_point_under_cursor() {
        let mut camera = Camera::default();
        let viewport = EguiRect::from_min_size(Pos2::new(30.0, 50.0), Vec2::new(500.0, 400.0));
        let document = EguiRect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 100.0));
        camera.initialize_fit(viewport, document);
        let document_point = Pos2::new(37.0, 62.0);
        let anchor = camera.document_to_screen(document_point, viewport);

        apply_canvas_gesture(&mut camera, Vec2::new(5.0, 8.0), 1.5, anchor, viewport);

        assert!((camera.document_to_screen(document_point, viewport) - anchor).length() < 0.001);
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
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
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
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
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
                rectangle_tool: RectangleTool::default(),
                ellipse_tool: EllipseTool::default(),
                selection_tool: SelectionTool::default(),
            }
        }
        assert_eq!(
            next_untitled_name(&[tab("Untitled-2"), tab("Logo")]),
            "Untitled-3"
        );
    }
}
