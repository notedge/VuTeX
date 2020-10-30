#![deny(missing_docs)]

//! Node-API export surface for VuTeX.

use std::path::Path;

use napi_derive::napi;
use serde::Serialize;
use wetex_wasm::{RenderOptions, measure, render, render_mathml, render_svg};

/// N-API document render response.
#[napi(object)]
pub struct RenderDocumentResponse {
    /// Process exit code. Zero means success.
    pub exit_code: u32,
    /// Requested output format id.
    pub output_format: String,
    /// SVG payload when `output_format` is `svg`.
    pub svg: Option<String>,
    /// Serialized render report JSON.
    pub report_json: String,
}

/// N-API document inspect response.
#[napi(object)]
pub struct InspectDocumentResponse {
    /// Guessed input format id from the path.
    pub input_format: String,
    /// Input path inspected.
    pub path: String,
    /// Serialized inspect report JSON.
    pub report_json: String,
}

#[derive(Serialize)]
struct DiagnosticRecord {
    code: String,
    severity: String,
    message: String,
    stage: String,
    path: Option<String>,
}

fn parse_render_options(raw: Option<String>) -> RenderOptions {
    match raw {
        Some(json) if !json.trim().is_empty() => serde_json::from_str(&json).unwrap_or_default(),
        _ => RenderOptions::default(),
    }
}

fn guess_input_format(path: &str) -> String {
    let extension = Path::new(path).extension().and_then(|value| value.to_str()).unwrap_or("").to_ascii_lowercase();
    match extension.as_str() {
        "md" | "markdown" => "markdown".to_string(),
        "tex" => "tex".to_string(),
        "typ" => "typst".to_string(),
        "nd" | "notedown" => "notedown".to_string(),
        "html" | "htm" => "html".to_string(),
        _ => "unknown".to_string(),
    }
}

fn diagnostic_report(operation: &str, status: &str, path: &str, diagnostics: Vec<DiagnosticRecord>) -> String {
    serde_json::json!({
        "schema_version": "vutex.report/v0",
        "operation": operation,
        "status": status,
        "inputs": [{ "path": path }],
        "diagnostics": diagnostics,
    })
    .to_string()
}

/// Returns the VuTeX N-API binding version.
#[napi]
pub fn vutex_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Measure a formula and return JSON (`FormulaRender`).
#[napi]
pub fn measure_formula(source: String, options_json: Option<String>) -> String {
    let options = parse_render_options(options_json);
    serde_json::to_string(&measure(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula to SVG and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula_svg(source: String, options_json: Option<String>) -> String {
    let options = parse_render_options(options_json);
    serde_json::to_string(&render_svg(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula to MathML and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula_mathml(source: String, options_json: Option<String>) -> String {
    let options = parse_render_options(options_json);
    serde_json::to_string(&render_mathml(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Render a formula with SVG and MathML and return JSON (`FormulaRender`).
#[napi]
pub fn render_formula(source: String, options_json: Option<String>) -> String {
    let options = parse_render_options(options_json);
    serde_json::to_string(&render(&source, &options)).unwrap_or_else(|_| "{}".to_string())
}

/// Inspect a document path and return structural metadata (scaffold).
#[napi]
pub fn inspect_document(input_path: String) -> InspectDocumentResponse {
    let path = input_path.clone();
    let input_format = guess_input_format(&path);
    let status = if Path::new(&path).exists() { "success" } else { "failed" };
    let diagnostics = if status == "success" {
        Vec::new()
    }
    else {
        vec![DiagnosticRecord {
            code: "VUTEX_INPUT_NOT_FOUND".into(),
            severity: "error".into(),
            message: format!("input path does not exist: {path}"),
            stage: "inspect".into(),
            path: Some(path.clone()),
        }]
    };
    let report_json = diagnostic_report("inspect", status, &path, diagnostics);
    InspectDocumentResponse { input_format, path, report_json }
}

/// Render a document file (scaffold — SVG placeholder until layout pipeline lands).
#[napi]
pub fn render_document(input_path: String, options_json: Option<String>) -> RenderDocumentResponse {
    let output_format = options_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| value.get("output").and_then(|item| item.as_str()).map(str::to_string))
        .unwrap_or_else(|| "svg".to_string());

    if !Path::new(&input_path).exists() {
        let report_json = diagnostic_report(
            "render",
            "failed",
            &input_path,
            vec![DiagnosticRecord {
                code: "VUTEX_INPUT_NOT_FOUND".into(),
                severity: "error".into(),
                message: format!("input path does not exist: {input_path}"),
                stage: "render".into(),
                path: Some(input_path.clone()),
            }],
        );
        return RenderDocumentResponse { exit_code: 1, output_format, svg: None, report_json };
    }

    let svg = if output_format == "svg" {
        Some(format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="120" data-vutex="scaffold"><text x="8" y="24" font-family="serif" font-size="14">VuTeX scaffold render: {path}</text></svg>"#,
            path = input_path.replace('&', "&amp;").replace('<', "&lt;")
        ))
    }
    else {
        None
    };

    let report_json = diagnostic_report("render", "success", &input_path, Vec::new());
    RenderDocumentResponse { exit_code: 0, output_format, svg, report_json }
}
