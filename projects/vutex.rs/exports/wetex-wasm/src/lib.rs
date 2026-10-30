#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod formula;
mod types;

pub use formula::{measure, render, render_mathml, render_svg};
pub use types::{DisplayMode, FormulaDiagnostic, FormulaMetrics, FormulaRender, RenderOptions};

use wasm_bindgen::prelude::*;

fn parse_options(options_json: Option<String>) -> RenderOptions {
    match options_json {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str(&raw).unwrap_or_default(),
        _ => RenderOptions::default(),
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

/// Returns the WeTeX WASM binding version.
#[wasm_bindgen(js_name = wetexVersion)]
pub fn wetex_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Measure formula metrics for the given TeX math source.
#[wasm_bindgen(js_name = measureFormula)]
pub fn measure_formula(source: &str, options_json: Option<String>) -> String {
    to_json(&measure(source, &parse_options(options_json)))
}

/// Render a formula to SVG.
#[wasm_bindgen(js_name = renderSvg)]
pub fn render_svg_wasm(source: &str, options_json: Option<String>) -> String {
    to_json(&render_svg(source, &parse_options(options_json)))
}

/// Render a formula to Presentation MathML.
#[wasm_bindgen(js_name = renderMathml)]
pub fn render_mathml_wasm(source: &str, options_json: Option<String>) -> String {
    to_json(&render_mathml(source, &parse_options(options_json)))
}

/// Render a formula with SVG and MathML payloads.
#[wasm_bindgen(js_name = renderFormula)]
pub fn render_formula(source: &str, options_json: Option<String>) -> String {
    to_json(&render(source, &parse_options(options_json)))
}
