//! The active canvas tool.

use crate::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Rectangle,
    Ellipse,
}

impl Tool {
    pub const ALL: [Tool; 3] = [Tool::Select, Tool::Rectangle, Tool::Ellipse];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selection",
            Tool::Rectangle => "Rectangle",
            Tool::Ellipse => "Ellipse",
        }
    }

    /// Illustrator-style single-key shortcut.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Rectangle => "M",
            Tool::Ellipse => "L",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tool::Select => Icon::Select,
            Tool::Rectangle => Icon::Rectangle,
            Tool::Ellipse => Icon::Ellipse,
        }
    }
}
