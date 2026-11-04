#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
};

use notedown_formats::import::markdown::import_markdown_bytes;
use notedown_ir::{Block, DocumentGraph, SemanticStatus};
use vutex_types::{
    ConversionReport, CoverageSummary, Diagnostic, LossyItem, OpaqueItem, PreservedItem, Severity, SourceSpan, UnsupportedItem,
};

/// Stable diagnostic codes emitted by this adapter.
pub mod codes {
    /// The selected Markdown import profile failed to parse the source.
    pub const PARSE_FAILED: &str = "vutex.input.markdown.parse_failed";
    /// The imported graph violates a `notedown-ir` structural invariant.
    pub const INVALID_GRAPH: &str = "vutex.input.markdown.invalid_graph";
    /// A block was retained as an opaque boundary instead of being lowered.
    pub const OPAQUE_BLOCK: &str = "vutex.input.markdown.opaque_block";
    /// A raw HTML block was preserved verbatim instead of being semanticized.
    pub const PRESERVED_RAW_HTML: &str = "vutex.input.markdown.preserved_raw_html";
}

/// Result of importing one Markdown source into `notedown-ir`.
#[derive(Debug)]
pub struct MarkdownImport {
    /// The imported semantic graph consumed by the layout stage.
    pub graph: DocumentGraph,
    /// Normalized coverage and loss report for the import.
    pub report: ConversionReport,
    /// Diagnostics collected while importing.
    pub diagnostics: Vec<Diagnostic>,
}

/// Why a Markdown source could not be imported into a usable graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkdownInputError {
    /// The selected import profile rejected the source.
    Parse {
        /// The structured parse diagnostic.
        diagnostic: Diagnostic,
    },
    /// The graph broke a `notedown-ir` invariant before any layout ran.
    InvalidGraph {
        /// One diagnostic per invariant violation.
        diagnostics: Vec<Diagnostic>,
    },
    /// The source used a construct this profile cannot lower.
    Unsupported {
        /// All diagnostics collected for the import, including the errors.
        diagnostics: Vec<Diagnostic>,
    },
}

impl MarkdownInputError {
    /// Diagnostics associated with the failure.
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match self {
            Self::Parse { diagnostic } => std::slice::from_ref(diagnostic),
            Self::InvalidGraph { diagnostics } | Self::Unsupported { diagnostics } => diagnostics,
        }
    }
}

impl Display for MarkdownInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Parse { diagnostic } => {
                write!(formatter, "markdown import failed: {}", diagnostic.message)
            }
            Self::InvalidGraph { diagnostics } => {
                write!(formatter, "markdown import produced {} graph invariant violation(s)", diagnostics.len())
            }
            Self::Unsupported { diagnostics } => {
                write!(formatter, "markdown import contains {} unsupported construct(s)", diagnostics.len())
            }
        }
    }
}

impl Error for MarkdownInputError {}

/// Import a Markdown source through the `notedown-formats` Markdown profile.
///
/// The graph is validated before it is returned. A source that the profile
/// reports as unsupported fails closed instead of yielding a graph whose
/// content would be dropped later in the pipeline without a diagnostic.
pub fn import_markdown(label: &str, text: &str) -> Result<MarkdownImport, MarkdownInputError> {
    let graph = import_markdown_bytes(label, text).map_err(|error| MarkdownInputError::Parse {
        diagnostic: Diagnostic {
            code: codes::PARSE_FAILED.to_owned(),
            severity: Severity::Error,
            message: error.to_string(),
            span: None,
        },
    })?;
    finish(graph)
}

fn finish(graph: DocumentGraph) -> Result<MarkdownImport, MarkdownInputError> {
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
        return Err(MarkdownInputError::InvalidGraph { diagnostics: collected });
    }
    if !report.unsupported.is_empty() {
        return Err(MarkdownInputError::Unsupported { diagnostics });
    }
    Ok(MarkdownImport { graph, report, diagnostics })
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

        match error {
            MarkdownInputError::Unsupported { diagnostics } => {
                assert_eq!(diagnostics.len(), 1);
                assert_eq!(diagnostics[0].code, "import.markdown.unsupported_block");
                assert_eq!(diagnostics[0].severity, Severity::Error);
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
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

        match error {
            MarkdownInputError::Unsupported { diagnostics } => {
                assert!(
                    diagnostics
                        .iter()
                        .any(|diagnostic| { diagnostic.code == codes::OPAQUE_BLOCK && diagnostic.severity == Severity::Error })
                );
            }
            other => panic!("expected unsupported error, got {other:?}"),
        }
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

        match error {
            MarkdownInputError::InvalidGraph { diagnostics } => {
                assert!(!diagnostics.is_empty());
                assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity == Severity::Error));
                assert!(diagnostics.iter().all(|diagnostic| diagnostic.code == codes::INVALID_GRAPH));
            }
            other => panic!("expected invalid graph error, got {other:?}"),
        }
    }
}
