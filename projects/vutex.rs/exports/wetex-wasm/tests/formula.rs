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
fn path_only_svg_covers_ascii_relations() {
    let value = parse(&render_svg_wasm(
        "\\begin{cases} a & x>0 \\\\ b & x\\le 0 \\end{cases}",
        Some("{\"displayMode\":\"block\"}".to_string()),
    ));
    let svg = value["svg"].as_str().unwrap_or_default();
    assert!(svg.contains("<path"), "the outline path emitted no paths: {value}");
    assert!(!svg.contains("<text"), "the outline path fell back to a host font: {svg}");
    assert_eq!(value["diagnostics"].as_array().map(Vec::len), Some(0), "the outline path reported diagnostics: {value}");
}

#[test]
fn a_glyph_without_an_outline_is_skipped_with_a_warning() {
    // CJK is absent from the embedded math faces. The single outline path skips
    // the glyph and names it in an `outline_missing` warning instead of painting
    // a host-font `<text>` run or failing the whole render.
    let value = parse(&render_svg_wasm("\\text{字}", None));
    let svg = value["svg"].as_str().unwrap_or_default();
    assert!(!svg.contains("<text"), "the public render must never depend on a host font: {value}");

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
fn alignedat_column_mismatch_reaches_the_host_diagnostics() {
    let value = parse(&render_svg_wasm("\\begin{alignedat}{2} q &= a \\\\ xyz &= b \\end{alignedat}", None));
    assert!(value["svg"].as_str().unwrap_or_default().contains("<path"), "the grid must still paint: {value}");
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1, "one mismatch, one warning: {value}");
    assert_eq!(diagnostics[0]["code"].as_str(), Some("vutex.layout.math.alignedat_columns"));
    assert_eq!(diagnostics[0]["severity"].as_str(), Some("warning"));
}

/// `\limits` after a plain atom is a misuse the host must see, and the shells
/// still render.
#[test]
fn limits_misuse_reaches_the_host_diagnostics() {
    let value = parse(&render_svg_wasm("x\\limits_1", None));
    assert!(value["svg"].as_str().unwrap_or_default().contains("<path"), "the script must still paint: {value}");
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
    assert_eq!(diagnostics.len(), 1, "one misuse, one warning: {value}");
    assert_eq!(diagnostics[0]["code"].as_str(), Some("vutex.layout.math.limits_misuse"));
    assert_eq!(diagnostics[0]["severity"].as_str(), Some("warning"));
}

#[test]
fn render_svg_is_path_only() {
    let value = parse(&render_svg_wasm("x + y", None));
    let diagnostics = value["diagnostics"].as_array().expect("diagnostics array");
    let error_count = diagnostics.iter().filter(|d| d["severity"].as_str() == Some("error")).count();
    assert_eq!(error_count, 0, "unexpected error diagnostics: {diagnostics:?}");
    let svg = value["svg"].as_str().expect("svg");
    assert!(svg.starts_with("<svg"), "unexpected svg: {svg}");
    assert!(svg.contains("<path"), "every covered glyph must paint an outline: {svg}");
    assert!(!svg.contains("<text"), "the SVG must be path-only, got text in: {svg}");
}

/// The JSON payload carries the semantic tree the layout consumed, and the
/// MathML in the same payload is rendered from that very tree — the host never
/// has to re-parse the source to see the authored structure.
#[test]
fn json_semantics_match_the_mathml_structure() {
    let cases: &[(&str, &str, &str)] = &[
        // fractions
        ("\\frac{a}{b}", "fraction", "<mfrac"),
        ("\\frac{\\frac{a}{b}}{c}", "fraction", "<mfrac"),
        ("\\dfrac{a}{b}", "fraction", "<mfrac"),
        // radicals
        ("\\sqrt[3]{x}", "radical", "<mroot"),
        ("\\sqrt{\\frac{a}{b}}", "radical", "<msqrt"),
        // scripts
        ("x_a^b", "sub-sup", "<msubsup"),
        ("\\prod_{i=1}^{n} i", "sub-sup", "<msubsup"),
        ("x^{a_b}", "sup", "<msup"),
        ("10^{10^{10}}", "sup", "<msup"),
        ("\\oint_C f\\,dz", "sub", "<msub"),
        // accents and over/under
        ("\\hat{f}", "accent", "<mover"),
        ("\\vec{v}", "accent", "<mover"),
        ("\\overset{!}{=}", "stack-above", "<mover"),
        ("\\underset{\\alpha}{\\rightarrow}", "stack-below", "<munder"),
        // delimited structures
        ("\\left\\{ \\frac{a}{b} \\right\\}", "delimited", "<mfrac"),
        ("\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]", "delimited", "<mtable"),
        // tables
        ("\\begin{aligned}a&=b\\\\c&=d\\end{aligned}", "table", "<mtable"),
        ("\\begin{smallmatrix} a & b \\\\ c & d \\end{smallmatrix}", "table", "<mtable"),
        // text
        ("\\text{hello world}", "text", "<mtext"),
    ];

    for (source, kind, element) in cases {
        let value = parse(&render_formula(source, Some("{\"displayMode\":\"block\"}".to_string())));
        let nodes = value["semantics"]["nodes"].as_array().unwrap_or_else(|| panic!("{source}: no semantics: {value}"));
        let first = nodes.first().and_then(Value::as_object).unwrap_or_else(|| panic!("{source}: no top node: {value}"));
        assert!(first.contains_key(*kind), "{source}: expected a `{kind}` node, got {value}");

        let mathml = value["mathml"].as_str().unwrap_or_else(|| panic!("{source}: no mathml: {value}"));
        assert!(mathml.contains(element), "{source}: mathml is missing {element}: {mathml}");
    }
}

/// A formula that never reached layout reports no semantics instead of an empty
/// tree, so an absent result is distinguishable from an empty formula.
#[test]
fn semantics_are_absent_when_the_formula_fails() {
    let value = parse(&render_mathml_wasm("   ", None));
    assert!(value["semantics"].is_null(), "empty source: {value}");

    let value = parse(&render_svg_wasm("\\frac{a}{b}", Some("{\"fontFamily\":\"stix-two-math\"}".to_string())));
    assert!(value["semantics"].is_null(), "failed render: {value}");
    assert!(value["diagnostics"].as_array().map(|items| !items.is_empty()).unwrap_or(false), "{value}");
}

/// `fontFamily` must reach the layout, not be silently ignored: the two
/// embedded faces draw this formula with different outlines and extents.
#[test]
fn font_family_option_changes_the_render() {
    let source = "\\frac{x}{y} + \\sqrt{x}";
    let options = |family: &str| Some(format!("{{\"displayMode\":\"block\",\"fontFamily\":\"{family}\"}}"));

    let xits = parse(&render_svg_wasm(source, options("xits-math")));
    let lm = parse(&render_svg_wasm(source, options("lm-math")));
    assert_eq!(xits["diagnostics"].as_array().map(Vec::len), Some(0), "{xits}");
    assert_eq!(lm["diagnostics"].as_array().map(Vec::len), Some(0), "{lm}");

    assert_ne!(xits["svg"].as_str(), lm["svg"].as_str(), "the family option must change the SVG");
    assert_ne!(xits["metrics"], lm["metrics"], "the family option must change the metrics");

    // Omitting the option resolves to the builtin default face, Latin Modern Math.
    let default = parse(&render_svg_wasm(source, Some("{\"displayMode\":\"block\"}".to_string())));
    assert_eq!(default["svg"].as_str(), lm["svg"].as_str(), "the default must render as Latin Modern Math: {default}");
}

/// A family whose asset is not embedded fails with a clear code instead of
/// quietly rendering synthetic stand-ins.
#[test]
fn unsupported_font_family_reports_a_diagnostic() {
    for render in [
        render_svg_wasm("\\frac{x}{y}", Some("{\"fontFamily\":\"stix-two-math\"}".to_string())),
        render_formula("\\frac{x}{y}", Some("{\"fontFamily\":\"stix-two-math\"}".to_string())),
        render_mathml_wasm("\\frac{x}{y}", Some("{\"fontFamily\":\"stix-two-math\"}".to_string())),
        measure_formula("\\frac{x}{y}", Some("{\"fontFamily\":\"stix-two-math\"}".to_string())),
    ] {
        let value = parse(&render);
        assert_eq!(value["svg"].as_str(), Some(""), "an unsupported family must not emit a render: {value}");
        assert!(value["mathml"].is_null(), "an unsupported family must not emit mathml: {value}");

        let diagnostics = value["diagnostics"].as_array().expect("diagnostics");
        assert_eq!(diagnostics.len(), 1, "one unsupported family, one diagnostic: {value}");
        assert_eq!(diagnostics[0]["code"].as_str(), Some("WETEX_UNSUPPORTED_FONT_FAMILY"));
        assert_eq!(diagnostics[0]["severity"].as_str(), Some("error"));
    }
}
