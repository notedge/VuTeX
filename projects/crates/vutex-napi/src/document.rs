//! Document pipeline: import, layout, presentation lowering, and rendering.
//!
//! The Node binding is a thin wrapper around this module so the whole
//! `import -> layout -> presentation -> output` chain stays testable as plain
//! Rust.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use notedown_formats::{
    FormatError,
    import::{
        html::import_html_bytes, markdown::import_markdown, notedown::import_notedown, tex::import_tex, typst::import_typst,
    },
};
use notedown_ir::{Block, DocumentGraph};
use serde::{Deserialize, Serialize};
use vutex_layout::{LayoutHint, document::layout_with_diagrams};
use vutex_presentation::builder::{build_display_presentation, build_flow_presentation};
use vutex_render_html::{render_html_reflow, render_html_snapshot};
use vutex_render_pdf::{PdfFontPolicy, PdfResources, render_pdf_with_resources_and_diagnostics};
use vutex_types::{Diagnostic as RenderDiagnostic, Severity};
use vutex_render_svg::render_svg;

use crate::{diagram::lower_mermaid_diagrams, tikz::lower_tikz_diagrams};

/// Report schema version stamped on every report payload.
pub const REPORT_SCHEMA_VERSION: &str = "vutex.report/v0";
/// Diagnostic code for a missing input path.
pub const INPUT_NOT_FOUND: &str = "VUTEX_INPUT_NOT_FOUND";
/// Diagnostic code for an import format that has no adapter.
pub const INPUT_FORMAT_UNSUPPORTED: &str = "VUTEX_INPUT_FORMAT_UNSUPPORTED";
/// Diagnostic code for an output format the pipeline cannot emit.
pub const OUTPUT_FORMAT_UNSUPPORTED: &str = "VUTEX_OUTPUT_FORMAT_UNSUPPORTED";
/// Diagnostic code for a failed import.
pub const IMPORT_FAILED: &str = "VUTEX_IMPORT_FAILED";
/// Diagnostic code for a failed render.
pub const RENDER_FAILED: &str = "VUTEX_RENDER_FAILED";

/// Import format resolved from the caller override or the path extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// Markdown source text.
    Markdown,
    /// Notedown source text.
    Notedown,
    /// HTML source text.
    Html,
    /// TeX/LaTeX source text.
    Tex,
    /// Typst source text.
    Typst,
    /// No supported format could be resolved.
    Unknown,
}

impl ImportFormat {
    /// Format id written into `inputs[].format`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Notedown => "notedown",
            Self::Html => "html",
            Self::Tex => "tex",
            Self::Typst => "typst",
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
            _ => Self::Unknown,
        }
    }

    /// Infer the format from the input path extension.
    pub fn from_path(path: &str) -> Self {
        let extension = Path::new(path).extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
        match extension.as_str() {
            "md" | "markdown" => Self::Markdown,
            "nd" | "notedown" => Self::Notedown,
            "html" | "htm" => Self::Html,
            "tex" | "latex" | "ltx" => Self::Tex,
            "typst" | "typ" => Self::Typst,
            _ => Self::Unknown,
        }
    }
}

/// Fixed output format selected for a render request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// One stacked SVG document.
    Svg,
    /// Semantic reflow HTML.
    Html,
    /// Fixed-geometry snapshot HTML.
    Snapshot,
    /// Paginated PDF.
    Pdf,
}

impl OutputFormat {
    /// Resolve an output format id, returning `None` when unsupported.
    pub fn from_option(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "svg" => Some(Self::Svg),
            "html" => Some(Self::Html),
            "snapshot" => Some(Self::Snapshot),
            "pdf" => Some(Self::Pdf),
            _ => None,
        }
    }

    /// Canonical format id written into the response and report.
    pub fn id(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Html => "html",
            Self::Snapshot => "snapshot",
            Self::Pdf => "pdf",
        }
    }
}

/// Render options decoded from the `options_json` payload.
#[derive(Debug, Default, Deserialize)]
pub struct RenderOptions {
    /// Requested output format id.
    pub output: Option<String>,
    /// Requested layout profile name, accepted for forward compatibility.
    pub profile: Option<String>,
    /// Import format override.
    pub input: Option<String>,
    /// Image payloads the host already fetched, keyed by source string.
    ///
    /// Each value is base64, either bare or wrapped in a `data:` URI. The key is
    /// matched verbatim against the source a document carries, so an `https://…`
    /// reference embeds without the renderer touching the network. A value that
    /// is not a PNG or JPEG falls back to the placeholder, and a source present
    /// here never reaches the filesystem.
    pub resources: Option<BTreeMap<String, String>>,
    /// Base directory for relative image sources.
    ///
    /// Defaults to the input document's parent directory, so a Markdown file
    /// that references `figure.png` finds its sibling image even when the
    /// process runs from another directory.
    pub base_dir: Option<String>,
}

impl RenderOptions {
    /// Decode options JSON; malformed or empty payloads fall back to defaults.
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some(json) if !json.trim().is_empty() => serde_json::from_str(json).unwrap_or_default(),
            _ => Self::default(),
        }
    }
}

/// Render outcome mapped into the Node-API response by the binding.
#[derive(Debug)]
pub struct RenderOutcome {
    /// Process exit code; zero means success.
    pub exit_code: u32,
    /// Resolved output format id.
    pub output_format: String,
    /// SVG payload for `svg` output.
    pub svg: Option<String>,
    /// HTML payload for `html` and `snapshot` output.
    pub html: Option<String>,
    /// PDF bytes for `pdf` output.
    pub pdf: Option<Vec<u8>>,
    /// Serialized render report JSON.
    pub report_json: String,
}

/// Inspect outcome mapped into the Node-API response by the binding.
#[derive(Debug)]
pub struct InspectOutcome {
    /// Resolved input format id.
    pub input_format: String,
    /// Input path that was inspected.
    pub path: String,
    /// Serialized inspect report JSON.
    pub report_json: String,
}

/// Diagnostic record serialized into a report payload.
#[derive(Debug, Serialize)]
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
}

/// Import a document and render it into SVG, reflow HTML, snapshot HTML, or PDF.
pub fn render_document(input_path: &str, options_json: Option<&str>) -> RenderOutcome {
    let options = RenderOptions::parse(options_json);
    let requested =
        options.output.as_deref().map(str::trim).filter(|value| !value.is_empty()).unwrap_or("svg").to_ascii_lowercase();
    let import_format = match options.input.as_deref() {
        Some(value) => ImportFormat::from_option(value),
        None => ImportFormat::from_path(input_path),
    };
    let format_label = import_format.id();

    let Some(output_format) = OutputFormat::from_option(&requested)
    else {
        let record = error(OUTPUT_FORMAT_UNSUPPORTED, format!("unsupported output format: {requested}"), "render", input_path);
        return render_failure(input_path, requested, format_label, record);
    };

    if !Path::new(input_path).exists() {
        let record = error(INPUT_NOT_FOUND, format!("input path does not exist: {input_path}"), "render", input_path);
        return render_failure(input_path, output_format.id().to_string(), format_label, record);
    }

    if import_format == ImportFormat::Unknown {
        let record = error(
            INPUT_FORMAT_UNSUPPORTED,
            unsupported_input_message(input_path, options.input.as_deref()),
            "import",
            input_path,
        );
        return render_failure(input_path, output_format.id().to_string(), format_label, record);
    }

    let graph = match import_document(input_path, import_format) {
        Ok(graph) => graph,
        Err(failure) => {
            let record = error(IMPORT_FAILED, failure.to_string(), "import", input_path);
            return render_failure(input_path, output_format.id().to_string(), format_label, record);
        }
    };

    let mut diagrams = lower_mermaid_diagrams(&graph, input_path);
    lower_tikz_diagrams(&graph, input_path, &mut diagrams);
    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams.diagrams);
    let display = build_display_presentation(&plan);
    let pages = display.pages.len();
    let items = display.pages.iter().map(|page| page.items.len()).sum::<usize>();

    let (svg, html, pdf) = match output_format {
        OutputFormat::Svg => match render_svg(&display) {
            Ok(svg) => (Some(svg), None, None),
            Err(failure) => {
                let record = error(RENDER_FAILED, failure.message, "render", input_path);
                return render_failure(input_path, output_format.id().to_string(), format_label, record);
            }
        },
        OutputFormat::Html => {
            let flow = build_flow_presentation(&graph);
            match render_html_reflow(&flow) {
                Ok(html) => (None, Some(html), None),
                Err(failure) => {
                    let record = error(RENDER_FAILED, failure.message, "render", input_path);
                    return render_failure(input_path, output_format.id().to_string(), format_label, record);
                }
            }
        }
        OutputFormat::Snapshot => match render_html_snapshot(&display) {
            Ok(html) => (None, Some(html), None),
            Err(failure) => {
                let record = error(RENDER_FAILED, failure.message, "render", input_path);
                return render_failure(input_path, output_format.id().to_string(), format_label, record);
            }
        },
        OutputFormat::Pdf => {
            let resources = pdf_resources(input_path, &options);
            match render_pdf_with_resources_and_diagnostics(&display, &PdfFontPolicy::Discover, &resources) {
                Ok(outcome) => {
                    for d in outcome.diagnostics {
                        diagrams.diagnostics.push(render_diagnostic_to_record(&d, input_path));
                    }
                    (None, None, Some(outcome.pdf))
                }
                Err(failure) => {
                    let record = error(RENDER_FAILED, failure.message, "render", input_path);
                    return render_failure(input_path, output_format.id().to_string(), format_label, record);
                }
            }
        }
    };

    let report_json = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "operation": "render",
        "status": "success",
        "inputs": [{ "path": input_path, "format": format_label }],
        "diagnostics": diagrams.diagnostics,
        "output": { "format": output_format.id(), "pages": pages, "items": items },
    })
    .to_string();

    RenderOutcome { exit_code: 0, output_format: output_format.id().to_string(), svg, html, pdf, report_json }
}

/// Import a document and report its structural summary.
pub fn inspect_document(input_path: &str) -> InspectOutcome {
    let import_format = ImportFormat::from_path(input_path);
    let format_label = import_format.id();

    if !Path::new(input_path).exists() {
        let record = error(INPUT_NOT_FOUND, format!("input path does not exist: {input_path}"), "inspect", input_path);
        return InspectOutcome {
            input_format: format_label.to_string(),
            path: input_path.to_string(),
            report_json: failure_report("inspect", input_path, format_label, record),
        };
    }

    if import_format == ImportFormat::Unknown {
        let record = error(INPUT_FORMAT_UNSUPPORTED, unsupported_input_message(input_path, None), "import", input_path);
        return InspectOutcome {
            input_format: format_label.to_string(),
            path: input_path.to_string(),
            report_json: failure_report("inspect", input_path, format_label, record),
        };
    }

    let graph = match import_document(input_path, import_format) {
        Ok(graph) => graph,
        Err(failure) => {
            let record = error(IMPORT_FAILED, failure.to_string(), "import", input_path);
            return InspectOutcome {
                input_format: format_label.to_string(),
                path: input_path.to_string(),
                report_json: failure_report("inspect", input_path, format_label, record),
            };
        }
    };

    let report_json = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "operation": "inspect",
        "status": "success",
        "inputs": [{ "path": input_path, "format": format_label }],
        "diagnostics": Vec::<DiagnosticRecord>::new(),
        "document": summarize_document(&graph),
    })
    .to_string();

    InspectOutcome { input_format: format_label.to_string(), path: input_path.to_string(), report_json }
}

/// Import one document file with the resolved format adapter.
fn import_document(input_path: &str, format: ImportFormat) -> Result<DocumentGraph, FormatError> {
    let path = Path::new(input_path);
    match format {
        ImportFormat::Markdown => import_markdown(path),
        ImportFormat::Notedown => import_notedown(path),
        ImportFormat::Html => {
            let bytes = std::fs::read(path)
                .map_err(|error| FormatError::invalid_input(format!("failed to read {}: {error}", path.display())))?;
            import_html_bytes(&path.display().to_string(), &bytes)
        }
        ImportFormat::Tex => import_tex(path),
        ImportFormat::Typst => import_typst(path),
        ImportFormat::Unknown => Err(FormatError::invalid_input(format!("unsupported input format for {}", path.display()))),
    }
}

/// Message for an unsupported import format, naming the override when present.
fn unsupported_input_message(input_path: &str, override_value: Option<&str>) -> String {
    match override_value {
        Some(value) => format!("unsupported input format `{value}` for path: {input_path}"),
        None => format!("inferred input format from path extension is not supported: {input_path}"),
    }
}

/// Build the PDF image resource context for one render request.
///
/// An explicit `base_dir` wins, otherwise relative sources resolve against the
/// input document's parent directory. A payload that is not valid base64 is
/// still registered for its source, so the slot falls back to the placeholder
/// instead of the renderer reading the filesystem for a source the host claimed.
fn pdf_resources(input_path: &str, options: &RenderOptions) -> PdfResources {
    let configured = options.base_dir.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(PathBuf::from);
    let base_dir = configured
        .or_else(|| Path::new(input_path).parent().filter(|parent| !parent.as_os_str().is_empty()).map(Path::to_path_buf));
    let mut resources = PdfResources::new();
    if let Some(base_dir) = base_dir {
        resources = resources.with_base_dir(base_dir);
    }
    if let Some(supplied) = &options.resources {
        for (source, payload) in supplied {
            resources = resources.with_supplied(source.clone(), decode_payload(payload).unwrap_or_default());
        }
    }
    resources
}

/// Decode one caller-supplied payload: bare base64 or a `data:` URI carrying it.
fn decode_payload(payload: &str) -> Option<Vec<u8>> {
    let payload = payload.trim();
    let body = if payload.get(..5).is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:")) {
        payload.split_once(',').map(|(_, body)| body)?
    }
    else {
        payload
    };
    base64_decode(body)
}

/// Decode standard base64, ignoring whitespace and stopping at the padding.
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

/// Build one error diagnostic record.
fn error(code: &str, message: String, stage: &str, path: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        code: code.to_string(),
        severity: "error".to_string(),
        message,
        stage: stage.to_string(),
        path: Some(path.to_string()),
    }
}

/// Convert a render-layer diagnostic into the pipeline report record.
fn render_diagnostic_to_record(d: &RenderDiagnostic, path: &str) -> DiagnosticRecord {
    DiagnosticRecord {
        code: d.code.clone(),
        severity: severity_label(d.severity),
        message: d.message.clone(),
        stage: "render".to_string(),
        path: Some(path.to_string()),
    }
}

/// Serialize a render severity into the report's three-value string.
fn severity_label(severity: Severity) -> String {
    match severity {
        Severity::Info => "info".to_string(),
        Severity::Warning => "warning".to_string(),
        Severity::Error => "error".to_string(),
    }
}

/// Failed render outcome carrying the requested format and one diagnostic.
fn render_failure(input_path: &str, output_format: String, format_label: &str, record: DiagnosticRecord) -> RenderOutcome {
    RenderOutcome {
        exit_code: 1,
        output_format,
        svg: None,
        html: None,
        pdf: None,
        report_json: failure_report("render", input_path, format_label, record),
    }
}

/// Report payload for a failed operation.
fn failure_report(operation: &str, input_path: &str, format_label: &str, record: DiagnosticRecord) -> String {
    serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "operation": operation,
        "status": "failed",
        "inputs": [{ "path": input_path, "format": format_label }],
        "diagnostics": [record],
    })
    .to_string()
}

/// Structural block counts for the inspect report.
fn summarize_document(graph: &DocumentGraph) -> serde_json::Value {
    let (mut sections, mut paragraphs, mut lists, mut code, mut quotes, mut tables, mut math, mut diagrams) =
        (0, 0, 0, 0, 0, 0, 0, 0);
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
            Block::Opaque { .. } => {}
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
        "assets": graph.assets.len(),
        "relations": graph.relations.len(),
        "coverage_complete": graph.coverage.complete,
        "loss": graph.coverage.loss.len(),
    })
}
