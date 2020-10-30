/// Stable document node identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DocumentId(pub u64);

/// Stable node identifier inside a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub u64);

/// Block-level semantic kinds supported by the canonical IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Document,
    Section,
    Heading,
    Paragraph,
    List,
    Table,
    Quote,
    CodeBlock,
    Image,
    Component,
    Opaque,
}

/// Inline semantic kinds supported by the canonical IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineKind {
    Text,
    Emphasis,
    Link,
    Code,
    Math,
    Citation,
    FootnoteRef,
}
