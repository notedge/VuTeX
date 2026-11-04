//! Typst import entry points and the closed loop to SVG output.

use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    path::Path,
};

use notedown_ir::DocumentGraph;
use vutex_layout::{Diagrams, LayoutHint, document::layout_with_diagnostics};
use vutex_presentation::build_display_presentation;
use vutex_types::{ConversionReport, Diagnostic, Severity};

use crate::report::{IMPORT_FAILED, build_diagnostics, build_report, unterminated_group};

/// Outcome of a Typst import: the document graph, its conversion report, and
/// the diagnostics the lowering produced.
#[derive(Debug, Clone, PartialEq)]
pub struct TypstDocument {
    /// Semantic document graph built by the Notedown Typst lowering.
    pub graph: DocumentGraph,
    /// Shared conversion report projected from the graph.
    pub report: ConversionReport,
    /// Structured diagnostics, including recovered source spans.
    pub diagnostics: Vec<Diagnostic>,
}

/// Outcome of the closed loop from Typst source to SVG output.
#[derive(Debug, Clone, PartialEq)]
pub struct TypstRender {
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

/// Typst import failure that cannot produce a document graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypstError {
    /// The diagnostic that explains the failure.
    pub diagnostic: Diagnostic,
}

impl TypstError {
    /// Stable diagnostic code of the failure.
    pub fn code(&self) -> &str {
        &self.diagnostic.code
    }

    /// Human-readable failure message.
    pub fn message(&self) -> &str {
        &self.diagnostic.message
    }

    pub(crate) fn import_failed(message: impl Into<String>) -> Self {
        Self {
            diagnostic: Diagnostic {
                code: IMPORT_FAILED.to_string(),
                severity: Severity::Error,
                message: message.into(),
                span: None,
            },
        }
    }
}

impl From<Diagnostic> for TypstError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self { diagnostic }
    }
}

impl Display for TypstError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(formatter, "{}: {}", self.diagnostic.code, self.diagnostic.message)
    }
}

impl std::error::Error for TypstError {}

/// Import Typst source into a Notedown document graph with a conversion report.
///
/// The report and diagnostics never abort the document for a construct that
/// stays opaque. A dynamic construct whose group continues past its first line
/// is refused, because the current lowering would otherwise turn the rest of the
/// group into page text.
pub fn import_source(label: &str, source: &str) -> Result<TypstDocument, TypstError> {
    let graph = notedown_formats::import::typst::import_typst_bytes(label, source)
        .map_err(|error| TypstError::import_failed(error.to_string()))?;
    if let Some(diagnostic) = unterminated_group(&graph, source) {
        return Err(TypstError::from(diagnostic));
    }
    let report = build_report(&graph);
    let diagnostics = build_diagnostics(&graph, source);
    Ok(TypstDocument { graph, report, diagnostics })
}

/// Read a Typst file and import it with the file path as the document label.
pub fn import_path(path: impl AsRef<Path>) -> Result<TypstDocument, TypstError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path)
        .map_err(|error| TypstError::import_failed(format!("failed to read {}: {error}", path.display())))?;
    import_source(&path.display().to_string(), &source)
}

/// Import Typst source and run it through layout, presentation, and SVG output.
pub fn render_svg(label: &str, source: &str) -> Result<TypstRender, TypstError> {
    let TypstDocument { graph, report, mut diagnostics } = import_source(label, source)?;
    let (plan, layout_diagnostics) = layout_with_diagnostics(&graph, &LayoutHint::a4(), &Diagrams::new());
    diagnostics.extend(layout_diagnostics);
    let display = build_display_presentation(&plan);
    let pages = display.pages.len();
    let items = display.pages.iter().map(|page| page.items.len()).sum();
    let svg = vutex_render_svg::render_svg(&display).map_err(TypstError::from)?;
    Ok(TypstRender { svg, report, diagnostics, pages, items })
}
