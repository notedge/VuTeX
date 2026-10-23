//! Reflow presentation: semantic blocks for HTML and accessibility targets.
//!
//! Every block variant whose content is authored inline text carries styled
//! runs: headings, paragraphs, list items, quotes, and table cells. Only
//! verbatim payloads such as code blocks and table captions stay plain strings
//! because the document graph stores them as text. A `FlowRun` records one
//! contiguous span of text plus the face style it was authored with, so
//! renderers can emit `<strong>`, `<em>`, or `<code>` without re-reading the
//! document graph.

use vutex_types::math::FontStyle;

/// One contiguous styled text span inside a reflow block.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowRun {
    /// Run text, escaped by the renderer.
    pub text: String,
    /// Face style the run was authored with.
    pub style: FontStyle,
    /// Authored external destination, such as `https://example.com/spec`.
    ///
    /// Only [`notedown_ir::RelationEndpoint::ExternalUri`] targets resolve to a
    /// destination. Internal targets stay `None` because reflow HTML emits no
    /// matching ids, so a renderer must treat a missing destination as plain
    /// text rather than inventing an anchor.
    pub href: Option<String>,
}

/// One row of a flow table, with the header role kept for `<thead>` output.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowTableRow {
    /// Whether this row is a header row.
    pub is_header: bool,
    /// Cell runs in column order; adjacent runs of the same style are merged.
    pub cells: Vec<Vec<FlowRun>>,
}

/// Semantic block content for reflow targets such as HTML.
///
/// Variants carry only semantics that reflow output can express as markup;
/// positioning, fonts, and page geometry stay in [`crate::display`].
#[derive(Debug, Clone, PartialEq)]
pub enum FlowBlock {
    /// Section heading with a level from 1 to 6.
    Heading {
        /// Heading level, 1 being the top level.
        level: u8,
        /// Styled heading runs in document order.
        runs: Vec<FlowRun>,
    },
    /// Paragraph text with its styled runs in document order.
    Paragraph {
        /// Styled runs; adjacent runs of the same style are already merged.
        runs: Vec<FlowRun>,
    },
    /// Ordered or unordered list.
    List {
        /// Whether list markers are numeric.
        ordered: bool,
        /// Item runs in document order.
        items: Vec<Vec<FlowRun>>,
    },
    /// Fenced or indented code block.
    CodeBlock {
        /// Optional language tag.
        language: Option<String>,
        /// Verbatim source text.
        text: String,
    },
    /// Block quotation.
    Quote {
        /// Styled quoted runs in document order.
        runs: Vec<FlowRun>,
    },
    /// Table with caption and header rows.
    Table {
        /// Optional human-readable caption.
        caption: Option<String>,
        /// Rows in document order.
        rows: Vec<FlowTableRow>,
    },
    /// Image reference with alternative text.
    Image {
        /// Alternative text.
        alt: String,
        /// Requested source location.
        source: String,
    },
    /// Thematic break.
    Rule,
}

/// Reflow-oriented presentation for HTML and accessibility targets.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowPresentation {
    /// Container tag wrapping all blocks, such as `article`.
    pub root_tag: String,
    /// Semantic blocks in document order.
    pub blocks: Vec<FlowBlock>,
}
