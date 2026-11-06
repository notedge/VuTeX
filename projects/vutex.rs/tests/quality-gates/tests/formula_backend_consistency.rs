//! Cross-backend consistency on the single immutable layout result.
//!
//! One [`FormulaLayoutResult`] is laid out per formula and then handed to the
//! SVG, PDF and MathML backends.  The geometry backends walk its paint tree and
//! the structured backend walks [`FormulaLayoutResult::semantics`], so all three
//! describe one formula without the source being parsed again per backend.

use vutex_compose::math::FormulaLayoutResult;
use vutex_layout::math::layout_formula_source;
use vutex_render_pdf::render_formula_result as render_pdf_result;
use vutex_render_svg::math::{render_formula_result as render_svg_result, render_mathml_result};

#[path = "support/mod.rs"]
mod support;

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
    support::ensure_math_fonts();
    let (result, _root, _diagnostics) =
        layout_formula_source(source, true, 16.0).unwrap_or_else(|e| panic!("failed to lay out {source}: {e}"));
    result
}

/// The single tag of the first top-level formula node, e.g. `fraction`.
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

// ——— G4: numeric consistency of the derived surfaces ——— //

/// Formatting rounds emitted coordinates to 4 decimal places, so half a unit
/// in the last place is the most any parsed number can drift.
const ROUND: f64 = 5e-5;

/// The viewport numbers the SVG document carries: `(viewBox x, y, w, h,
/// width attribute, height attribute)`.
fn parse_svg_viewport(svg: &str, source: &str) -> (f64, f64, f64, f64, f64, f64) {
    let head = svg.split_once('>').unwrap_or_else(|| panic!("{source}: svg has no opening tag")).0;
    let attr = |key: &str| -> String {
        let rest = head.split_once(key).unwrap_or_else(|| panic!("{source}: svg missing {key} attribute")).1;
        rest.split_once('"').unwrap_or_else(|| panic!("{source}: svg attribute {key} is not quoted")).0.to_string()
    };
    let attr_w: f64 = attr("width=\"").parse().expect("svg width number");
    let attr_h: f64 = attr("height=\"").parse().expect("svg height number");
    let box_attr = attr("viewBox=\"");
    let nums: Vec<f64> = box_attr
        .split_whitespace()
        .map(|token| token.parse().unwrap_or_else(|_| panic!("{source}: bad viewBox number {token}")))
        .collect();
    assert_eq!(nums.len(), 4, "{source}: viewBox carries four numbers, got {box_attr}");
    (nums[0], nums[1], nums[2], nums[3], attr_w, attr_h)
}

/// G4 (design doc 16): every geometry backend derives its canvas from the
/// single [`FormulaMetrics`] this result carries, and the derivation must be
/// exact up to number formatting — the 0.01em budget the gate allows is far
/// wider than the formatting rounding checked here.  `measure` is
/// `result.metrics` itself; the SVG viewport and the PDF page are the
/// derived surfaces.  Drift beyond rounding is a renderer bug.
#[test]
fn backend_canvases_derive_from_the_single_metrics() {
    for (source, _, _) in CASES {
        let result = layout(source);
        let w = result.metrics.width.to_f64();
        let hd = result.metrics.height.to_f64() + result.metrics.depth.to_f64();

        // PDF: the page is the formula box padded by 4 pt on every side, so
        // the MediaBox minus the padding must equal the metrics exactly.
        let pdf = render_pdf_result(&result).unwrap_or_else(|e| panic!("pdf failed for {source}: {e:?}"));
        let text = String::from_utf8_lossy(&pdf);
        let after = text.split_once("/MediaBox [0 0 ").unwrap_or_else(|| panic!("{source}: pdf lost its MediaBox")).1;
        let (pw, rest) = after.split_once(' ').expect("MediaBox width");
        let (ph, _) = rest.split_once(']').expect("MediaBox height");
        let page_w: f64 = pw.parse().expect("MediaBox width number");
        let page_h: f64 = ph.parse().expect("MediaBox height number");
        assert!(
            (page_w - (w.max(1.0) + 8.0)).abs() <= ROUND,
            "{source}: pdf page width {page_w} must be the metrics width {w} plus 8pt padding"
        );
        assert!(
            (page_h - (hd.max(1.0) + 8.0)).abs() <= ROUND,
            "{source}: pdf page height {page_h} must be the metrics extent {hd} plus 8pt padding"
        );

        // SVG: the viewport is the union of the formula box and the painted
        // ink, padded by `max(4% of the span, 0.5px)`.  The box alone pins
        // one-sided bounds: the viewport starts no later than `-pad_min` and
        // reaches at least `extent + pad_min` on both axes, whatever the ink.
        let svg = render_svg_result(&result).markup;
        let (vb_x, vb_y, vb_w, vb_h, attr_w, attr_h) = parse_svg_viewport(&svg, source);
        let pad_min = (w.max(hd) * 0.04).max(0.5);
        let slack = ROUND * 2.0;
        assert!(vb_x <= -pad_min + slack, "{source}: viewport must pad left of the box: vb_x={vb_x}, pad_min={pad_min}");
        assert!(vb_y <= -pad_min + slack, "{source}: viewport must pad above the box: vb_y={vb_y}, pad_min={pad_min}");
        assert!(
            vb_x + vb_w >= w + pad_min - slack,
            "{source}: viewport must reach the box right edge plus padding: vb_x+vb_w={}, need {}",
            vb_x + vb_w,
            w + pad_min
        );
        assert!(
            vb_y + vb_h >= hd + pad_min - slack,
            "{source}: viewport must reach the box bottom plus padding: vb_y+vb_h={}, need {}",
            vb_y + vb_h,
            hd + pad_min
        );
        assert!(
            (attr_w - vb_w).abs() <= slack && (attr_h - vb_h).abs() <= slack,
            "{source}: svg width/height attributes must equal the viewBox extent"
        );
    }
}
