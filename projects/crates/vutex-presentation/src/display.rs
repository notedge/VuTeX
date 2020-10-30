/// One page in a fixed-geometry display presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayPage {
    pub width: f64,
    pub height: f64,
}

/// Fixed-geometry presentation for PDF, SVG, and raster backends.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayPresentation {
    pub pages: Vec<DisplayPage>,
}
