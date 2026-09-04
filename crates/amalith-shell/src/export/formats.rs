//! The Formats table — one row per output (a scale, a filename suffix, a
//! file format). "+ Add Scale" appends a row; the × on a row removes it.
//! Pure data; [`super`] lays it out and paints it.

/// Output file format for a row.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpg,
    Svg,
    Pdf,
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Png, Format::Jpg, Format::Svg, Format::Pdf];

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpg => "JPG",
            Format::Svg => "SVG",
            Format::Pdf => "PDF",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpg => "jpg",
            Format::Svg => "svg",
            Format::Pdf => "pdf",
        }
    }

    /// Vector formats ignore the scale factor.
    pub fn is_vector(self) -> bool {
        matches!(self, Format::Svg | Format::Pdf)
    }
}

/// The scale multipliers offered in the Scale dropdown.
pub const SCALES: [f64; 6] = [0.5, 1.0, 1.5, 2.0, 3.0, 4.0];

pub fn scale_label(s: f64) -> String {
    if (s - s.round()).abs() < 1e-6 {
        format!("{}x", s.round() as i64)
    } else {
        format!("{s}x")
    }
}

/// One row of the table.
#[derive(Clone, Debug)]
pub struct Row {
    pub scale: f64,
    /// Filename suffix, e.g. `@2x`. Empty = none.
    pub suffix: String,
    pub format: Format,
}

impl Row {
    pub fn png_1x() -> Self {
        Self {
            scale: 1.0,
            suffix: String::new(),
            format: Format::Png,
        }
    }
}

/// The default set for a fresh dialog: a single PNG @1x.
pub fn defaults() -> Vec<Row> {
    vec![Row::png_1x()]
}

/// Whether any row emits a PDF — gates the "Export PDFs as" control.
pub fn any_pdf(rows: &[Row]) -> bool {
    rows.iter().any(|r| r.format == Format::Pdf)
}
