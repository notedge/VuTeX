//! TeX/LaTeX input profile for VuTeX.
//!
//! VuTeX does not own TeX syntax. The `oak-tex` frontend parses the source, the
//! document model lowers it into `notedown-ir`, and this crate selects that
//! profile and turns the recorded outcome into a conversion report. Math enters
//! the shared formula pipeline as `notedown-ir` math, so the profile never
//! carries a second set of mathematical semantics.
//!
//! Every construct the profile cannot map into document semantics is reported
//! with an explicit [`ConversionStatus`], a diagnostic code, and a source span
//! when the input or the lowered graph still knows one. Nothing is dropped
//! silently and nothing is approximated to look supported.

#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use std::{
    error::Error,
    fmt::{Display, Formatter, Result as FmtResult},
    path::Path,
};

use notedown_formats::{FormatError, import::tex::import_tex_bytes};
use notedown_ir::{Block, DocumentGraph, Inline, MathFormula, NodeId, SourceSpan as IrSpan};
use vutex_types::{Diagnostic, Severity, SourceSpan};

/// Stable identifier of the TeX input profile.
///
/// The same string is written into an inspect report and into
/// [`MathFormula::language`] for TeX math, so a consumer can tell which profile
/// produced the document.
pub const PROFILE_ID: &str = "latex";

/// Code recorded for a source the TeX frontend accepted.
pub const SOURCE_PARSED: &str = "vutex.input.tex.parsed";
/// Code recorded when the TeX frontend rejects the source.
pub const PARSE_FAILED: &str = "vutex.input.tex.parse_failed";
/// Code recorded when a source file cannot be read.
pub const IO_FAILED: &str = "vutex.input.tex.io_failed";
/// Code recorded when the lowered graph violates a document-model invariant.
pub const INVALID_GRAPH: &str = "vutex.input.tex.invalid_graph";
/// Code recorded for a construct kept opaque instead of lowered.
pub const OPAQUE_CONSTRUCT: &str = "vutex.input.tex.opaque_construct";
/// Code recorded for math whose language tag the TeX formula pipeline cannot
/// lower.
pub const MATH_LANGUAGE: &str = "vutex.input.tex.math_language";

/// Conversion status of one TeX construct.
///
/// The set mirrors the language-neutral vocabulary every input adapter reports,
/// so a host never has to interpret a TeX-specific state name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionStatus {
    /// Syntax was accepted by the frontend.
    Parsed,
    /// Semantics reached the document model.
    Lowered,
    /// Authored content was preserved without document semantics.
    Preserved,
    /// The construct is known but has no document mapping.
    Unsupported,
    /// The construct was kept as an opaque payload.
    Opaque,
    /// Mapping succeeded with a recorded loss.
    Lossy,
}

/// One reported construct of the conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructEntry {
    /// Conversion status of the construct.
    pub status: ConversionStatus,
    /// Stable code identifying what was reported.
    ///
    /// Upstream document-model codes such as `import.tex.math_numbering` are
    /// passed through verbatim, so a host can filter on the code that the
    /// lowering actually recorded.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Source byte range the construct occupies, when it is known.
    pub span: Option<SourceSpan>,
}

/// Outcome of converting one TeX source into document semantics.
///
/// The report never collapses into a single completeness flag. A caller reads
/// the per-construct statuses, or the diagnostics the conversion emitted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ConversionReport {
    /// Reported constructs in discovery order.
    pub entries: Vec<ConstructEntry>,
    /// Diagnostics a host can surface in a build report.
    pub diagnostics: Vec<Diagnostic>,
}

impl ConversionReport {
    /// Number of entries carrying `status`.
    pub fn count(&self, status: ConversionStatus) -> usize {
        self.entries.iter().filter(|entry| entry.status == status).count()
    }

    /// Whether any diagnostic reached error severity.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

/// A TeX source converted into the document model.
#[derive(Debug, Clone)]
pub struct TexDocument {
    /// Language-neutral document the layout pipeline consumes.
    pub graph: DocumentGraph,
    /// What the conversion did and did not map.
    pub report: ConversionReport,
}

/// Failure that stops the profile before any document reaches layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TexInputError {
    /// Stable diagnostic code.
    pub code: String,
    /// Human-readable message, including the frontend position when it reported
    /// one.
    pub message: String,
    /// Source byte range, when the failure is known to sit in one.
    pub span: Option<SourceSpan>,
}

impl Display for TexInputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}: {}", self.code, self.message)
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
    let report = conversion_report(&graph, source);
    Ok(TexDocument { graph, report })
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
    Err(TexInputError {
        code: INVALID_GRAPH.to_string(),
        message: format!("lowered graph breaks document-model invariants: {:?}", validation.issues),
        span: None,
    })
}

/// Convert one TeX file into the document model.
pub fn import_file(path: impl AsRef<Path>) -> Result<TexDocument, TexInputError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|error| TexInputError {
        code: IO_FAILED.to_string(),
        message: format!("failed to read {}: {error}", path.display()),
        span: None,
    })?;
    import_source(&path.display().to_string(), &source)
}

/// Audit a document graph that already carries TeX-lowered content.
///
/// Hosts that obtain the graph through another entry point still get the same
/// conversion report, including the checks this profile owns rather than the
/// document model.
pub fn conversion_report(graph: &DocumentGraph, source: &str) -> ConversionReport {
    let mut report = ConversionReport::default();
    report.entries.push(ConstructEntry {
        status: ConversionStatus::Parsed,
        code: SOURCE_PARSED.to_string(),
        message: "the TeX frontend accepted the source".to_string(),
        span: document_span(source),
    });

    for (index, loss) in graph.coverage.loss.iter().enumerate() {
        let status = loss_status(loss.status);
        let entry = ConstructEntry {
            status,
            code: loss.code.clone(),
            message: loss.message.clone(),
            span: graph.loss_span(index).map(span_of),
        };
        let severity = loss_severity(status);
        report.diagnostics.push(Diagnostic {
            code: entry.code.clone(),
            severity,
            message: entry.message.clone(),
            span: entry.span,
        });
        report.entries.push(entry);
    }

    for node in &graph.blocks {
        audit_block(node.id, &node.block, graph, source, &mut report);
    }
    report
}

/// Report one block: opaque payloads it kept and math it handed to the formula
/// pipeline.
fn audit_block(id: NodeId, block: &Block, graph: &DocumentGraph, source: &str, report: &mut ConversionReport) {
    if let Block::Opaque { kind, payload_hint, .. } = block {
        let span = graph.block_span(id).map(span_of).or_else(|| unique_payload_span(source, payload_hint));
        let message = format!(
            "TeX construct `{kind}` at block {} has no document semantics and stays opaque: {payload_hint}",
            id.0
        );
        report.entries.push(ConstructEntry {
            status: ConversionStatus::Opaque,
            code: OPAQUE_CONSTRUCT.to_string(),
            message: message.clone(),
            span,
        });
        report.diagnostics.push(Diagnostic { code: OPAQUE_CONSTRUCT.to_string(), severity: Severity::Warning, message, span });
    }

    let location = format!("block {}", id.0);
    match block {
        Block::Math { formula } => audit_math(formula, &location, report),
        Block::Section { title, .. } => audit_inlines(title, &location, report),
        Block::Paragraph { content } => audit_inlines(content, &location, report),
        Block::Quote { content, .. } => audit_inlines(content, &location, report),
        Block::List { items, .. } => {
            for item in items {
                audit_inlines(&item.content, &location, report);
            }
        }
        Block::Table { rows, .. } => {
            for row in rows {
                for cell in &row.cells {
                    audit_inlines(&cell.content, &location, report);
                }
            }
        }
        _ => {}
    }
}

/// Audit every inline formula inside `content`.
fn audit_inlines(content: &[Inline], location: &str, report: &mut ConversionReport) {
    let mut formulas: Vec<&MathFormula> = Vec::new();
    collect_inlines(content, &mut formulas);
    for formula in formulas {
        audit_math(formula, location, report);
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

/// Fail closed when math carries a language the TeX formula pipeline cannot
/// lower.
///
/// The profile only produces `latex` math, so this check guards the contract
/// rather than a normal path. A mismatch is an error: feeding another
/// mathematical grammar to the TeX layout would produce wrong structure, not a
/// degradation.
fn audit_math(formula: &MathFormula, location: &str, report: &mut ConversionReport) {
    if is_tex_language(formula.language.as_deref()) {
        return;
    }
    let message = format!(
        "math at {location} is tagged `{}`, which the TeX formula pipeline cannot lower",
        formula.language.as_deref().unwrap_or_default()
    );
    report.entries.push(ConstructEntry {
        status: ConversionStatus::Unsupported,
        code: MATH_LANGUAGE.to_string(),
        message: message.clone(),
        span: None,
    });
    report.diagnostics.push(Diagnostic { code: MATH_LANGUAGE.to_string(), severity: Severity::Error, message, span: None });
}

/// Whether a math language tag belongs to the TeX profile.
///
/// This repeats the dispatch rule the layout pipeline applies: no tag, an empty
/// tag, `tex`, and `latex` all select TeX math. The profile cannot call the
/// layout predicate without inverting the adapter dependency direction, so the
/// rule lives here explicitly instead.
fn is_tex_language(language: Option<&str>) -> bool {
    match language {
        None => true,
        Some(value) => {
            let value = value.trim();
            value.is_empty() || value.eq_ignore_ascii_case("tex") || value.eq_ignore_ascii_case("latex")
        }
    }
}

/// Conversion status of one document-model loss.
fn loss_status(status: notedown_ir::SemanticStatus) -> ConversionStatus {
    match status {
        notedown_ir::SemanticStatus::Resolved => ConversionStatus::Lowered,
        notedown_ir::SemanticStatus::Inferred => ConversionStatus::Lossy,
        notedown_ir::SemanticStatus::Partial | notedown_ir::SemanticStatus::Lossy => ConversionStatus::Lossy,
        notedown_ir::SemanticStatus::Unsupported | notedown_ir::SemanticStatus::Unresolved => ConversionStatus::Unsupported,
    }
}

/// Severity a host sees for one recorded loss.
///
/// A recorded loss is a warning, never an error: the construct keeps an
/// explicit status in the report, and the host decides whether the document is
/// acceptable. Errors are reserved for input the profile refuses to lower, such
/// as math carrying a foreign grammar.
fn loss_severity(status: ConversionStatus) -> Severity {
    match status {
        ConversionStatus::Parsed | ConversionStatus::Lowered => Severity::Info,
        ConversionStatus::Preserved | ConversionStatus::Lossy | ConversionStatus::Unsupported | ConversionStatus::Opaque => {
            Severity::Warning
        }
    }
}

/// Map a document-model span onto the diagnostic span type.
fn span_of(span: IrSpan) -> SourceSpan {
    SourceSpan { start: span.start, end: span.end }
}

/// Span covering the whole source, for the accepted-syntax entry.
fn document_span(source: &str) -> Option<SourceSpan> {
    (!source.is_empty()).then(|| SourceSpan { start: 0, end: source.len() as u32 })
}

/// Recover the span of an opaque payload from its authored text.
///
/// The document model records opaque blocks without a block span, so the
/// authored payload text is matched against the source. Only an unambiguous
/// single occurrence yields a span: a repeated payload could belong to any of
/// the occurrences, and pointing at the wrong one would be worse than pointing
/// nowhere. The block ordinal in the message stays as the locator when no span
/// is recovered.
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
    TexInputError { code: PARSE_FAILED.to_string(), message: error.to_string(), span: None }
}
