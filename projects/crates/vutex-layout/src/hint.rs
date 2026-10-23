//! Page geometry and body metrics expressed as layout intent.

/// A4 portrait page width in points (210 mm).
pub const A4_WIDTH: f64 = 595.276;
/// A4 portrait page height in points (297 mm).
pub const A4_HEIGHT: f64 = 841.89;
/// Default inward margin in points (1 inch = 72 pt).
pub const DEFAULT_MARGIN: f64 = 72.0;

/// Inward page margins in points, measured from each page edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Margin {
    /// Top margin in points.
    pub top: f64,
    /// Right margin in points.
    pub right: f64,
    /// Bottom margin in points.
    pub bottom: f64,
    /// Left margin in points.
    pub left: f64,
}

impl Margin {
    /// Same margin on all four sides.
    pub const fn uniform(value: f64) -> Self {
        Self { top: value, right: value, bottom: value, left: value }
    }
}

/// Layout intent expressed by source languages or style rules.
///
/// The hint fixes page geometry and body metrics; the document layout pass
/// fills pages inside the margins. [`Default`] is A4 portrait with 72 pt
/// margins and 11 pt body text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutHint {
    /// Page width in points.
    pub page_width: f64,
    /// Page height in points.
    pub page_height: f64,
    /// Inward page margins.
    pub margin: Margin,
    /// Body text size in points; headings scale from it.
    pub base_font_size: f64,
    /// Line box height as a multiple of the font size.
    pub line_height: f64,
}

impl Default for LayoutHint {
    fn default() -> Self {
        Self::a4()
    }
}

impl LayoutHint {
    /// A4 portrait page with 72 pt margins, 11 pt body text, and 1.45 line height.
    pub const fn a4() -> Self {
        Self {
            page_width: A4_WIDTH,
            page_height: A4_HEIGHT,
            margin: Margin::uniform(DEFAULT_MARGIN),
            base_font_size: 11.0,
            line_height: 1.45,
        }
    }

    /// Same hint with a different page size, keeping margins and body metrics.
    pub const fn with_page_size(mut self, width: f64, height: f64) -> Self {
        self.page_width = width;
        self.page_height = height;
        self
    }

    /// Same hint with different inward margins.
    pub const fn with_margin(mut self, margin: Margin) -> Self {
        self.margin = margin;
        self
    }

    /// Usable width between the left and right margins, never negative.
    pub fn content_width(&self) -> f64 {
        (self.page_width - self.margin.left - self.margin.right).max(0.0)
    }

    /// Usable height between the top and bottom margins, never negative.
    pub fn content_height(&self) -> f64 {
        (self.page_height - self.margin.top - self.margin.bottom).max(0.0)
    }

    /// Bottom edge of the content area measured from the page top, in points.
    pub fn content_bottom(&self) -> f64 {
        self.page_height - self.margin.bottom
    }
}
