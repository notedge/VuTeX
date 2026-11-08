#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

mod formula;
mod html;
mod types;

pub use formula::{measure, render, render_html, render_mathml, render_svg};
pub use types::{ConformanceMode, DisplayMode, FormulaDiagnostic, FormulaMetrics, FormulaRender, RenderOptions};

use wasm_bindgen::prelude::*;

/// Install the two math faces from bytes the host supplies.
///
/// The WASM artifact ships no font. `document.text` resolves the default face
/// through its installed set, so a host must call this before any entry that
/// lays out a formula. The bytes are leaked because the math registry borrows
/// them for the whole process; a second call is a no-op, the first pack wins.
#[wasm_bindgen(js_name = installMathFonts)]
pub fn install_math_fonts(latin_modern: &[u8], xits: &[u8]) {
    document_fonts::install_math_fonts(&document_fonts::MathFontPack::new(leak(latin_modern), leak(xits)));
}

/// Leak one payload for the process-wide math registry.
fn leak(bytes: &[u8]) -> &'static [u8] {
    Box::leak(bytes.to_vec().into_boxed_slice())
}

fn parse_options(options_json: Option<String>) -> RenderOptions {
    match options_json {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str(&raw).unwrap_or_default(),
        _ => RenderOptions::default(),
    }
}

fn to_json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

/// Returns the WaTeX WASM binding version.
#[wasm_bindgen(js_name = watexVersion)]
pub fn watex_version() -> String {
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

/// Render a formula to an HTML fragment.
#[wasm_bindgen(js_name = renderHtml)]
pub fn render_html_wasm(source: &str, options_json: Option<String>) -> String {
    to_json(&render_html(source, &parse_options(options_json)))
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
