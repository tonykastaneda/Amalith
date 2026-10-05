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
    MagicWand,
    RasterMarquee,
    RasterEllipse,
    RasterLasso,
    RasterBrush,
    RasterEraser,
    RasterFill,
    RasterCloneStamp,
    AddAnchor,
    DeleteAnchor,
    AnchorPoint,
    Curvature,
    Scissors,
    GroupSelect,
    Reshape,
    RectangularGrid,
    PolarGrid,
    Warp,
    Twirl,
    Pucker,
    Bloat,
    Scallop,
    Crystallize,
    Wrinkle,
}

impl Tool {
    pub fn is_raster_selection(self) -> bool {
        matches!(self, Tool::RasterMarquee | Tool::RasterEllipse | Tool::RasterLasso)
    }
    pub fn is_raster_tool(self) -> bool {
        self.is_raster_selection()
            || matches!(self, Tool::RasterBrush | Tool::RasterEraser | Tool::RasterFill | Tool::RasterCloneStamp)
    }
    pub const ALL: [Tool; 56] = [
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
        Tool::MagicWand,
        Tool::RasterMarquee,
        Tool::RasterEllipse,
        Tool::RasterLasso,
        Tool::RasterBrush,
        Tool::RasterEraser,
        Tool::RasterFill,
        Tool::RasterCloneStamp,
        Tool::AddAnchor,
        Tool::DeleteAnchor,
        Tool::AnchorPoint,
        Tool::Curvature,
        Tool::Scissors,
        Tool::GroupSelect,
        Tool::Reshape,
        Tool::RectangularGrid,
        Tool::PolarGrid,
        Tool::Warp,
        Tool::Twirl,
        Tool::Pucker,
        Tool::Bloat,
        Tool::Scallop,
        Tool::Crystallize,
        Tool::Wrinkle,
    ];

    /// The liquify brush this tool paints with, for the Warp flyout's
    /// seven tools.
    pub fn liquify_kind(self) -> Option<amalith_core::liquify::LiquifyKind> {
        use amalith_core::liquify::LiquifyKind as K;
        Some(match self {
            Tool::Warp => K::Warp,
            Tool::Twirl => K::Twirl,
            Tool::Pucker => K::Pucker,
            Tool::Bloat => K::Bloat,
            Tool::Scallop => K::Scallop,
            Tool::Crystallize => K::Crystallize,
            Tool::Wrinkle => K::Wrinkle,
            _ => return None,
        })
    }

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
        self.is_shape() || matches!(self, Tool::Arc | Tool::Spiral | Tool::RectangularGrid | Tool::PolarGrid)
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
            Tool::MagicWand => "Magic Wand",
            Tool::RasterMarquee => "Rectangular Marquee",
            Tool::RasterEllipse => "Elliptical Marquee",
            Tool::RasterLasso => "Lasso",
            Tool::RasterBrush => "Brush",
            Tool::RasterEraser => "Pixel Eraser",
            Tool::RasterFill => "Paint Bucket",
            Tool::RasterCloneStamp => "Clone Stamp",
            Tool::AddAnchor => "Add Anchor Point",
            Tool::DeleteAnchor => "Delete Anchor Point",
            Tool::AnchorPoint => "Anchor Point",
            Tool::Curvature => "Curvature",
            Tool::Scissors => "Scissors",
            Tool::GroupSelect => "Group Selection",
            Tool::Reshape => "Reshape",
            Tool::RectangularGrid => "Rectangular Grid",
            Tool::PolarGrid => "Polar Grid",
            Tool::Warp => "Warp",
            Tool::Twirl => "Twirl",
            Tool::Pucker => "Pucker",
            Tool::Bloat => "Bloat",
            Tool::Scallop => "Scallop",
            Tool::Crystallize => "Crystallize",
            Tool::Wrinkle => "Wrinkle",
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
            Tool::MagicWand => "Y",
            Tool::RasterMarquee => "M",
            Tool::RasterEllipse => "⇧M",
            Tool::RasterLasso => "L",
            Tool::RasterBrush => "B",
            Tool::RasterEraser => "⇧E",
            Tool::RasterFill => "K",
            Tool::RasterCloneStamp => "C",
            Tool::AddAnchor => "+",
            Tool::DeleteAnchor => "-",
            Tool::AnchorPoint => "⇧C",
            Tool::Curvature => "⇧~",
            Tool::Scissors => "C",
            Tool::Warp => "⇧R",
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
            Tool::MagicWand => Icon::MagicWand,
            Tool::RasterMarquee => Icon::RasterMarquee,
            Tool::RasterEllipse => Icon::RasterEllipse,
            Tool::RasterLasso => Icon::Lasso,
            Tool::RasterBrush => Icon::Paintbrush,
            Tool::RasterEraser => Icon::Eraser,
            Tool::RasterFill => Icon::PaintBucket,
            Tool::RasterCloneStamp => Icon::CloneStamp,
            Tool::AddAnchor => Icon::AddAnchor,
            Tool::DeleteAnchor => Icon::DeleteAnchor,
            Tool::AnchorPoint => Icon::AnchorPoint,
            Tool::Curvature => Icon::CurvaturePen,
            Tool::Scissors => Icon::Scissors,
            Tool::GroupSelect => Icon::GroupSelect,
            Tool::Reshape => Icon::Reshape,
            Tool::RectangularGrid => Icon::RectangularGrid,
            Tool::PolarGrid => Icon::PolarGrid,
            Tool::Warp => Icon::Warp,
            Tool::Twirl => Icon::Twirl,
            Tool::Pucker => Icon::Pucker,
            Tool::Bloat => Icon::Bloat,
            Tool::Scallop => Icon::Scallop,
            Tool::Crystallize => Icon::Crystallize,
            Tool::Wrinkle => Icon::Wrinkle,
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
    Pen,
    DirectSelect,
    Eraser,
    Grid,
    Width,
}

impl ToolGroup {
    pub const ALL: [ToolGroup; 8] = [
        ToolGroup::RotateReflect,
        ToolGroup::ScaleShear,
        ToolGroup::Type,
        ToolGroup::Pen,
        ToolGroup::DirectSelect,
        ToolGroup::Eraser,
        ToolGroup::Grid,
        ToolGroup::Width,
    ];

    pub fn tools(self) -> &'static [Tool] {
        match self {
            ToolGroup::RotateReflect => &[Tool::Rotate, Tool::Reflect],
            ToolGroup::ScaleShear => &[Tool::Scale, Tool::Shear, Tool::Reshape],
            // Matches Illustrator's own Type flyout order.
            ToolGroup::Type => &[
                Tool::Text,
                Tool::AreaType,
                Tool::PathType,
                Tool::VerticalText,
                Tool::VerticalAreaType,
                Tool::VerticalPathType,
            ],
            // Illustrator's Pen flyout (Curvature has its own slot).
            ToolGroup::Pen => &[Tool::Pen, Tool::AddAnchor, Tool::DeleteAnchor, Tool::AnchorPoint],
            ToolGroup::DirectSelect => &[Tool::DirectSelect, Tool::GroupSelect],
            ToolGroup::Eraser => &[Tool::Eraser, Tool::Scissors],
            ToolGroup::Grid => &[Tool::RectangularGrid, Tool::PolarGrid],
            // Illustrator's Width flyout: Width plus the liquify brushes.
            ToolGroup::Width => &[
                Tool::Width,
                Tool::Warp,
                Tool::Twirl,
                Tool::Pucker,
                Tool::Bloat,
                Tool::Scallop,
                Tool::Crystallize,
                Tool::Wrinkle,
            ],
        }
    }

    pub fn contains(self, t: Tool) -> bool {
        self.tools().contains(&t)
    }

    /// The group `t` belongs to, if any.
    pub fn of(t: Tool) -> Option<ToolGroup> {
        ToolGroup::ALL.into_iter().find(|g| g.contains(t))
    }
}

/// Which tool each flyout group's toolbar slot currently shows — the one
/// last picked from it, Illustrator-style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GroupTools([Tool; ToolGroup::ALL.len()]);

impl Default for GroupTools {
    fn default() -> Self {
        Self(ToolGroup::ALL.map(|g| g.tools()[0]))
    }
}

impl GroupTools {
    pub fn get(&self, group: ToolGroup) -> Tool {
        self.0[group as usize]
    }

    /// Remembers `t` as its group's current tool (no-op outside a group).
    pub fn remember(&mut self, t: Tool) {
        if let Some(g) = ToolGroup::of(t) {
            self.0[g as usize] = t;
        }
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
                | Tool::VerticalPathType
                | Tool::MagicWand
                | Tool::RasterMarquee
                | Tool::RasterEllipse
                | Tool::RasterLasso => true,
                Tool::RasterBrush | Tool::RasterEraser | Tool::RasterFill | Tool::RasterCloneStamp => true,
                Tool::AddAnchor
                | Tool::DeleteAnchor
                | Tool::AnchorPoint
                | Tool::Curvature
                | Tool::Scissors
                | Tool::GroupSelect
                | Tool::Reshape => true,
                Tool::RectangularGrid | Tool::PolarGrid => true,
                Tool::Warp
                | Tool::Twirl
                | Tool::Pucker
                | Tool::Bloat
                | Tool::Scallop
                | Tool::Crystallize
                | Tool::Wrinkle => true,
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
                ToolGroup::RotateReflect
                | ToolGroup::ScaleShear
                | ToolGroup::Type
                | ToolGroup::Pen
                | ToolGroup::DirectSelect
                | ToolGroup::Eraser
                | ToolGroup::Grid
                | ToolGroup::Width => true,
            }
        }
        for g in ToolGroup::ALL {
            assert!(covered(g), "{g:?} missing from the exhaustive check above");
            // `GroupTools` indexes by discriminant.
            assert_eq!(ToolGroup::ALL[g as usize], g);
        }
        let mut seen: Vec<ToolGroup> = Vec::new();
        for g in ToolGroup::ALL {
            assert!(!seen.contains(&g), "{g:?} appears more than once in ToolGroup::ALL");
            seen.push(g);
        }
    }
}
