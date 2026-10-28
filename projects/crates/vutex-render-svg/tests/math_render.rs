//! Integration tests for the SVG and MathML math backends.

use vutex_layout::math::{FormulaLayout, LayoutBackend, LayoutNode, layout_formula, parse_formula, render_layout_node};
use vutex_render_svg::math::{MathmlBackend, SvgBackend};
use vutex_types::math::MathSemantics;

fn tree(source: &str) -> (LayoutNode, FormulaLayout) {
    layout_formula(source, false).expect("formula should lay out")
}

/// Render the structured MathML of one formula source.
fn render_mathml(source: &str) -> String {
    let tree = parse_formula(source).expect("formula should parse");
    let semantics = MathSemantics::from_semantic_tree(&tree);
    let mut backend = MathmlBackend::new();
    backend.render_semantics(&semantics);
    backend.finish()
}

#[test]
fn svg_backend_emits_svg_tags_for_a_fraction() {
    let (root, layout) = tree("\\frac{a}{b}");
    let mut backend = SvgBackend::new(layout.width, layout.height + layout.depth);
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("svg rendering");

    let svg = backend.finish();
    assert!(svg.starts_with("<svg"), "unexpected output: {svg}");
    assert!(svg.ends_with("</svg>"), "unexpected output: {svg}");
    // Glyphs now have real outline paths from Latin Modern Math, so they
    // render as <path> nodes instead of host-font <text> fallbacks.
    assert!(svg.contains("<path"), "glyphs should be emitted as paths: {svg}");
    assert!(svg.contains("<rect"), "fraction rule should be a rect: {svg}");
    assert!(svg.contains("<g transform="), "boxes should be grouped: {svg}");
}

#[test]
fn svg_backend_emits_paths_for_radicals() {
    let (root, layout) = tree("\\sqrt{x}");
    let mut backend = SvgBackend::new(layout.width, layout.height + layout.depth);
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("svg rendering");

    let svg = backend.finish();
    assert!(svg.contains("<path"), "radical should emit a path: {svg}");
}

#[test]
fn svg_backend_reports_unbalanced_groups() {
    let mut backend = SvgBackend::new(10.0, 10.0);
    assert!(backend.end_group().is_err());
}

#[test]
fn mathml_backend_emits_presentation_math_tags() {
    let mathml = render_mathml("\\frac{a}{b}");
    assert!(mathml.starts_with("<math"), "unexpected output: {mathml}");
    assert!(mathml.ends_with("</math>"), "unexpected output: {mathml}");
    assert!(mathml.contains("<mrow>"), "unexpected output: {mathml}");
    assert!(mathml.contains("<mi>a</mi>"), "letters should be identifiers: {mathml}");
}

#[test]
fn mathml_fraction_nests_numerator_and_denominator() {
    let mathml = render_mathml("\\frac{a}{b}");
    assert!(mathml.contains("<mfrac><mi>a</mi><mi>b</mi></mfrac>"), "fraction must nest its parts: {mathml}");
}

#[test]
fn mathml_radical_degree_becomes_mroot() {
    let mathml = render_mathml("\\sqrt[3]{x}");
    assert!(mathml.contains("<mroot>"), "an indexed root is mroot: {mathml}");
    assert!(mathml.contains("<mroot><mi>x</mi><mn>3</mn></mroot>"), "degree must stay nested: {mathml}");
    assert!(render_mathml("\\sqrt{x}").contains("<msqrt><mi>x</mi></msqrt>"), "a plain root is msqrt");
}

#[test]
fn mathml_scripts_keep_their_nesting() {
    // `x^{a_b}` is a superscript whose body carries its own subscript.
    let mathml = render_mathml("x^{a_b}");
    assert!(mathml.contains("<msup><mi>x</mi><msub><mi>a</mi><mi>b</mi></msub></msup>"), "nested scripts: {mathml}");
    // `x_a^b` is the one base carrying both scripts.
    let mathml = render_mathml("x_a^b");
    assert!(mathml.contains("<msubsup><mi>x</mi><mi>a</mi><mi>b</mi></msubsup>"), "combined scripts: {mathml}");
}

#[test]
fn mathml_matrix_becomes_a_table() {
    let mathml = render_mathml("\\begin{matrix}a&b\\\\c&d\\end{matrix}");
    assert!(mathml.contains("<mtable>"), "a matrix is a table: {mathml}");
    assert_eq!(mathml.matches("<mtr>").count(), 2, "one row per matrix row: {mathml}");
    assert_eq!(mathml.matches("<mtd>").count(), 4, "one cell per matrix cell: {mathml}");
    assert!(mathml.contains("<mtr><mtd><mi>a</mi></mtd><mtd><mi>b</mi></mtd></mtr>"), "cell order: {mathml}");
}

#[test]
fn mathml_accent_becomes_mover() {
    let mathml = render_mathml("\\hat{f}");
    assert!(mathml.contains("<mover><mi>f</mi>"), "an accent is an over-script: {mathml}");
    assert!(mathml.contains("</mi><mo>ˆ</mo></mover>"), "the accent glyph must stay attached: {mathml}");
}

#[test]
fn mathml_backend_marks_operators() {
    let mathml = render_mathml("a + b");
    assert!(mathml.contains("<mo>+</mo>"), "operators should be operators: {mathml}");
}

#[test]
fn mathml_backend_escapes_markup_in_text() {
    let mathml = render_mathml("\\text{<}");
    assert!(mathml.contains("<mtext>&lt;</mtext>"), "markup must be escaped: {mathml}");
}
