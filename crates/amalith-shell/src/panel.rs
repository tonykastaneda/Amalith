//! Panels and the registry that owns them.
//!
//! A panel is a self-contained piece of UI (Artboards, Layers, later the
//! tools bar, swatches, properties…). The dock tree in [`crate::dock`]
//! only ever refers to panels by [`PanelId`]; this module is where an id
//! becomes something that draws and takes input.
//!
//! The shell never hard-codes a panel. It asks the [`PanelRegistry`] for
//! the panel behind an id and calls [`Panel::paint`] / [`Panel::event`].
//! Adding a panel is: implement the trait, `register` it once at startup.

use std::collections::HashMap;

use vello::kurbo::Rect;
use vello::Scene;

use crate::dock::PanelId;

/// A single dockable panel.
///
/// Implementors hold their own state. `paint` and `event` receive the
/// panel's content rect in logical points (tab strip excluded — the chrome
/// renderer draws that). Coordinates in [`PanelEvent`] are already
/// panel-local.
pub trait Panel {
    /// Stable id. Must match whatever the registry is keyed by and match
    /// the ids used to build the dock tree.
    fn id(&self) -> PanelId;

    /// Human-readable, shown on the tab.
    fn title(&self) -> &str;

    /// Draw the panel body into `scene`, clipped to `bounds` by the caller.
    fn paint(&mut self, scene: &mut Scene, bounds: Rect);

    /// Handle one input event that fell inside this panel. Return `true` if
    /// it was consumed (stops fall-through to the canvas / chrome).
    fn event(&mut self, _event: &PanelEvent) -> bool {
        false
    }

    /// Called once per frame before `paint`, for animation / async polling.
    fn tick(&mut self, _dt: f32) {}
}

/// Input delivered to a panel, in panel-local logical points.
#[derive(Clone, Debug, PartialEq)]
pub enum PanelEvent {
    PointerDown {
        x: f32,
        y: f32,
        button: PointerButton,
    },
    PointerUp {
        x: f32,
        y: f32,
        button: PointerButton,
    },
    PointerMove {
        x: f32,
        y: f32,
    },
    Scroll {
        dx: f32,
        dy: f32,
    },
    Key {
        key: Key,
        pressed: bool,
        mods: Mods,
    },
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Backspace,
    Delete,
    Tab,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Space,
    Char(char),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Cmd on macOS, Super elsewhere.
    pub meta: bool,
}

/// Owns every panel instance, looked up by id.
#[derive(Default)]
pub struct PanelRegistry {
    panels: HashMap<&'static str, Box<dyn Panel>>,
}

impl PanelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a panel. Panics if its id is already taken — ids are a
    /// compile-time-ish contract, a clash is a bug.
    pub fn register(&mut self, panel: Box<dyn Panel>) {
        let PanelId(key) = panel.id();
        if self.panels.insert(key, panel).is_some() {
            panic!("panel id {key:?} registered twice");
        }
    }

    pub fn get_mut(&mut self, id: PanelId) -> Option<&mut (dyn Panel + 'static)> {
        self.panels.get_mut(id.0).map(|b| b.as_mut())
    }

    pub fn contains(&self, id: PanelId) -> bool {
        self.panels.contains_key(id.0)
    }

    pub fn ids(&self) -> impl Iterator<Item = PanelId> + '_ {
        self.panels.keys().copied().map(PanelId)
    }

    pub fn title(&self, id: PanelId) -> Option<&str> {
        self.panels.get(id.0).map(|p| p.title())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Stub(PanelId, &'static str);
    impl Panel for Stub {
        fn id(&self) -> PanelId {
            self.0
        }
        fn title(&self) -> &str {
            self.1
        }
        fn paint(&mut self, _scene: &mut Scene, _bounds: Rect) {}
    }

    #[test]
    fn register_and_look_up_by_id() {
        let mut reg = PanelRegistry::new();
        reg.register(Box::new(Stub(PanelId("layers"), "Layers")));
        assert!(reg.contains(PanelId("layers")));
        assert_eq!(reg.title(PanelId("layers")), Some("Layers"));
        assert!(reg.get_mut(PanelId("layers")).is_some());
        assert!(reg.get_mut(PanelId("missing")).is_none());
    }

    #[test]
    #[should_panic(expected = "registered twice")]
    fn duplicate_id_panics() {
        let mut reg = PanelRegistry::new();
        reg.register(Box::new(Stub(PanelId("dup"), "One")));
        reg.register(Box::new(Stub(PanelId("dup"), "Two")));
    }
}
