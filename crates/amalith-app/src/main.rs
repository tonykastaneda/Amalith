mod artboard_tool;
mod camera;
mod direct_selection;
mod ellipse_tool;
mod fill_mesh;
mod rectangle_tool;
mod selection;

use amalith_commands::{Command, CommandOutcome, Editor, PasteStack};
use amalith_core::{
    ArtboardId, Bleed, ColorMode, Document, LayerId, Length, ObjectId, ObjectKind, ObjectParent,
    PathData, Point, PreviewMode, RasterEffects, Rect, Unit,
};
use artboard_tool::{next_artboard_name, ArtboardTool, DragKind, Handle};
use camera::Camera;
use direct_selection::DirectSelectionTool;
use eframe::egui::{self, Color32, FontId, Pos2, Rect as EguiRect, Stroke, Vec2};
use ellipse_tool::EllipseTool;
use rectangle_tool::RectangleTool;
use selection::SelectionTool;
use std::collections::HashSet;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
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
    fill_stroke_panel: FillStrokePanelState,
    #[cfg(target_os = "macos")]
    native_menu: Option<NativeMenu>,
}

/// Which color-picker popup (if either) the fill/stroke swatch widget has
/// open. See `Self::fill_stroke_widget_ui`.
#[derive(Default)]
struct FillStrokePanelState {
    /// The slot currently in front and controlled by the compact widget's
    /// Color/Gradient/None buttons. This persists independently of whether
    /// the full color picker is open.
    active: PaintSlot,
    /// Which slot's picker is open, plus its *working* color — a copy the
    /// dialog edits freely and only commits to the document on OK
    /// (Cancel, or closing the window, discards it). Seeded from the
    /// current selection's color exactly once, at the moment the dialog
    /// opens — never re-derived from the live selection on later frames,
    /// or every edit would immediately get overwritten back to the
    /// unchanged document value.
    open: Option<(PaintSlot, egui::ecolor::Hsva)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaintSlot {
    Fill,
    Stroke,
}

impl Default for PaintSlot {
    fn default() -> Self {
        Self::Fill
    }
}

struct DocumentTab {
    editor: Box<Editor>,
    asset_store: amalith_io::AssetStore,
    file_path: Option<PathBuf>,
    camera: Camera,
    artboard_tool: ArtboardTool,
    rectangle_tool: RectangleTool,
    ellipse_tool: EllipseTool,
    selection_tool: SelectionTool,
    direct_selection_tool: DirectSelectionTool,
}

#[cfg(target_os = "macos")]
struct NativeMenu {
    _menu: muda::Menu,
    new_item: muda::MenuItem,
    open_item: muda::MenuItem,
    save_item: muda::MenuItem,
    save_as_item: muda::MenuItem,
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
    renaming: Option<LayersRenameTarget>,
    rename_text: String,
    focus_rename: bool,
    /// Groups collapsed in the panel tree. Absence means expanded, so a
    /// newly-created group (nothing in this set yet) shows its contents
    /// immediately.
    collapsed_groups: HashSet<ObjectId>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LayersRenameTarget {
    Layer(LayerId),
    Object(ObjectId),
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
            renaming: None,
            rename_text: String::new(),
            focus_rename: false,
            collapsed_groups: HashSet::new(),
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
    DirectSelection,
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
    direct_selection_tool: &mut DirectSelectionTool,
    first_artboard: Option<ArtboardId>,
) {
    artboard_tool.set_active(tool == ToolKind::Artboard, first_artboard);
    rectangle_tool.set_active(tool == ToolKind::Rectangle);
    ellipse_tool.set_active(tool == ToolKind::Ellipse);
    selection_tool.set_active(tool == ToolKind::Selection);
    direct_selection_tool.set_active(tool == ToolKind::DirectSelection);
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
        #[cfg(target_os = "macos")]
        let native_menu = Some(Self::build_native_menu());
        Self {
            open: Vec::new(),
            active: 0,
            creating: Some(NewDocumentForm::default()),
            error: None,
            artboards_panel: ArtboardsPanelState::default(),
            layers_panel: LayersPanelState::default(),
            fill_stroke_panel: FillStrokePanelState::default(),
            #[cfg(target_os = "macos")]
            native_menu,
        }
    }

    #[cfg(target_os = "macos")]
    fn build_native_menu() -> NativeMenu {
        use muda::{
            accelerator::{Accelerator, Code, Modifiers},
            Menu, MenuItem, PredefinedMenuItem, Submenu,
        };
        let menu = Menu::new();
        let file = Submenu::new("File", true);
        let command = Some(Modifiers::SUPER);
        let new_item = MenuItem::new("New", true, Some(Accelerator::new(command, Code::KeyN)));
        let open_item = MenuItem::new("Open", true, Some(Accelerator::new(command, Code::KeyO)));
        let save_item = MenuItem::new("Save", true, Some(Accelerator::new(command, Code::KeyS)));
        let save_as_item = MenuItem::new(
            "Save As…",
            true,
            Some(Accelerator::new(
                Some(Modifiers::SUPER | Modifiers::SHIFT),
                Code::KeyS,
            )),
        );
        let quit_item = PredefinedMenuItem::quit(Some("Quit Amalith"));
        file.append_items(&[&new_item, &open_item, &save_item, &save_as_item])
            .expect("append File menu items");
        let app =
            Submenu::with_items("Amalith", true, &[&quit_item]).expect("build application menu");
        menu.append(&app).expect("append application menu");
        menu.append(&file).expect("append File menu");
        menu.init_for_nsapp();
        NativeMenu {
            _menu: menu,
            new_item,
            open_item,
            save_item,
            save_as_item,
        }
    }

    #[cfg(target_os = "macos")]
    fn process_native_menu_events(&mut self) {
        let Some(native_menu) = &self.native_menu else {
            return;
        };
        let mut actions = Vec::new();
        while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
            if event.id == *native_menu.new_item.id() {
                actions.push(0);
            } else if event.id == *native_menu.open_item.id() {
                actions.push(1);
            } else if event.id == *native_menu.save_item.id() {
                actions.push(2);
            } else if event.id == *native_menu.save_as_item.id() {
                actions.push(3);
            }
        }
        for action in actions {
            match action {
                0 => self.open_new_document(),
                1 => self.open_document_from_disk(),
                2 => self.save_active_document(false),
                3 => self.save_active_document(true),
                _ => unreachable!(),
            }
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
            asset_store: amalith_io::AssetStore::new(),
            file_path: None,
            camera: Camera::default(),
            artboard_tool: ArtboardTool::default(),
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
            direct_selection_tool: DirectSelectionTool::default(),
        });
        self.active = self.open.len() - 1;
        self.creating = None;
    }

    fn open_document_from_disk(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Amalith document", &["amalith"])
            .pick_file()
        else {
            return;
        };
        match amalith_io::load(&path) {
            Ok((document, asset_store)) => {
                self.open.push(DocumentTab {
                    editor: Box::new(Editor::new(document)),
                    asset_store,
                    file_path: Some(path),
                    camera: Camera::default(),
                    artboard_tool: ArtboardTool::default(),
                    rectangle_tool: RectangleTool::default(),
                    ellipse_tool: EllipseTool::default(),
                    selection_tool: SelectionTool::default(),
                    direct_selection_tool: DirectSelectionTool::default(),
                });
                self.active = self.open.len() - 1;
                self.creating = None;
                self.error = None;
            }
            Err(err) => self.error = Some(format!("Could not open document: {err}")),
        }
    }

    fn save_active_document(&mut self, save_as: bool) {
        let Some(index) = (!self.open.is_empty()).then_some(self.active) else {
            return;
        };
        let path = if !save_as {
            self.open[index].file_path.clone()
        } else {
            None
        };
        let path = path.or_else(|| {
            rfd::FileDialog::new()
                .add_filter("Amalith document", &["amalith"])
                .set_file_name("Untitled.amalith")
                .save_file()
        });
        let Some(path) = path else { return };
        let result = {
            let tab = &self.open[index];
            amalith_io::save(tab.editor.document(), &tab.asset_store, &path)
        };
        match result {
            Ok(()) => self.open[index].file_path = Some(path),
            Err(err) => self.error = Some(format!("Could not save document: {err}")),
        }
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
        #[cfg(not(target_os = "macos"))]
        self.app_menu_ui(ctx);
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
                    &mut tab.direct_selection_tool,
                    &mut self.artboards_panel,
                    &mut self.layers_panel,
                    &mut self.fill_stroke_panel,
                    &mut self.error,
                );
            }
        }
    }

    #[cfg(not(target_os = "macos"))]
    fn app_menu_ui(&mut self, ctx: &egui::Context) {
        let mut action = None;
        egui::TopBottomPanel::top("app_menu")
            .exact_height(26.0)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(39, 39, 39)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("New").clicked() {
                            action = Some(0);
                            ui.close();
                        }
                        if ui.button("Open…").clicked() {
                            action = Some(1);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Save").clicked() {
                            action = Some(2);
                            ui.close();
                        }
                        if ui.button("Save As…").clicked() {
                            action = Some(3);
                            ui.close();
                        }
                    });
                });
            });
        match action {
            Some(0) => self.open_new_document(),
            Some(1) => self.open_document_from_disk(),
            Some(2) => self.save_active_document(false),
            Some(3) => self.save_active_document(true),
            None => {}
            _ => unreachable!(),
        }
    }

    /// A dismissible toast for `self.error`, which `canvas_ui` and friends
    /// write into on every failed command — without this, those failures
    /// (an empty-clipboard paste, a stale-id delete, ...) were completely
    /// silent during normal editing: the only place that ever *read*
    /// `self.error` was the New Document dialog.
    fn error_toast_ui(ctx: &egui::Context, error: &mut Option<String>) {
        let Some(message) = error.clone() else {
            return;
        };
        let mut dismiss = false;
        egui::Area::new(egui::Id::new("error_toast"))
            .anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(12.0, -12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(Color32::from_rgb(120, 30, 30))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(Color32::WHITE, message);
                            if ui
                                .add(egui::Button::new("×").frame(false))
                                .on_hover_text("Dismiss")
                                .clicked()
                            {
                                dismiss = true;
                            }
                        });
                    });
            });
        if dismiss {
            *error = None;
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
        direct_selection_tool: &mut DirectSelectionTool,
        artboards_panel: &mut ArtboardsPanelState,
        layers_panel: &mut LayersPanelState,
        fill_stroke_panel: &mut FillStrokePanelState,
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
        Self::options_bar_ui(ctx, editor, selection_tool, fill_stroke_panel, error);
        Self::tools_bar_ui(
            ctx,
            editor,
            artboard_tool,
            rectangle_tool,
            ellipse_tool,
            selection_tool,
            direct_selection_tool,
            fill_stroke_panel,
            error,
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
                let direct_selection_pressed = !navigation_key_down
                    && canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::A) > 0
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
                        direct_selection_tool.set_active(false);
                    } else {
                        activate_tool(
                            ToolKind::Artboard,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            selection_tool,
                            direct_selection_tool,
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
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if direct_selection_pressed {
                    activate_tool(
                        ToolKind::DirectSelection,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if rectangle_pressed {
                    activate_tool(
                        ToolKind::Rectangle,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if ellipse_pressed {
                    activate_tool(
                        ToolKind::Ellipse,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        selection_tool,
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if escape_pressed {
                    artboard_tool.set_active(false, first_artboard);
                    rectangle_tool.set_active(false);
                    ellipse_tool.set_active(false);
                    selection_tool.set_active(false);
                    direct_selection_tool.set_active(false);
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

                if canvas_shortcuts_enabled && !artboard_tool.active && selection_tool.active {
                    // egui-winit intercepts Cmd+C/Cmd+X/Cmd+V at the
                    // platform layer and turns them into `Event::Copy` /
                    // `Event::Cut` / `Event::Paste(String)` — it never
                    // emits a plain `Event::Key{key: C/V, modifiers:
                    // COMMAND}` for those, on any platform (see
                    // `is_copy_command`/`is_paste_command` in egui-winit).
                    // So these must be read as those events, not via
                    // `count_and_consume_key`. `Event::Paste` additionally
                    // only fires when the OS clipboard already holds
                    // non-empty text, which is why `copy_pressed` always
                    // primes it below (real SVG when we have it, a
                    // placeholder otherwise) — that also means the OS
                    // clipboard now holds real, portable SVG a paste into
                    // *any other app* can render, not just an Amalith
                    // marker string.
                    let (copy_pressed, paste_text) = ctx.input_mut(|input| {
                        let mut copy = false;
                        let mut paste = None;
                        input.events.retain(|event| match event {
                            egui::Event::Copy => {
                                copy = true;
                                false
                            }
                            egui::Event::Paste(text) => {
                                paste = Some(text.clone());
                                false
                            }
                            _ => true,
                        });
                        (copy, paste)
                    });
                    if copy_pressed && !selection_tool.selected.is_empty() {
                        let ids = selection_tool.selected_in_paint_order(editor.document());
                        if let Err(err) = editor.copy(&ids) {
                            *error = Some(err.to_string());
                        } else {
                            let svg = amalith_io::export_svg(editor.document(), &ids)
                                .unwrap_or_else(|| "amalith-object-clipboard".to_string());
                            ctx.copy_text(svg);
                        }
                    }

                    let paste_in_front_pressed = ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::F) > 0
                    });
                    let paste_in_back_pressed = ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::B) > 0
                    });

                    // If the OS clipboard's current text parses as SVG,
                    // that always wins over whatever `Editor` already had
                    // internally — it covers both round-tripping our own
                    // last export (identical result either way) and
                    // pasting content actually copied from another app
                    // since. If it *doesn't* parse (arbitrary unrelated
                    // clipboard text), silently keep using whatever's
                    // already in `Editor`'s own clipboard instead of
                    // erroring the paste over it.
                    if let Some(text) = &paste_text {
                        let _ = editor.copy_from_svg(text);
                    }

                    // `Event::Paste` doesn't carry whether Shift was also
                    // held (egui-winit's `is_paste_command` ignores it), so
                    // Plain Paste vs. Paste in Place is disambiguated from
                    // the live modifier state instead. Plain Paste lands
                    // the selection's bounds-center on the visible view's
                    // center (Illustrator's "you copied it to put it
                    // somewhere new" behavior); the other three keep exact
                    // X/Y (zero delta).
                    let paste_fired = paste_text.is_some();
                    let paste = if paste_fired && ctx.input(|i| i.modifiers.shift) {
                        Some((amalith_core::Vec2::ZERO, PasteStack::Top))
                    } else if paste_fired {
                        let delta = editor
                            .clipboard_bounds()
                            .map(|bounds| visible_document.center() - bounds.center())
                            .unwrap_or(amalith_core::Vec2::ZERO);
                        Some((delta, PasteStack::Top))
                    } else if paste_in_front_pressed {
                        Some((amalith_core::Vec2::ZERO, PasteStack::InFront))
                    } else if paste_in_back_pressed {
                        Some((amalith_core::Vec2::ZERO, PasteStack::Behind))
                    } else {
                        None
                    };
                    if let Some((delta, stack)) = paste {
                        match editor.paste(delta, stack) {
                            Ok(new_ids) => {
                                selection_tool.cancel_drag();
                                selection_tool.selected.clear();
                                selection_tool.selected.extend(new_ids);
                            }
                            Err(err) => *error = Some(err.to_string()),
                        }
                    }
                }

                if canvas_shortcuts_enabled
                    && !artboard_tool.active
                    && selection_tool.active
                    && !selection_tool.selected.is_empty()
                {
                    // Cmd+Shift+G (Ungroup) must be checked *before* plain
                    // Cmd+G: `count_and_consume_key` matches modifiers
                    // logically, so a bare `COMMAND` pattern also matches
                    // while an extra Shift is held (same gotcha as
                    // Paste/Paste-in-Place above).
                    let ungroup_pressed = ctx.input_mut(|input| {
                        input.count_and_consume_key(
                            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                            egui::Key::G,
                        ) > 0
                    });
                    let group_pressed = !ungroup_pressed
                        && ctx.input_mut(|input| {
                            input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::G) > 0
                        });
                    if group_pressed {
                        let ids = selection_tool.selected_in_paint_order(editor.document());
                        let name = next_group_name(editor);
                        match editor.execute(Command::Group {
                            ids,
                            name: Some(name),
                        }) {
                            Ok(CommandOutcome::Object(group_id)) => {
                                selection_tool.cancel_drag();
                                selection_tool.selected.clear();
                                selection_tool.selected.insert(group_id);
                            }
                            Ok(_) => {}
                            Err(err) => *error = Some(err.to_string()),
                        }
                    }
                    if ungroup_pressed {
                        // Only the groups actually in the selection get
                        // dissolved; a non-group in the same selection is
                        // simply left alone rather than failing the whole
                        // command.
                        let groups: Vec<_> = selection_tool
                            .selected_in_paint_order(editor.document())
                            .into_iter()
                            .filter(|&id| {
                                editor
                                    .document()
                                    .object(id)
                                    .is_some_and(|object| object.is_group())
                            })
                            .collect();
                        if !groups.is_empty() {
                            match editor.ungroup(&groups) {
                                Ok(freed_ids) => {
                                    selection_tool.cancel_drag();
                                    selection_tool.selected.clear();
                                    selection_tool.selected.extend(freed_ids);
                                }
                                Err(err) => *error = Some(err.to_string()),
                            }
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
                    direct_selection_tool.retain_existing(editor.document());
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
                        direct_selection_tool.cancel_drag();
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
                        direct_selection_tool.cancel_drag();
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
                            direct_selection_tool.cancel_drag();
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
                        } else if direct_selection_tool.active {
                            if let Some(pointer) = pointer_position {
                                let pointer_doc = camera.screen_to_document(pointer, available);
                                let point = Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                if primary_pressed
                                    && available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    direct_selection_tool.press(
                                        editor.document(),
                                        point,
                                        6.0 / camera.scale as f64,
                                        shift_down,
                                    );
                                } else if primary_down {
                                    direct_selection_tool.drag(point);
                                }
                            }
                            if primary_released {
                                if let Err(err) = direct_selection_tool.finish_drag(editor) {
                                    *error = Some(err.to_string());
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
                    let layer_paths =
                        paint_order_paths(editor.document(), ObjectParent::Layer(layer.id));
                    for id in layer_paths {
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
                                let fill = object
                                    .appearance
                                    .fill
                                    .color()
                                    .map(|color| {
                                        color32_with_opacity(color, object.appearance.opacity)
                                    })
                                    .unwrap_or(Color32::TRANSPARENT);
                                let stroke = match object.appearance.stroke.color() {
                                    Some(color) => Stroke::new(
                                        (object.appearance.stroke_width * camera.scale as f64)
                                            as f32,
                                        color32_with_opacity(color, object.appearance.opacity),
                                    ),
                                    None => Stroke::NONE,
                                };
                                // All subpaths of this Path are filled as
                                // one nonzero-winding tessellation (so holes
                                // and self-intersections render right); the
                                // stroke stays a per-subpath closed polyline.
                                // Deformed live by any of this path's
                                // anchors currently being node-dragged, so
                                // the shape reacts in real time instead of
                                // only snapping to its new form on release.
                                let preview_geometry =
                                    direct_selection_tool.preview_geometry(id, &path.geometry);
                                let screen_subpaths: Vec<Vec<Pos2>> =
                                    amalith_core::geom::flattened_points(&preview_geometry, 0.5)
                                        .into_iter()
                                        .map(|local_points| {
                                            local_points
                                                .into_iter()
                                                .map(|point| {
                                                    let point = transform * point + move_delta;
                                                    camera.document_to_screen(
                                                        Pos2::new(point.x as f32, point.y as f32),
                                                        available,
                                                    )
                                                })
                                                .collect()
                                        })
                                        .collect();
                                if fill.a() > 0 {
                                    if let Some(mesh) = fill_mesh::fill_mesh(&screen_subpaths, fill)
                                    {
                                        painter.add(egui::Shape::mesh(mesh));
                                    }
                                }
                                if stroke != Stroke::NONE {
                                    for points in &screen_subpaths {
                                        if points.len() >= 2 {
                                            painter.add(egui::Shape::closed_line(
                                                points.clone(),
                                                stroke,
                                            ));
                                        }
                                    }
                                }
                                if direct_selection_tool.active {
                                    for index in amalith_core::geom::anchor_indices(&path.geometry)
                                    {
                                        let Some(position) = direct_selection_tool
                                            .display_anchor_position(editor.document(), id, index)
                                        else {
                                            continue;
                                        };
                                        let screen = camera.document_to_screen(
                                            Pos2::new(position.x as f32, position.y as f32),
                                            available,
                                        );
                                        let marker =
                                            EguiRect::from_center_size(screen, Vec2::splat(7.0));
                                        let selected =
                                            direct_selection_tool.selected.contains(&(id, index));
                                        painter.rect_filled(
                                            marker,
                                            0.0,
                                            if selected {
                                                Color32::from_rgb(59, 155, 255)
                                            } else {
                                                Color32::WHITE
                                            },
                                        );
                                        painter.rect_stroke(
                                            marker,
                                            0.0,
                                            Stroke::new(1.25_f32, Color32::from_rgb(59, 155, 255)),
                                            egui::StrokeKind::Outside,
                                        );
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
                        // Live duplicate-drag ghost: every selected object
                        // gets one, not just a lone selection, so a
                        // multi-select alt-drag previews the whole copy as
                        // it moves instead of only appearing on release.
                        // Rendered with the object's *real* fill/stroke —
                        // not a flat gray placeholder — so a copy looks
                        // live the whole drag instead of its appearance
                        // disappearing and popping back on release.
                        if let (Some(transform), ObjectKind::Path(path)) = (
                            selection_tool.duplicate_preview_transform(editor.document(), id),
                            &object.kind,
                        ) {
                            let ghost_fill = object
                                .appearance
                                .fill
                                .color()
                                .map(|color| color32_with_opacity(color, object.appearance.opacity))
                                .unwrap_or(Color32::TRANSPARENT);
                            let ghost_stroke = match object.appearance.stroke.color() {
                                Some(color) => Stroke::new(
                                    (object.appearance.stroke_width * camera.scale as f64) as f32,
                                    color32_with_opacity(color, object.appearance.opacity),
                                ),
                                None => Stroke::NONE,
                            };
                            let ghost_subpaths: Vec<Vec<Pos2>> = path
                                .flattened_points(0.5)
                                .into_iter()
                                .map(|local_points| {
                                    local_points
                                        .into_iter()
                                        .map(|point| {
                                            let point = transform * point;
                                            camera.document_to_screen(
                                                Pos2::new(point.x as f32, point.y as f32),
                                                available,
                                            )
                                        })
                                        .collect()
                                })
                                .collect();
                            if ghost_fill.a() > 0 {
                                if let Some(mesh) =
                                    fill_mesh::fill_mesh(&ghost_subpaths, ghost_fill)
                                {
                                    painter.add(egui::Shape::mesh(mesh));
                                }
                            }
                            if ghost_stroke != Stroke::NONE {
                                for points in &ghost_subpaths {
                                    if points.len() >= 2 {
                                        painter.add(egui::Shape::closed_line(
                                            points.clone(),
                                            ghost_stroke,
                                        ));
                                    }
                                }
                            }
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
                if let Some(marquee) = direct_selection_tool.marquee_rect() {
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
                    let preview_subpaths: Vec<Vec<Pos2>> = path
                        .flattened_points(0.5)
                        .into_iter()
                        .map(|local_points| {
                            local_points
                                .into_iter()
                                .map(|point| {
                                    camera.document_to_screen(
                                        Pos2::new(point.x as f32, point.y as f32),
                                        available,
                                    )
                                })
                                .collect()
                        })
                        .collect();
                    if let Some(mesh) =
                        fill_mesh::fill_mesh(&preview_subpaths, Color32::from_white_alpha(80))
                    {
                        painter.add(egui::Shape::mesh(mesh));
                    }
                    let preview_stroke = Stroke::new(1.0_f32, Color32::from_gray(35));
                    for points in &preview_subpaths {
                        if points.len() >= 2 {
                            painter.add(egui::Shape::closed_line(points.clone(), preview_stroke));
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

    /// A compact, Illustrator-style control bar for the currently selected
    /// objects. It intentionally favors the same first-leaf appearance and
    /// one-command-to-all-targets behavior as the existing swatch widget.
    fn options_bar_ui(
        ctx: &egui::Context,
        editor: &mut Editor,
        selection_tool: &SelectionTool,
        state: &mut FillStrokePanelState,
        error: &mut Option<String>,
    ) {
        let selected = selection_tool.selected_in_paint_order(editor.document());
        let representative = representative_appearance(editor.document(), &selected);
        let targets = selected_path_targets(editor.document(), &selected);
        let has_targets = !targets.is_empty();
        let label = selection_label(editor.document(), &selected);

        egui::TopBottomPanel::top("options_bar")
            .exact_height(36.0)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(47, 47, 47)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(6.0, 0.0);
                ui.horizontal_centered(|ui| {
                    ui.label(egui::RichText::new(label).color(Color32::from_gray(220)));
                    ui.separator();

                    ui.label("Fill:");
                    if swatch_with_dropdown(
                        ui,
                        ui.id().with("options_fill_swatch"),
                        PaintSlot::Fill,
                        representative.fill.color(),
                        state.active == PaintSlot::Fill,
                    ) && has_targets
                    {
                        select_or_open_swatch(state, PaintSlot::Fill, representative.fill);
                    }

                    ui.label("Stroke:");
                    if swatch_with_dropdown(
                        ui,
                        ui.id().with("options_stroke_swatch"),
                        PaintSlot::Stroke,
                        representative.stroke.color(),
                        state.active == PaintSlot::Stroke,
                    ) && has_targets
                    {
                        select_or_open_swatch(state, PaintSlot::Stroke, representative.stroke);
                    }

                    ui.separator();
                    ui.label("Stroke:");
                    let mut width = representative.stroke_width;
                    if ui
                        .add(
                            egui::DragValue::new(&mut width)
                                .speed(0.25)
                                .range(0.0..=10_000.0)
                                .suffix(" px"),
                        )
                        .changed()
                        && has_targets
                    {
                        if let Err(err) = editor.execute(Command::SetStrokeWidth {
                            objects: targets.clone(),
                            width,
                        }) {
                            *error = Some(err.to_string());
                        }
                    }
                    ui.label("▾");

                    ui.separator();
                    ui.label("Opacity:");
                    let mut opacity_percent = (representative.opacity * 100.0).clamp(0.0, 100.0);
                    if ui
                        .add(
                            egui::DragValue::new(&mut opacity_percent)
                                .speed(1.0)
                                .range(0.0..=100.0)
                                .suffix("%"),
                        )
                        .changed()
                        && has_targets
                    {
                        if let Err(err) = editor.execute(Command::SetOpacity {
                            objects: targets,
                            opacity: (opacity_percent / 100.0).clamp(0.0, 1.0),
                        }) {
                            *error = Some(err.to_string());
                        }
                    }
                    ui.label("›");
                });
            });
    }

    fn tools_bar_ui(
        ctx: &egui::Context,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        rectangle_tool: &mut RectangleTool,
        ellipse_tool: &mut EllipseTool,
        selection_tool: &mut SelectionTool,
        direct_selection_tool: &mut DirectSelectionTool,
        fill_stroke_panel: &mut FillStrokePanelState,
        error: &mut Option<String>,
    ) {
        let first_artboard = editor.document().artboards().first().map(|board| board.id);
        // Two tool columns, Illustrator-style, rather than one — the panel
        // width below is sized for exactly that (2 * 32px buttons + the
        // gaps/margins around them), not just "wide enough for whatever's
        // in it today".
        egui::SidePanel::left("tools_bar")
            .exact_width(76.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(Color32::from_rgb(42, 42, 42)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(8.0);
                            let button = |ui: &mut egui::Ui,
                                          label: &str,
                                          tooltip: &str,
                                          active: bool|
                             -> bool {
                                ui.add_sized(
                                    [32.0, 32.0],
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

                            egui::Grid::new("tool_grid")
                                .num_columns(2)
                                .min_col_width(32.0)
                                .spacing([4.0, 4.0])
                                .show(ui, |ui| {
                                    if button(ui, "V", "Selection (V)", selection_tool.active) {
                                        activate_tool(
                                            ToolKind::Selection,
                                            artboard_tool,
                                            rectangle_tool,
                                            ellipse_tool,
                                            selection_tool,
                                            direct_selection_tool,
                                            first_artboard,
                                        );
                                    }
                                    if button(
                                        ui,
                                        "A",
                                        "Direct Selection (A)",
                                        direct_selection_tool.active,
                                    ) {
                                        activate_tool(
                                            ToolKind::DirectSelection,
                                            artboard_tool,
                                            rectangle_tool,
                                            ellipse_tool,
                                            selection_tool,
                                            direct_selection_tool,
                                            first_artboard,
                                        );
                                    }
                                    ui.end_row();
                                    if button(ui, "M", "Rectangle (M)", rectangle_tool.active) {
                                        activate_tool(
                                            ToolKind::Rectangle,
                                            artboard_tool,
                                            rectangle_tool,
                                            ellipse_tool,
                                            selection_tool,
                                            direct_selection_tool,
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
                                            direct_selection_tool,
                                            first_artboard,
                                        );
                                    }
                                    ui.end_row();
                                    if button(ui, "Art", "Artboard (Shift+O)", artboard_tool.active)
                                    {
                                        activate_tool(
                                            ToolKind::Artboard,
                                            artboard_tool,
                                            rectangle_tool,
                                            ellipse_tool,
                                            selection_tool,
                                            direct_selection_tool,
                                            first_artboard,
                                        );
                                    }
                                    ui.end_row();
                                });
                            ui.add_space(14.0);
                            Self::fill_stroke_widget_ui(
                                ui,
                                editor,
                                selection_tool,
                                fill_stroke_panel,
                                error,
                            );
                        });
                    });
            });
    }

    /// Illustrator's fill/stroke swatch widget: two overlapping color
    /// squares (fill in front, stroke behind), a swap button, a
    /// reset-to-default button, and quick-pick presets (black / white /
    /// none). Reflects (and edits) the fill/stroke of the *first* selected
    /// object in paint order when the selection is non-uniform — same
    /// simplification `Command::SetFill`/`SetStroke` already make by
    /// applying one paint to every selected object at once. With nothing
    /// selected, it shows the appearance new objects get but can't edit
    /// anything (there's nothing to apply a change to).
    fn fill_stroke_widget_ui(
        ui: &mut egui::Ui,
        editor: &mut Editor,
        selection_tool: &mut SelectionTool,
        state: &mut FillStrokePanelState,
        error: &mut Option<String>,
    ) {
        let selected = selection_tool.selected_in_paint_order(editor.document());
        let representative = representative_appearance(editor.document(), &selected);
        let has_selection = !selected_path_targets(editor.document(), &selected).is_empty();

        let apply = |editor: &mut Editor,
                     error: &mut Option<String>,
                     slot: PaintSlot,
                     paint: amalith_core::Paint| {
            if let Err(err) = apply_paint_to_selection(editor, &selected, slot, paint) {
                *error = Some(err.to_string());
            }
        };

        let swatch_size = 24.0;
        let offset = 10.0;
        let (rect, _response) = ui.allocate_exact_size(
            Vec2::new(swatch_size + offset, swatch_size + offset),
            egui::Sense::hover(),
        );
        let front_at = rect.min;
        let back_at = rect.min + Vec2::new(offset, offset);
        let fill_rect = EguiRect::from_min_size(
            if state.active == PaintSlot::Fill {
                front_at
            } else {
                back_at
            },
            Vec2::splat(swatch_size),
        );
        let stroke_rect = EguiRect::from_min_size(
            if state.active == PaintSlot::Stroke {
                front_at
            } else {
                back_at
            },
            Vec2::splat(swatch_size),
        );

        // Paint and interact with the back slot first, then the active slot,
        // so the front swatch visibly and interactively owns the overlap.
        let (back_slot, back_rect, back_paint, front_slot, front_rect, front_paint) =
            if state.active == PaintSlot::Fill {
                (
                    PaintSlot::Stroke,
                    stroke_rect,
                    representative.stroke,
                    PaintSlot::Fill,
                    fill_rect,
                    representative.fill,
                )
            } else {
                (
                    PaintSlot::Fill,
                    fill_rect,
                    representative.fill,
                    PaintSlot::Stroke,
                    stroke_rect,
                    representative.stroke,
                )
            };
        paint_slot_swatch(
            ui.painter(),
            back_rect,
            back_slot,
            back_paint.color(),
            false,
        );
        let back_response = ui.interact(
            back_rect,
            ui.id().with("inactive_swatch"),
            egui::Sense::click(),
        );
        paint_slot_swatch(
            ui.painter(),
            front_rect,
            front_slot,
            front_paint.color(),
            true,
        );
        let front_response = ui.interact(
            front_rect,
            ui.id().with("active_swatch"),
            egui::Sense::click(),
        );
        if has_selection {
            if front_response.clicked() {
                select_or_open_swatch(state, front_slot, front_paint);
            } else if back_response.clicked() {
                select_or_open_swatch(state, back_slot, back_paint);
            }
        }

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Swap fill and stroke on the current selection.
            if ui
                .add(egui::Button::new("\u{21C4}").small().frame(false))
                .on_hover_text("Swap fill and stroke")
                .clicked()
                && has_selection
            {
                apply(editor, error, PaintSlot::Fill, representative.stroke);
                apply(editor, error, PaintSlot::Stroke, representative.fill);
            }
            // Reset to the default appearance (light fill, dark stroke).
            if ui
                .add(egui::Button::new("\u{21BA}").small().frame(false))
                .on_hover_text("Default fill and stroke")
                .clicked()
                && has_selection
            {
                let default = amalith_core::Appearance::default();
                apply(editor, error, PaintSlot::Fill, default.fill);
                apply(editor, error, PaintSlot::Stroke, default.stroke);
            }
        });

        // Color / Gradient / None mode icons for the active slot, drawn as
        // small (~16px) square swatch icons — Illustrator's compact row, not
        // text-label buttons. Anything wider here grows the panel's layout
        // past the 76px column that its frame is clipped to and leaves an
        // unpainted black gap between the sidebar and the canvas.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let active_paint = paint_for_slot(representative, state.active);
            let icon = Vec2::splat(16.0);

            // Solid color: a plain filled swatch, highlighted when the active
            // slot is currently a solid paint.
            let (solid_rect, solid_response) = ui.allocate_exact_size(icon, egui::Sense::click());
            paint_slot_swatch(
                ui.painter(),
                solid_rect,
                PaintSlot::Fill,
                Some(
                    active_paint
                        .color()
                        .unwrap_or(amalith_core::Color::rgb(0.0, 0.0, 0.0)),
                ),
                matches!(active_paint, amalith_core::Paint::Solid(_)),
            );
            if solid_response.on_hover_text("Solid color").clicked()
                && has_selection
                && matches!(active_paint, amalith_core::Paint::None)
            {
                apply(
                    editor,
                    error,
                    state.active,
                    amalith_core::Paint::Solid(amalith_core::Color::rgb(0.0, 0.0, 0.0)),
                );
            }

            // Gradient: inert until gradients exist, but shown so the row
            // matches the reference layout.
            let (gradient_rect, gradient_response) =
                ui.allocate_exact_size(icon, egui::Sense::hover());
            paint_gradient_icon(ui.painter(), gradient_rect);
            gradient_response.on_hover_text("Gradients aren't implemented yet");

            // No paint: reuse the swatch's own white-square / red-slash
            // rendering, highlighted when the active slot has no paint.
            let (none_rect, none_response) = ui.allocate_exact_size(icon, egui::Sense::click());
            paint_slot_swatch(
                ui.painter(),
                none_rect,
                state.active,
                None,
                matches!(active_paint, amalith_core::Paint::None),
            );
            if none_response.on_hover_text("No paint").clicked() && has_selection {
                apply(editor, error, state.active, amalith_core::Paint::None);
            }
        });

        if let Some((slot, mut working)) = state.open {
            let mut still_open = true;
            let mut action = ColorPickerAction::None;
            egui::Window::new("Color Picker")
                .id(egui::Id::new("fill_stroke_picker"))
                .resizable(false)
                .collapsible(false)
                .open(&mut still_open)
                .show(ui.ctx(), |ui| {
                    action = color_picker_dialog_ui(ui, &mut working);
                });
            match action {
                ColorPickerAction::None => {}
                ColorPickerAction::Ok => {
                    if has_selection {
                        apply(
                            editor,
                            error,
                            slot,
                            amalith_core::Paint::Solid(color_from_color32(working.into())),
                        );
                    }
                    still_open = false;
                }
                ColorPickerAction::Cancel => still_open = false,
                ColorPickerAction::SetNone => {
                    if has_selection {
                        apply(editor, error, slot, amalith_core::Paint::None);
                    }
                    still_open = false;
                }
            }
            state.open = if still_open {
                Some((slot, working))
            } else {
                None
            };
        }
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
                    let renaming_layer =
                        state.renaming == Some(LayersRenameTarget::Layer(layer.id));
                    egui::Frame::NONE
                        .fill(Color32::TRANSPARENT)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let content_width = (ui.available_width() - 20.0).max(0.0);
                                if renaming_layer {
                                    let response = ui.add_sized(
                                        [content_width, 20.0],
                                        egui::TextEdit::singleline(&mut state.rename_text)
                                            .id_salt(("layer_rename", layer.id)),
                                    );
                                    if state.focus_rename {
                                        response.request_focus();
                                        state.focus_rename = false;
                                    }
                                    let enter = response.has_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter));
                                    if enter || response.lost_focus() {
                                        let name = state.rename_text.trim();
                                        if !name.is_empty() && name != layer.name {
                                            if let Err(err) = editor.execute(Command::RenameLayer {
                                                id: layer.id,
                                                name: name.to_owned(),
                                            }) {
                                                *error = Some(err.to_string());
                                            }
                                        }
                                        state.renaming = None;
                                    }
                                } else {
                                    let response = ui.add_sized(
                                        [content_width, 20.0],
                                        egui::Label::new(
                                            egui::RichText::new(&layer.name)
                                                .strong()
                                                .color(Color32::from_gray(225)),
                                        )
                                        .sense(egui::Sense::click()),
                                    );
                                    if response.double_clicked() {
                                        state.renaming = Some(LayersRenameTarget::Layer(layer.id));
                                        state.rename_text = layer.name.clone();
                                        state.focus_rename = true;
                                    }
                                    if response.clicked() {
                                        state.selected_layer = Some(layer.id);
                                    }
                                }

                                // The active layer's indicator: a small
                                // filled square at the row's right edge,
                                // instead of highlighting the whole row.
                                let (indicator_rect, _) = ui.allocate_exact_size(
                                    Vec2::new(10.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                if layer_selected {
                                    ui.painter().rect_filled(
                                        indicator_rect,
                                        1.0,
                                        Color32::from_rgb(66, 133, 244),
                                    );
                                }
                            });
                        });

                    for &id in layer.children.iter().rev() {
                        Self::object_row_ui(
                            ui,
                            editor,
                            selection_tool,
                            state,
                            error,
                            layer.id,
                            id,
                            1,
                        );
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

    /// One row in the Layers panel tree: a plain object, or a group with
    /// an expand/collapse arrow that recurses into its own children
    /// (indented one level further) when expanded. Both plain objects and
    /// groups share the same click-to-select / double-click-to-rename
    /// behavior as the top-level layer row above.
    fn object_row_ui(
        ui: &mut egui::Ui,
        editor: &mut Editor,
        selection_tool: &mut SelectionTool,
        state: &mut LayersPanelState,
        error: &mut Option<String>,
        layer_id: LayerId,
        id: ObjectId,
        depth: u8,
    ) {
        let Some(object) = editor.document().object(id) else {
            return;
        };
        let name = object.name.clone();
        let children: Vec<ObjectId> = match &object.kind {
            ObjectKind::Group(group) => group.children.clone(),
            _ => Vec::new(),
        };
        let is_group = matches!(object.kind, ObjectKind::Group(_));
        let selected = selection_tool.selected.contains(&id);
        let renaming = state.renaming == Some(LayersRenameTarget::Object(id));

        egui::Frame::NONE
            .fill(if selected {
                Color32::from_rgb(62, 82, 103)
            } else {
                Color32::TRANSPARENT
            })
            .inner_margin(egui::Margin {
                left: (depth as i8).saturating_mul(16).saturating_add(12),
                right: 10,
                top: 4,
                bottom: 4,
            })
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    if !children.is_empty() {
                        let collapsed = state.collapsed_groups.contains(&id);
                        let arrow = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
                        let arrow_response = ui.add(
                            egui::Label::new(
                                egui::RichText::new(arrow).color(Color32::from_gray(160)),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if arrow_response.clicked() {
                            if collapsed {
                                state.collapsed_groups.remove(&id);
                            } else {
                                state.collapsed_groups.insert(id);
                            }
                        }
                    } else {
                        ui.add_space(14.0);
                    }

                    if renaming {
                        let response = ui.add_sized(
                            [ui.available_width(), 20.0],
                            egui::TextEdit::singleline(&mut state.rename_text)
                                .id_salt(("object_rename", id)),
                        );
                        if state.focus_rename {
                            response.request_focus();
                            state.focus_rename = false;
                        }
                        let enter = response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter));
                        if enter || response.lost_focus() {
                            let trimmed = state.rename_text.trim();
                            let new_name = (!trimmed.is_empty()).then(|| trimmed.to_owned());
                            if new_name != name {
                                if let Err(err) =
                                    editor.execute(Command::RenameObject { id, name: new_name })
                                {
                                    *error = Some(err.to_string());
                                }
                            }
                            state.renaming = None;
                        }
                    } else {
                        let fallback = if is_group { "Group" } else { "Object" };
                        let response = ui.add(
                            egui::Label::new(
                                egui::RichText::new(name.as_deref().unwrap_or(fallback))
                                    .color(Color32::from_gray(210)),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if response.double_clicked() {
                            state.renaming = Some(LayersRenameTarget::Object(id));
                            state.rename_text = name.clone().unwrap_or_default();
                            state.focus_rename = true;
                        }
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
                            state.selected_layer = Some(layer_id);
                        }
                    }
                });
            });

        if is_group && !children.is_empty() && !state.collapsed_groups.contains(&id) {
            for &child_id in children.iter().rev() {
                Self::object_row_ui(
                    ui,
                    editor,
                    selection_tool,
                    state,
                    error,
                    layer_id,
                    child_id,
                    depth + 1,
                );
            }
        }
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
        #[cfg(target_os = "macos")]
        self.process_native_menu_events();
        // File commands belong to the document workspace; never let a
        // focused New Document/editor field consume them.
        let shortcuts_enabled = self.creating.is_none() && !ctx.wants_keyboard_input();
        let save_as_pressed = shortcuts_enabled
            && ctx.input_mut(|input| {
                input.count_and_consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::S,
                ) > 0
            });
        let save_pressed = shortcuts_enabled
            && !save_as_pressed
            && ctx.input_mut(|input| {
                input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::S) > 0
            });
        let open_pressed = shortcuts_enabled
            && ctx.input_mut(|input| {
                input.count_and_consume_key(egui::Modifiers::COMMAND, egui::Key::O) > 0
            });
        if save_as_pressed {
            self.save_active_document(true);
        } else if save_pressed {
            self.save_active_document(false);
        } else if open_pressed {
            self.open_document_from_disk();
        }
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
            Self::error_toast_ui(ctx, &mut self.error);
        }
    }
}

fn paint_slot_swatch(
    painter: &egui::Painter,
    rect: EguiRect,
    slot: PaintSlot,
    color: Option<amalith_core::Color>,
    active: bool,
) {
    match color {
        Some(color) => {
            let color = color32_from(color);
            match slot {
                PaintSlot::Fill => {
                    painter.rect_filled(rect, 2.0, color);
                }
                PaintSlot::Stroke => {
                    painter.rect_filled(rect, 2.0, Color32::from_rgb(47, 47, 47));
                    painter.rect_stroke(
                        rect.shrink(1.5),
                        1.0,
                        Stroke::new(3.0_f32, color),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        None => {
            painter.rect_filled(rect, 2.0, Color32::WHITE);
            painter.line_segment(
                [rect.left_top(), rect.right_bottom()],
                Stroke::new(1.5_f32, Color32::from_rgb(214, 48, 48)),
            );
        }
    }
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );
    if active {
        painter.rect_stroke(
            rect.expand(1.0),
            3.0,
            Stroke::new(2.0_f32, Color32::from_gray(235)),
            egui::StrokeKind::Outside,
        );
    }
}

/// A compact swatch plus the dropdown affordance used by the options bar.
/// The swatch deliberately renders the paint's native alpha, never an
/// object's compositing opacity: it represents the paint color itself.
fn swatch_with_dropdown(
    ui: &mut egui::Ui,
    id: egui::Id,
    slot: PaintSlot,
    color: Option<amalith_core::Color>,
    active: bool,
) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
        paint_slot_swatch(ui.painter(), rect, slot, color, active);
        clicked |= response.clicked();
        clicked |= ui
            .push_id(id, |ui| ui.add(egui::Button::new("▾").small().frame(false)))
            .inner
            .clicked();
    });
    clicked
}

fn select_or_open_swatch(
    state: &mut FillStrokePanelState,
    slot: PaintSlot,
    paint: amalith_core::Paint,
) {
    if state.active == slot {
        state.open = Some((slot, hsva_from_paint(paint)));
    } else {
        state.active = slot;
    }
}

fn paint_for_slot(appearance: amalith_core::Appearance, slot: PaintSlot) -> amalith_core::Paint {
    match slot {
        PaintSlot::Fill => appearance.fill,
        PaintSlot::Stroke => appearance.stroke,
    }
}

/// Draws a small diagonal light→dark gradient swatch icon (two triangles
/// plus a border) into `rect`, matching `paint_slot_swatch`'s border style.
/// Used for the inert "gradient" paint mode in the fill/stroke widget.
fn paint_gradient_icon(painter: &egui::Painter, rect: EguiRect) {
    painter.rect_filled(rect, 2.0, Color32::from_gray(58));
    painter.add(egui::Shape::convex_polygon(
        vec![rect.left_top(), rect.right_top(), rect.left_bottom()],
        Color32::from_gray(212),
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![rect.right_top(), rect.right_bottom(), rect.left_bottom()],
        Color32::from_gray(70),
        Stroke::NONE,
    ));
    painter.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );
}

fn color32_from(color: amalith_core::Color) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn color32_with_opacity(color: amalith_core::Color, opacity: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (color.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (color.a.clamp(0.0, 1.0) * opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn color_from_color32(color: Color32) -> amalith_core::Color {
    amalith_core::Color::rgba(
        color.r() as f32 / 255.0,
        color.g() as f32 / 255.0,
        color.b() as f32 / 255.0,
        color.a() as f32 / 255.0,
    )
}

fn hsva_from_paint(paint: amalith_core::Paint) -> egui::ecolor::Hsva {
    let color32 = paint.color().map(color32_from).unwrap_or(Color32::WHITE);
    egui::ecolor::Hsva::from(color32)
}

/// What the standalone Color Picker dialog (`color_picker_dialog_ui`) did
/// this frame — `None` on every frame except the one where a button was
/// actually clicked.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ColorPickerAction {
    None,
    Ok,
    Cancel,
    SetNone,
}

/// A from-scratch HSB color picker matching the classic macOS/Illustrator
/// "Color Picker" dialog: a saturation/brightness square, a hue strip, a
/// preview swatch, OK/Cancel/None buttons, and H/S/B, R/G/B, and hex
/// fields, all kept in sync through `hsva` (the working color the caller
/// owns — see `FillStrokePanelState::open`). Deliberately omits the
/// original's eyedropper (needs OS-level screen color picking, which
/// nothing here has access to), CMYK fields (Amalith's `Color` has no
/// CMYK model to convert through), "Only Web Colors", and "Color
/// Swatches" (would need real integration with the swatches panel) — pure
/// decoration with nothing behind it isn't worth adding.
fn color_picker_dialog_ui(ui: &mut egui::Ui, hsva: &mut egui::ecolor::Hsva) -> ColorPickerAction {
    let mut action = ColorPickerAction::None;

    ui.label("Select Color:");
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        sv_square_ui(ui, hsva, 160.0);
        ui.add_space(6.0);
        hue_strip_ui(ui, hsva, 160.0);
        ui.add_space(12.0);

        ui.vertical(|ui| {
            let (preview_rect, _) =
                ui.allocate_exact_size(Vec2::new(70.0, 70.0), egui::Sense::hover());
            ui.painter()
                .rect_filled(preview_rect, 2.0, Color32::from(*hsva));
            ui.painter().rect_stroke(
                preview_rect,
                2.0,
                Stroke::new(1.0_f32, Color32::from_gray(90)),
                egui::StrokeKind::Outside,
            );
            ui.add_space(8.0);
            if ui.button("OK").clicked() {
                action = ColorPickerAction::Ok;
            }
            if ui.button("Cancel").clicked() {
                action = ColorPickerAction::Cancel;
            }
            if ui.button("None").clicked() {
                action = ColorPickerAction::SetNone;
            }
        });
    });

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(6.0);

    let mut h_deg = hsva.h * 360.0;
    let mut s_pct = hsva.s * 100.0;
    let mut v_pct = hsva.v * 100.0;
    ui.horizontal(|ui| {
        ui.label("H:");
        if ui
            .add(
                egui::DragValue::new(&mut h_deg)
                    .range(0.0..=360.0)
                    .suffix("\u{b0}"),
            )
            .changed()
        {
            hsva.h = (h_deg / 360.0).rem_euclid(1.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label("S:");
        if ui
            .add(
                egui::DragValue::new(&mut s_pct)
                    .range(0.0..=100.0)
                    .suffix("%"),
            )
            .changed()
        {
            hsva.s = (s_pct / 100.0).clamp(0.0, 1.0);
        }
    });
    ui.horizontal(|ui| {
        ui.label("B:");
        if ui
            .add(
                egui::DragValue::new(&mut v_pct)
                    .range(0.0..=100.0)
                    .suffix("%"),
            )
            .changed()
        {
            hsva.v = (v_pct / 100.0).clamp(0.0, 1.0);
        }
    });

    ui.add_space(6.0);

    let color32 = Color32::from(*hsva);
    let mut r = color32.r();
    let mut g = color32.g();
    let mut b = color32.b();
    let mut rgb_changed = false;
    ui.horizontal(|ui| {
        ui.label("R:");
        rgb_changed |= ui
            .add(egui::DragValue::new(&mut r).range(0..=255))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("G:");
        rgb_changed |= ui
            .add(egui::DragValue::new(&mut g).range(0..=255))
            .changed();
    });
    ui.horizontal(|ui| {
        ui.label("B:");
        rgb_changed |= ui
            .add(egui::DragValue::new(&mut b).range(0..=255))
            .changed();
    });
    if rgb_changed {
        *hsva = egui::ecolor::Hsva::from(Color32::from_rgb(r, g, b));
    }

    ui.add_space(6.0);

    let mut hex = format!("{:02X}{:02X}{:02X}", color32.r(), color32.g(), color32.b());
    ui.horizontal(|ui| {
        ui.label("#");
        if ui
            .add(egui::TextEdit::singleline(&mut hex).desired_width(70.0))
            .changed()
        {
            if let Some(parsed) = parse_hex6(&hex) {
                *hsva = egui::ecolor::Hsva::from(parsed);
            }
        }
    });

    action
}

/// The saturation/brightness square: horizontal = saturation (0 at left),
/// vertical = brightness (0 at bottom), at the picker's current hue.
/// Painted as a grid of flat-filled cells rather than a single gradient
/// mesh — simple, and `Hsva -> Color32` (via `egui::ecolor`) is already
/// exact, so there's no interpolation error to worry about, just
/// resolution (24 cells is smooth enough at this widget's size).
fn sv_square_ui(ui: &mut egui::Ui, hsva: &mut egui::ecolor::Hsva, size: f32) {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::click_and_drag());
    if let Some(pos) = response.interact_pointer_pos() {
        hsva.s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        hsva.v = (1.0 - (pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    }

    let painter = ui.painter();
    const CELLS: i32 = 24;
    let cell_w = rect.width() / CELLS as f32;
    let cell_h = rect.height() / CELLS as f32;
    for i in 0..CELLS {
        for j in 0..CELLS {
            let s = (i as f32 + 0.5) / CELLS as f32;
            let v = 1.0 - (j as f32 + 0.5) / CELLS as f32;
            let color = Color32::from(egui::ecolor::Hsva::new(hsva.h, s, v, 1.0));
            let cell_rect = EguiRect::from_min_size(
                rect.min + Vec2::new(i as f32 * cell_w, j as f32 * cell_h),
                Vec2::new(cell_w + 0.5, cell_h + 0.5),
            );
            painter.rect_filled(cell_rect, 0.0, color);
        }
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );

    let marker = rect.min + Vec2::new(hsva.s * rect.width(), (1.0 - hsva.v) * rect.height());
    painter.circle_stroke(marker, 4.0, Stroke::new(1.5_f32, Color32::WHITE));
    painter.circle_stroke(marker, 4.0, Stroke::new(1.0_f32, Color32::BLACK));
}

/// The vertical hue strip: a full rainbow, top (h=0) to bottom (h=1).
fn hue_strip_ui(ui: &mut egui::Ui, hsva: &mut egui::ecolor::Hsva, height: f32) {
    let width = 20.0;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click_and_drag());
    if let Some(pos) = response.interact_pointer_pos() {
        hsva.h = ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
    }

    let painter = ui.painter();
    const CELLS: i32 = 48;
    let cell_h = rect.height() / CELLS as f32;
    for j in 0..CELLS {
        let h = (j as f32 + 0.5) / CELLS as f32;
        let color = Color32::from(egui::ecolor::Hsva::new(h, 1.0, 1.0, 1.0));
        let cell_rect = EguiRect::from_min_size(
            rect.min + Vec2::new(0.0, j as f32 * cell_h),
            Vec2::new(rect.width(), cell_h + 0.5),
        );
        painter.rect_filled(cell_rect, 0.0, color);
    }
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0_f32, Color32::from_gray(90)),
        egui::StrokeKind::Outside,
    );

    let marker_y = rect.top() + hsva.h * rect.height();
    painter.line_segment(
        [
            Pos2::new(rect.left() - 3.0, marker_y),
            Pos2::new(rect.left(), marker_y),
        ],
        Stroke::new(2.0_f32, Color32::WHITE),
    );
    painter.line_segment(
        [
            Pos2::new(rect.right(), marker_y),
            Pos2::new(rect.right() + 3.0, marker_y),
        ],
        Stroke::new(2.0_f32, Color32::WHITE),
    );
}

fn parse_hex6(hex: &str) -> Option<Color32> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

fn next_group_name(editor: &Editor) -> String {
    let highest = editor
        .document()
        .objects()
        .filter(|object| matches!(object.kind, ObjectKind::Group(_)))
        .filter_map(|object| object.name.as_deref())
        .filter_map(|name| name.strip_prefix("Group "))
        .filter_map(|suffix| suffix.parse::<usize>().ok())
        .max()
        .unwrap_or(0);
    format!("Group {}", highest + 1)
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

/// Every `Path` object under `parent`, in paint order (bottom to top),
/// with any `Group` expanded in place rather than rendered itself — a
/// group has no geometry of its own, only its descendants do, so the
/// canvas render loop needs the fully flattened leaf list, not just a
/// layer's direct children (which would silently skip everything inside
/// a group, exactly the "grouped objects vanish" bug this fixes).
fn paint_order_paths(document: &Document, parent: ObjectParent) -> Vec<ObjectId> {
    let mut ids = Vec::new();
    for &id in document.children_of(parent) {
        let Some(object) = document.object(id) else {
            continue;
        };
        match &object.kind {
            ObjectKind::Group(_) => {
                ids.extend(paint_order_paths(document, ObjectParent::Group(id)));
            }
            ObjectKind::Path(_) => ids.push(id),
            _ => {}
        }
    }
    ids
}

/// Expands selected groups into their recursive leaf paths while preserving
/// each selected root's paint order. This makes appearance commands visible
/// for group selections without pretending groups have rendered paint of
/// their own.
fn selected_path_targets(document: &Document, selected: &[ObjectId]) -> Vec<ObjectId> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for &id in selected {
        let Some(object) = document.object(id) else {
            continue;
        };
        let paths = match object.kind {
            ObjectKind::Group(_) => paint_order_paths(document, ObjectParent::Group(id)),
            ObjectKind::Path(_) => vec![id],
            _ => Vec::new(),
        };
        for path in paths {
            if seen.insert(path) {
                targets.push(path);
            }
        }
    }
    targets
}

/// The first selected path leaf is the representative for mixed selection
/// controls. For a selected group, that means its first descendant path.
fn representative_appearance(
    document: &Document,
    selected: &[ObjectId],
) -> amalith_core::Appearance {
    selected_path_targets(document, selected)
        .into_iter()
        .find_map(|id| document.object(id).map(|object| object.appearance))
        .unwrap_or_default()
}

fn apply_paint_to_selection(
    editor: &mut Editor,
    selected: &[ObjectId],
    slot: PaintSlot,
    paint: amalith_core::Paint,
) -> Result<(), amalith_commands::CommandError> {
    let objects = selected_path_targets(editor.document(), selected);
    let command = match slot {
        PaintSlot::Fill => Command::SetFill { objects, paint },
        PaintSlot::Stroke => Command::SetStroke { objects, paint },
    };
    editor.execute(command)?;
    Ok(())
}

/// UI-only selection name heuristic for the options bar. Paths carry no
/// stored primitive kind, so rectangle/ellipse labels intentionally infer
/// from their current Bezier structure rather than adding document-model
/// state solely for presentation text.
fn selection_label(document: &Document, selected: &[ObjectId]) -> String {
    match selected {
        [] => "No Selection".to_string(),
        [id] => document
            .object(*id)
            .map(object_label)
            .unwrap_or_else(|| "No Selection".to_string()),
        _ => format!("{} objects", selected.len()),
    }
}

fn object_label(object: &amalith_core::Object) -> String {
    if let Some(name) = &object.name {
        return name.clone();
    }
    match &object.kind {
        ObjectKind::Group(_) => "Group".to_string(),
        ObjectKind::Path(path) => path_label(path),
        _ => "Object".to_string(),
    }
}

fn path_label(path: &PathData) -> String {
    use amalith_core::geom::PathEl;

    let elements = path.geometry.elements();
    let closed = matches!(elements.last(), Some(PathEl::ClosePath));
    let rectangle = closed
        && amalith_core::geom::anchor_indices(&path.geometry).len() == 4
        && elements.iter().all(|element| {
            matches!(
                element,
                PathEl::MoveTo(_) | PathEl::LineTo(_) | PathEl::ClosePath
            )
        });
    if rectangle {
        return "Rectangle".to_string();
    }
    let ellipse = closed
        && elements.iter().all(|element| {
            matches!(
                element,
                PathEl::MoveTo(_) | PathEl::CurveTo(_, _, _) | PathEl::ClosePath
            )
        });
    if ellipse {
        "Ellipse".to_string()
    } else {
        "Path".to_string()
    }
}

#[cfg(test)]
mod canvas_input_tests {
    use super::*;
    use amalith_commands::Command;

    #[test]
    fn inactive_swatch_selects_and_active_swatch_opens_its_picker() {
        let mut state = FillStrokePanelState::default();
        let red = amalith_core::Paint::Solid(amalith_core::Color::rgb(1.0, 0.0, 0.0));
        assert_eq!(state.active, PaintSlot::Fill);
        assert!(state.open.is_none());

        select_or_open_swatch(&mut state, PaintSlot::Stroke, red);
        assert_eq!(state.active, PaintSlot::Stroke);
        assert!(state.open.is_none());

        select_or_open_swatch(&mut state, PaintSlot::Stroke, red);
        assert!(matches!(state.open, Some((PaintSlot::Stroke, _))));
    }

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
        let mut direct_selection = DirectSelectionTool::default();

        activate_tool(
            ToolKind::Rectangle,
            &mut artboard,
            &mut rectangle,
            &mut ellipse,
            &mut selection,
            &mut direct_selection,
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
            &mut direct_selection,
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
            asset_store: amalith_io::AssetStore::new(),
            file_path: None,
            camera,
            artboard_tool: ArtboardTool::default(),
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
            direct_selection_tool: DirectSelectionTool::default(),
        };

        assert_eq!(tab_label(&tab, 3), "Untitled-3* @ 343.77 % (RGB/Preview)");
    }

    #[test]
    fn tab_label_uses_numbered_fallback_and_clean_state() {
        let mut document = Document::new("ignored");
        document.metadata.title = None;
        let tab = DocumentTab {
            editor: Box::new(Editor::new(document)),
            asset_store: amalith_io::AssetStore::new(),
            file_path: None,
            camera: Camera::default(),
            artboard_tool: ArtboardTool::default(),
            rectangle_tool: RectangleTool::default(),
            ellipse_tool: EllipseTool::default(),
            selection_tool: SelectionTool::default(),
            direct_selection_tool: DirectSelectionTool::default(),
        };
        assert_eq!(tab_label(&tab, 4), "Untitled-4 @ 100.00 % (CMYK/Preview)");
    }

    #[test]
    fn untitled_names_increment_from_open_titles() {
        fn tab(title: &str) -> DocumentTab {
            DocumentTab {
                editor: Box::new(Editor::new(Document::new(title))),
                asset_store: amalith_io::AssetStore::new(),
                file_path: None,
                camera: Camera::default(),
                artboard_tool: ArtboardTool::default(),
                rectangle_tool: RectangleTool::default(),
                ellipse_tool: EllipseTool::default(),
                selection_tool: SelectionTool::default(),
                direct_selection_tool: DirectSelectionTool::default(),
            }
        }
        assert_eq!(
            next_untitled_name(&[tab("Untitled-2"), tab("Logo")]),
            "Untitled-3"
        );
    }

    #[test]
    fn paint_order_paths_expands_groups_instead_of_skipping_them() {
        // Regression test: the canvas render loop used to iterate a
        // layer's direct children only, so grouping an object (which
        // reparents it under the new Group, off the layer) made it
        // vanish from the canvas even though the Layers panel still
        // showed it as grouped.
        let document = Document::new("Untitled");
        let mut editor = Editor::new(document);
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(ungrouped) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(a) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(b) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(40.0, 0.0, 50.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        editor
            .execute(Command::Group {
                ids: vec![a, b],
                name: Some("Group 1".into()),
            })
            .unwrap();

        let ids = paint_order_paths(editor.document(), ObjectParent::Layer(layer));

        assert_eq!(ids.len(), 3, "the grouped objects must still be listed");
        assert!(ids.contains(&ungrouped));
        assert!(ids.contains(&a));
        assert!(ids.contains(&b));
        // The group replaced its members at their old stacking position
        // (see Command::Group's docs), so paint order stays: ungrouped,
        // then the group's own children in their relative order.
        assert_eq!(ids, vec![ungrouped, a, b]);
    }

    #[test]
    fn options_label_classifies_paths_and_custom_group_names() {
        let mut editor = Editor::new(Document::new("Untitled"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(rectangle) = editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let CommandOutcome::Object(ellipse) = editor
            .execute(Command::CreateEllipse {
                layer,
                rect: Rect::new(20.0, 0.0, 30.0, 10.0),
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            selection_label(editor.document(), &[rectangle]),
            "Rectangle"
        );
        assert_eq!(selection_label(editor.document(), &[ellipse]), "Ellipse");
        assert_eq!(
            selection_label(editor.document(), &[rectangle, ellipse]),
            "2 objects"
        );

        let CommandOutcome::Object(group) = editor
            .execute(Command::Group {
                ids: vec![rectangle, ellipse],
                name: Some("Logo Mark".into()),
            })
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(selection_label(editor.document(), &[group]), "Logo Mark");
    }

    #[test]
    fn selected_path_targets_expands_group_to_leaf_paths() {
        let mut editor = Editor::new(Document::new("Untitled"));
        let CommandOutcome::Layer(layer) = editor
            .execute(Command::CreateLayer {
                name: "Layer 1".into(),
                index: None,
            })
            .unwrap()
        else {
            panic!()
        };
        let create = |editor: &mut Editor, x| match editor
            .execute(Command::CreateRect {
                layer,
                rect: Rect::new(x, 0.0, x + 10.0, 10.0),
                name: None,
            })
            .unwrap()
        {
            CommandOutcome::Object(id) => id,
            _ => panic!(),
        };
        let first = create(&mut editor, 0.0);
        let second = create(&mut editor, 20.0);
        let CommandOutcome::Object(group) = editor
            .execute(Command::Group {
                ids: vec![first, second],
                name: None,
            })
            .unwrap()
        else {
            panic!()
        };

        assert_eq!(
            selected_path_targets(editor.document(), &[group]),
            vec![first, second]
        );
    }
}
