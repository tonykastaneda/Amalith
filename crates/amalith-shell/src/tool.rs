//! The active canvas tool.

use crate::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    DirectSelect,
    Pen,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Artboard,
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Select,
        Tool::DirectSelect,
        Tool::Pen,
        Tool::Rectangle,
        Tool::RoundedRect,
        Tool::Ellipse,
        Tool::Polygon,
        Tool::Star,
        Tool::Artboard,
    ];

    /// A drag-a-box shape tool.
    pub fn is_shape(self) -> bool {
        matches!(
            self,
            Tool::Rectangle | Tool::RoundedRect | Tool::Ellipse | Tool::Polygon | Tool::Star
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Selection",
            Tool::DirectSelect => "Direct Selection",
            Tool::Pen => "Pen",
            Tool::Rectangle => "Rectangle",
            Tool::RoundedRect => "Rounded Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon",
            Tool::Star => "Star",
            Tool::Artboard => "Artboard",
        }
    }

    /// Illustrator-style single-key shortcut (empty = none).
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::DirectSelect => "A",
            Tool::Pen => "P",
            Tool::Rectangle => "M",
            Tool::Ellipse => "L",
            Tool::Artboard => "⇧O",
            _ => "",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tool::Select => Icon::Select,
            Tool::DirectSelect => Icon::DirectSelect,
            Tool::Pen => Icon::Pen,
            Tool::Rectangle => Icon::Rectangle,
            Tool::RoundedRect => Icon::RoundedRect,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Polygon => Icon::Polygon,
            Tool::Star => Icon::Star,
            Tool::Artboard => Icon::Artboard,
        }
    }
}
