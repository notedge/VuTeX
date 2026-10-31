//! Integration tests for the SVG and MathML math backends.

use vutex_layout::math::{default_base_size, layout_formula_with_result, parse_formula};
use vutex_render_svg::math::{MathmlBackend, render_formula_result};
use vutex_types::math::MathSemantics;

/// Render one formula through the SVG backend with host-font fallback enabled.
fn render_svg(source: &str) -> String {
    let (result, _, _) = layout_formula_with_result(source, false, default_base_size(false)).expect("formula should lay out");
    render_formula_result(&result, true).expect("svg rendering").markup
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
    let svg = render_svg("\\frac{a}{b}");
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
    let svg = render_svg("\\sqrt{x}");
    assert!(svg.contains("<path"), "radical should emit a path: {svg}");
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
fn mathml_environment_keeps_its_column_alignment() {
    // An AMS environment knows how each column is aligned, and `mtable` can say
    // it: `aligned` alternates right/left around its `&` anchors.
    let mathml = render_mathml("\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}");
    assert!(mathml.contains(r#"<mtable columnalign="right left">"#), "aligned alternates right/left: {mathml}");

    // `gather` has no anchors and centres its rows.
    let mathml = render_mathml("\\begin{gather} a \\\\ b \\end{gather}");
    assert!(mathml.contains(r#"<mtable columnalign="center">"#), "gather centres its column: {mathml}");

    // A matrix authors no column alignment, so it stays a bare `mtable` and
    // keeps MathML's default instead of inheriting an environment's grid.
    let mathml = render_mathml("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
    assert!(mathml.contains("<mtable>"), "a matrix has no authored alignment: {mathml}");
    assert!(!mathml.contains("columnalign"), "a matrix must not claim an alignment it never authored: {mathml}");
}

#[test]
fn mathml_accent_becomes_mover() {
    let mathml = render_mathml("\\hat{f}");
    // TeX's `\hat` compresses the accent onto its base, which MathML expresses
    // through `accent="true"` on the mover — the same mark KaTeX emits.
    assert!(mathml.contains(r#"<mover accent="true"><mi>f</mi><mo>ˆ</mo></mover>"#), "an accent is a marked mover: {mathml}");
}

#[test]
fn mathml_overset_stays_an_unmarked_mover() {
    // `\overset` stacks a full expression above its base, so it is not a
    // compressed accent and must not claim `accent="true"`.
    let mathml = render_mathml("\\overset{a}{b}");
    assert!(mathml.contains("<mover><mi>b</mi><mi>a</mi></mover>"), "overset stays bare: {mathml}");
    assert!(!mathml.contains("accent="), "only an authored accent is marked: {mathml}");
}

#[test]
fn mathml_backend_marks_operators() {
    let mathml = render_mathml("a + b");
    assert!(mathml.contains("<mo>+</mo>"), "operators should be operators: {mathml}");
}

#[test]
fn mathml_left_right_fences_are_marked_as_fences() {
    // `\left` … `\right` authors a delimiter pair, and KaTeX marks the same
    // structure `<mo fence="true">(</mo>`.
    let mathml = render_mathml("\\left(\\frac{a}{b}\\right)");
    assert!(mathml.contains(r#"<mo fence="true">(</mo>"#), "the left fence is marked: {mathml}");
    assert!(mathml.contains(r#"<mo fence="true">)</mo>"#), "the right fence is marked: {mathml}");

    // A plain parenthesis is an ordinary atom, not an authored fence, and a
    // fenced matrix marks its delimiters like any other `\left` pair.
    let mathml = render_mathml("(x)");
    assert!(mathml.contains("<mo>(</mo><mi>x</mi><mo>)</mo>"), "plain parens stay bare: {mathml}");
    let mathml = render_mathml("\\begin{pmatrix} a \\end{pmatrix}");
    assert!(mathml.contains(r#"<mo fence="true">(</mo>"#), "a pmatrix fence is a fence: {mathml}");
}

#[test]
fn mathml_backend_escapes_markup_in_text() {
    let mathml = render_mathml("\\text{<}");
    assert!(mathml.contains("<mtext>&lt;</mtext>"), "markup must be escaped: {mathml}");
}
