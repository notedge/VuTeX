#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    path::Path,
};

use notedown_formats::import::markdown::import_markdown_bytes;
use notedown_ir::{Block, DocumentGraph, SemanticStatus};
use vutex_layout::{Diagrams, LayoutHint, document::layout_with_diagnostics};
use vutex_presentation::build_display_presentation;
use vutex_render_svg::render_svg as render_display_svg;
use vutex_types::{
    ConversionReport, CoverageSummary, Diagnostic, LossyItem, OpaqueItem, PreservedItem, Severity, SourceSpan, UnsupportedItem,
};

/// Stable diagnostic codes emitted by this adapter.
pub mod codes {
    /// The selected Markdown import profile failed to parse the source.
    pub const PARSE_FAILED: &str = "vutex.input.markdown.parse_failed";
    /// The source file could not be read.
    pub const FILE_READ_FAILED: &str = "vutex.input.markdown.file_read_failed";
    /// The imported graph violates a `notedown-ir` structural invariant.
    pub const INVALID_GRAPH: &str = "vutex.input.markdown.invalid_graph";
    /// A block was retained as an opaque boundary instead of being lowered.
    pub const OPAQUE_BLOCK: &str = "vutex.input.markdown.opaque_block";
    /// A raw HTML block was preserved verbatim instead of being semanticized.
    pub const PRESERVED_RAW_HTML: &str = "vutex.input.markdown.preserved_raw_html";
}

/// Outcome of a Markdown import: the document graph, its conversion report, and
/// the diagnostics the lowering produced.
#[derive(Debug)]
pub struct MarkdownDocument {
    /// The imported semantic graph consumed by the layout stage.
    pub graph: DocumentGraph,
    /// Normalized coverage and loss report for the import.
    pub report: ConversionReport,
    /// Diagnostics collected while importing.
    pub diagnostics: Vec<Diagnostic>,
}

/// Outcome of the closed loop from Markdown source to SVG output.
#[derive(Debug)]
pub struct MarkdownRender {
    /// Rendered SVG document.
    pub svg: String,
    /// Shared conversion report projected from the imported graph.
    pub report: ConversionReport,
    /// Import diagnostics followed by the layout stage diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Number of laid out pages.
    pub pages: usize,
    /// Number of positioned items across all pages.
    pub items: usize,
}

/// Why a Markdown source could not be imported or rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownInputError {
    diagnostics: Vec<Diagnostic>,
}

impl MarkdownInputError {
    /// Stable diagnostic code of the first failure.
    #[must_use]
    pub fn code(&self) -> &str {
        self.diagnostics.first().map_or("", |diagnostic| diagnostic.code.as_str())
    }

    /// Human-readable message of the first failure.
    #[must_use]
    pub fn message(&self) -> &str {
        self.diagnostics.first().map_or("", |diagnostic| diagnostic.message.as_str())
    }

    /// All diagnostics associated with the failure.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }
}

impl From<Diagnostic> for MarkdownInputError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostics: vec![diagnostic] }
    }
}

impl From<Vec<Diagnostic>> for MarkdownInputError {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

impl Display for MarkdownInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl Error for MarkdownInputError {}

/// Import a Markdown source through the `notedown-formats` Markdown profile.
///
/// The graph is validated before it is returned. A source that the profile
/// reports as unsupported fails closed instead of yielding a graph whose
/// content would be dropped later in the pipeline without a diagnostic.
pub fn import_source(label: &str, source: &str) -> Result<MarkdownDocument, MarkdownInputError> {
    let graph = import_markdown_bytes(label, source).map_err(|error| {
        MarkdownInputError::from(Diagnostic {
            code: codes::PARSE_FAILED.to_owned(),
            severity: Severity::Error,
            message: error.to_string(),
            span: None,
        })
    })?;
    finish(graph)
}

/// Read a Markdown file and import it with the file path as the document label.
pub fn import_file(path: impl AsRef<Path>) -> Result<MarkdownDocument, MarkdownInputError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|error| {
        MarkdownInputError::from(Diagnostic {
            code: codes::FILE_READ_FAILED.to_owned(),
            severity: Severity::Error,
            message: format!("failed to read {}: {error}", path.display()),
            span: None,
        })
    })?;
    import_source(&path.display().to_string(), &source)
}

/// Import Markdown source and run it through layout, presentation, and SVG output.
///
/// Diagram fences stay owned by the host: register their lowerings in `diagrams`
/// before calling. An unregistered diagram is left to the layout stage, which
/// keeps the authored source visible instead of guessing at a scene.
pub fn render_svg(label: &str, source: &str, diagrams: &Diagrams) -> Result<MarkdownRender, MarkdownInputError> {
    let MarkdownDocument { graph, report, mut diagnostics } = import_source(label, source)?;
    let (plan, layout_diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), diagrams);
    diagnostics.extend(layout_diagnostics);
    let display = build_display_presentation(&plan);
    let pages = display.pages.len();
    let items = display.pages.iter().map(|page| page.items.len()).sum();
    let svg = render_display_svg(&display).map_err(MarkdownInputError::from)?;
    Ok(MarkdownRender { svg, report, diagnostics, pages, items })
}

fn finish(graph: DocumentGraph) -> Result<MarkdownDocument, MarkdownInputError> {
    let validation = graph.validate();
    let (report, diagnostics) = analyze(&graph);
    if !validation.is_valid() {
        let mut collected: Vec<Diagnostic> = validation
            .issues
            .iter()
            .map(|issue| Diagnostic {
                code: codes::INVALID_GRAPH.to_owned(),
                severity: Severity::Error,
                message: format!("{issue:?}"),
                span: None,
            })
            .collect();
        collected.extend(diagnostics);
        return Err(MarkdownInputError::from(collected));
    }
    if !report.unsupported.is_empty() {
        return Err(MarkdownInputError::from(diagnostics));
    }
    Ok(MarkdownDocument { graph, report, diagnostics })
}

fn analyze(graph: &DocumentGraph) -> (ConversionReport, Vec<Diagnostic>) {
    let mut diagnostics = Vec::new();
    let mut preserved = Vec::new();
    let mut unsupported = Vec::new();
    let mut opaque = Vec::new();
    let mut lossy = Vec::new();

    for (index, marker) in graph.coverage.loss.iter().enumerate() {
        let span = graph.loss_span(index).map(|span| SourceSpan { start: span.start, end: span.end });
        match marker.status {
            SemanticStatus::Unsupported => {
                unsupported.push(UnsupportedItem { code: marker.code.clone(), detail: marker.message.clone() });
                diagnostics.push(Diagnostic {
                    code: marker.code.clone(),
                    severity: Severity::Error,
                    message: marker.message.clone(),
                    span,
                });
            }
            SemanticStatus::Unresolved | SemanticStatus::Partial | SemanticStatus::Inferred | SemanticStatus::Lossy => {
                lossy.push(LossyItem { code: marker.code.clone(), detail: marker.message.clone() });
                diagnostics.push(Diagnostic {
                    code: marker.code.clone(),
                    severity: Severity::Warning,
                    message: marker.message.clone(),
                    span,
                });
            }
            SemanticStatus::Resolved => {}
        }
    }

    let mut considered = 0_u32;
    let mut lowered = 0_u32;
    for node in &graph.blocks {
        considered += 1;
        match &node.block {
            Block::Opaque { kind, status, .. } => {
                let span = graph.block_span(node.id).map(|span| SourceSpan { start: span.start, end: span.end });
                let detail = format!("opaque `{kind}` block retained without semantic lowering");
                opaque.push(OpaqueItem { code: codes::OPAQUE_BLOCK.to_owned(), detail: detail.clone() });
                diagnostics.push(Diagnostic {
                    code: codes::OPAQUE_BLOCK.to_owned(),
                    severity: Severity::Info,
                    message: detail.clone(),
                    span,
                });
                if *status == SemanticStatus::Unsupported {
                    unsupported.push(UnsupportedItem { code: codes::OPAQUE_BLOCK.to_owned(), detail });
                    diagnostics.push(Diagnostic {
                        code: codes::OPAQUE_BLOCK.to_owned(),
                        severity: Severity::Error,
                        message: format!("unsupported opaque `{kind}` block"),
                        span,
                    });
                }
            }
            Block::RawHtml { html } => {
                preserved.push(PreservedItem {
                    code: codes::PRESERVED_RAW_HTML.to_owned(),
                    detail: format!("raw HTML block preserved verbatim ({} bytes)", html.len()),
                });
                lowered += 1;
            }
            _ => lowered += 1,
        }
    }

    let report = ConversionReport {
        parsed: CoverageSummary { considered, handled: considered },
        lowered: CoverageSummary { considered, handled: lowered },
        preserved,
        unsupported,
        opaque,
        lossy,
    };
    (report, diagnostics)
}

#[cfg(test)]
mod tests {
    use notedown_ir::{Block, DocumentId, Inline, LossMarker, NodeId, SemanticStatus};

    use super::*;

    #[test]
    fn unsupported_loss_fails_closed() {
        let mut graph = DocumentGraph::new(DocumentId(7));
        graph.push_loss(LossMarker {
            code: "import.markdown.unsupported_block".to_owned(),
            message: "unsupported markdown block".to_owned(),
            status: SemanticStatus::Unsupported,
        });

        let error = finish(graph).expect_err("unsupported constructs must fail closed");

        assert_eq!(error.code(), "import.markdown.unsupported_block");
        assert_eq!(error.diagnostics().len(), 1);
        assert_eq!(error.diagnostics()[0].severity, Severity::Error);
    }

    #[test]
    fn unsupported_opaque_block_fails_closed() {
        let mut graph = DocumentGraph::new(DocumentId(9));
        graph.push_block(Block::Opaque {
            kind: "unknown_payload".to_owned(),
            payload_hint: "payload".to_owned(),
            status: SemanticStatus::Unsupported,
        });

        let error = finish(graph).expect_err("unsupported opaque payloads must fail closed");

        assert!(
            error
                .diagnostics()
                .iter()
                .any(|diagnostic| { diagnostic.code == codes::OPAQUE_BLOCK && diagnostic.severity == Severity::Error })
        );
    }

    #[test]
    fn invalid_graph_is_rejected_before_layout() {
        let mut graph = DocumentGraph::new(DocumentId(11));
        graph.push_block(Block::Section {
            level: 1,
            title: vec![Inline::Text { text: "Title".to_owned() }],
            children: vec![NodeId(999)],
        });

        let error = finish(graph).expect_err("dangling section children must be rejected");

        assert!(!error.diagnostics().is_empty());
        assert!(error.diagnostics().iter().all(|diagnostic| diagnostic.severity == Severity::Error));
        assert!(error.diagnostics().iter().any(|diagnostic| diagnostic.code == codes::INVALID_GRAPH));
    }
}
