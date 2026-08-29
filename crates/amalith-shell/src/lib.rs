//! Amalith's custom UI shell.
//!
//! No widget toolkit. The stack is winit (windows + input), wgpu + vello
//! (GPU 2D rendering, for both chrome and artwork), and the modules here:
//!
//! - [`dock`]   — the pure layout-tree model: splits, tab groups, detach,
//!               dock, hit-testing. No rendering, fully unit-tested.
//! - [`panel`]  — the [`Panel`](panel::Panel) trait every dockable pane
//!               implements, and the registry that maps a [`PanelId`] to
//!               its instance.
//!
//! The binary (`main.rs`) owns the winit event loop and the per-window
//! render state, and drives these modules.

pub mod dock;
pub mod panel;

pub use dock::{Axis, Child, DockModel, DropTarget, Floating, Node, NodePath, PanelId, Side};
pub use panel::{Key, Mods, Panel, PanelEvent, PanelRegistry, PointerButton};
