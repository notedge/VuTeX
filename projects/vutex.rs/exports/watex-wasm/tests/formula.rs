//! Host-side smoke tests for the wasm-facing formula facade.
//!
//! The exported `#[wasm_bindgen]` functions are plain Rust functions on non-wasm
//! targets. Only the entry points are guarded here: payload shape, a non-empty
//! SVG and one diagnostic code. Layout facts, geometry and semantic coverage are
//! judged downstream by the render cross-check harness.

use serde_json::Value;
use watex_wasm::{install_math_fonts, render_svg_wasm, watex_version};

fn parse(json: &str) -> Value {
    serde_json::from_str(json).expect("export must return valid JSON")
}

/// Install the repository math faces, since the artifact ships none.
fn install_fonts() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts");
    let latin_modern = std::fs::read(dir.join("LatinModernMath-Regular.otf")).expect("read Latin Modern Math");
    let xits = std::fs::read(dir.join("XITSMath-Regular.otf")).expect("read XITS Math");
    install_math_fonts(&latin_modern, &xits);
}

#[test]
fn version_is_exposed() {
    assert_eq!(watex_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn render_svg_returns_non_empty_svg() {
    install_fonts();
    let value = parse(&render_svg_wasm("\\frac{a}{b}", None));
    let svg = value["svg"].as_str().expect("svg");
    assert!(svg.starts_with("<svg"), "unexpected svg: {svg}");
    assert!(value["metrics"]["height"].as_f64().unwrap_or_default() > 0.0);
}

#[test]
fn empty_source_reports_a_diagnostic() {
    let value = parse(&render_svg_wasm("   ", None));
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1, "unexpected diagnostics: {value}");
    assert_eq!(diagnostics[0]["code"].as_str(), Some("WATEX_EMPTY_SOURCE"));
    assert_eq!(value["svg"].as_str(), Some(""));
}
