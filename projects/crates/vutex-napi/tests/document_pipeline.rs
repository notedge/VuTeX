//! Integration tests for the document import, layout, and render pipeline.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use serde_json::Value;
use vutex_napi::document::{ImportFormat, OutputFormat, inspect_document, render_document};

/// Unique scratch directory per test, removed by [`ScratchDir`] on drop.
struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn new(name: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let sequence = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vutex-napi-{}-{name}-{sequence}", std::process::id()));
        std::fs::create_dir_all(&path).expect("create scratch directory");
        Self { path }
    }

    fn write(&self, file_name: &str, content: &str) -> PathBuf {
        let path = self.path.join(file_name);
        std::fs::write(&path, content).expect("write scratch file");
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn path_string(path: &PathBuf) -> String {
    path.to_string_lossy().into_owned()
}

fn report(outcome: &vutex_napi::document::RenderOutcome) -> Value {
    serde_json::from_str(&outcome.report_json).expect("render report is valid JSON")
}

fn inspect_report(outcome: &vutex_napi::document::InspectOutcome) -> Value {
    serde_json::from_str(&outcome.report_json).expect("inspect report is valid JSON")
}

const SAMPLE: &str = "# 标题\n\n第一段正文。\n\n- 项目一\n- 项目二\n";

const INSPECT_SAMPLE: &str = "# 标题\n\n第一段正文。\n\n1. 项目一\n2. 项目二\n";

#[test]
fn markdown_renders_svg_with_page_statistics() {
    let dir = ScratchDir::new("svg");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "svg");
    assert!(outcome.html.is_none());
    let svg = outcome.svg.expect("svg payload");
    assert!(svg.contains("<svg"), "{svg}");
    assert_eq!(payload["status"], "success");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["output"]["items"].as_u64().unwrap_or(0) > 0, "{payload}");
    assert_eq!(payload["output"]["format"], "svg");
    assert_eq!(payload["inputs"][0]["format"], "markdown");
}

#[test]
fn markdown_renders_reflow_html() {
    let dir = ScratchDir::new("html");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"html"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "html");
    assert!(outcome.svg.is_none());
    let html = outcome.html.expect("html payload");
    assert!(html.contains("<article"), "{html}");
    assert!(html.contains(r#"data-vutex-block="heading""#), "{html}");
    assert_eq!(payload["output"]["format"], "html");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
}

#[test]
fn markdown_renders_snapshot_html() {
    let dir = ScratchDir::new("snapshot");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"snapshot"}"#));

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "snapshot");
    assert!(outcome.svg.is_none());
    let html = outcome.html.expect("html payload");
    assert!(html.contains(r#"data-vutex-page="1""#), "{html}");
}

#[test]
fn notedown_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("notedown");
    let path = dir.write("sample.nd", SAMPLE);

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "notedown");
    assert!(outcome.svg.expect("svg payload").contains("<svg"));
}

#[test]
fn html_file_is_imported_and_rendered() {
    let dir = ScratchDir::new("html-input");
    let path = dir.write("sample.html", "<html><body><h1>Title</h1><p>Hello world.</p></body></html>");

    let outcome = render_document(&path_string(&path), None);
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "html");
    assert!(outcome.svg.expect("svg payload").contains("<svg"));
}

#[test]
fn markdown_renders_pdf_bytes() {
    let dir = ScratchDir::new("pdf");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"pdf"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(outcome.output_format, "pdf");
    assert!(outcome.svg.is_none());
    assert!(outcome.html.is_none());
    let pdf = outcome.pdf.expect("pdf payload");
    assert!(!pdf.is_empty(), "pdf payload is never empty");
    assert!(pdf.starts_with(b"%PDF-1.7"), "pdf header: {:?}", &pdf[..pdf.len().min(16)]);
    assert!(pdf.ends_with(b"%%EOF\n"), "pdf trailer terminator");
    assert_eq!(payload["output"]["format"], "pdf");
    assert!(payload["output"]["pages"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["output"]["items"].as_u64().unwrap_or(0) > 0, "{payload}");
}

#[test]
fn missing_file_reports_input_not_found() {
    let dir = ScratchDir::new("missing");
    let path = path_string(&dir.path.join("absent.md"));

    let outcome = render_document(&path, None);

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.svg.is_none());
    assert!(outcome.html.is_none());
    assert!(outcome.report_json.contains("VUTEX_INPUT_NOT_FOUND"), "{}", outcome.report_json);
}

#[test]
fn unsupported_extension_reports_input_format_unsupported() {
    let dir = ScratchDir::new("unsupported");
    let path = dir.write("sample.xyz", "plain text");

    let outcome = render_document(&path_string(&path), None);

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.report_json.contains("VUTEX_INPUT_FORMAT_UNSUPPORTED"), "{}", outcome.report_json);
}

#[test]
fn unsupported_output_reports_output_format_unsupported() {
    let dir = ScratchDir::new("bad-output");
    let path = dir.write("sample.md", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"output":"docx"}"#));

    assert_eq!(outcome.exit_code, 1);
    assert!(outcome.pdf.is_none());
    assert!(outcome.report_json.contains("VUTEX_OUTPUT_FORMAT_UNSUPPORTED"), "{}", outcome.report_json);
}

#[test]
fn input_override_selects_the_import_adapter() {
    let dir = ScratchDir::new("override");
    let path = dir.write("sample.txt", SAMPLE);

    let outcome = render_document(&path_string(&path), Some(r#"{"input":"markdown"}"#));
    let payload = report(&outcome);

    assert_eq!(outcome.exit_code, 0, "{}", outcome.report_json);
    assert_eq!(payload["inputs"][0]["format"], "markdown");
}

#[test]
fn inspect_reports_document_structure() {
    let dir = ScratchDir::new("inspect");
    let path = dir.write("sample.md", INSPECT_SAMPLE);

    let outcome = inspect_document(&path_string(&path));
    let payload = inspect_report(&outcome);

    assert_eq!(outcome.input_format, "markdown");
    assert_eq!(payload["status"], "success");
    assert_eq!(payload["inputs"][0]["format"], "markdown");
    assert!(payload["document"]["blocks"].as_u64().unwrap_or(0) >= 3, "{payload}");
    assert!(payload["document"]["sections"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["document"]["paragraphs"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert!(payload["document"]["lists"].as_u64().unwrap_or(0) >= 1, "{payload}");
    assert_eq!(payload["document"]["coverage_complete"], true);
}

#[test]
fn format_ids_match_the_report_contract() {
    assert_eq!(ImportFormat::from_path("a.md").id(), "markdown");
    assert_eq!(ImportFormat::from_path("a.nd").id(), "notedown");
    assert_eq!(ImportFormat::from_path("a.htm").id(), "html");
    assert_eq!(ImportFormat::from_path("a.tex").id(), "unknown");
    assert_eq!(ImportFormat::from_option("tex").id(), "unknown");
    assert_eq!(OutputFormat::from_option("SVG").map(OutputFormat::id), Some("svg"));
    assert_eq!(OutputFormat::from_option("PDF").map(OutputFormat::id), Some("pdf"));
    assert!(OutputFormat::from_option("docx").is_none());
}
