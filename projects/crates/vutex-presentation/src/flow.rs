//! Reflow presentation: semantic blocks for HTML and accessibility targets.

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
    /// Paragraph text.
    Paragraph {
        /// Plain paragraph text.
        text: String,
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
