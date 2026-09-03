//! The active canvas tool.

use crate::icons::Icon;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    DirectSelect,
    Pen,
    Line,
    Text,
    Rectangle,
    RoundedRect,
    Ellipse,
    Polygon,
    Star,
    Artboard,
    Hand,
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 13] = [
        Tool::Select,
        Tool::DirectSelect,
        Tool::Pen,
        Tool::Line,
        Tool::Text,
        Tool::Rectangle,
        Tool::RoundedRect,
        Tool::Ellipse,
        Tool::Polygon,
        Tool::Star,
        Tool::Artboard,
        Tool::Hand,
        Tool::Zoom,
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
            Tool::Line => "Line Segment",
            Tool::Text => "Type",
            Tool::Rectangle => "Rectangle",
            Tool::RoundedRect => "Rounded Rectangle",
            Tool::Ellipse => "Ellipse",
            Tool::Polygon => "Polygon",
            Tool::Star => "Star",
            Tool::Artboard => "Artboard",
            Tool::Hand => "Hand",
            Tool::Zoom => "Zoom",
        }
    }

    /// Illustrator-style single-key shortcut (empty = none).
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::DirectSelect => "A",
            Tool::Pen => "P",
            Tool::Line => "\\",
            Tool::Text => "T",
            Tool::Rectangle => "M",
            Tool::Ellipse => "L",
            Tool::Artboard => "⇧O",
            Tool::Hand => "H",
            Tool::Zoom => "Z",
            _ => "",
        }
    }

    pub fn icon(self) -> Icon {
        match self {
            Tool::Select => Icon::Select,
            Tool::DirectSelect => Icon::DirectSelect,
            Tool::Pen => Icon::Pen,
            Tool::Line => Icon::Line,
            Tool::Text => Icon::Text,
            Tool::Rectangle => Icon::Rectangle,
            Tool::RoundedRect => Icon::RoundedRect,
            Tool::Ellipse => Icon::Ellipse,
            Tool::Polygon => Icon::Polygon,
            Tool::Star => Icon::Star,
            Tool::Artboard => Icon::Artboard,
            Tool::Hand => Icon::Hand,
            Tool::Zoom => Icon::Zoom,
        }
    }
}
