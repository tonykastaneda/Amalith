mod artboard_tool;
mod camera;
mod direct_selection;
mod ellipse_tool;
mod fill_mesh;
mod pen_tool;
mod primitive_tool;
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
use pen_tool::PenTool;
use primitive_tool::{PrimitiveKind, PrimitiveTool};
use rectangle_tool::RectangleTool;
use selection::SelectionTool;
use std::collections::HashSet;
use std::path::PathBuf;

const APP_BAR: Color32 = Color32::from_rgb(48, 48, 48);
const PANEL: Color32 = Color32::from_rgb(43, 43, 43);
const PANEL_RAISED: Color32 = Color32::from_rgb(53, 53, 53);
const PANEL_BORDER: Color32 = Color32::from_rgb(31, 31, 31);
const CANVAS: Color32 = Color32::from_rgb(51, 51, 51);
const ACCENT: Color32 = Color32::from_rgb(54, 132, 217);
const TOOL_SELECTION_SVG: &str = include_str!("../../../branding/SVG/V-selection.svg");
const TOOL_DIRECT_SELECTION_SVG: &str = include_str!("../../../branding/SVG/A-selection.svg");
const TOOL_RECTANGLE_SVG: &str = include_str!("../../../branding/SVG/Square.svg");
const TOOL_PEN_SVG: &str = include_str!("../../../branding/SVG/Pen.svg");
const CURSOR_PEN_DRAWING_SVG: &str = include_str!("../../../branding/SVG/Pen-drawing.svg");
const CURSOR_PEN_CLOSE_SVG: &str = include_str!("../../../branding/SVG/Pen-closeShape.svg");
const CURSOR_SELECTION_SVG: &str = include_str!("../../../branding/SVG/V-selection.svg");
const CURSOR_DIRECT_SELECTION_SVG: &str = include_str!("../../../branding/SVG/A-selection.svg");
const TOOL_ARTBOARD_SVG: &str = include_str!("../../../branding/SVG/Artboard Tool.svg");

#[derive(Clone, Copy)]
enum ToolIcon {
    Selection,
    DirectSelection,
    Pen,
    Rectangle,
    Artboard,
}

impl ToolIcon {
    const fn svg(self) -> &'static str {
        match self {
            Self::Selection => TOOL_SELECTION_SVG,
            Self::DirectSelection => TOOL_DIRECT_SELECTION_SVG,
            Self::Pen => TOOL_PEN_SVG,
            Self::Rectangle => TOOL_RECTANGLE_SVG,
            Self::Artboard => TOOL_ARTBOARD_SVG,
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([620.0, 620.0])
            .with_title("Amalith Ver. Alpha")
            // On macOS, let Amalith paint behind the title bar. The native
            // traffic lights stay in place while the app supplies the same
            // dark surface and a properly centered product name.
            .with_fullsize_content_view(true)
            .with_title_shown(false)
            .with_titlebar_shown(false),
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
    panel_layout: PanelLayout,
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
    pen_tool: PenTool,
    primitive_tool: PrimitiveTool,
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
    artboards_item: muda::CheckMenuItem,
    layers_item: muda::CheckMenuItem,
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
    /// Size of this panel's floating window (an in-app, app-styled overlay,
    /// not an OS window). `PanelDock::Floating`'s `pos` is its top-left in
    /// app-frame-local coordinates.
    float_size: Vec2,
}

const FLOAT_DEFAULT_SIZE: Vec2 = Vec2::new(248.0, 360.0);

/// An in-progress panel drag. The dock layout is NOT mutated while a drag is
/// live — a translucent ghost and a blue drop indicator preview where the
/// panel will land, and the move is committed once on release. Held across
/// frames on [`PanelLayout`].
#[derive(Clone, Debug, PartialEq)]
struct PanelDrag {
    panel: PanelId,
    /// Where the panel sat when the drag began (a docked panel previews with
    /// a ghost; an already-floating panel is moved live).
    origin: PanelDock,
    /// Cursor position minus the dragged widget's top-left, so the ghost /
    /// dropped window keeps the same grab point under the cursor.
    grab_offset: Vec2,
    cursor: Pos2,
    /// Set the frame the pointer is released; `panels_ui` commits then.
    released: bool,
}

/// Where a live [`PanelDrag`] would land if dropped now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DropTarget {
    /// Insert as a new group on `side` at `index` (0 = above the top group,
    /// `groups.len()` = below the bottom group).
    Rail { side: PanelDock, index: usize },
    /// Tab into the group already on `side` (or dock alone if it is empty).
    TabInto { side: PanelDock },
    /// Float at `pos` (app-frame-local top-left).
    Float { pos: Pos2 },
}

/// A rail showing both panels stacked vertically instead of tabbed. `top`
/// is the upper panel (the other visible panel takes the lower slot);
/// `split` is the upper panel's share of the body height.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StackedRail {
    top: PanelId,
    split: f32,
    top_collapsed: bool,
    bottom_collapsed: bool,
}

impl StackedRail {
    const MIN_SPLIT: f32 = 0.2;
    const MAX_SPLIT: f32 = 0.8;

    fn bottom(&self) -> PanelId {
        match self.top {
            PanelId::Artboards => PanelId::Layers,
            PanelId::Layers => PanelId::Artboards,
        }
    }
}

/// The small but extensible panel-dock model. Both Artboards and Layers can
/// share a rail as tabs, while a panel dragged away keeps its own floating
/// window. Keeping rail state separate from panel content state is what makes
/// the dock behave as a system instead of two unrelated sidebars.
struct PanelLayout {
    left: PanelRail,
    right: PanelRail,
    /// The live drag, if any. Survives across frames until release.
    drag: Option<PanelDrag>,
    /// Per-frame scratch rebuilt by `panels_ui`: each rail's screen rect and
    /// the screen rects of its group headers, top to bottom. The blue drop
    /// indicator and `resolve_drop` are computed from these.
    left_rect: Option<EguiRect>,
    right_rect: Option<EguiRect>,
    left_headers: Vec<EguiRect>,
    right_headers: Vec<EguiRect>,
    /// Label + size of the panel currently being dragged, for the ghost.
    ghost: Option<(&'static str, Vec2)>,
}

struct PanelRail {
    width: f32,
    active: PanelId,
    collapsed: bool,
    /// `Some` when this rail shows both panels stacked vertically rather
    /// than tabbed. Reconciled every frame by `panels_ui` from the panels
    /// actually docked here plus any `drop_hint` a drag just produced.
    stacked: Option<StackedRail>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PanelId {
    Artboards,
    Layers,
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
            float_size: FLOAT_DEFAULT_SIZE,
        }
    }
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            left: PanelRail::default(),
            right: PanelRail::default(),
            drag: None,
            left_rect: None,
            right_rect: None,
            left_headers: Vec::new(),
            right_headers: Vec::new(),
            ghost: None,
        }
    }
}

impl Default for PanelRail {
    fn default() -> Self {
        Self {
            width: 220.0,
            active: PanelId::Layers,
            collapsed: false,
            stacked: None,
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

enum CanvasCursor {
    Pen { pointer: Pos2, closing_path: bool },
    Selection { pointer: Pos2, direct: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolKind {
    Selection,
    DirectSelection,
    Pen,
    Rectangle,
    Ellipse,
    RoundedRectangle,
    Polygon,
    Star,
    Artboard,
}

fn activate_tool(
    tool: ToolKind,
    artboard_tool: &mut ArtboardTool,
    rectangle_tool: &mut RectangleTool,
    ellipse_tool: &mut EllipseTool,
    pen_tool: &mut PenTool,
    primitive_tool: &mut PrimitiveTool,
    selection_tool: &mut SelectionTool,
    direct_selection_tool: &mut DirectSelectionTool,
    first_artboard: Option<ArtboardId>,
) {
    artboard_tool.set_active(tool == ToolKind::Artboard, first_artboard);
    rectangle_tool.set_active(tool == ToolKind::Rectangle);
    ellipse_tool.set_active(tool == ToolKind::Ellipse);
    pen_tool.set_active(tool == ToolKind::Pen);
    primitive_tool.set_active(match tool {
        ToolKind::RoundedRectangle => Some(PrimitiveKind::RoundedRectangle),
        ToolKind::Polygon => Some(PrimitiveKind::Polygon),
        ToolKind::Star => Some(PrimitiveKind::Star),
        _ => None,
    });
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

/// Illustrator's temporary Direct Selection gesture: holding Command while
/// the black arrow is active routes pointer work through the white arrow.
/// Cmd+Space remains scrubby zoom, so it always takes precedence.
fn temporary_direct_selection(
    selection_active: bool,
    command_down: bool,
    space_down: bool,
) -> bool {
    selection_active && command_down && !space_down
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
        visuals.panel_fill = PANEL;
        visuals.window_fill = PANEL_RAISED;
        visuals.widgets.inactive.bg_fill = PANEL;
        visuals.widgets.hovered.bg_fill = PANEL_RAISED;
        visuals.widgets.active.bg_fill = Color32::from_rgb(62, 82, 103);
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, PANEL_BORDER);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_gray(82));
        visuals.selection.bg_fill = ACCENT;
        visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(112, 178, 245));
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
            panel_layout: PanelLayout::default(),
            fill_stroke_panel: FillStrokePanelState::default(),
            #[cfg(target_os = "macos")]
            native_menu,
        }
    }

    #[cfg(target_os = "macos")]
    fn build_native_menu() -> NativeMenu {
        use muda::{
            accelerator::{Accelerator, Code, Modifiers},
            CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu,
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
        let window = Submenu::new("Window", true);
        let artboards_item = CheckMenuItem::new("Artboards", true, true, None);
        let layers_item = CheckMenuItem::new("Layers", true, true, None);
        file.append_items(&[&new_item, &open_item, &save_item, &save_as_item])
            .expect("append File menu items");
        let app =
            Submenu::with_items("Amalith", true, &[&quit_item]).expect("build application menu");
        window
            .append_items(&[&artboards_item, &layers_item])
            .expect("append Window items");
        menu.append(&app).expect("append application menu");
        menu.append(&file).expect("append File menu");
        menu.append(&window).expect("append Window menu");
        menu.init_for_nsapp();
        NativeMenu {
            _menu: menu,
            new_item,
            open_item,
            save_item,
            save_as_item,
            artboards_item,
            layers_item,
        }
    }

    #[cfg(target_os = "macos")]
    fn process_native_menu_events(&mut self) {
        let mut actions = Vec::new();
        let (artboards_item, layers_item) = {
            let Some(native_menu) = &self.native_menu else {
                return;
            };
            while let Ok(event) = muda::MenuEvent::receiver().try_recv() {
                if event.id == *native_menu.new_item.id() {
                    actions.push(0);
                } else if event.id == *native_menu.open_item.id() {
                    actions.push(1);
                } else if event.id == *native_menu.save_item.id() {
                    actions.push(2);
                } else if event.id == *native_menu.save_as_item.id() {
                    actions.push(3);
                } else if event.id == *native_menu.artboards_item.id() {
                    actions.push(4);
                } else if event.id == *native_menu.layers_item.id() {
                    actions.push(5);
                }
            }
            (
                native_menu.artboards_item.clone(),
                native_menu.layers_item.clone(),
            )
        };
        for action in actions {
            match action {
                0 => self.open_new_document(),
                1 => self.open_document_from_disk(),
                2 => self.save_active_document(false),
                3 => self.save_active_document(true),
                4 => self.artboards_panel.chrome.hidden = !self.artboards_panel.chrome.hidden,
                5 => self.layers_panel.chrome.hidden = !self.layers_panel.chrome.hidden,
                _ => unreachable!(),
            }
        }
        artboards_item.set_checked(!self.artboards_panel.chrome.hidden);
        layers_item.set_checked(!self.layers_panel.chrome.hidden);
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
            pen_tool: PenTool::default(),
            primitive_tool: PrimitiveTool::default(),
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
                    pen_tool: PenTool::default(),
                    primitive_tool: PrimitiveTool::default(),
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
            .frame(egui::Frame::NONE.fill(CANVAS))
            .show(ctx, |ui| {
                let panel_width = 420.0_f32.min(ui.available_width() - 32.0);
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(panel_width, ui.available_height() - 36.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                            ui.label(
                                egui::RichText::new("PRESET DETAILS")
                                    .strong()
                                    .size(10.0)
                                    .color(Color32::from_gray(190)),
                            );
                            ui.add_space(2.0);
                            ui.add_sized(
                                [panel_width, 28.0],
                                egui::TextEdit::singleline(&mut form.name)
                                    .font(FontId::proportional(16.0)),
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
                            ui.add_space(6.0);
                            let _ = ui.button("More Settings");
                            if let Some(error) = error {
                                ui.colored_label(Color32::from_rgb(245, 110, 110), error);
                            }
                            ui.add_space((ui.available_height() - 42.0).max(8.0));
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_sized(
                                            [82.0, 28.0],
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
                                        .add_sized([82.0, 28.0], egui::Button::new("Close"))
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
            .exact_height(30.0)
            .frame(egui::Frame::NONE.fill(APP_BAR))
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
                                    let fill = if active { PANEL_RAISED } else { APP_BAR };
                                    egui::Frame::NONE
                                        .fill(fill)
                                        .inner_margin(egui::Margin::symmetric(8, 2))
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            egui::RichText::new("×").size(14.0),
                                                        )
                                                        .frame(false),
                                                    )
                                                    .clicked()
                                                {
                                                    close = Some(index);
                                                }
                                                let text =
                                                    egui::RichText::new(tab_label(tab, index + 1))
                                                        .strong()
                                                        .size(12.0)
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
                                            if active {
                                                let underline = EguiRect::from_min_size(
                                                    Pos2::new(
                                                        ui.min_rect().left(),
                                                        ui.min_rect().bottom() - 2.0,
                                                    ),
                                                    Vec2::new(ui.min_rect().width(), 2.0),
                                                );
                                                ui.painter().rect_filled(underline, 0.0, ACCENT);
                                            }
                                        });
                                }
                            });
                        });
                    ui.add_space(12.0);
                    if ui
                        .add(egui::Button::new(egui::RichText::new("+").size(18.0)).frame(false))
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
                    &mut tab.pen_tool,
                    &mut tab.primitive_tool,
                    &mut tab.selection_tool,
                    &mut tab.direct_selection_tool,
                    &mut self.artboards_panel,
                    &mut self.layers_panel,
                    &mut self.panel_layout,
                    &mut self.fill_stroke_panel,
                    &mut self.error,
                );
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn title_bar_ui(ctx: &egui::Context) {
        egui::TopBottomPanel::top("amalith_title_bar")
            .exact_height(30.0)
            .frame(egui::Frame::NONE.fill(APP_BAR))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Amalith Ver. Alpha",
                    FontId::proportional(13.0),
                    Color32::from_gray(205),
                );
            });
    }

    #[cfg(not(target_os = "macos"))]
    fn app_menu_ui(&mut self, ctx: &egui::Context) {
        let mut action = None;
        egui::TopBottomPanel::top("app_menu")
            .exact_height(24.0)
            .frame(egui::Frame::NONE.fill(APP_BAR))
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
                    ui.menu_button("Window", |ui| {
                        let mut artboards_visible = !self.artboards_panel.chrome.hidden;
                        if ui.checkbox(&mut artboards_visible, "Artboards").changed() {
                            self.artboards_panel.chrome.hidden = !artboards_visible;
                        }
                        let mut layers_visible = !self.layers_panel.chrome.hidden;
                        if ui.checkbox(&mut layers_visible, "Layers").changed() {
                            self.layers_panel.chrome.hidden = !layers_visible;
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
        pen_tool: &mut PenTool,
        primitive_tool: &mut PrimitiveTool,
        selection_tool: &mut SelectionTool,
        direct_selection_tool: &mut DirectSelectionTool,
        artboards_panel: &mut ArtboardsPanelState,
        layers_panel: &mut LayersPanelState,
        panel_layout: &mut PanelLayout,
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
            panel_layout,
            error,
        );
        Self::options_bar_ui(ctx, editor, selection_tool, fill_stroke_panel, error);
        Self::tools_bar_ui(
            ctx,
            editor,
            artboard_tool,
            rectangle_tool,
            ellipse_tool,
            pen_tool,
            primitive_tool,
            selection_tool,
            direct_selection_tool,
            fill_stroke_panel,
            error,
        );
        let pasteboard = if artboard_tool.active {
            Color32::from_rgb(91, 91, 91)
        } else {
            CANVAS
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(pasteboard))
            .show(ctx, |ui| {
                let available = ui.max_rect().shrink2(Vec2::new(32.0, 36.0));
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
                let pen_pressed = !navigation_key_down
                    && canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::P) > 0
                    });
                let enter_pressed = canvas_shortcuts_enabled
                    && ctx.input_mut(|input| {
                        input.count_and_consume_key(egui::Modifiers::NONE, egui::Key::Enter) > 0
                    });
                let first_artboard = boards.first().map(|board| board.id);
                if shift_o_pressed {
                    if artboard_tool.active {
                        artboard_tool.set_active(false, first_artboard);
                        rectangle_tool.set_active(false);
                        ellipse_tool.set_active(false);
                        pen_tool.set_active(false);
                        primitive_tool.set_active(None);
                        selection_tool.set_active(false);
                        direct_selection_tool.set_active(false);
                    } else {
                        activate_tool(
                            ToolKind::Artboard,
                            artboard_tool,
                            rectangle_tool,
                            ellipse_tool,
                            pen_tool,
                            primitive_tool,
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
                        pen_tool,
                        primitive_tool,
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
                        pen_tool,
                        primitive_tool,
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
                        pen_tool,
                        primitive_tool,
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
                        pen_tool,
                        primitive_tool,
                        selection_tool,
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if pen_pressed {
                    activate_tool(
                        ToolKind::Pen,
                        artboard_tool,
                        rectangle_tool,
                        ellipse_tool,
                        pen_tool,
                        primitive_tool,
                        selection_tool,
                        direct_selection_tool,
                        first_artboard,
                    );
                } else if enter_pressed && pen_tool.active && pen_tool.is_drawing() {
                    if let Err(err) = pen_tool.finish(editor) {
                        *error = Some(err.to_string());
                    }
                } else if escape_pressed && pen_tool.active && pen_tool.is_drawing() {
                    pen_tool.cancel();
                } else if escape_pressed {
                    artboard_tool.set_active(false, first_artboard);
                    rectangle_tool.set_active(false);
                    ellipse_tool.set_active(false);
                    pen_tool.set_active(false);
                    primitive_tool.set_active(None);
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
                let temporary_direct_selection =
                    temporary_direct_selection(selection_tool.active, command_down, space_down);
                let direct_selection_active =
                    direct_selection_tool.active || temporary_direct_selection;
                let mut canvas_cursor = None;

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
                        pen_tool.cancel();
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
                        pen_tool.cancel();
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
                            pen_tool.cancel();
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
                        } else if pen_tool.active {
                            if let Some(pointer) = pointer_position {
                                if available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    let pointer_doc = camera.screen_to_document(pointer, available);
                                    let point =
                                        Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                    let close_target =
                                        pen_tool.can_close_at(point, 8.0 / camera.scale as f64);
                                    ctx.set_cursor_icon(egui::CursorIcon::None);
                                    canvas_cursor = Some(CanvasCursor::Pen {
                                        pointer,
                                        closing_path: close_target,
                                    });
                                    if primary_pressed {
                                        if pen_tool.press(
                                            point,
                                            8.0 / camera.scale as f64,
                                            shift_down,
                                        ) {
                                            if let Err(err) = pen_tool.finish(editor) {
                                                *error = Some(err.to_string());
                                            }
                                        }
                                    } else {
                                        pen_tool.update_hover(point, shift_down);
                                    }
                                }
                            }
                        } else if primitive_tool.active.is_some() {
                            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                            if let Some(pointer) = pointer_position {
                                let pointer_doc = camera.screen_to_document(pointer, available);
                                let point = Point::new(pointer_doc.x as f64, pointer_doc.y as f64);
                                if primary_pressed
                                    && available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    primitive_tool.begin_drag(point);
                                } else if primary_down {
                                    primitive_tool.update_drag(point, shift_down);
                                }
                            }
                            if primary_released {
                                if let Err(err) = primitive_tool.finish_drag(editor) {
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
                        } else if direct_selection_active {
                            if let Some(pointer) = pointer_position {
                                if available.contains(pointer)
                                    && ui.rect_contains_pointer(available)
                                {
                                    ctx.set_cursor_icon(egui::CursorIcon::None);
                                    canvas_cursor = Some(CanvasCursor::Selection {
                                        pointer,
                                        direct: true,
                                    });
                                }
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
                                let over_canvas = available.contains(pointer)
                                    && ui.rect_contains_pointer(available);
                                if over_canvas
                                    && !selection_tool.is_duplicate_drag()
                                    && hovered_handle.is_none()
                                    && !hovered_rotate
                                {
                                    ctx.set_cursor_icon(egui::CursorIcon::None);
                                    canvas_cursor = Some(CanvasCursor::Selection {
                                        pointer,
                                        direct: false,
                                    });
                                }
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
                // Both arrow tools expose a selected path's actual geometry
                // continuously. This is separate from the black-arrow
                // bounding box and is intentionally present before any node
                // movement, matching Illustrator/Inkscape behavior.
                let selected_paths = selected_path_targets(
                    editor.document(),
                    &selection_tool.selected_in_paint_order(editor.document()),
                );
                let path_overlay_active = selection_tool.active || direct_selection_tool.active;
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
                                let path_closed = path_is_closed(path);
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
                                if path_closed && fill.a() > 0 {
                                    if let Some(mesh) = fill_mesh::fill_mesh(&screen_subpaths, fill)
                                    {
                                        painter.add(egui::Shape::mesh(mesh));
                                    }
                                }
                                if stroke != Stroke::NONE {
                                    for points in &screen_subpaths {
                                        if points.len() >= 2 {
                                            painter.add(if path_closed {
                                                egui::Shape::closed_line(points.clone(), stroke)
                                            } else {
                                                egui::Shape::line(points.clone(), stroke)
                                            });
                                        }
                                    }
                                }
                                let path_is_selected = selected_paths.contains(&id)
                                    || direct_selection_tool.has_selected_anchor_on(id);
                                if path_overlay_active && path_is_selected {
                                    for points in &screen_subpaths {
                                        if points.len() >= 2 {
                                            let overlay = Stroke::new(1.5_f32, ACCENT);
                                            painter.add(if path_closed {
                                                egui::Shape::closed_line(points.clone(), overlay)
                                            } else {
                                                egui::Shape::line(points.clone(), overlay)
                                            });
                                        }
                                    }
                                }
                                if direct_selection_active {
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
                                        // In node mode, a filled blue square
                                        // means that anchor is selected.
                                        // Unselected anchors stay white with
                                        // a blue border, so a marquee's exact
                                        // node selection is immediately clear.
                                        if direct_selection_tool.selected.contains(&(id, index)) {
                                            painter.rect_filled(marker, 0.0, ACCENT);
                                        } else {
                                            painter.rect_filled(marker, 0.0, Color32::WHITE);
                                            painter.rect_stroke(
                                                marker,
                                                0.0,
                                                Stroke::new(1.25_f32, ACCENT),
                                                egui::StrokeKind::Outside,
                                            );
                                        }
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
                            let path_closed = path_is_closed(path);
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
                            if path_closed && ghost_fill.a() > 0 {
                                if let Some(mesh) =
                                    fill_mesh::fill_mesh(&ghost_subpaths, ghost_fill)
                                {
                                    painter.add(egui::Shape::mesh(mesh));
                                }
                            }
                            if ghost_stroke != Stroke::NONE {
                                for points in &ghost_subpaths {
                                    if points.len() >= 2 {
                                        painter.add(if path_closed {
                                            egui::Shape::closed_line(points.clone(), ghost_stroke)
                                        } else {
                                            egui::Shape::line(points.clone(), ghost_stroke)
                                        });
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
                if let Some(preview) = &primitive_tool.preview {
                    let points: Vec<Vec<Pos2>> = preview
                        .flattened_points(0.5)
                        .into_iter()
                        .map(|subpath| {
                            subpath
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
                    for subpath in points {
                        if subpath.len() >= 2 {
                            painter.add(egui::Shape::closed_line(
                                subpath,
                                Stroke::new(1.5_f32, ACCENT),
                            ));
                        }
                    }
                }
                if pen_tool.active && pen_tool.is_drawing() {
                    if let Some(preview) = &pen_tool.preview {
                        for subpath in preview.flattened_points(0.5) {
                            let points: Vec<_> = subpath
                                .into_iter()
                                .map(|point| {
                                    camera.document_to_screen(
                                        Pos2::new(point.x as f32, point.y as f32),
                                        available,
                                    )
                                })
                                .collect();
                            if points.len() >= 2 {
                                painter
                                    .add(egui::Shape::line(points, Stroke::new(1.5_f32, ACCENT)));
                            }
                        }
                    }
                    for point in pen_tool.anchors() {
                        let point = camera.document_to_screen(
                            Pos2::new(point.x as f32, point.y as f32),
                            available,
                        );
                        let marker = EguiRect::from_center_size(point, Vec2::splat(7.0));
                        painter.rect_filled(marker, 0.0, Color32::WHITE);
                        painter.rect_stroke(
                            marker,
                            0.0,
                            Stroke::new(1.25_f32, ACCENT),
                            egui::StrokeKind::Outside,
                        );
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
                if let Some(cursor) = canvas_cursor {
                    paint_canvas_cursor(painter, cursor);
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
            .exact_height(30.0)
            .frame(egui::Frame::NONE.fill(APP_BAR))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(5.0, 0.0);
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(label)
                            .size(11.0)
                            .color(Color32::from_gray(182)),
                    );
                    ui.separator();

                    ui.label(egui::RichText::new("Fill").size(11.0));
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

                    ui.label(egui::RichText::new("Stroke").size(11.0));
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
                    ui.label(egui::RichText::new("Weight").size(11.0));
                    let mut width = representative.stroke_width;
                    if ui
                        .add(
                            egui::DragValue::new(&mut width)
                                .max_decimals(2)
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

                    ui.separator();
                    ui.label(egui::RichText::new("Opacity").size(11.0));
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
                });
            });
    }

    fn tools_bar_ui(
        ctx: &egui::Context,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        rectangle_tool: &mut RectangleTool,
        ellipse_tool: &mut EllipseTool,
        pen_tool: &mut PenTool,
        primitive_tool: &mut PrimitiveTool,
        selection_tool: &mut SelectionTool,
        direct_selection_tool: &mut DirectSelectionTool,
        fill_stroke_panel: &mut FillStrokePanelState,
        error: &mut Option<String>,
    ) {
        let first_artboard = editor.document().artboards().first().map(|board| board.id);
        egui::SidePanel::left("tools_bar")
            .exact_width(52.0)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(PANEL))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.add_space(5.0);
                            let button = |ui: &mut egui::Ui,
                                          icon: ToolIcon,
                                          tooltip: &str,
                                          active: bool|
                             -> bool {
                                let (rect, response) =
                                    ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::click());
                                if active {
                                    ui.painter().rect_filled(
                                        rect,
                                        3.0,
                                        Color32::from_rgb(61, 92, 123),
                                    );
                                } else if response.hovered() {
                                    ui.painter().rect_filled(rect, 3.0, PANEL_RAISED);
                                }
                                paint_brand_tool_icon(ui.painter(), rect.shrink(5.0), icon, active);
                                response.on_hover_text(tooltip).clicked()
                            };

                            if button(
                                ui,
                                ToolIcon::Selection,
                                "Selection (V)",
                                selection_tool.active,
                            ) {
                                activate_tool(
                                    ToolKind::Selection,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                            }
                            if button(
                                ui,
                                ToolIcon::DirectSelection,
                                "Direct Selection (A)",
                                direct_selection_tool.active,
                            ) {
                                activate_tool(
                                    ToolKind::DirectSelection,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                            }
                            if button(ui, ToolIcon::Pen, "Pen (P)", pen_tool.active) {
                                activate_tool(
                                    ToolKind::Pen,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                            }
                            let shape_active = rectangle_tool.active
                                || ellipse_tool.active
                                || primitive_tool.active.is_some();
                            let (shape_rect, shape_response) =
                                ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::click());
                            if shape_active {
                                ui.painter().rect_filled(
                                    shape_rect,
                                    3.0,
                                    Color32::from_rgb(61, 92, 123),
                                );
                            } else if shape_response.hovered() {
                                ui.painter().rect_filled(shape_rect, 3.0, PANEL_RAISED);
                            }
                            paint_brand_tool_icon(
                                ui.painter(),
                                shape_rect.shrink(5.0),
                                ToolIcon::Rectangle,
                                shape_active,
                            );
                            let flyout_id = ui.id().with("shape_tool_flyout");
                            // Desktop Illustrator exposes a stacked tool by
                            // pressing and holding its toolbar slot. Open
                            // the flyout as soon as that press is held so a
                            // mouse user gets the same behavior as touch.
                            if shape_response.is_pointer_button_down_on() {
                                egui::Popup::open_id(ui.ctx(), flyout_id);
                            }
                            let mut flyout_choice = None;
                            egui::Popup::from_response(&shape_response)
                                .id(flyout_id)
                                .open_memory(None)
                                .show(|ui| {
                                    ui.set_min_width(210.0);
                                    if ui.button("□  Rectangle Tool                 (M)").clicked()
                                    {
                                        flyout_choice = Some(ToolKind::Rectangle);
                                    }
                                    if ui.button("▢  Rounded Rectangle Tool").clicked() {
                                        flyout_choice = Some(ToolKind::RoundedRectangle);
                                    }
                                    if ui
                                        .button("○  Ellipse Tool                     (L)")
                                        .clicked()
                                    {
                                        flyout_choice = Some(ToolKind::Ellipse);
                                    }
                                    if ui.button("⬡  Polygon Tool").clicked() {
                                        flyout_choice = Some(ToolKind::Polygon);
                                    }
                                    if ui.button("☆  Star Tool").clicked() {
                                        flyout_choice = Some(ToolKind::Star);
                                    }
                                });
                            if let Some(tool) = flyout_choice {
                                activate_tool(
                                    tool,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                                egui::Popup::close_id(ui.ctx(), flyout_id);
                            } else if shape_response.clicked() {
                                egui::Popup::close_id(ui.ctx(), flyout_id);
                                activate_tool(
                                    ToolKind::Rectangle,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                            }
                            if button(
                                ui,
                                ToolIcon::Artboard,
                                "Artboard (Shift+O)",
                                artboard_tool.active,
                            ) {
                                activate_tool(
                                    ToolKind::Artboard,
                                    artboard_tool,
                                    rectangle_tool,
                                    ellipse_tool,
                                    pen_tool,
                                    primitive_tool,
                                    selection_tool,
                                    direct_selection_tool,
                                    first_artboard,
                                );
                            }
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(5.0);
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

        let swatch_size = 20.0;
        let offset = 8.0;
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

        ui.add_space(3.0);
        ui.horizontal(|ui| {
            // Swap fill and stroke on the current selection.
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("⇄").size(13.0))
                        .small()
                        .frame(false),
                )
                .on_hover_text("Swap fill and stroke")
                .clicked()
                && has_selection
            {
                apply(editor, error, PaintSlot::Fill, representative.stroke);
                apply(editor, error, PaintSlot::Stroke, representative.fill);
            }
            // Reset to the default appearance (light fill, dark stroke).
            if ui
                .add(
                    egui::Button::new(egui::RichText::new("↶").size(13.0))
                        .small()
                        .frame(false),
                )
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
        ui.add_space(3.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let active_paint = paint_for_slot(representative, state.active);
            let icon = Vec2::splat(13.0);

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
            .id_salt("artboards_panel_rows")
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
            .id_salt("layers_panel_rows")
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
        layout: &mut PanelLayout,
        error: &mut Option<String>,
    ) {
        layout.left_headers.clear();
        layout.right_headers.clear();
        layout.left_rect = None;
        layout.right_rect = None;
        let frame = ctx.input(|input| input.screen_rect());

        // Esc aborts a live drag, leaving the dock untouched.
        if layout.drag.is_some() && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            layout.drag = None;
        }
        if layout.drag.is_none() {
            layout.ghost = None;
        }

        // A rail is only a vertical stack while it actually holds both
        // panels; otherwise it renders as a plain (tabbed) group.
        for (side, rail) in [
            (PanelDock::Left, &mut layout.left),
            (PanelDock::Right, &mut layout.right),
        ] {
            let count = [&artboards.chrome, &layers.chrome]
                .iter()
                .filter(|chrome| !chrome.hidden && chrome.dock == side)
                .count();
            if count < 2 {
                rail.stacked = None;
            }
        }

        let left_on = (!artboards.chrome.hidden && artboards.chrome.dock == PanelDock::Left)
            || (!layers.chrome.hidden && layers.chrome.dock == PanelDock::Left);
        if left_on {
            let response = egui::SidePanel::left("left_panel_dock")
                .exact_width(layout.left.width)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(PANEL))
                .show(ctx, |ui| {
                    Self::dock_column_body(
                        ui,
                        ctx,
                        PanelDock::Left,
                        &mut layout.left,
                        &mut layout.left_headers,
                        &mut layout.drag,
                        &mut layout.ghost,
                        editor,
                        artboard_tool,
                        selection_tool,
                        artboards,
                        layers,
                        error,
                    );
                });
            layout.left_rect = Some(response.response.rect);
        }

        let right_on = (!artboards.chrome.hidden && artboards.chrome.dock == PanelDock::Right)
            || (!layers.chrome.hidden && layers.chrome.dock == PanelDock::Right);
        if right_on {
            let response = egui::SidePanel::right("right_panel_dock")
                .exact_width(layout.right.width)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(PANEL))
                .show(ctx, |ui| {
                    Self::dock_column_body(
                        ui,
                        ctx,
                        PanelDock::Right,
                        &mut layout.right,
                        &mut layout.right_headers,
                        &mut layout.drag,
                        &mut layout.ghost,
                        editor,
                        artboard_tool,
                        selection_tool,
                        artboards,
                        layers,
                        error,
                    );
                });
            layout.right_rect = Some(response.response.rect);
        }

        for panel in [PanelId::Artboards, PanelId::Layers] {
            Self::floating_panel_overlay(
                ctx,
                panel,
                &mut layout.drag,
                &mut layout.ghost,
                editor,
                artboard_tool,
                selection_tool,
                artboards,
                layers,
                error,
            );
        }

        // A live drag is preview-only: draw the ghost and the blue drop
        // indicator now, and commit the move once, on release.
        if let Some(mut drag) = layout.drag.take() {
            let target = Self::resolve_drop(
                drag.cursor,
                drag.grab_offset,
                layout.left_rect,
                &layout.left_headers,
                layout.right_rect,
                &layout.right_headers,
                frame,
            );
            if drag.released {
                Self::apply_drop(target, drag.panel, artboards, layers, layout);
                layout.ghost = None;
            } else {
                if matches!(drag.origin, PanelDock::Floating { .. }) {
                    // An already-floating panel just follows the cursor; the
                    // blue indicator still shows when it is over a rail.
                    let chrome = match drag.panel {
                        PanelId::Artboards => &mut artboards.chrome,
                        PanelId::Layers => &mut layers.chrome,
                    };
                    chrome.dock = PanelDock::Floating {
                        pos: drag.cursor - drag.grab_offset,
                    };
                } else {
                    Self::paint_drag_ghost(ctx, drag.cursor - drag.grab_offset, layout.ghost);
                }
                Self::paint_drop_indicator(ctx, target, layout, frame);
                drag.released = false;
                layout.drag = Some(drag);
            }
            ctx.request_repaint();
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn dock_column_body(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        side: PanelDock,
        rail: &mut PanelRail,
        headers: &mut Vec<EguiRect>,
        drag: &mut Option<PanelDrag>,
        ghost: &mut Option<(&'static str, Vec2)>,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        selection_tool: &mut SelectionTool,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let show_artboards = !artboards.chrome.hidden && artboards.chrome.dock == side;
        let show_layers = !layers.chrome.hidden && layers.chrome.dock == side;
        let visible: Vec<_> = [
            (PanelId::Artboards, show_artboards),
            (PanelId::Layers, show_layers),
        ]
        .into_iter()
        .filter_map(|(panel, visible)| visible.then_some(panel))
        .collect();
        if visible.is_empty() {
            return;
        }
        if !visible.contains(&rail.active) {
            rail.active = visible[0];
            rail.collapsed = false;
        }

        // The rail owns one shared width. Its inside edge is a direct
        // horizontal resize target, just like Illustrator's dock divider.
        let available = ui.max_rect();
        let resize_rect = if side == PanelDock::Left {
            EguiRect::from_min_max(
                Pos2::new(available.right() - 3.0, available.top()),
                available.right_bottom(),
            )
        } else {
            EguiRect::from_min_max(
                available.left_top(),
                Pos2::new(available.left() + 3.0, available.bottom()),
            )
        };
        let resize = ui.interact(
            resize_rect,
            ui.id().with("rail_resize"),
            egui::Sense::drag(),
        );
        if resize.dragged() {
            let dx = ui.input(|input| input.pointer.delta().x);
            let direction = if side == PanelDock::Left { 1.0 } else { -1.0 };
            rail.width = (rail.width + dx * direction).clamp(180.0, 460.0);
            ctx.request_repaint();
        }
        if resize.hovered() || resize.dragged() {
            ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        ui.painter().line_segment(
            [resize_rect.center_top(), resize_rect.center_bottom()],
            Stroke::new(1.0_f32, PANEL_BORDER),
        );

        // Two panels stacked above/below each other on this rail render as a
        // vertical stack with a draggable divider instead of tabs.
        if visible.len() == 2 {
            if let Some(stack) = rail
                .stacked
                .filter(|s| visible.contains(&s.top) && visible.contains(&s.bottom()))
            {
                Self::dock_stacked_body(
                    ui,
                    ctx,
                    side,
                    rail,
                    stack,
                    headers,
                    drag,
                    ghost,
                    editor,
                    artboard_tool,
                    selection_tool,
                    artboards,
                    layers,
                    error,
                );
                return;
            }
        }

        let mut header_drag: Option<(PanelId, egui::Response)> = None;
        let header_frame = egui::Frame::NONE.fill(PANEL_RAISED).show(ui, |ui| {
            ui.set_height(26.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for &panel in &visible {
                    let active = rail.active == panel;
                    let response = ui.add_sized(
                        [86.0, 22.0],
                        egui::Button::new(
                            egui::RichText::new(Self::panel_label(panel))
                                .size(10.0)
                                .color(if active {
                                    Color32::from_gray(235)
                                } else {
                                    Color32::from_gray(160)
                                }),
                        )
                        .selected(active)
                        .frame(false)
                        .sense(egui::Sense::click_and_drag()),
                    );
                    if response.double_clicked() {
                        rail.collapsed = !rail.collapsed;
                    } else if response.clicked() {
                        rail.active = panel;
                        rail.collapsed = false;
                    }
                    if response.drag_started() || response.dragged() || response.drag_stopped() {
                        header_drag = Some((panel, response));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new("×").small().frame(false))
                        .on_hover_text("Close panel (Window menu restores it)")
                        .clicked()
                    {
                        match rail.active {
                            PanelId::Artboards => artboards.chrome.hidden = true,
                            PanelId::Layers => layers.chrome.hidden = true,
                        }
                    }
                    let glyph = if rail.collapsed {
                        "\u{25B8}"
                    } else {
                        "\u{25BE}"
                    };
                    if ui
                        .add(egui::Button::new(glyph).small().frame(false))
                        .on_hover_text("Collapse / expand this group")
                        .clicked()
                    {
                        rail.collapsed = !rail.collapsed;
                    }
                });
            });
        });
        headers.push(header_frame.response.rect);

        if let Some((panel, response)) = header_drag {
            Self::track_panel_drag(
                &response,
                panel,
                side,
                drag,
                ghost,
                Vec2::new(rail.width, 320.0),
            );
        }
        if rail.collapsed {
            return;
        }
        ui.separator();
        match rail.active {
            PanelId::Artboards => {
                Self::artboards_panel_body(ui, editor, artboard_tool, artboards, error)
            }
            PanelId::Layers => Self::layers_panel_body(ui, editor, selection_tool, layers, error),
        }
    }

    /// Renders a rail's two panels as a vertical stack: a header per panel,
    /// a draggable divider that repartitions the body height, and per-panel
    /// collapse. Dragging a header previews a move via `track_panel_drag`,
    /// exactly like the tabbed group's header.
    #[allow(clippy::too_many_arguments)]
    fn dock_stacked_body(
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        side: PanelDock,
        rail: &mut PanelRail,
        stack: StackedRail,
        headers: &mut Vec<EguiRect>,
        drag: &mut Option<PanelDrag>,
        ghost: &mut Option<(&'static str, Vec2)>,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        selection_tool: &mut SelectionTool,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        const HEADER: f32 = 26.0;
        const DIVIDER: f32 = 6.0;
        let width = ui.available_width();
        let body_area = (ui.available_height() - HEADER * 2.0 - DIVIDER).max(0.0);
        let (top_body, bottom_body) = match (stack.top_collapsed, stack.bottom_collapsed) {
            (true, true) => (0.0, 0.0),
            (true, false) => (0.0, body_area),
            (false, true) => (body_area, 0.0),
            (false, false) => (body_area * stack.split, body_area * (1.0 - stack.split)),
        };
        let mut next = stack;
        let mut dragged: Option<(PanelId, egui::Response)> = None;

        for (slot, panel, body_h, collapsed) in [
            (0usize, stack.top, top_body, stack.top_collapsed),
            (1usize, stack.bottom(), bottom_body, stack.bottom_collapsed),
        ] {
            let section = ui.allocate_ui_with_layout(
                Vec2::new(width, HEADER + body_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    let mut header = None;
                    let header_frame = egui::Frame::NONE.fill(PANEL_RAISED).show(ui, |ui| {
                        ui.set_height(HEADER);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            let response = ui.add_sized(
                                [110.0, 22.0],
                                egui::Button::new(
                                    egui::RichText::new(Self::panel_label(panel))
                                        .size(10.0)
                                        .color(Color32::from_gray(220)),
                                )
                                .frame(false)
                                .sense(egui::Sense::click_and_drag()),
                            );
                            let toggle = response.double_clicked();
                            header = Some(response);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(egui::Button::new("×").small().frame(false))
                                        .on_hover_text("Close panel (Window menu restores it)")
                                        .clicked()
                                    {
                                        match panel {
                                            PanelId::Artboards => artboards.chrome.hidden = true,
                                            PanelId::Layers => layers.chrome.hidden = true,
                                        }
                                    }
                                    let glyph = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
                                    let clicked = ui
                                        .add(egui::Button::new(glyph).small().frame(false))
                                        .on_hover_text("Collapse / expand this panel")
                                        .clicked();
                                    if clicked || toggle {
                                        if slot == 0 {
                                            next.top_collapsed = !next.top_collapsed;
                                        } else {
                                            next.bottom_collapsed = !next.bottom_collapsed;
                                        }
                                    }
                                },
                            );
                        });
                    });
                    if let Some(response) = header {
                        if response.drag_started() || response.dragged() || response.drag_stopped()
                        {
                            dragged = Some((panel, response));
                        }
                    }
                    if !collapsed {
                        ui.separator();
                        match panel {
                            PanelId::Artboards => Self::artboards_panel_body(
                                ui,
                                editor,
                                artboard_tool,
                                artboards,
                                error,
                            ),
                            PanelId::Layers => {
                                Self::layers_panel_body(ui, editor, selection_tool, layers, error)
                            }
                        }
                    }
                    header_frame.response.rect
                },
            );
            headers.push(section.inner);

            if slot == 0 {
                let (rect, resize) =
                    ui.allocate_exact_size(Vec2::new(width, DIVIDER), egui::Sense::drag());
                if resize.dragged() && body_area > 0.0 {
                    let dy = ui.input(|input| input.pointer.delta().y);
                    next.split = (next.split + dy / body_area)
                        .clamp(StackedRail::MIN_SPLIT, StackedRail::MAX_SPLIT);
                    ctx.request_repaint();
                }
                if resize.hovered() || resize.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
                }
                ui.painter().hline(
                    rect.x_range(),
                    rect.center().y,
                    Stroke::new(1.0_f32, PANEL_BORDER),
                );
            }
        }

        rail.stacked = Some(next);

        if let Some((panel, response)) = dragged {
            Self::track_panel_drag(&response, panel, side, drag, ghost, Vec2::new(width, 300.0));
        }
    }

    fn panel_label(panel: PanelId) -> &'static str {
        match panel {
            PanelId::Artboards => "ARTBOARDS",
            PanelId::Layers => "LAYERS",
        }
    }

    fn panel_title(panel: PanelId) -> &'static str {
        match panel {
            PanelId::Artboards => "Artboards",
            PanelId::Layers => "Layers",
        }
    }

    fn panel_key(panel: PanelId) -> &'static str {
        match panel {
            PanelId::Artboards => "artboards",
            PanelId::Layers => "layers",
        }
    }

    fn rail_mut(side: PanelDock, layout: &mut PanelLayout) -> &mut PanelRail {
        match side {
            PanelDock::Left => &mut layout.left,
            _ => &mut layout.right,
        }
    }

    /// Records/updates the live [`PanelDrag`] from a header (or float
    /// title-bar) response. Does NOT touch the dock layout — `panels_ui`
    /// previews and commits.
    fn track_panel_drag(
        response: &egui::Response,
        panel: PanelId,
        origin: PanelDock,
        drag: &mut Option<PanelDrag>,
        ghost: &mut Option<(&'static str, Vec2)>,
        ghost_size: Vec2,
    ) {
        let Some(cursor) = response.interact_pointer_pos() else {
            return;
        };
        if response.drag_started() {
            *drag = Some(PanelDrag {
                panel,
                origin,
                grab_offset: cursor - response.rect.left_top(),
                cursor,
                released: false,
            });
            *ghost = Some((Self::panel_title(panel), ghost_size));
        } else if let Some(active) = drag.as_mut().filter(|d| d.panel == panel) {
            active.cursor = cursor;
            active.released = response.drag_stopped();
            *ghost = Some((Self::panel_title(panel), ghost_size));
        }
    }

    /// Where a live drag would land if released now. Near a rail's edge or a
    /// gap between its groups it inserts a new group; over a group's tab
    /// strip it tabs in; near a bare screen edge it docks a new column on
    /// that side; anywhere else it floats. Pure — geometry in, decision out.
    #[allow(clippy::too_many_arguments)]
    fn resolve_drop(
        cursor: Pos2,
        grab_offset: Vec2,
        left_rect: Option<EguiRect>,
        left_headers: &[EguiRect],
        right_rect: Option<EguiRect>,
        right_headers: &[EguiRect],
        frame: EguiRect,
    ) -> DropTarget {
        const EDGE: f32 = 44.0;
        let near_left = cursor.x <= frame.left() + EDGE;
        let near_right = cursor.x >= frame.right() - EDGE;
        let over = |rect: Option<EguiRect>| rect.is_some_and(|r| r.expand(EDGE).contains(cursor));

        let (side, rect, headers) = if over(left_rect) || (near_left && left_rect.is_some()) {
            (PanelDock::Left, left_rect, left_headers)
        } else if over(right_rect) || (near_right && right_rect.is_some()) {
            (PanelDock::Right, right_rect, right_headers)
        } else if near_left {
            return DropTarget::TabInto {
                side: PanelDock::Left,
            };
        } else if near_right {
            return DropTarget::TabInto {
                side: PanelDock::Right,
            };
        } else {
            return DropTarget::Float {
                pos: cursor - grab_offset,
            };
        };

        let Some(rect) = rect else {
            return DropTarget::TabInto { side };
        };
        if headers.is_empty() {
            return DropTarget::TabInto { side };
        }

        if let Some((index, header)) = headers
            .iter()
            .enumerate()
            .find(|(_, h)| cursor.y >= h.top() && cursor.y <= h.bottom())
        {
            let t = (cursor.y - header.top()) / header.height().max(1.0);
            if t < 0.3 {
                return DropTarget::Rail { side, index };
            }
            if t > 0.7 {
                return DropTarget::Rail {
                    side,
                    index: index + 1,
                };
            }
            return DropTarget::TabInto { side };
        }

        let mut boundaries = vec![headers[0].top()];
        for pair in headers.windows(2) {
            boundaries.push((pair[0].bottom() + pair[1].top()) * 0.5);
        }
        boundaries.push(rect.bottom());
        let index = boundaries
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| (**a - cursor.y).abs().total_cmp(&(**b - cursor.y).abs()))
            .map(|(i, _)| i)
            .unwrap_or(0);
        DropTarget::Rail { side, index }
    }

    /// Commits a resolved [`DropTarget`] to the panel + rail state. Called
    /// once, on release.
    fn apply_drop(
        target: DropTarget,
        panel: PanelId,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        layout: &mut PanelLayout,
    ) {
        let other = match panel {
            PanelId::Artboards => PanelId::Layers,
            PanelId::Layers => PanelId::Artboards,
        };
        let set_dock =
            |a: &mut ArtboardsPanelState, l: &mut LayersPanelState, dock: PanelDock| match panel {
                PanelId::Artboards => a.chrome.dock = dock,
                PanelId::Layers => l.chrome.dock = dock,
            };
        match target {
            DropTarget::Float { pos } => set_dock(artboards, layers, PanelDock::Floating { pos }),
            DropTarget::TabInto { side } => {
                set_dock(artboards, layers, side);
                let rail = Self::rail_mut(side, layout);
                rail.stacked = None;
                rail.active = panel;
                rail.collapsed = false;
            }
            DropTarget::Rail { side, index } => {
                let other_here = {
                    let chrome = match other {
                        PanelId::Artboards => &artboards.chrome,
                        PanelId::Layers => &layers.chrome,
                    };
                    !chrome.hidden && chrome.dock == side
                };
                set_dock(artboards, layers, side);
                let rail = Self::rail_mut(side, layout);
                if other_here {
                    let split = rail
                        .stacked
                        .map(|s| s.split)
                        .unwrap_or(0.5)
                        .clamp(StackedRail::MIN_SPLIT, StackedRail::MAX_SPLIT);
                    let top = if index == 0 { panel } else { other };
                    rail.stacked = Some(StackedRail {
                        top,
                        split,
                        top_collapsed: false,
                        bottom_collapsed: false,
                    });
                } else {
                    rail.stacked = None;
                    rail.active = panel;
                }
                rail.collapsed = false;
            }
        }
    }

    /// The translucent proxy that follows the cursor while a docked panel is
    /// being dragged out (an already-floating panel moves for real instead).
    fn paint_drag_ghost(ctx: &egui::Context, top_left: Pos2, ghost: Option<(&'static str, Vec2)>) {
        let Some((label, size)) = ghost else {
            return;
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("panel_drag_ghost"),
        ));
        let rect = EguiRect::from_min_size(top_left, size);
        painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(43, 43, 43, 180));
        painter.rect_filled(
            EguiRect::from_min_size(rect.min, Vec2::new(rect.width(), 24.0)),
            4.0,
            Color32::from_rgba_unmultiplied(58, 58, 58, 210),
        );
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(18, 18, 18, 200)),
            egui::StrokeKind::Outside,
        );
        painter.text(
            rect.min + Vec2::new(8.0, 12.0),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(11.0),
            Color32::from_gray(215),
        );
    }

    /// The blue drop indicator: a full-width line at the target insertion
    /// boundary, a boxed group for a tab-in, or a full-height edge line for
    /// a new dock column — matching Illustrator's dock previews.
    fn paint_drop_indicator(
        ctx: &egui::Context,
        target: DropTarget,
        layout: &PanelLayout,
        frame: EguiRect,
    ) {
        const BLUE: Color32 = Color32::from_rgb(29, 122, 240);
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("panel_drop_indicator"),
        ));
        let rail_geom = |side: PanelDock| match side {
            PanelDock::Left => (layout.left_rect, layout.left_headers.as_slice()),
            _ => (layout.right_rect, layout.right_headers.as_slice()),
        };
        let edge_line = |side: PanelDock| {
            let x = match side {
                PanelDock::Left => frame.left() + 1.5,
                _ => frame.right() - 1.5,
            };
            painter.rect_filled(
                EguiRect::from_min_max(
                    Pos2::new(x - 1.5, frame.top()),
                    Pos2::new(x + 1.5, frame.bottom()),
                ),
                0.0,
                BLUE,
            );
        };
        match target {
            DropTarget::Float { .. } => {}
            DropTarget::TabInto { side } => {
                let (rect, headers) = rail_geom(side);
                match rect {
                    Some(r) => {
                        let band = headers.first().copied().unwrap_or_else(|| {
                            EguiRect::from_min_size(r.min, Vec2::new(r.width(), 26.0))
                        });
                        painter.rect_stroke(
                            band.expand(1.0),
                            2.0,
                            Stroke::new(2.0_f32, BLUE),
                            egui::StrokeKind::Outside,
                        );
                    }
                    None => edge_line(side),
                }
            }
            DropTarget::Rail { side, index } => {
                let (rect, headers) = rail_geom(side);
                let Some(r) = rect else {
                    edge_line(side);
                    return;
                };
                let y = if headers.is_empty() {
                    r.top() + 1.5
                } else if index == 0 {
                    headers[0].top()
                } else if index >= headers.len() {
                    r.bottom() - 1.5
                } else {
                    (headers[index - 1].bottom() + headers[index].top()) * 0.5
                };
                painter.rect_filled(
                    EguiRect::from_min_max(
                        Pos2::new(r.left(), y - 1.5),
                        Pos2::new(r.right(), y + 1.5),
                    ),
                    0.0,
                    BLUE,
                );
                for cx in [r.left() + 2.0, r.right() - 2.0] {
                    painter.rect_filled(
                        EguiRect::from_center_size(Pos2::new(cx, y), Vec2::splat(5.0)),
                        1.0,
                        BLUE,
                    );
                }
            }
        }
    }

    /// One floating panel: an in-app, app-styled overlay window (custom dark
    /// title bar, no OS chrome) that can be moved freely and dragged back
    /// onto a rail. Not an OS window — it lives inside the app frame.
    #[allow(clippy::too_many_arguments)]
    fn floating_panel_overlay(
        ctx: &egui::Context,
        panel: PanelId,
        drag: &mut Option<PanelDrag>,
        ghost: &mut Option<(&'static str, Vec2)>,
        editor: &mut Editor,
        artboard_tool: &mut ArtboardTool,
        selection_tool: &mut SelectionTool,
        artboards: &mut ArtboardsPanelState,
        layers: &mut LayersPanelState,
        error: &mut Option<String>,
    ) {
        let chrome = match panel {
            PanelId::Artboards => &artboards.chrome,
            PanelId::Layers => &layers.chrome,
        };
        let PanelDock::Floating { pos } = chrome.dock else {
            return;
        };
        if chrome.hidden {
            return;
        }
        let size = chrome.float_size;
        let mut close = false;

        egui::Window::new(Self::panel_title(panel))
            .id(egui::Id::new((
                "amalith_float_panel",
                Self::panel_key(panel),
            )))
            .title_bar(false)
            .resizable(false)
            .movable(false)
            .fixed_pos(pos)
            .default_width(size.x)
            .frame(
                egui::Frame::NONE
                    .fill(PANEL)
                    .stroke(Stroke::new(1.0_f32, PANEL_BORDER))
                    .inner_margin(egui::Margin::ZERO),
            )
            .show(ctx, |ui| {
                ui.set_width(size.x);
                let bar = egui::Frame::NONE
                    .fill(PANEL_RAISED)
                    .inner_margin(egui::Margin::symmetric(6, 3))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.set_width(size.x - 12.0);
                            if ui
                                .add(egui::Button::new("×").small().frame(false))
                                .on_hover_text("Close panel (Window menu restores it)")
                                .clicked()
                            {
                                close = true;
                            }
                            ui.label(
                                egui::RichText::new(Self::panel_title(panel))
                                    .size(10.0)
                                    .color(Color32::from_gray(220)),
                            );
                        });
                    });
                let bar = bar.response.interact(egui::Sense::click_and_drag());
                if bar.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                Self::track_panel_drag(&bar, panel, PanelDock::Floating { pos }, drag, ghost, size);
                egui::ScrollArea::vertical()
                    .id_salt(("amalith_float_body", Self::panel_key(panel)))
                    .max_height((size.y - 30.0).max(60.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(size.x);
                        match panel {
                            PanelId::Artboards => Self::artboards_panel_body(
                                ui,
                                editor,
                                artboard_tool,
                                artboards,
                                error,
                            ),
                            PanelId::Layers => {
                                Self::layers_panel_body(ui, editor, selection_tool, layers, error)
                            }
                        }
                    });
            });

        if close {
            match panel {
                PanelId::Artboards => artboards.chrome.hidden = true,
                PanelId::Layers => layers.chrome.hidden = true,
            }
        }
    }
}

impl eframe::App for AmalithApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        {
            self.process_native_menu_events();
            Self::title_bar_ui(ctx);
        }
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

/// Paint the supplied branding SVGs directly into egui's vector canvas. The
/// artwork deliberately stays in SVG form in `branding/SVG`; this small
/// reader supports the primitive shapes used by Amalith's tool glyphs and
/// avoids fuzzy bitmap icons on high-density displays.
fn paint_brand_tool_icon(painter: &egui::Painter, rect: EguiRect, icon: ToolIcon, active: bool) {
    let source = icon.svg();
    let outline = if active {
        Color32::from_gray(245)
    } else {
        Color32::from_rgb(188, 188, 190)
    };
    let dark_fill = if active {
        Color32::from_rgb(66, 101, 135)
    } else {
        Color32::from_rgb(57, 58, 57)
    };
    let direct_fill = if active {
        Color32::from_rgb(235, 241, 247)
    } else {
        Color32::from_rgb(188, 188, 190)
    };
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x / 100.0,
            rect.top() + rect.height() * y / 100.0,
        )
    };
    let stroke = |tag: &str| {
        let width = svg_attribute(tag, "stroke-width")
            .and_then(|value| value.strip_suffix("px"))
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(4.0);
        Stroke::new((width * rect.width() / 100.0).max(1.0), outline)
    };

    for tag in svg_tags(source, "polygon") {
        let points = svg_attribute(tag, "points")
            .map(svg_numbers)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| point(pair[0], pair[1]))
            .collect::<Vec<_>>();
        if points.len() >= 3 {
            let fill = if tag.contains("cls-3") || matches!(icon, ToolIcon::DirectSelection) {
                direct_fill
            } else {
                dark_fill
            };
            painter.add(egui::Shape::convex_polygon(points, fill, stroke(tag)));
        }
    }
    for tag in svg_tags(source, "rect") {
        let (Some(x), Some(y), Some(width), Some(height)) = (
            svg_number(tag, "x"),
            svg_number(tag, "y"),
            svg_number(tag, "width"),
            svg_number(tag, "height"),
        ) else {
            continue;
        };
        let shape = EguiRect::from_min_max(point(x, y), point(x + width, y + height));
        painter.rect_filled(shape, 0.0, dark_fill);
        painter.rect_stroke(shape, 0.0, stroke(tag), egui::StrokeKind::Middle);
    }
    for tag in svg_tags(source, "ellipse") {
        let (Some(cx), Some(cy), Some(rx), Some(ry)) = (
            svg_number(tag, "cx"),
            svg_number(tag, "cy"),
            svg_number(tag, "rx"),
            svg_number(tag, "ry"),
        ) else {
            continue;
        };
        let points = (0..24)
            .map(|step| {
                let theta = step as f32 / 24.0 * std::f32::consts::TAU;
                point(cx + rx * theta.cos(), cy + ry * theta.sin())
            })
            .collect();
        painter.add(egui::Shape::convex_polygon(points, dark_fill, stroke(tag)));
    }
    for tag in svg_tags(source, "circle") {
        let (Some(cx), Some(cy), Some(radius)) = (
            svg_number(tag, "cx"),
            svg_number(tag, "cy"),
            svg_number(tag, "r"),
        ) else {
            continue;
        };
        painter.circle_filled(point(cx, cy), radius * rect.width() / 100.0, outline);
    }
    for tag in svg_tags(source, "line") {
        let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
            svg_number(tag, "x1"),
            svg_number(tag, "y1"),
            svg_number(tag, "x2"),
            svg_number(tag, "y2"),
        ) else {
            continue;
        };
        painter.line_segment([point(x1, y1), point(x2, y2)], stroke(tag));
    }
}

/// Draws the Pen SVG at the exact pointer hotspot. egui only exposes system
/// cursor names, so a canvas-painted cursor is the way to use the supplied
/// vector artwork while keeping it sharp at every display scale.
fn paint_pen_cursor(painter: &egui::Painter, pointer: Pos2, closing_path: bool) {
    let size = 32.0;
    // The nib in the source artwork is at approximately (20, 14) in its
    // 100×100 view box. Pin it to the pointer rather than centering the
    // artwork, so clicking lands exactly at the apparent pen tip.
    let rect = EguiRect::from_min_size(
        pointer - Vec2::new(size * 0.2017, size * 0.1362),
        Vec2::splat(size),
    );
    let source = if closing_path {
        CURSOR_PEN_CLOSE_SVG
    } else {
        CURSOR_PEN_DRAWING_SVG
    };
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x / 100.0,
            rect.top() + rect.height() * y / 100.0,
        )
    };
    let dark = Color32::from_rgb(81, 83, 81);
    let stroke = |tag: &str| {
        let width = svg_attribute(tag, "stroke-width")
            .and_then(|value| value.strip_suffix("px"))
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(3.0);
        Stroke::new((width * rect.width() / 100.0).max(1.0), dark)
    };

    for tag in svg_tags(source, "polygon") {
        let points = svg_attribute(tag, "points")
            .map(svg_numbers)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| point(pair[0], pair[1]))
            .collect::<Vec<_>>();
        if points.len() >= 3 {
            painter.add(egui::Shape::convex_polygon(points, dark, stroke(tag)));
        }
    }
    for tag in svg_tags(source, "circle") {
        let (Some(cx), Some(cy), Some(radius)) = (
            svg_number(tag, "cx"),
            svg_number(tag, "cy"),
            svg_number(tag, "r"),
        ) else {
            continue;
        };
        let center = point(cx, cy);
        let radius = radius * rect.width() / 100.0;
        // Only the final circle in Pen-closeShape is the outline badge; the
        // other circle is part of the pen body and remains filled.
        if closing_path && tag.contains("cls-2") {
            painter.circle_stroke(center, radius, stroke(tag));
        } else {
            painter.circle_filled(center, radius, dark);
        }
    }
    for tag in svg_tags(source, "line") {
        let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
            svg_number(tag, "x1"),
            svg_number(tag, "y1"),
            svg_number(tag, "x2"),
            svg_number(tag, "y2"),
        ) else {
            continue;
        };
        painter.line_segment([point(x1, y1), point(x2, y2)], stroke(tag));
    }
}

fn paint_canvas_cursor(painter: &egui::Painter, cursor: CanvasCursor) {
    match cursor {
        CanvasCursor::Pen {
            pointer,
            closing_path,
        } => paint_pen_cursor(painter, pointer, closing_path),
        CanvasCursor::Selection { pointer, direct } => {
            paint_selection_cursor(painter, pointer, direct)
        }
    }
}

/// Paints the supplied V/A selection artwork with its pointed top vertex as
/// the cursor hotspot. The V arrow remains dark with a light outline; the A
/// arrow intentionally reverses that treatment, matching the SVGs.
fn paint_selection_cursor(painter: &egui::Painter, pointer: Pos2, direct: bool) {
    let size = 30.0;
    let hotspot = if direct {
        Vec2::new(size * 0.3431, size * 0.1652)
    } else {
        Vec2::new(size * 0.3082, size * 0.1798)
    };
    let rect = EguiRect::from_min_size(pointer - hotspot, Vec2::splat(size));
    let source = if direct {
        CURSOR_DIRECT_SELECTION_SVG
    } else {
        CURSOR_SELECTION_SVG
    };
    let fill = if direct {
        Color32::from_rgb(176, 175, 177)
    } else {
        Color32::from_rgb(81, 82, 81)
    };
    let outline = if direct {
        Color32::from_rgb(81, 82, 81)
    } else {
        Color32::from_rgb(176, 175, 177)
    };
    let point = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x / 100.0,
            rect.top() + rect.height() * y / 100.0,
        )
    };
    for tag in svg_tags(source, "polygon") {
        let points = svg_attribute(tag, "points")
            .map(svg_numbers)
            .unwrap_or_default()
            .chunks_exact(2)
            .map(|pair| point(pair[0], pair[1]))
            .collect::<Vec<_>>();
        if points.len() >= 3 {
            let width = svg_attribute(tag, "stroke-width")
                .and_then(|value| value.strip_suffix("px"))
                .and_then(|value| value.parse::<f32>().ok())
                .unwrap_or(3.6);
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                Stroke::new((width * size / 100.0).max(1.0), outline),
            ));
        }
    }
}

fn svg_tags<'a>(source: &'a str, element: &str) -> Vec<&'a str> {
    let needle = format!("<{element}");
    source
        .split(&needle)
        .skip(1)
        .filter_map(|rest| rest.split('>').next())
        .collect()
}

fn svg_attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("{name}=\"");
    tag.split_once(&marker)?
        .1
        .split_once('"')
        .map(|(value, _)| value)
}

fn svg_numbers(value: &str) -> Vec<f32> {
    value
        .split(|character: char| {
            !character.is_ascii_digit() && character != '.' && character != '-'
        })
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn svg_number(tag: &str, name: &str) -> Option<f32> {
    svg_attribute(tag, name)?
        .trim_end_matches("px")
        .parse()
        .ok()
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

fn path_is_closed(path: &PathData) -> bool {
    matches!(
        path.geometry.elements().last(),
        Some(amalith_core::geom::PathEl::ClosePath)
    )
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
    fn dock_resolve_drop_classifies_positions() {
        use egui::{pos2, vec2};
        let frame = EguiRect::from_min_max(pos2(0.0, 0.0), pos2(1000.0, 800.0));
        let right = EguiRect::from_min_max(pos2(780.0, 0.0), pos2(1000.0, 800.0));
        let headers = [EguiRect::from_min_max(pos2(780.0, 0.0), pos2(1000.0, 26.0))];
        let grab = vec2(10.0, 8.0);
        let r = |c| AmalithApp::resolve_drop(c, grab, None, &[], Some(right), &headers, frame);

        // Top quarter of the only header -> insert a new group above it.
        assert_eq!(
            r(pos2(880.0, 3.0)),
            DropTarget::Rail {
                side: PanelDock::Right,
                index: 0
            }
        );
        // Middle of the header -> tab into that group.
        assert_eq!(
            r(pos2(880.0, 13.0)),
            DropTarget::TabInto {
                side: PanelDock::Right
            }
        );
        // Well below the header but still over the rail -> new group at the bottom.
        assert_eq!(
            r(pos2(880.0, 700.0)),
            DropTarget::Rail {
                side: PanelDock::Right,
                index: 1
            }
        );
        // Far from any rail -> float at cursor minus the grab offset.
        assert_eq!(
            r(pos2(400.0, 400.0)),
            DropTarget::Float {
                pos: pos2(390.0, 392.0)
            }
        );
        // Near the bare left edge -> dock a column on the left (empty -> tab-in).
        assert_eq!(
            r(pos2(12.0, 400.0)),
            DropTarget::TabInto {
                side: PanelDock::Left
            }
        );
    }

    #[test]
    fn command_temporarily_activates_direct_selection_from_the_black_arrow() {
        assert!(temporary_direct_selection(true, true, false));
        assert!(!temporary_direct_selection(false, true, false));
        assert!(!temporary_direct_selection(true, false, false));
        assert!(!temporary_direct_selection(true, true, true));
    }

    #[test]
    fn tool_switch_is_exclusive() {
        let mut artboard = ArtboardTool::default();
        let mut rectangle = RectangleTool::default();
        let mut ellipse = EllipseTool::default();
        let mut pen = PenTool::default();
        let mut primitive = PrimitiveTool::default();
        let mut selection = SelectionTool::default();
        let mut direct_selection = DirectSelectionTool::default();

        activate_tool(
            ToolKind::Rectangle,
            &mut artboard,
            &mut rectangle,
            &mut ellipse,
            &mut pen,
            &mut primitive,
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
            &mut pen,
            &mut primitive,
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
            pen_tool: PenTool::default(),
            primitive_tool: PrimitiveTool::default(),
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
            pen_tool: PenTool::default(),
            primitive_tool: PrimitiveTool::default(),
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
                pen_tool: PenTool::default(),
                primitive_tool: PrimitiveTool::default(),
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
