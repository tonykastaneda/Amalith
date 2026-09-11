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
    Width,
    Arc,
    Spiral,
    FreeTransform,
    Join,
    ShapeBuilder,
    Eraser,
    VerticalText,
    AreaType,
    PathType,
    VerticalAreaType,
    VerticalPathType,
}

impl Tool {
    pub const ALL: [Tool; 32] = [
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
        Tool::Width,
        Tool::Arc,
        Tool::Spiral,
        Tool::FreeTransform,
        Tool::Join,
        Tool::ShapeBuilder,
        Tool::Eraser,
        Tool::VerticalText,
        Tool::AreaType,
        Tool::PathType,
        Tool::VerticalAreaType,
        Tool::VerticalPathType,
    ];

    /// A drag-a-box shape tool — the five that share the toolbar's Shape
    /// flyout slot. Arc has its own exact-size dialog too (see
    /// [`Self::has_exact_size_dialog`]) but isn't part of that flyout
    /// group, so it's deliberately excluded here.
    pub fn is_shape(self) -> bool {
        matches!(
            self,
            Tool::Rectangle | Tool::RoundedRect | Tool::Ellipse | Tool::Polygon | Tool::Star
        )
    }

    /// A plain click (no drag) with this tool pops an exact-size dialog
    /// instead of rubber-banding a shape.
    pub fn has_exact_size_dialog(self) -> bool {
        self.is_shape() || matches!(self, Tool::Arc | Tool::Spiral)
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
            Tool::Width => "Width",
            Tool::Arc => "Arc",
            Tool::Spiral => "Spiral",
            Tool::FreeTransform => "Free Transform",
            Tool::Join => "Join",
            Tool::ShapeBuilder => "Shape Builder",
            Tool::Eraser => "Eraser",
            Tool::VerticalText => "Vertical Type",
            Tool::AreaType => "Area Type",
            Tool::PathType => "Type on a Path",
            Tool::VerticalAreaType => "Vertical Area Type",
            Tool::VerticalPathType => "Vertical Type on a Path",
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
            Tool::Width => "⇧W",
            Tool::FreeTransform => "E",
            Tool::ShapeBuilder => "⇧M",
            Tool::Eraser => "⇧E",
            // Matching Illustrator's own Type flyout: only the plain Type
            // Tool has a default shortcut: the other five (Area/Path ×
            // horizontal/vertical) are flyout-only.
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
            Tool::Width => Icon::Width,
            Tool::Arc => Icon::Arc,
            Tool::Spiral => Icon::Spiral,
            Tool::FreeTransform => Icon::FreeTransform,
            Tool::Join => Icon::Join,
            Tool::ShapeBuilder => Icon::ShapeBuilder,
            Tool::Eraser => Icon::Eraser,
            Tool::VerticalText => Icon::VerticalText,
            Tool::AreaType => Icon::AreaType,
            Tool::PathType => Icon::PathType,
            Tool::VerticalAreaType => Icon::VerticalAreaType,
            Tool::VerticalPathType => Icon::VerticalPathType,
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
    Type,
}

impl ToolGroup {
    pub const ALL: [ToolGroup; 3] = [ToolGroup::RotateReflect, ToolGroup::ScaleShear, ToolGroup::Type];

    pub fn tools(self) -> &'static [Tool] {
        match self {
            ToolGroup::RotateReflect => &[Tool::Rotate, Tool::Reflect],
            ToolGroup::ScaleShear => &[Tool::Scale, Tool::Shear],
            // Matches Illustrator's own Type flyout order.
            ToolGroup::Type => &[
                Tool::Text,
                Tool::AreaType,
                Tool::PathType,
                Tool::VerticalText,
                Tool::VerticalAreaType,
                Tool::VerticalPathType,
            ],
        }
    }

    pub fn contains(self, t: Tool) -> bool {
        self.tools().contains(&t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Tool::ALL` is a hand-written fixed-size array with no compiler tie
    /// to the enum's variant list — see
    /// `01-prefaction-and-tool-all-sync-easy.md`. A forgotten variant here
    /// still compiles; the tool just quietly has no default shortcut, no
    /// toolbar slot, and never shows up in any `Tool::ALL`-indexed array
    /// (`Settings::tool_keys` chief among them). `covered` is built via an
    /// exhaustive match with no wildcard, so this test itself fails to
    /// *compile* — not just fails to pass — the moment a variant is added
    /// to `Tool` and forgotten here.
    #[test]
    fn tool_all_covers_every_variant_exactly_once() {
        fn covered(t: Tool) -> bool {
            match t {
                Tool::Select
                | Tool::DirectSelect
                | Tool::Pen
                | Tool::Line
                | Tool::Text
                | Tool::Rectangle
                | Tool::RoundedRect
                | Tool::Ellipse
                | Tool::Polygon
                | Tool::Star
                | Tool::Artboard
                | Tool::Hand
                | Tool::Zoom
                | Tool::Eyedropper
                | Tool::Gradient
                | Tool::Rotate
                | Tool::Reflect
                | Tool::Shear
                | Tool::Scale
                | Tool::Blend
                | Tool::Width
                | Tool::Arc
                | Tool::Spiral
                | Tool::FreeTransform
                | Tool::Join
                | Tool::ShapeBuilder
                | Tool::Eraser
                | Tool::VerticalText
                | Tool::AreaType
                | Tool::PathType
                | Tool::VerticalAreaType
                | Tool::VerticalPathType => true,
            }
        }
        for t in Tool::ALL {
            assert!(covered(t), "{t:?} missing from the exhaustive check above");
        }
        let mut seen: Vec<Tool> = Vec::new();
        for t in Tool::ALL {
            assert!(!seen.contains(&t), "{t:?} appears more than once in Tool::ALL");
            seen.push(t);
        }
    }

    /// Same guarantee for the smaller `ToolGroup::ALL`.
    #[test]
    fn tool_group_all_covers_every_variant_exactly_once() {
        fn covered(g: ToolGroup) -> bool {
            match g {
                ToolGroup::RotateReflect | ToolGroup::ScaleShear | ToolGroup::Type => true,
            }
        }
        for g in ToolGroup::ALL {
            assert!(covered(g), "{g:?} missing from the exhaustive check above");
        }
        let mut seen: Vec<ToolGroup> = Vec::new();
        for g in ToolGroup::ALL {
            assert!(!seen.contains(&g), "{g:?} appears more than once in ToolGroup::ALL");
            seen.push(g);
        }
    }
}
