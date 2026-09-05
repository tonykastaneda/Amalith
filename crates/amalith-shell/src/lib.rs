//! Amalith's custom UI shell.
//!
//! No widget toolkit. The stack is winit (windows + input), wgpu + vello
//! (GPU 2D rendering, for both chrome and artwork), and the modules here:
//!
//! - [`app`]    — the whole application: `App` state, the winit event loop,
//!               input routing ([`app::input`]), and rendering
//!               ([`app::render`]). [`app::run`] is the entry point.
//! - [`dock`]   — the pure layout-tree model: splits, tab groups, detach,
//!               dock. No rendering, fully unit-tested.
//! - [`layout`] — turns a [`dock::Node`] tree + a rect into concrete
//!               rectangles, and a rect + cursor into a [`dock::DropTarget`]
//!               ([`layout::hit_test`]). Also pure.
//! - [`theme`]  — colors and metrics for the chrome.
//! - [`chrome`] — draws a [`layout::Layout`] into a vello scene, plus the
//!               drop indicator.
//! - [`panels`] — the dockable panel bodies (Tools, Layers, Artboards,
//!               Character, Swatches). Dispatched by string [`PanelId`] in
//!               `panels::{paint, hit, tip}`; each is a stateless renderer
//!               of a `Ctx` that returns a `panels::Action` the shell
//!               applies.
//! - [`context_bar`] — the control bar, assembled the same way from a
//!               priority-ordered list of self-contained segments.
//!
//! [`main.rs`](../../main.rs) is a launcher that calls [`app::run`].

pub mod about;
pub mod anchors;
pub mod app;
pub mod appicon;
pub mod canvas;
pub mod chrome;
pub mod colormanage;
pub mod confirm_close;
pub mod context_bar;
pub mod convert;
pub mod dock;
pub mod export;
pub mod handles;
pub mod home;
pub mod icons;
#[cfg(target_os = "macos")]
pub mod imageio;
pub mod keymap;
pub mod layout;
pub mod lod;
#[cfg(target_os = "macos")]
pub mod macdrop;
pub mod newdoc;
pub mod palette;
pub mod panel_icon;
pub mod panels;
pub mod pdfexport;
pub mod picker;
pub mod prefs;
pub mod recent;
pub mod rulers;
pub mod sample;
pub mod scripts;
pub mod scroll_view;
pub mod select;
pub mod shapedialog;
pub mod settings;
pub mod stroke_panel;
pub mod text;
pub mod text_field;
pub mod textedit;
pub mod thread;
pub mod theme;
pub mod tool;
pub mod workspace;
pub mod workspace_dialog;
pub mod workspaces;

pub use dock::{DockModel, Group, Master, MasterKind, MasterLayout, PanelId, Side, ToolsDensity};
pub use layout::{GroupArea, GroupDrop, MasterFrame, PanelDrop, PanelRow, TabRect};
pub use theme::Theme;
