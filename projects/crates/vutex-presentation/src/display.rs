//! Fixed-geometry display presentation: pages of positioned primitives.

use vutex_types::math::FontStyle;

/// One positioned text line in display coordinates.
///
/// `y` is the text baseline measured from the page top; the line box top is
/// `y - 0.8 * size`.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayTextRun {
    /// Left edge in points, measured from the page left.
    pub x: f64,
    /// Text baseline in points, measured from the page top.
    pub y: f64,
    /// Font size in points.
    pub size: f64,
    /// Horizontal width of the run in page coordinates, in points.
    pub width: f64,
    /// Font family name.
    pub font_family: String,
    /// Face style carried from layout, such as bold or italic.
    pub style: FontStyle,
    /// Line text.
    pub text: String,
    /// External link destination the run was authored inside, if any.
    ///
    /// Only [`notedown_ir::RelationEndpoint::ExternalUri`] targets resolve to a
    /// destination. Internal `Node`, `Document`, and `Asset` targets stay
    /// `None`, because fixed-geometry output emits no anchor ids for them.
    /// Renderers must drop a destination they cannot safely emit while keeping
    /// the text.
    pub href: Option<String>,
    /// Hover text authored on the link that contains the run, if any.
    ///
    /// Only link display content carries a title; all other runs keep `None`.
    /// Renderers emit it with the anchor, never for a dropped destination.
    pub title: Option<String>,
}

/// One stroked line in display coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRule {
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

/// One box in display coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayRect {
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

/// One reserved image slot in display coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayImage {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Box width in points.
    pub width: f64,
    /// Box height in points.
    pub height: f64,
    /// Alternative text for accessibility and diagnostics.
    pub alt: String,
    /// Authored image location, kept so a renderer can resolve the pixels.
    ///
    /// The destination is carried verbatim from the document, so it may be a
    /// `data:` URI or a filesystem path. Renderers resolve it themselves and
    /// keep the dashed slot when the value is absent or unresolvable.
    pub source: Option<String>,
}

/// One placed diagram scene in display coordinates.
///
/// `x`/`y`/`width`/`height` describe the page-space box; the scene keeps its own
/// canvas coordinates, so a renderer scales the scene by
/// `width / scene.size.width` and `height / scene.size.height`.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayGraphic {
    /// Left edge in points.
    pub x: f64,
    /// Top edge in points.
    pub y: f64,
    /// Placed box width in points.
    pub width: f64,
    /// Placed box height in points.
    pub height: f64,
    /// Resolved display scene emitted by the diagram producer.
    pub scene: davinci_scene::DisplayScene,
    /// Alternative text for backends that cannot paint the scene.
    pub alt: String,
}

/// A fixed-geometry primitive painted by SVG, PDF, and raster backends.
#[derive(Debug, Clone, PartialEq)]
pub enum DisplayItem {
    /// A single text line.
    TextRun(DisplayTextRun),
    /// A stroked line.
    Rule(DisplayRule),
    /// A box, either outlined or filled.
    Rect(DisplayRect),
    /// An image slot reserved at a fixed size.
    Image(DisplayImage),
    /// A diagram scene placed at a fixed size.
    Graphic(DisplayGraphic),
}

/// One page in a fixed-geometry display presentation.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayPage {
    /// Page width in points.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// Positioned primitives in paint order.
    pub items: Vec<DisplayItem>,
}

/// Fixed-geometry presentation for PDF, SVG, and raster backends.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayPresentation {
    /// Pages in document order.
    pub pages: Vec<DisplayPage>,
}
