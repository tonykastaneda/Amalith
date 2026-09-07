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
    Eyedropper,
    Gradient,
    Rotate,
    Reflect,
    Shear,
    Scale,
    Blend,
}

impl Tool {
    pub const ALL: [Tool; 20] = [
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
        Tool::Eyedropper,
        Tool::Gradient,
        Tool::Rotate,
        Tool::Reflect,
        Tool::Shear,
        Tool::Scale,
        Tool::Blend,
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
            Tool::Eyedropper => "Eyedropper",
            Tool::Gradient => "Gradient",
            Tool::Rotate => "Rotate",
            Tool::Reflect => "Reflect",
            Tool::Shear => "Shear",
            Tool::Scale => "Scale",
            Tool::Blend => "Blend",
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
            Tool::Eyedropper => "I",
            Tool::Gradient => "G",
            Tool::Rotate => "R",
            Tool::Reflect => "O",
            Tool::Scale => "S",
            Tool::Blend => "W",
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
            Tool::Eyedropper => Icon::Eyedropper,
            Tool::Gradient => Icon::Gradient,
            Tool::Rotate => Icon::Rotate,
            Tool::Reflect => Icon::Reflect,
            Tool::Shear => Icon::Shear,
            Tool::Scale => Icon::Scale,
            Tool::Blend => Icon::Blend,
        }
    }
}

/// A press-and-hold flyout group of related tools in the Tools panel —
/// same idea as the primitive-shape slot, just rendered as a labeled list
/// (icon + name + shortcut) like Illustrator's own tool flyouts, since
/// these hold more than interchangeable shape variants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolGroup {
    RotateReflect,
    ScaleShear,
}

impl ToolGroup {
    pub const ALL: [ToolGroup; 2] = [ToolGroup::RotateReflect, ToolGroup::ScaleShear];

    pub fn tools(self) -> &'static [Tool] {
        match self {
            ToolGroup::RotateReflect => &[Tool::Rotate, Tool::Reflect],
            ToolGroup::ScaleShear => &[Tool::Scale, Tool::Shear],
        }
    }

    pub fn contains(self, t: Tool) -> bool {
        self.tools().contains(&t)
    }
}
