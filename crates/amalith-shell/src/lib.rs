//! Amalith's custom UI shell.
//!
//! No widget toolkit. The stack is winit (windows + input), wgpu + vello
//! (GPU 2D rendering, for both chrome and artwork), and the modules here:
//!
//! - [`dock`]   — the pure layout-tree model: splits, tab groups, detach,
//!               dock. No rendering, fully unit-tested.
//! - [`layout`] — turns a [`dock::Node`] tree + a rect into concrete
//!               rectangles, and a rect + cursor into a [`dock::DropTarget`]
//!               ([`layout::hit_test`]). Also pure.
//! - [`theme`]  — colors and metrics for the chrome.
//! - [`chrome`] — draws a [`layout::Layout`] into a vello scene, plus the
//!               blue drop indicator.
//! - [`panel`]  — the [`Panel`](panel::Panel) trait every dockable pane
//!               implements, and the registry mapping [`PanelId`] to an
//!               instance.
//!
//! The binary (`main.rs`) owns the winit event loop and per-window render
//! state, and drives these modules.

pub mod canvas;
pub mod chrome;
pub mod convert;
pub mod dock;
pub mod handles;
pub mod icons;
pub mod layout;
pub mod panel;
pub mod panels;
pub mod sample;
pub mod select;
pub mod text;
pub mod theme;
pub mod tool;

pub use dock::{Axis, Child, DockModel, DropTarget, Floating, Node, NodePath, PanelId, Side};
pub use layout::{hit_test, Layout, PanelArea, SplitterHandle, TabRect};
pub use panel::{Key, Mods, Panel, PanelEvent, PanelRegistry, PointerButton};
pub use theme::Theme;
