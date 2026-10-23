//! Reflow presentation: semantic blocks for HTML and accessibility targets.
//!
//! Only paragraphs carry styled runs this round: headings, list items, quotes,
//! and table cells stay plain text because their reflow markup is fixed by the
//! block role. A `FlowRun` records one contiguous span of text plus the face
//! style it was authored with, so renderers can emit `<strong>`, `<em>`, or
//! `<code>` without re-reading the document graph.

use vutex_types::math::FontStyle;

/// One contiguous styled text span inside a reflow paragraph.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowRun {
    /// Run text, escaped by the renderer.
    pub text: String,
    /// Face style the run was authored with.
    pub style: FontStyle,
}

/// One row of a flow table, with the header role kept for `<thead>` output.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowTableRow {
    /// Whether this row is a header row.
    pub is_header: bool,
    /// Cell texts in column order.
    pub cells: Vec<String>,
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
        /// Plain heading text.
        text: String,
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
        /// Item texts in document order.
        items: Vec<String>,
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
        /// Plain quoted text.
        text: String,
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
