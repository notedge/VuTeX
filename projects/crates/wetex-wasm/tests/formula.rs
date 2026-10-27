//! Host-side smoke tests for the wasm-facing formula facade.
//!
//! The exported `#[wasm_bindgen]` functions are plain Rust functions on non-wasm
//! targets, so the JSON contract can be asserted natively.

use serde_json::Value;
use wetex_wasm::{measure_formula, render_formula, render_mathml_wasm, render_svg_wasm, wetex_version};

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("export must return valid JSON")
}

#[test]
fn version_is_exposed() {
    assert_eq!(wetex_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn measure_returns_metrics_without_payloads() {
    let value = parse(&measure_formula("x^2 + y^2", None));
    let width = value["metrics"]["width"].as_f64().expect("width");
    assert!(width > 0.0, "unexpected payload: {value}");
    assert_eq!(value["svg"].as_str(), Some(""));
    assert!(value["mathml"].is_null());
    assert_eq!(value["diagnostics"].as_array().map(Vec::len), Some(0));
}

#[test]
fn render_svg_returns_svg_markup() {
    let value = parse(&render_svg_wasm("\\frac{a}{b}", None));
    let svg = value["svg"].as_str().expect("svg");
    assert!(svg.starts_with("<svg"), "unexpected svg: {svg}");
    assert!(svg.contains("<rect"), "fraction rule missing: {svg}");
    assert!(value["metrics"]["height"].as_f64().unwrap_or_default() > 0.0);
    assert!(value["mathml"].is_null());
}

#[test]
fn render_mathml_returns_presentation_mathml() {
    let value = parse(&render_mathml_wasm("x", Some("{\"displayMode\":\"block\"}".to_string())));
    let mathml = value["mathml"].as_str().expect("mathml");
    assert!(mathml.starts_with("<math"), "unexpected mathml: {mathml}");
    assert!(value["svg"].as_str() == Some(""));
}

#[test]
fn render_formula_returns_both_payloads() {
    let value = parse(&render_formula("\\sqrt{x}", None));
    assert!(value["svg"].as_str().unwrap_or_default().contains("<path"));
    assert!(value["mathml"].as_str().unwrap_or_default().contains("<math"));
}

#[test]
fn strict_outline_covers_ascii_relations() {
    let value = parse(&render_svg_wasm(
        "\\begin{cases} a & x>0 \\\\ b & x\\le 0 \\end{cases}",
        Some("{\"displayMode\":\"block\",\"outline\":true}".to_string()),
    ));
    let svg = value["svg"].as_str().unwrap_or_default();
    assert!(svg.contains("<path"), "strict outline emitted no paths: {value}");
    assert!(!svg.contains("<text"), "strict outline fell back to a host font: {svg}");
    assert_eq!(
        value["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "strict outline reported diagnostics: {value}"
    );
}

#[test]
fn missing_outline_warns_alongside_the_host_font_fallback() {
    // CJK is absent from Latin Modern Math, so the non-strict backend paints a
    // `<text>` run whose metrics came from the synthetic fallback. That host-font
    // dependency must be reported instead of passing silently.
    let value = parse(&render_svg_wasm("\\text{字}", None));
    let svg = value["svg"].as_str().unwrap_or_default();
    assert!(svg.contains("<text"), "the non-strict backend must paint the fallback run: {value}");

    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    let warnings: Vec<&Value> = diagnostics.iter().filter(|item| item["severity"] == "warning").collect();
    assert_eq!(warnings.len(), 1, "exactly one glyph has no outline: {value}");
    assert_eq!(warnings[0]["code"].as_str(), Some("vutex.layout.math.outline_missing"));
    assert!(
        warnings[0]["message"].as_str().unwrap_or_default().contains('字'),
        "the warning must name the glyph it skipped: {value}"
    );
}

#[test]
fn strict_outline_rejects_a_glyph_without_an_outline() {
    let value = parse(&render_svg_wasm("\\text{字}", Some("{\"outline\":true}".to_string())));
    assert_eq!(value["svg"].as_str(), Some(""), "strict outline must not emit a host-font run: {value}");

    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1, "unexpected diagnostics: {value}");
    assert_eq!(diagnostics[0]["code"].as_str(), Some("WETEX_MATH_ERROR"));
    assert_eq!(diagnostics[0]["severity"].as_str(), Some("error"));
}

#[test]
fn empty_source_reports_a_diagnostic() {
    let value = parse(&render_svg_wasm("   ", None));
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1, "unexpected diagnostics: {value}");
    assert_eq!(diagnostics[0]["code"].as_str(), Some("WETEX_EMPTY_SOURCE"));
    assert_eq!(diagnostics[0]["severity"].as_str(), Some("error"));
    assert_eq!(value["svg"].as_str(), Some(""));
}

#[test]
fn unsupported_command_reports_a_diagnostic() {
    let value = parse(&render_svg_wasm("\\usepackage{amsmath}", None));
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert!(diagnostics.iter().any(|item| item["code"] == "WETEX_UNSUPPORTED_COMMAND"));
}

#[test]
fn invalid_option_json_falls_back_to_defaults() {
    let value = parse(&render_svg_wasm("x", Some("not json".to_string())));
    assert_eq!(value["diagnostics"].as_array().map(Vec::len), Some(0));
    assert!(!value["svg"].as_str().unwrap_or_default().is_empty());
}

#[test]
fn render_svg_with_outline_option_successfully_returns_svg() {
    // With outline=true, the deterministic SvgBackend is wired in.
    // Block A (real glyph outline paths) has landed, so ALL formula glyphs
    // should now render as <path> nodes without falling back to <text>.
    let value = parse(&render_svg_wasm("x + y", Some("{\"outline\": true}".to_string())));
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics array");
    let error_count = diagnostics.iter().filter(|d| d["severity"].as_str() == Some("error")).count();
    assert_eq!(error_count, 0, "unexpected error diagnostics with outline=true: {diagnostics:?}");
    let svg = value["svg"].as_str().expect("svg");
    assert!(svg.starts_with("<svg"), "unexpected svg: {svg}");
    // Deterministic path-only render must NOT emit any <text> nodes.
    assert!(!svg.contains("<text"), "deterministic SVG must be path-only, got text in: {svg}");
}
