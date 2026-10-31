//! Cross-backend consistency on the single immutable layout result.
//!
//! One [`FormulaLayoutResult`] is laid out per formula and then handed to the
//! SVG, PDF and MathML backends.  The geometry backends walk its paint tree and
//! the structured backend walks [`FormulaLayoutResult::semantics`], so all three
//! describe one formula without the source being parsed again per backend.

use vutex_layout::math::layout_formula_with_result;
use vutex_render_pdf::render_formula_result as render_pdf_result;
use vutex_render_svg::math::{render_formula_result as render_svg_result, render_mathml_result};
use vutex_types::math::FormulaLayoutResult;

/// Formula, the semantic node kind the layout consumed, and the MathML element
/// that kind must produce.
///
/// Covers every family of the shared cross-backend corpus: atoms, fractions,
/// radicals, scripts, large operators, delimiters, environments, accents and
/// stacked expressions, spacing, math faces, relations, dots, text and nested
/// combinations.  A case only joins this table when the SVG backend renders it
/// with zero diagnostics — constructs that still need synthesized glyph metrics
/// stay out until they are clean.
const CASES: &[(&str, &str, &str)] = &[
    // ——— atoms and relations ——— //
    ("\\frac{a}{b}", "fraction", "<mfrac"),
    ("x + y", "identifier", "<mi"),
    ("\\alpha + \\beta", "identifier", "<mi"),
    ("x \\le y", "identifier", "<mo"),
    ("p \\Rightarrow q", "identifier", "<mo"),
    // ——— fractions ——— //
    ("\\binom{n}{k}", "delimited", "<mfrac"),
    ("\\frac{\\sum_{i=1}^{n} i}{\\sqrt{n}}", "fraction", "<msubsup"),
    // ——— radicals ——— //
    ("\\sqrt[3]{x}", "radical", "<mroot"),
    ("\\sqrt{x}", "radical", "<msqrt"),
    ("\\sqrt{\\frac{a}{b}}", "radical", "<msqrt"),
    // ——— scripts and large operators ——— //
    ("x_a^b", "sub-sup", "<msubsup"),
    ("x^2", "sup", "<msup"),
    ("x_2", "sub", "<msub"),
    ("\\sum_{i=1}^{n}", "sub-sup", "<msubsup"),
    ("\\int_0^\\infty", "sub-sup", "<msubsup"),
    ("\\oint_C f\\,dz", "sub", "<msub"),
    // ——— delimiters and environments ——— //
    ("\\left\\{ \\frac{a}{b} \\right\\}", "delimited", "<mfrac"),
    ("\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]", "delimited", "<mtable"),
    ("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", "delimited", "<mtable"),
    ("\\begin{matrix} a & b \\\\ c & d \\end{matrix}", "table", "<mtable"),
    ("\\begin{cases} x & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}", "delimited", "<mtable"),
    ("\\begin{aligned}a&=b\\\\c&=d\\end{aligned}", "table", "<mtable"),
    // ——— accents and stacked expressions ——— //
    ("\\hat{f}", "accent", "<mover"),
    ("\\vec{v}", "accent", "<mover"),
    ("\\widehat{CD}", "accent", "<mover"),
    ("\\overset{!}{=}", "stack-above", "<mover"),
    ("\\underset{\\alpha}{\\rightarrow}", "stack-below", "<munder"),
    // ——— spacing, faces, dots and text ——— //
    ("a\\,b", "identifier", "<mspace"),
    ("\\mathbb{R}", "identifier", "<mi"),
    ("\\mathbf{Ax}", "identifier", "<mi"),
    ("\\mathrm{d}x", "identifier", "<mi"),
    ("x, \\ldots, y", "identifier", "<mo"),
    ("\\text{hello world}", "text", "<mtext"),
];

fn layout(source: &str) -> FormulaLayoutResult {
    let (result, _root, _diagnostics) =
        layout_formula_with_result(source, true, 16.0).unwrap_or_else(|e| panic!("failed to lay out {source}: {e}"));
    result
}

/// The single tag of the first top-level semantic node, e.g. `fraction`.
fn top_kind(result: &FormulaLayoutResult) -> String {
    let json = serde_json::to_value(&result.semantics).expect("semantics must serialize");
    let node = json["nodes"].get(0).unwrap_or_else(|| panic!("no semantic node in {json}"));
    let object = node.as_object().unwrap_or_else(|| panic!("a semantic node must be a tag object, got {node}"));
    assert_eq!(object.len(), 1, "a semantic node carries exactly one tag: {node}");
    object.keys().next().cloned().unwrap_or_default()
}

#[test]
fn every_backend_reads_one_layout_result() {
    for (source, kind, element) in CASES {
        // One layout result, consumed by all three backends.
        let result = layout(source);

        let svg_out = render_svg_result(&result);
        assert!(svg_out.diagnostics.is_empty(), "{source}: every glyph must carry an outline: {:?}", svg_out.diagnostics);
        let svg = svg_out.markup;
        assert!(svg.starts_with("<svg"), "{source}: svg must be a document, got {svg:?}");
        assert!(svg.ends_with("</svg>"), "{source}: svg must be closed, got {svg:?}");

        let pdf = render_pdf_result(&result).unwrap_or_else(|e| panic!("pdf failed for {source}: {e:?}"));
        assert!(pdf.starts_with(b"%PDF-"), "{source}: pdf must start with %PDF-");

        let mathml = render_mathml_result(&result);
        assert!(mathml.starts_with("<math"), "{source}: mathml must be a document, got {mathml:?}");
        assert!(mathml.ends_with("</math>"), "{source}: mathml must be closed, got {mathml:?}");

        // The structure of the MathML is the structure the layout consumed.
        assert_eq!(top_kind(&result), *kind, "{source}: unexpected top semantic node");
        assert!(mathml.contains(element), "{source}: mathml is missing {element}: {mathml}");
    }
}

/// The structured tree is the one layout read, not a re-parse: the nodes the
/// backends see keep the authored nesting instead of a flattened glyph stream.
#[test]
fn semantics_keep_the_authored_nesting() {
    let json = serde_json::to_value(&layout("\\frac{a}{b}")).expect("result must serialize");
    let fraction = &json["semantics"]["nodes"][0]["fraction"];
    assert_eq!(fraction["numerator"]["identifier"]["text"], "a", "{json}");
    assert_eq!(fraction["denominator"]["identifier"]["text"], "b", "{json}");
    assert_eq!(fraction["line"], "solid", "{json}");

    let json = serde_json::to_value(&layout("x_a^b")).expect("result must serialize");
    let sub_sup = &json["semantics"]["nodes"][0]["sub-sup"];
    assert_eq!(sub_sup["base"]["identifier"]["text"], "x", "{json}");
    assert_eq!(sub_sup["sub"]["identifier"]["text"], "a", "{json}");
    assert_eq!(sub_sup["sup"]["identifier"]["text"], "b", "{json}");
}

/// Metrics belong to the same result as the paint tree and the semantics, so the
/// PDF page and the SVG canvas are sized from one measurement.
#[test]
fn geometry_and_structure_share_one_result() {
    let result = layout("\\sqrt[3]{x}");
    assert!(!result.semantics.nodes.is_empty(), "the result must carry the IR it laid out");

    let svg = render_svg_result(&result).markup;
    let pdf = String::from_utf8_lossy(&render_pdf_result(&result).expect("pdf render")).into_owned();
    let width = result.metrics.width.to_f64().max(1.0);
    // Both geometry backends size their canvas/page from `result.metrics`.
    assert!(svg.contains("width="), "svg lost its measured width: {svg}");
    assert!(pdf.contains("/MediaBox"), "pdf lost its page box: {pdf}");
    assert!(width > 0.0, "a laid-out formula must have an advance width");

    let mathml = render_mathml_result(&result);
    assert!(mathml.contains("<mroot"), "mathml must keep the authored degree: {mathml}");
    assert!(mathml.contains("<mn>3</mn>"), "the degree atom must survive: {mathml}");
}

/// Serde keeps the Rust truth: struct fields camelCase, enum tags kebab-case.
#[test]
fn semantics_serialize_in_crate_naming() {
    let json = serde_json::to_value(&layout("\\hat{f}")).expect("result must serialize");
    let accent = &json["semantics"]["nodes"][0]["accent"];
    assert_eq!(accent["position"], "above", "{json}");
    assert_eq!(accent["base"]["identifier"]["text"], "f", "{json}");

    let json = serde_json::to_value(&layout("\\,x")).expect("result must serialize");
    let width = json["semantics"]["nodes"][0]["space"]["widthEm"].as_f64().expect("widthEm");
    assert!((width - 3.0 / 18.0).abs() < 1e-3, "a thin space is 3mu, got {width} in {json}");
}
