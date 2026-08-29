//! The active canvas tool.

use crate::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Pen,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
}

impl Tool {
    pub const ALL: [Tool; 7] = [
        Tool::Select,
        Tool::Pen,
        Tool::Rectangle,
        Tool::RoundedRect,
        Tool::Ellipse,
        Tool::Polygon,
        Tool::Star,
    ];

    /// A drag-a-box shape tool (everything but Select and Pen).
    pub fn is_shape(self) -> bool {
        !matches!(self, Tool::Select | Tool::Pen)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selection",
            Tool::Pen => "Pen",
            Tool::Rectangle => "Rectangle",
            Tool::RoundedRect => "Rounded Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon",
            Tool::Star => "Star",
        }
    }

    /// Illustrator-style single-key shortcut (empty = none).
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Pen => "P",
            Tool::Rectangle => "M",
            Tool::Ellipse => "L",
            _ => "",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tool::Select => Icon::Select,
            Tool::Pen => Icon::Pen,
            Tool::Rectangle => Icon::Rectangle,
            Tool::RoundedRect => Icon::RoundedRect,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Polygon => Icon::Polygon,
            Tool::Star => Icon::Star,
        }
    }
}
