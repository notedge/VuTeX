//! Fine-grained document primitives shared by every host surface.
//!
//! The Node binding and the TypeScript CLI only orchestrate these calls. Each
//! function takes data in and returns data out:
//!
//! * [`parse_document`] turns source bytes plus an explicit format into a
//!   document graph.
//! * [`lower_document`] resolves diagram and footnote nodes into a layout input.
//! * [`build_display`] breaks the lowered graph into pages and keeps the scene
//!   in memory for the emission primitives.
//! * [`render_display_svg`], [`render_display_snapshot_html`],
//!   [`render_flow_html`], and [`render_display_pdf`] each emit one format and
//!   embed the bundled fonts themselves.
//! * [`summarize_document`] and [`assemble_report`] build report payloads.
//!
//! No primitive reads an input path, writes an output file, prints a message,
//! or returns an exit code. Paths are only labels used in diagnostics.

use std::collections::BTreeMap;
use std::path::PathBuf;

use document_formats::FormatError;
use document_formats::import::{
    docx::import_docx_bytes, epub::import_epub_bytes, html::import_html_bytes,
    markdown::import_markdown_bytes, notedown::import_notedown_bytes, tex::import_tex_bytes,
    typst::import_typst_bytes,
};
use document_ir::{Block, DocumentGraph};
use serde::{Deserialize, Serialize};
use vutex_compose::{Diagnostic as RenderDiagnostic, Severity};
use vutex_layout::hint::ChromeHeader;
use vutex_layout::{Diagrams, layout_with_metrics};
use vutex_presentation::{
    DisplayPresentation, FlowPresentation, build_display_presentation, build_flow_presentation,
};
use vutex_render_html::{render_html_reflow, render_html_snapshot};
use vutex_render_pdf::{PdfFontPolicy, PdfResources, render_pdf_with_resources_and_diagnostics};
use vutex_render_svg::render_svg;
use vutex_text::math::MathFontFamily;

use crate::{
    diagram::lower_diagrams,
    footnote::{FootnoteLowering, absorb_definition_continuations, lower_footnotes},
    svg_fonts::{embed_body_fonts, embed_html_fonts},
    theme::Theme,
    tikz::lower_tikz_diagrams,
};

/// Report schema version stamped on every report payload.
pub const REPORT_SCHEMA_VERSION: &str = "vutex.report/v0";
/// Diagnostic code for an import format that has no adapter.
pub const INPUT_FORMAT_UNSUPPORTED: &str = "VUTEX_INPUT_FORMAT_UNSUPPORTED";
/// Diagnostic code for a failed import.
pub const IMPORT_FAILED: &str = "VUTEX_IMPORT_FAILED";
/// Diagnostic code for a failed render.
pub const RENDER_FAILED: &str = "VUTEX_RENDER_FAILED";
/// Diagnostic code for an input whose bytes are not valid UTF-8 text.
pub const INPUT_TEXT_INVALID: &str = "VUTEX_INPUT_TEXT_INVALID";

/// Import format resolved from the caller or the path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// Markdown source text.
    Markdown,
    /// Notedown source text.
    Notedown,
    /// HTML source text or file.
    Html,
    /// TeX/LaTeX source text.
    Tex,
    /// Typst source text.
    Typst,
    /// Office Open XML WordprocessingML package.
    Docx,
    /// EPUB OCF package.
    Epub,
    /// No supported format could be resolved.
    Unknown,
}

impl ImportFormat {
    /// Format id written into `inputs[].format` and selected by callers.
    pub fn id(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Notedown => "notedown",
            Self::Html => "html",
            Self::Tex => "tex",
            Self::Typst => "typst",
            Self::Docx => "docx",
            Self::Epub => "epub",
            Self::Unknown => "unknown",
        }
    }

    /// Resolve an explicit `input` option value.
    pub fn from_option(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "markdown" | "md" => Self::Markdown,
            "notedown" | "nd" => Self::Notedown,
            "html" | "htm" => Self::Html,
            "tex" | "latex" | "ltx" => Self::Tex,
            "typst" | "typ" => Self::Typst,
            "docx" => Self::Docx,
            "epub" => Self::Epub,
            _ => Self::Unknown,
        }
    }

    /// Infer the format from the input path extension.
    pub fn from_path(path: &str) -> Self {
        let extension = std::path::Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" => Self::Markdown,
            "nd" | "notedown" => Self::Notedown,
            "html" | "htm" => Self::Html,
            "tex" | "latex" | "ltx" => Self::Tex,
            "typst" | "typ" => Self::Typst,
            "docx" => Self::Docx,
            "epub" => Self::Epub,
            _ => Self::Unknown,
        }
    }
}

/// Byte range inside the input source that a diagnostic refers to.
///
/// Offsets are UTF-8 byte offsets into the block source the producer lowered,
/// so a host can slice the authored text and show exactly what was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DiagnosticSpan {
    /// First byte offset of the diagnosed range.
    pub start: usize,
    /// Exclusive end byte offset of the diagnosed range.
    pub end: usize,
}

/// Diagnostic record serialized into a report payload.
#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticRecord {
    /// Stable diagnostic code.
    pub code: String,
    /// Severity label (`info`, `warning`, or `error`).
    pub severity: String,
    /// Human-readable message.
    pub message: String,
    /// Pipeline stage that produced the diagnostic.
    pub stage: String,
    /// Input path the diagnostic refers to.
    pub path: Option<String>,
    /// Source byte range the producer reported, when it reported one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<DiagnosticSpan>,
}

/// Result of parsing source bytes into the document IR.
#[derive(Debug)]
pub struct ParsedDocument {
    /// Imported document graph.
    pub graph: DocumentGraph,
    /// Resolved format.
    pub format: ImportFormat,
    /// Diagnostics raised while importing.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Result of lowering diagrams and footnotes into layout input.
pub struct LoweredDocument {
    /// Graph whose footnote references and definitions are projected.
    pub graph: DocumentGraph,
    /// Resolved diagram scenes.
    pub diagrams: Diagrams,
    /// Footnote counts written into the report.
    pub footnotes: FootnoteCounts,
    /// Diagnostics raised while lowering.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Footnote counts carried by the report.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FootnoteCounts {
    /// Number of references the contract carried.
    pub references: usize,
    /// Number of authored definitions the contract carried.
    pub definitions: usize,
    /// Number of notes that reached the endnote area.
    pub notes: usize,
}

impl From<&FootnoteLowering> for FootnoteCounts {
    fn from(lowering: &FootnoteLowering) -> Self {
        Self { references: lowering.references, definitions: lowering.definitions, notes: lowering.notes }
    }
}

/// Paginated display scene with diagnostics.
pub struct DisplayScene {
    /// Pages the layout pass produced.
    pub display: DisplayPresentation,
    /// Typography theme the scene was laid out and paints with.
    pub theme: Theme,
    /// MATH family every formula was laid out with, when a document named one.
    pub math_font: Option<&'static str>,
    /// Diagnostics raised while lowering and laying out.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Semantic reflow scene with the theme it paints with.
pub struct FlowScene {
    /// Reflow blocks in document order.
    pub flow: FlowPresentation,
    /// Typography theme the payload paints with.
    pub theme: Theme,
}

/// Typography and font facts a host can show without rendering.
#[derive(Debug, Serialize)]
pub struct ThemeFacts {
    /// Theme id.
    pub id: &'static str,
    /// Body text size in points.
    pub base_font_size: f64,
    /// Line box as a multiple of the body size.
    pub line_height: f64,
    /// Left and right margin in points.
    pub side_margin: f64,
    /// Usable width in points.
    pub content_width: f64,
    /// Latin reading face.
    pub latin_family: &'static str,
    /// Han reading face.
    pub cjk_family: &'static str,
    /// Fixed-advance face.
    pub mono_family: &'static str,
}

/// Parse source bytes with the resolved format adapter.
pub fn parse_document(label: &str, format: ImportFormat, bytes: &[u8]) -> Result<ParsedDocument, DiagnosticRecord> {
    if format == ImportFormat::Unknown {
        return Err(error(INPUT_FORMAT_UNSUPPORTED, format!("unsupported input format for {label}"), "import", label));
    }
    let mut graph = match format {
        ImportFormat::Markdown => text_of(bytes, format, label).and_then(|text| import_markdown_bytes(label, text)),
        ImportFormat::Notedown => text_of(bytes, format, label).and_then(|text| import_notedown_bytes(label, text)),
        ImportFormat::Html => import_html_bytes(label, bytes),
        ImportFormat::Tex => text_of(bytes, format, label).and_then(|text| import_tex_bytes(label, text)),
        ImportFormat::Typst => text_of(bytes, format, label).and_then(|text| import_typst_bytes(label, text)),
        ImportFormat::Docx => import_docx_bytes(label, bytes),
        ImportFormat::Epub => import_epub_bytes(label, bytes),
        ImportFormat::Unknown => unreachable!("unknown formats return before the adapter match"),
    }
    .map_err(|failure| error(IMPORT_FAILED, failure.to_string(), "import", label))?;
    // GFM keeps a definition open over blank lines while its paragraphs stay
    // indented. The Markdown adapter lowers those paragraphs as code blocks, so
    // fold them back into the authored definition before anyone lays it out.
    if format == ImportFormat::Markdown {
        if let Ok(text) = std::str::from_utf8(bytes) {
            absorb_definition_continuations(&mut graph, text);
        }
    }
    Ok(ParsedDocument { graph, format, diagnostics: Vec::new() })
}

fn text_of<'a>(bytes: &'a [u8], format: ImportFormat, label: &str) -> Result<&'a str, FormatError> {
    std::str::from_utf8(bytes).map_err(|failure| {
        FormatError::invalid_input(format!("{label} is not valid UTF-8 {} text: {failure}", format.id()))
    })
}

/// Lower every diagram and footnote in the graph.
pub fn lower_document(graph: DocumentGraph, label: &str) -> LoweredDocument {
    let mut diagrams = lower_diagrams(&graph, label);
    lower_tikz_diagrams(&graph, label, &mut diagrams);
    let footnotes = lower_footnotes(&graph, label);
    let counts = FootnoteCounts::from(&footnotes);
    let FootnoteLowering { graph, diagnostics: footnote_diagnostics, .. } = footnotes;
    let mut diagnostics = diagrams.diagnostics;
    diagnostics.extend(footnote_diagnostics);
    LoweredDocument { graph, diagrams: diagrams.diagrams, footnotes: counts, diagnostics }
}

/// Page geometry contributed by a template resource package.
///
/// Margins are optional so a template that only names a page size keeps the
/// margins of the active theme. The remaining fields are the template's second
/// layer: page chrome overrides, section page breaks, and a body style reflow.
/// Every override is optional, so a template that declares only geometry keeps
/// exactly the theme behaviour it had before.
#[derive(Debug, Clone, Copy, Default)]
pub struct TemplateGeometry {
    /// Page width in PostScript points.
    pub page_width: f64,
    /// Page height in PostScript points.
    pub page_height: f64,
    /// Top margin in PostScript points.
    pub margin_top: Option<f64>,
    /// Right margin in PostScript points.
    pub margin_right: Option<f64>,
    /// Bottom margin in PostScript points.
    pub margin_bottom: Option<f64>,
    /// Left margin in PostScript points.
    pub margin_left: Option<f64>,
    /// Header source: `title`, `none`, or literal text.
    pub chrome_header: Option<&'static str>,
    /// Whether pages carry a folio.
    pub chrome_folio: Option<bool>,
    /// 1-based page index where chrome starts.
    pub chrome_start_page: Option<usize>,
    /// Horizontal alignment of the running header.
    pub header_align: Option<vutex_layout::hint::Align>,
    /// Heading levels that start a new page, H1 to H4.
    pub section_breaks: [bool; 4],
    /// Body text size in points.
    pub body_size: Option<f64>,
    /// Line box as a multiple of the body size.
    pub line_height: Option<f64>,
    /// Heading size as a multiple of the body size, H1 to H4.
    pub heading_scales: Option<[f64; 4]>,
}

/// Break the lowered graph into pages under the selected theme.
///
/// `math_font` is the OpenType MATH family a document declared. Without one the
/// pass keeps the host-installed default, so a host that names no math package
/// observes exactly the face it installed at startup.
///
/// `template` overrides the page geometry of the theme, which is how a
/// document declared template package reaches the layout pass.
pub fn build_display(
    lowered: LoweredDocument,
    theme: Theme,
    math_font: Option<MathFontFamily>,
    template: Option<TemplateGeometry>,
) -> DisplayScene {
    let mut hint = theme.layout_hint();
    hint.math_font = math_font;
    if let Some(geometry) = template {
        hint.page_width = geometry.page_width;
        hint.page_height = geometry.page_height;
        if let Some(value) = geometry.margin_top {
            hint.margin.top = value;
        }
        if let Some(value) = geometry.margin_right {
            hint.margin.right = value;
        }
        if let Some(value) = geometry.margin_bottom {
            hint.margin.bottom = value;
        }
        if let Some(value) = geometry.margin_left {
            hint.margin.left = value;
        }
        if let Some(header) = geometry.chrome_header {
            hint.chrome.header = match header {
                "title" => ChromeHeader::Title,
                "none" => ChromeHeader::None,
                text => ChromeHeader::Literal(text),
            };
        }
        if let Some(folio) = geometry.chrome_folio {
            hint.chrome.folio = folio;
        }
        if let Some(start) = geometry.chrome_start_page {
            hint.chrome.start_page = start;
        }
        if let Some(align) = geometry.header_align {
            hint.style.header_align = align;
        }
        hint.section_breaks = geometry.section_breaks;
        if let Some(size) = geometry.body_size {
            hint.base_font_size = size;
        }
        if let Some(line_height) = geometry.line_height {
            hint.line_height = line_height;
        }
        if let Some(scales) = geometry.heading_scales {
            hint.style.heading_scales = scales;
        }
    }
    let measurements = theme.body_metrics();
    let (plan, layout_diagnostics) =
        layout_with_metrics(&lowered.graph, &hint, &lowered.diagrams, measurements.get());
    let display = build_display_presentation(&plan);
    let mut diagnostics = lowered.diagnostics;
    diagnostics.extend(layout_diagnostics.iter().map(|diagnostic| stage_record(diagnostic, "layout", None)));
    DisplayScene { display, theme, math_font: math_font.map(MathFontFamily::label), diagnostics }
}

/// Build the semantic reflow scene for HTML output.
pub fn build_flow(graph: &DocumentGraph, theme: Theme) -> FlowScene {
    FlowScene { flow: build_flow_presentation(graph), theme }
}

/// Emit a display scene as SVG bytes with the bundled fonts embedded.
///
/// This is the only document-to-SVG path in the repository. Font embedding
/// happens here, so no caller can emit SVG without it.
pub fn render_display_svg(scene: &DisplayScene) -> Result<String, DiagnosticRecord> {
    let svg = render_svg(&scene.display).map_err(|failure| {
        error(RENDER_FAILED, failure.message, "render", "")
    })?;
    Ok(embed_body_fonts(&svg, scene.theme))
}

/// Emit a display scene as fixed-geometry snapshot HTML with bundled fonts.
pub fn render_display_snapshot_html(scene: &DisplayScene) -> Result<String, DiagnosticRecord> {
    let html = render_html_snapshot(&scene.display)
        .map_err(|failure| error(RENDER_FAILED, failure.message, "render", ""))?;
    Ok(embed_html_fonts(&html, scene.theme))
}

/// Emit a flow scene as reflow HTML with bundled fonts.
pub fn render_flow_html(scene: &FlowScene) -> Result<String, DiagnosticRecord> {
    let html = render_html_reflow(&scene.flow)
        .map_err(|failure| error(RENDER_FAILED, failure.message, "render", ""))?;
    Ok(embed_html_fonts(&html, scene.theme))
}

/// Emit a display scene as PDF bytes.
///
/// The bundled policy is fixed: a delivered PDF never depends on faces the
/// reader happens to have installed, so no system CJK face can paint a page.
pub fn render_display_pdf(
    scene: &DisplayScene,
    resources_json: Option<&str>,
) -> Result<PdfPrimitiveOutcome, DiagnosticRecord> {
    let settings = PdfSettings::parse(resources_json);
    let resources = settings.resources();
    let outcome =
        render_pdf_with_resources_and_diagnostics(&scene.display, &PdfFontPolicy::Provided(crate::assets::body_set()), &resources)
        .map_err(|failure| error(RENDER_FAILED, failure.message, "render", ""))?;
    let diagnostics = outcome
        .diagnostics
        .iter()
        .map(|diagnostic| stage_record(diagnostic, "render", None))
        .collect();
    Ok(PdfPrimitiveOutcome { pdf: outcome.pdf, diagnostics })
}

/// PDF bytes plus the diagnostics the renderer collected.
pub struct PdfPrimitiveOutcome {
    /// Rendered PDF program.
    pub pdf: Vec<u8>,
    /// Render-layer diagnostics.
    pub diagnostics: Vec<DiagnosticRecord>,
}

/// Embed bundled fonts into an SVG payload a host already holds.
///
/// Kept as a separate primitive so a host that stores scene SVG can upgrade it
/// without re-running layout. The document emission path does not need it.
pub fn embed_svg_fonts(svg: &str, theme: Theme) -> String {
    embed_body_fonts(svg, theme)
}

/// Structural block counts for the inspect report.
pub fn summarize_document(graph: &DocumentGraph) -> serde_json::Value {
    let (mut sections, mut paragraphs, mut lists, mut code, mut quotes, mut tables, mut math, mut diagrams, mut charts) =
        (0, 0, 0, 0, 0, 0, 0, 0, 0);
    for node in &graph.blocks {
        match &node.block {
            Block::Section { .. } => sections += 1,
            Block::Paragraph { .. } => paragraphs += 1,
            Block::List { .. } => lists += 1,
            Block::Code { .. } => code += 1,
            Block::Quote { .. } => quotes += 1,
            Block::Table { .. } => tables += 1,
            Block::Math { .. } => math += 1,
            Block::Diagram { .. } => diagrams += 1,
            Block::Chart { .. } => charts += 1,
            Block::Opaque { .. } | Block::RawHtml { .. } => {}
        }
    }
    serde_json::json!({
        "title": graph.metadata.title.clone(),
        "language": graph.metadata.language.clone(),
        "blocks": graph.blocks.len(),
        "sections": sections,
        "paragraphs": paragraphs,
        "lists": lists,
        "code": code,
        "quotes": quotes,
        "tables": tables,
        "math": math,
        "diagrams": diagrams,
        "charts": charts,
        "assets": graph.assets.len(),
        "relations": graph.relations.len(),
        "coverage_complete": graph.coverage.complete,
        "loss": graph.coverage.loss.len(),
    })
}

/// Assemble a report from caller-supplied sections.
///
/// Every section is data the caller already computed, so this primitive has no
/// opinion about success, failure, or process status.
pub fn assemble_report(
    operation: &str,
    inputs_json: &str,
    diagnostics_json: &str,
    output_json: Option<&str>,
    extra_json: Option<&str>,
) -> String {
    let mut report = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "operation": operation,
        "inputs": serde_json::from_str::<serde_json::Value>(inputs_json).unwrap_or_else(|_| serde_json::json!([])),
        "diagnostics": serde_json::from_str::<serde_json::Value>(diagnostics_json).unwrap_or_else(|_| serde_json::json!([])),
    });
    if let Some(output) = output_json.filter(|value| !value.trim().is_empty()) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(output) {
            report["output"] = value;
        }
    }
    if let Some(extra) = extra_json.filter(|value| !value.trim().is_empty()) {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(extra) {
            for (key, value) in map {
                report[&key] = value;
            }
        }
    }
    report.to_string()
}

/// Typography facts for one theme.
pub fn theme_facts(theme: Theme) -> ThemeFacts {
    let hint = theme.layout_hint();
    ThemeFacts {
        id: theme.id(),
        base_font_size: theme.base_font_size(),
        line_height: theme.line_height(),
        side_margin: hint.margin.left,
        content_width: hint.content_width(),
        latin_family: match theme {
            Theme::Traditional => "Tinos",
            Theme::Baseline | Theme::Editorial | Theme::Classic => "Noto Serif SC",
        },
        cjk_family: "Noto Serif SC",
        mono_family: "Noto Sans Mono",
    }
}

/// PDF resource settings decoded from the caller payload.
#[derive(Debug, Default, Deserialize)]
struct PdfSettings {
    /// Base directory for relative image sources.
    base_dir: Option<String>,
    /// Base64 payloads keyed by source.
    resources: Option<BTreeMap<String, String>>,
}

impl PdfSettings {
    fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some(json) if !json.trim().is_empty() => serde_json::from_str(json).unwrap_or_default(),
            _ => Self::default(),
        }
    }

    fn resources(&self) -> PdfResources {
        let mut resources = PdfResources::new();
        if let Some(base_dir) = self.base_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            resources = resources.with_base_dir(PathBuf::from(base_dir));
        }
        if let Some(supplied) = &self.resources {
            for (source, payload) in supplied {
                resources = resources.with_supplied(source.clone(), decode_payload(payload).unwrap_or_default());
            }
        }
        resources
    }
}

/// Build one error diagnostic record.
pub fn error(code: &str, message: String, stage: &str, path: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        code: code.to_string(),
        severity: "error".to_string(),
        message,
        stage: stage.to_string(),
        path: (!path.is_empty()).then(|| path.to_string()),
        span: None,
    }
}

/// Convert a pipeline diagnostic into a report record.
pub fn stage_record(diagnostic: &RenderDiagnostic, stage: &str, path: Option<&str>) -> DiagnosticRecord {
    DiagnosticRecord {
        code: diagnostic.code.clone(),
        severity: severity_label(diagnostic.severity),
        message: diagnostic.message.clone(),
        stage: stage.to_string(),
        path: path.map(str::to_string),
        span: None,
    }
}

fn severity_label(severity: Severity) -> String {
    match severity {
        Severity::Info => "info".to_string(),
        Severity::Warning => "warning".to_string(),
        Severity::Error => "error".to_string(),
    }
}

/// Decode one caller-supplied payload: bare base64 or a `data:` URI carrying it.
fn decode_payload(payload: &str) -> Option<Vec<u8>> {
    let payload = payload.trim();
    let body = if payload.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:")) {
        payload.split_once(',').map(|(_, body)| body)?
    } else {
        payload
    };
    base64_decode(body)
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_formats_report_an_error_without_touching_the_filesystem() {
        let failure = parse_document("x.bin", ImportFormat::Unknown, b"data").unwrap_err();
        assert_eq!(failure.code, INPUT_FORMAT_UNSUPPORTED);
        assert_eq!(failure.severity, "error");
    }

    #[test]
    fn markdown_bytes_parse_into_a_graph() {
        let parsed = parse_document("doc.md", ImportFormat::Markdown, b"# Title\n\nBody.\n").unwrap();
        assert_eq!(parsed.format, ImportFormat::Markdown);
        assert!(!parsed.graph.blocks.is_empty());
    }
}
