//! The active canvas tool.

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

    /// One-letter badge, Illustrator-style.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Rectangle => "M",
            Tool::Ellipse => "L",
        }
    }
}
