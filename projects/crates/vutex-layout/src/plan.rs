//! Computed layout output: pages of positioned primitives in points.

/// One positioned text line.
///
/// `x` is the left edge of the line and `y` the text baseline. Layouts place
/// the baseline at `y = line_box_top + 0.8 * size`, so consumers that only
/// know CSS-style top offsets can recover the line box top.
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    /// Left edge in points, measured from the page left.
    pub x: f64,
    /// Text baseline in points, measured from the page top.
    pub y: f64,
    /// Font size in points.
    pub size: f64,
    /// Font family name such as `serif`, `sans-serif`, or `monospace`.
    pub font_family: String,
    /// Line text.
    pub text: String,
}

/// A straight stroked line, used for thematic breaks and table rules.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Start x in points.
    pub x1: f64,
    /// Start y in points.
    pub y1: f64,
    /// End x in points.
    pub x2: f64,
    /// End y in points.
    pub y2: f64,
    /// Stroke width in points.
    pub thickness: f64,
}

/// A closed box in page coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct Rect {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Box width in points.
    pub width: f64,
    /// Box height in points.
    pub height: f64,
    /// Filled boxes carry a light background, unfilled boxes only an outline.
    pub filled: bool,
}

/// Reserved area for an image whose intrinsic size is not known to layout.
#[derive(Debug, Clone, PartialEq)]
pub struct ImagePlaceholder {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Box width in points.
    pub width: f64,
    /// Box height in points.
    pub height: f64,
    /// Alternative text carried for accessibility and diagnostics.
    pub alt: String,
}

/// A positioned page primitive produced by the document layout pass.
///
/// All coordinates use points with the origin at the page's top-left corner.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutItem {
    /// A single text line.
    TextRun(TextRun),
    /// A stroked line.
    Rule(Rule),
    /// A box, either outlined or filled.
    Rect(Rect),
    /// An image slot reserved at a fixed size.
    ImagePlaceholder(ImagePlaceholder),
}

/// One laid-out page before presentation lowering.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPage {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Positioned primitives in paint order.
    pub items: Vec<LayoutItem>,
}

/// Computed layout output consumed by presentation builders.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPlan {
    /// Pages in document order.
    pub pages: Vec<LayoutPage>,
}
