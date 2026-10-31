//! Cross-backend consistency test for formula layout.
//!
//! Verifies that the same [`FormulaLayoutResult`] renders successfully through
//! both SVG and MathML math backends without error-level diagnostics, that
//! every laid-out formula produces sane box metrics (positive width,
//! non-negative height and depth), and that each semantic construct lowers to
//! its fixed Presentation MathML element.

use vutex_layout::math::{LayoutNode, default_base_size, layout_formula_with_result, parse_formula};
use vutex_render_svg::math::{MathmlBackend, render_formula_result};
use vutex_types::{
    Severity,
    math::{Fixed, FormulaLayoutResult, MathSemantics},
};

/// A curated list of representative formulas that exercise every major node
/// kind in the layout engine: plain atoms, fractions, radicals, sub/super-
/// scripts, delimiters, matrices and environments, accents and stacked
/// expressions, large operators, explicit spacing, math faces, relations,
/// dots, authored text and nested combinations.
///
/// This is the VuTeX-side superset of the shared cross-backend corpus.  A
/// formula appears here as soon as the parser, the layout engine and both
/// renderers accept it; constructs the engine cannot parse (e.g. `\breve`,
/// `\bigcup`, `\pmod`, `\bigl`, `\enspace`, `\bmod`, `\vdots`, `\dots`) stay
/// out until they are implemented, instead of turning the gate red.
fn formula_list() -> &'static [&'static str] {
    &[
        // ——— atoms, fractions, radicals, scripts ——— //
        "x + y",
        "\\frac{a}{b}",
        "\\sqrt{x}",
        "x_n^2",
        "\\sum_{i=1}^{n}",
        "\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}",
        "\\binom{n}{k}",
        "\\int_0^\\infty",
        "\\alpha + \\beta",
        "\\hat{x}",
        "\\frac{\\frac{a}{b}}{c}",
        "\\dfrac{a}{b}",
        "\\tfrac{a}{b}",
        "\\sqrt[3]{x+y}",
        "\\sqrt{\\frac{a}{b}}",
        "\\sqrt{\\sqrt{x}}",
        // ——— accents and stacked expressions ——— //
        "\\vec{v}",
        "\\tilde{a}",
        "\\dot{x}",
        "\\ddot{y}",
        "\\widehat{CD}",
        "\\overset{!}{=}",
        "\\underset{\\alpha}{\\rightarrow}",
        // ——— large operators and scripts ——— //
        "\\prod_{i=1}^{n} i",
        "\\oint_C f\\,dz",
        "\\lim_{x \\to 0} \\frac{\\sin x}{x}",
        "x^{a_b}",
        "10^{10^{10}}",
        "e^{-x^2}",
        "x'",
        "f''(x)",
        // ——— delimiters ——— //
        "\\left\\lVert x \\right\\rVert",
        "\\left\\{ \\frac{a}{b} \\right\\}",
        "\\left\\langle v \\right\\rangle",
        "\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]",
        "\\left. \\frac{dy}{dx} \\right|_{0}",
        // ——— environments ——— //
        "\\begin{matrix} a & b \\\\ c & d \\end{matrix}",
        "\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}",
        "\\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix}",
        "\\begin{Vmatrix} a & b \\\\ c & d \\end{Vmatrix}",
        "\\begin{cases} x & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}",
        "\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}",
        "\\begin{alignedat}{2} a &= b & c &= d \\end{alignedat}",
        "\\begin{split} a &= b \\\\ &= c \\end{split}",
        "\\begin{smallmatrix} a & b \\\\ c & d \\end{smallmatrix}",
        "\\begin{gathered} a+b \\\\ c+d \\end{gathered}",
        // ——— spacing ——— //
        "a\\,b",
        "a\\;b",
        "a\\quad b",
        "a\\qquad b",
        // ——— math faces ——— //
        "\\mathbf{Ax}",
        "\\mathbb{R}",
        "\\mathcal{L}",
        "\\mathrm{d}x",
        "\\mathsf{s}",
        "\\mathit{x}",
        // ——— relations and operators ——— //
        "x \\le y \\ge z",
        "p \\Rightarrow q",
        "A \\leftarrow B",
        "a \\cdot b \\times c \\div d",
        // ——— dots and text ——— //
        "x, \\ldots, y",
        "a_1 + \\cdots + a_n",
        "\\text{hello world}",
        "x + \\text{ text } + y",
        // ——— nested combination ——— //
        "\\frac{\\sum_{i=1}^{n} i}{\\sqrt{n}}",
    ]
}

/// Formula, the top-level semantic node kind the layout consumed, and the
/// MathML element that kind must produce (or, for multi-node formulas, an
/// element the lowering of that construct must emit somewhere).
///
/// This is the same semantics-to-MathML mapping invariant as
/// `vutex-render-svg`'s `formula_backend_consistency` cases, widened to every
/// family of the shared corpus.  It asserts no geometry and no character data:
/// only that one semantic construct maps to one fixed element.
const STRUCTURE_CASES: &[(&str, &str, &str)] = &[
    // ——— atoms ——— //
    ("x + y", "identifier", "<mi"),
    ("\\alpha + \\beta", "identifier", "<mi"),
    ("x'", "identifier", "<mo"),
    // ——— fractions ——— //
    ("\\frac{a}{b}", "fraction", "<mfrac"),
    ("\\frac{\\frac{a}{b}}{c}", "fraction", "<mfrac"),
    ("\\dfrac{a}{b}", "fraction", "<mfrac"),
    ("\\tfrac{a}{b}", "fraction", "<mfrac"),
    ("\\binom{n}{k}", "delimited", "<mfrac"),
    // ——— radicals ——— //
    ("\\sqrt{x}", "radical", "<msqrt"),
    ("\\sqrt[3]{x+y}", "radical", "<mroot"),
    ("\\sqrt{\\frac{a}{b}}", "radical", "<msqrt"),
    ("\\sqrt{\\sqrt{x}}", "radical", "<msqrt"),
    // ——— scripts ——— //
    ("x_n^2", "sub-sup", "<msubsup"),
    ("x^{a_b}", "sup", "<msup"),
    // The parser atomizes digits one by one, so a multi-digit base leaves a
    // leading `number` node and the script binds to the final digit only.
    // Assert the emitted structure as-is; the grouping gap is reported, not
    // papered over.
    ("10^{10^{10}}", "number", "<msup"),
    ("e^{-x^2}", "sup", "<msup"),
    // ——— large operators ——— //
    ("\\sum_{i=1}^{n}", "sub-sup", "<msubsup"),
    ("\\prod_{i=1}^{n} i", "sub-sup", "<msubsup"),
    ("\\int_0^\\infty", "sub-sup", "<msubsup"),
    ("\\oint_C f\\,dz", "sub", "<msub"),
    ("\\lim_{x \\to 0} \\frac{\\sin x}{x}", "sub", "<msub"),
    // ——— accents and stacked expressions ——— //
    ("\\hat{x}", "accent", "<mover"),
    ("\\vec{v}", "accent", "<mover"),
    ("\\tilde{a}", "accent", "<mover"),
    ("\\dot{x}", "accent", "<mover"),
    ("\\ddot{y}", "accent", "<mover"),
    ("\\widehat{CD}", "accent", "<mover"),
    ("\\overset{!}{=}", "stack-above", "<mover"),
    ("\\underset{\\alpha}{\\rightarrow}", "stack-below", "<munder"),
    // ——— delimiters ——— //
    ("\\left\\lVert x \\right\\rVert", "delimited", "<mo"),
    ("\\left\\{ \\frac{a}{b} \\right\\}", "delimited", "<mfrac"),
    ("\\left\\langle v \\right\\rangle", "delimited", "<mo"),
    ("\\left[ \\begin{matrix} a & b \\\\ c & d \\end{matrix} \\right]", "delimited", "<mtable"),
    ("\\left. \\frac{dy}{dx} \\right|_{0}", "sub", "<mfrac"),
    // ——— environments ——— //
    ("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}", "delimited", "<mtable"),
    ("\\begin{matrix} a & b \\\\ c & d \\end{matrix}", "table", "<mtable"),
    ("\\begin{bmatrix} a & b \\\\ c & d \\end{bmatrix}", "delimited", "<mtable"),
    ("\\begin{vmatrix} a & b \\\\ c & d \\end{vmatrix}", "delimited", "<mtable"),
    ("\\begin{Vmatrix} a & b \\\\ c & d \\end{Vmatrix}", "delimited", "<mtable"),
    ("\\begin{cases} x & x > 0 \\\\ 0 & \\text{otherwise} \\end{cases}", "delimited", "<mtable"),
    ("\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}", "table", "<mtable"),
    ("\\begin{alignedat}{2} a &= b & c &= d \\end{alignedat}", "table", "<mtable"),
    ("\\begin{split} a &= b \\\\ &= c \\end{split}", "table", "<mtable"),
    ("\\begin{smallmatrix} a & b \\\\ c & d \\end{smallmatrix}", "table", "<mtable"),
    ("\\begin{gathered} a+b \\\\ c+d \\end{gathered}", "table", "<mtable"),
    // ——— spacing ——— //
    ("a\\,b", "identifier", "<mspace"),
    ("a\\;b", "identifier", "<mspace"),
    ("a\\quad b", "identifier", "<mspace"),
    ("a\\qquad b", "identifier", "<mspace"),
    // ——— math faces ——— //
    ("\\mathbf{Ax}", "text", "<mtext"),
    ("\\mathbb{R}", "text", "<mtext"),
    ("\\mathcal{L}", "text", "<mtext"),
    ("\\mathrm{d}x", "identifier", "<mi"),
    ("\\mathsf{s}", "text", "<mtext"),
    ("\\mathit{x}", "text", "<mtext"),
    // ——— relations and operators ——— //
    ("x \\le y \\ge z", "identifier", "<mo"),
    ("p \\Rightarrow q", "identifier", "<mo"),
    ("A \\leftarrow B", "identifier", "<mo"),
    ("a \\cdot b \\times c \\div d", "identifier", "<mo"),
    // ——— dots and text ——— //
    ("x, \\ldots, y", "identifier", "<mo"),
    ("a_1 + \\cdots + a_n", "sub", "<msub"),
    ("\\text{hello world}", "text", "<mtext"),
    ("x + \\text{ text } + y", "identifier", "<mtext"),
    // ——— nested combination ——— //
    ("\\frac{\\sum_{i=1}^{n} i}{\\sqrt{n}}", "fraction", "<msubsup"),
];

// ——— helpers ——— //

fn layout(source: &str) -> (LayoutNode, FormulaLayoutResult) {
    let (result, root, _) = layout_formula_with_result(source, false, default_base_size(false))
        .unwrap_or_else(|e| panic!("failed to layout {source}: {e}"));
    (root, result)
}

fn layout_display(source: &str) -> (LayoutNode, FormulaLayoutResult) {
    let (result, root, _) = layout_formula_with_result(source, true, default_base_size(true))
        .unwrap_or_else(|e| panic!("failed to layout (display) {source}: {e}"));
    (root, result)
}

/// Render the structured MathML of one formula source.
fn render_mathml(source: &str) -> String {
    let tree = parse_formula(source).unwrap_or_else(|e| panic!("failed to parse {source}: {e}"));
    let semantics = MathSemantics::from_semantic_tree(&tree);
    let mut backend = MathmlBackend::new();
    backend.render_semantics(&semantics);
    backend.finish()
}

/// The single tag of the first top-level semantic node, e.g. `fraction`.
fn top_kind(result: &FormulaLayoutResult) -> String {
    let json = serde_json::to_value(&result.semantics).expect("semantics must serialize");
    let node = json["nodes"].get(0).unwrap_or_else(|| panic!("no semantic node in {json}"));
    let object = node.as_object().unwrap_or_else(|| panic!("a semantic node must be a tag object, got {node}"));
    assert_eq!(object.len(), 1, "a semantic node carries exactly one tag: {node}");
    object.keys().next().cloned().unwrap_or_default()
}

// ——— group 1: box-metric sanity ——— //

/// Every laid-out formula must have strictly positive width and non-negative
/// height/depth on both the node accessors and the produced
/// [`FormulaLayoutResult`].
#[test]
fn all_formulas_layout_with_sane_box_metrics() {
    for formula in formula_list() {
        let (node, result) = layout(formula);
        let metrics = &result.metrics;

        // Result metrics (Fixed, from the same packed box the renderers walk)
        assert!(metrics.width > Fixed::ZERO, "{formula}: result width must be positive, got {:?}", metrics.width);
        assert!(metrics.height >= Fixed::ZERO, "{formula}: result height must be non-negative, got {:?}", metrics.height);
        assert!(metrics.depth >= Fixed::ZERO, "{formula}: result depth must be non-negative, got {:?}", metrics.depth);
        assert_eq!(
            node.height() + node.depth(),
            metrics.height + metrics.depth,
            "{formula}: the result's vertical extent must match the laid-out box"
        );

        // LayoutNode accessors (Fixed-based)
        assert!(node.width() > Fixed::ZERO, "{formula}: LayoutNode::width must be positive, got {:?}", node.width());
        assert!(node.height() >= Fixed::ZERO, "{formula}: LayoutNode::height must be non-negative, got {:?}", node.height());
        assert!(node.depth() >= Fixed::ZERO, "{formula}: LayoutNode::depth must be non-negative, got {:?}", node.depth());
    }
}

/// Every formula must successfully lay out in both inline (Text) and display
/// style — the pipeline must accept both [`MathStyle`]s without panicking or
/// emitting parse errors.  Width/height ordering between the two styles is
/// deliberately NOT asserted because large operators grow limits vertically in
/// display mode (which can shrink horizontal width) while fractions keep the
/// same relative proportions; the only invariant here is "both succeed".
#[test]
fn every_formula_lays_out_in_both_inline_and_display_style() {
    for formula in formula_list() {
        let (_, inline) = layout(formula);
        let (_, display) = layout_display(formula);

        for (label, m) in [("inline", inline), ("display", display)] {
            assert!(m.metrics.width > Fixed::ZERO, "{formula} ({label}): width must be positive, got {:?}", m.metrics.width);
            assert!(
                m.metrics.height >= Fixed::ZERO,
                "{formula} ({label}): height must be non-negative, got {:?}",
                m.metrics.height
            );
            assert!(
                m.metrics.depth >= Fixed::ZERO,
                "{formula} ({label}): depth must be non-negative, got {:?}",
                m.metrics.depth
            );
        }
    }
}

// ——— group 2: SVG backend renders every formula ——— //

/// Every formula laid-out by the math engine must render through the SVG
/// backend without error-level diagnostics and produce an SVG document.
#[test]
fn all_formulas_render_through_svg_backend() {
    for formula in formula_list() {
        let (_, result) = layout(formula);
        let svg = render_formula_result(&result);

        assert!(svg.markup.starts_with("<svg"), "{formula}: svg output must start with <svg, got {:?}", svg.markup);
        assert!(svg.markup.ends_with("</svg>"), "{formula}: svg output must end with </svg>, got {:?}", svg.markup);

        assert!(
            svg.diagnostics.iter().all(|d| d.severity == Severity::Warning),
            "{formula}: the SVG backend must only report warnings, got {:?}",
            svg.diagnostics
        );
    }
}

// ——— group 3: MathML backend renders every formula ——— //

/// The same set of formulas must also render cleanly through the MathML
/// backend — this gives us a real cross-backend comparison: the exact same
/// parsed formula feeds both sinks (geometry vs. semantic tree) and both must
/// succeed at producing well-formed output.
#[test]
fn all_formulas_render_through_mathml_backend() {
    for formula in formula_list() {
        let mathml = render_mathml(formula);
        assert!(mathml.starts_with("<math"), "{formula}: mathml output must start with <math, got {mathml:?}");
        assert!(mathml.ends_with("</math>"), "{formula}: mathml output must end with </math>, got {mathml:?}");
    }
}

// ——— group 4: SVG vs MathML cross-backend coherence ——— //

/// For every formula, both backends must successfully render the same formula
/// source — this is the actual cross-backend consistency check.  The two sinks
/// produce completely different wire formats, but they *must* both be able to
/// paint every construct the parser and layout engine can emit.
#[test]
fn svg_and_mathml_backends_both_render_every_formula() {
    for formula in formula_list() {
        let (_, result) = layout(formula);
        let svg_ok = render_formula_result(&result).markup.starts_with("<svg");

        let mathml_ok = render_mathml(formula).starts_with("<math");

        assert!(
            svg_ok && mathml_ok,
            "{formula}: one backend succeeded but the other did not \
             (svg_ok={svg_ok}, mathml_ok={mathml_ok})"
        );
    }
}

// ——— group 5: semantics-to-MathML mapping invariants ——— //

/// Each semantic construct must lower to its fixed MathML element: the first
/// top-level node of the serialized semantics carries the expected kind tag,
/// and the MathML rendered from the same semantics contains the element that
/// kind maps to.  No geometry, no character data, no font-dependent value is
/// compared — only the structure both sinks describe from one parse.
#[test]
fn every_semantic_construct_maps_to_its_mathml_element() {
    for (source, kind, element) in STRUCTURE_CASES {
        let (_, result) = layout(source);
        assert_eq!(top_kind(&result), *kind, "{source}: unexpected top semantic node kind");

        let mathml = render_mathml(source);
        assert!(mathml.contains(element), "{source}: mathml is missing {element}: {mathml}");
    }
}
