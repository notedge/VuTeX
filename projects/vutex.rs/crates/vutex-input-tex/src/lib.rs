//! TeX/LaTeX input profile for VuTeX.
//!
//! VuTeX does not own TeX syntax. The `oak-tex` frontend parses the source, the
//! document model lowers it into `notedown-ir`, and this crate selects that
//! profile and projects the recorded outcome into the shared conversion report.
//! Math enters the shared formula pipeline as `notedown-ir` math, so the profile
//! never carries a second set of mathematical semantics.
//!
//! Every construct the profile cannot map into document semantics is reported
//! in the report or in the diagnostics, with a source span when the input or the
//! lowered graph still knows one. Nothing is dropped silently and nothing is
//! approximated to look supported.

#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    path::Path,
};

use notedown_formats::{FormatError, import::tex::import_tex_bytes};
use notedown_ir::{Block, DocumentGraph, Inline, MathExpression, MathFormula, SemanticStatus, SourceSpan as IrSpan};
use vutex_types::{
    ConversionReport, CoverageSummary, Diagnostic, LossyItem, OpaqueItem, PreservedItem, Severity, SourceSpan, UnsupportedItem,
};

/// Stable identifier of the TeX input profile.
///
/// The same string is written into [`MathFormula::language`] for TeX math, so a
/// consumer can tell which profile produced the document.
pub const PROFILE_ID: &str = "latex";

/// Code recorded when the TeX frontend rejects the source.
pub const PARSE_FAILED: &str = "vutex.input.tex.parse_failed";
/// Code recorded when a source file cannot be read.
pub const IO_FAILED: &str = "vutex.input.tex.io_failed";
/// Code recorded when the lowered graph violates a document-model invariant.
pub const INVALID_GRAPH: &str = "vutex.input.tex.invalid_graph";
/// Code recorded for math whose language tag the TeX formula pipeline cannot
/// lower.
pub const MATH_LANGUAGE: &str = "vutex.input.tex.math_language";

/// Loss codes that summarize the whole import instead of one source construct.
const SUMMARY_LOSS_CODES: [&str; 1] = ["import.tex.partial_coverage"];

/// A TeX source converted into the document model.
#[derive(Debug, Clone, PartialEq)]
pub struct TexDocument {
    /// Language-neutral document the layout pipeline consumes.
    pub graph: DocumentGraph,
    /// Shared conversion report projected from the graph.
    pub report: ConversionReport,
    /// Structured diagnostics, including recovered source spans.
    pub diagnostics: Vec<Diagnostic>,
}

/// Failure that stops the profile before any document reaches layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexInputError {
    /// The diagnostic that explains the failure.
    pub diagnostic: Diagnostic,
}

impl TexInputError {
    /// Stable diagnostic code of the failure.
    pub fn code(&self) -> &str {
        &self.diagnostic.code
    }

    /// Human-readable failure message.
    pub fn message(&self) -> &str {
        &self.diagnostic.message
    }

    /// Source byte range, when the failure is known to sit in one.
    pub fn span(&self) -> Option<SourceSpan> {
        self.diagnostic.span
    }

    /// Build an error whose diagnostic carries `code` and `message`.
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic { code: code.to_string(), severity: Severity::Error, message: message.into(), span: None },
        }
    }
}

impl Display for TexInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}: {}", self.code(), self.message())
    }
}

impl Error for TexInputError {}

/// Convert TeX source text into the document model.
///
/// A rejected source returns an error instead of a partial document, so a host
/// never lays out a graph that the frontend did not finish reading. A graph that
/// breaks a document-model invariant is rejected for the same reason: layout
/// trusts those invariants and cannot detect the violation on its own.
pub fn import_source(label: &str, source: &str) -> Result<TexDocument, TexInputError> {
    let graph = import_tex_bytes(label, source).map_err(parse_failure)?;
    validate_graph(&graph)?;
    let report = conversion_report(&graph);
    let diagnostics = conversion_diagnostics(&graph, source);
    Ok(TexDocument { graph, report, diagnostics })
}

/// Convert one TeX file into the document model.
pub fn import_file(path: impl AsRef<Path>) -> Result<TexDocument, TexInputError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|error| {
        TexInputError::new(IO_FAILED, format!("failed to read {}: {error}", path.display()))
    })?;
    import_source(&path.display().to_string(), &source)
}

/// Check that a lowered graph satisfies the document-model invariants layout
/// relies on.
///
/// The profile rejects an invalid graph instead of handing it to layout, which
/// has no independent way to detect a broken identity, list numbering, or
/// containment relation.
pub fn validate_graph(graph: &DocumentGraph) -> Result<(), TexInputError> {
    let validation = graph.validate();
    if validation.issues.is_empty() {
        return Ok(());
    }
    Err(TexInputError::new(
        INVALID_GRAPH,
        format!("lowered graph breaks document-model invariants: {:?}", validation.issues),
    ))
}

/// Project a document graph that already carries TeX-lowered content into the
/// shared conversion report.
///
/// Hosts that obtain the graph through another entry point still get the same
/// report, including the checks this profile owns rather than the document
/// model.
pub fn conversion_report(graph: &DocumentGraph) -> ConversionReport {
    let blocks = graph.blocks.len();
    let construct_losses = graph.coverage.loss.iter().filter(|marker| !is_summary(&marker.code)).count();
    let lowered_blocks = graph.blocks.iter().filter(|node| is_lowered(&node.block)).count();

    let mut unsupported = Vec::new();
    let mut lossy = Vec::new();
    for marker in &graph.coverage.loss {
        match marker.status {
            SemanticStatus::Unsupported | SemanticStatus::Unresolved => {
                unsupported.push(UnsupportedItem { code: marker.code.clone(), detail: marker.message.clone() });
            }
            SemanticStatus::Partial | SemanticStatus::Lossy | SemanticStatus::Inferred => {
                lossy.push(LossyItem { code: marker.code.clone(), detail: marker.message.clone() });
            }
            SemanticStatus::Resolved => {}
        }
    }

    for mismatch in math_mismatches(graph) {
        unsupported.push(UnsupportedItem { code: MATH_LANGUAGE.to_string(), detail: mismatch.message() });
    }

    let opaque = graph
        .blocks
        .iter()
        .filter_map(|node| match &node.block {
            Block::Opaque { kind, payload_hint, .. } => {
                Some(OpaqueItem { code: kind.clone(), detail: payload_hint.clone() })
            }
            _ => None,
        })
        .collect();

    ConversionReport {
        parsed: CoverageSummary { considered: count(blocks + construct_losses), handled: count(blocks) },
        lowered: CoverageSummary { considered: count(blocks), handled: count(lowered_blocks) },
        preserved: Vec::<PreservedItem>::new(),
        unsupported,
        opaque,
        lossy,
    }
}

/// Build the diagnostics that explain what the conversion could not map.
///
/// Document-level coverage statements stay unpositioned. A construct-level loss
/// uses its recorded span when the graph has one, otherwise the payload named by
/// its message is matched against the source. An opaque block also gets its own
/// diagnostic, because the block keeps the authored payload and therefore
/// recovers the precise source range a loss message may not name.
pub fn conversion_diagnostics(graph: &DocumentGraph, source: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    for (index, marker) in graph.coverage.loss.iter().enumerate() {
        if is_summary(&marker.code) {
            continue;
        }
        let payload = payload_after_colon(&marker.message);
        let span = graph
            .loss_span(index)
            .map(span_of)
            .or_else(|| payload.and_then(|text| unique_payload_span(source, text)));
        diagnostics.push(Diagnostic {
            code: marker.code.clone(),
            severity: severity_of(marker.status),
            message: marker.message.clone(),
            span,
        });
    }

    for node in &graph.blocks {
        if let Block::Opaque { kind, payload_hint, status } = &node.block {
            let payload = payload_hint.trim();
            if payload.is_empty() {
                continue;
            }
            diagnostics.push(Diagnostic {
                code: kind.clone(),
                severity: severity_of(*status),
                message: format!("TeX construct kept opaque without document semantics: {payload}"),
                span: unique_payload_span(source, payload),
            });
        }
    }

    for mismatch in math_mismatches(graph) {
        diagnostics.push(Diagnostic {
            code: MATH_LANGUAGE.to_string(),
            severity: Severity::Error,
            message: mismatch.message(),
            span: None,
        });
    }

    diagnostics
}

/// One formula whose language the TeX pipeline cannot lower.
#[derive(Debug, Clone, PartialEq, Eq)]
struct MathMismatch {
    location: String,
    language: String,
}

impl MathMismatch {
    fn message(&self) -> String {
        format!("math at {} is tagged `{}`, which the TeX formula pipeline cannot lower", self.location, self.language)
    }
}

/// Collect every formula whose language tag does not belong to the TeX profile.
///
/// The profile only produces `latex` math, so this check guards the contract
/// rather than a normal path. A mismatch is an error: feeding another
/// mathematical grammar to the TeX layout would produce wrong structure, not a
/// degradation.
fn math_mismatches(graph: &DocumentGraph) -> Vec<MathMismatch> {
    let mut mismatches = Vec::new();
    for node in &graph.blocks {
        let location = format!("block {}", node.id.0);
        match &node.block {
            Block::Math { formula } => check_math(formula, &location, &mut mismatches),
            Block::Section { title, .. } => check_inlines(title, &location, &mut mismatches),
            Block::Paragraph { content } => check_inlines(content, &location, &mut mismatches),
            Block::Quote { content, .. } => check_inlines(content, &location, &mut mismatches),
            Block::List { items, .. } => {
                for item in items {
                    check_inlines(&item.content, &location, &mut mismatches);
                }
            }
            Block::Table { rows, .. } => {
                for row in rows {
                    for cell in &row.cells {
                        check_inlines(&cell.content, &location, &mut mismatches);
                    }
                }
            }
            _ => {}
        }
    }
    mismatches
}

/// Check every inline formula inside `content`.
fn check_inlines(content: &[Inline], location: &str, mismatches: &mut Vec<MathMismatch>) {
    let mut formulas: Vec<&MathFormula> = Vec::new();
    collect_inlines(content, &mut formulas);
    for formula in formulas {
        check_math(formula, location, mismatches);
    }
}

/// Collect inline formulas, descending through the inline containers that own
/// nested content.
fn collect_inlines<'a>(content: &'a [Inline], formulas: &mut Vec<&'a MathFormula>) {
    for inline in content {
        match inline {
            Inline::InlineMath { formula } => formulas.push(formula),
            Inline::Styled { children, .. } | Inline::Attributed { children, .. } => collect_inlines(children, formulas),
            Inline::Link { display, .. } => collect_inlines(display, formulas),
            Inline::Image { alt, .. } => collect_inlines(alt, formulas),
            Inline::Note { display: Some(display), .. } | Inline::Citation { display: Some(display), .. } => {
                collect_inlines(display, formulas)
            }
            _ => {}
        }
    }
}

/// Record one formula if its language tag is not Tex.
fn check_math(formula: &MathFormula, location: &str, mismatches: &mut Vec<MathMismatch>) {
    let language = formula.language.as_deref().unwrap_or_default().trim();
    if is_tex_language(language) {
        return;
    }
    mismatches.push(MathMismatch { location: location.to_string(), language: language.to_string() });
}

/// Whether a math language tag belongs to the TeX profile.
///
/// This repeats the dispatch rule the layout pipeline applies: no tag, an empty
/// tag, `tex`, and `latex` all select TeX math. The profile cannot call the
/// layout predicate without inverting the adapter dependency direction, so the
/// rule lives here explicitly instead.
fn is_tex_language(language: &str) -> bool {
    language.is_empty() || language.eq_ignore_ascii_case("tex") || language.eq_ignore_ascii_case("latex")
}

/// Whether one block reached document semantics.
fn is_lowered(block: &Block) -> bool {
    match block {
        Block::Opaque { .. } => false,
        Block::Math { formula } => !matches!(formula.expression, MathExpression::Unsupported { .. }),
        _ => true,
    }
}

/// Whether one loss code summarizes the whole import.
fn is_summary(code: &str) -> bool {
    SUMMARY_LOSS_CODES.contains(&code)
}

/// Severity a host sees for one recorded loss.
fn severity_of(status: SemanticStatus) -> Severity {
    match status {
        SemanticStatus::Unsupported | SemanticStatus::Unresolved => Severity::Error,
        SemanticStatus::Partial | SemanticStatus::Lossy | SemanticStatus::Inferred => Severity::Warning,
        SemanticStatus::Resolved => Severity::Info,
    }
}

/// Map a document-model span onto the diagnostic span type.
fn span_of(span: IrSpan) -> SourceSpan {
    SourceSpan { start: span.start, end: span.end }
}

/// The payload a loss message names after its last colon.
fn payload_after_colon(message: &str) -> Option<&str> {
    let (_, payload) = message.rsplit_once(':')?;
    let payload = payload.trim();
    (!payload.is_empty()).then_some(payload)
}

/// Recover the span of a payload from its authored text.
///
/// Only an unambiguous single occurrence yields a span: a repeated payload could
/// belong to any occurrence, and pointing at the wrong one would be worse than
/// pointing nowhere.
fn unique_payload_span(source: &str, payload_hint: &str) -> Option<SourceSpan> {
    let needle = payload_hint.trim();
    if needle.is_empty() {
        return None;
    }
    let mut occurrences = source.match_indices(needle);
    let first = occurrences.next()?;
    if occurrences.next().is_some() {
        return None;
    }
    Some(SourceSpan { start: first.0 as u32, end: (first.0 + needle.len()) as u32 })
}

/// Turn a frontend rejection into the profile's structured error.
fn parse_failure(error: FormatError) -> TexInputError {
    TexInputError::new(PARSE_FAILED, error.to_string())
}

/// Count a collection into the report's integer type.
fn count(value: usize) -> u32 {
    value as u32
}
