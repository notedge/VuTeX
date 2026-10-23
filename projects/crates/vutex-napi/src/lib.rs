#![deny(missing_docs)]

//! Node-API export surface for VuTeX.

pub mod document;

use napi::bindgen_prelude::Buffer;
use napi_derive::napi;
use wetex_wasm::{RenderOptions, measure, render, render_mathml, render_svg};

/// N-API document render response.
#[napi(object)]
pub struct RenderDocumentResponse {
    /// Process exit code. Zero means success.
    pub exit_code: u32,
    /// Requested output format id (`svg`, `html`, `snapshot`, or `pdf`).
    pub output_format: String,
    /// SVG payload when `output_format` is `svg`.
    pub svg: Option<String>,
    /// HTML payload when `output_format` is `html` or `snapshot`.
    pub html: Option<String>,
    /// PDF bytes when `output_format` is `pdf`.
    pub pdf: Option<Buffer>,
    /// Serialized render report JSON.
    pub report_json: String,
}

/// N-API document inspect response.
#[napi(object)]
pub struct InspectDocumentResponse {
    /// Resolved input format id.
    pub input_format: String,
    /// Input path inspected.
    pub path: String,
    /// Serialized inspect report JSON.
    pub report_json: String,
}

fn parse_render_options(raw: Option<String>) -> RenderOptions {
    match raw {
        Some(json) if !json.trim().is_empty() => serde_json::from_str(&json).unwrap_or_default(),
        _ => RenderOptions::default(),
    }
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

/// Inspect a document file and return structural metadata.
#[napi]
pub fn inspect_document(input_path: String) -> InspectDocumentResponse {
    let outcome = document::inspect_document(&input_path);
    InspectDocumentResponse { input_format: outcome.input_format, path: outcome.path, report_json: outcome.report_json }
}

/// Render a document file into SVG, reflow HTML, snapshot HTML, or PDF.
#[napi]
pub fn render_document(input_path: String, options_json: Option<String>) -> RenderDocumentResponse {
    let outcome = document::render_document(&input_path, options_json.as_deref());
    RenderDocumentResponse {
        exit_code: outcome.exit_code,
        output_format: outcome.output_format,
        svg: outcome.svg,
        html: outcome.html,
        pdf: outcome.pdf.map(Buffer::from),
        report_json: outcome.report_json,
    }
}
