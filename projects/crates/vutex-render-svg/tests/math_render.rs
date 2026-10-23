//! Integration tests for the SVG and MathML math backends.

use vutex_layout::math::{FormulaLayout, LayoutBackend, LayoutNode, layout_formula, render_layout_node};
use vutex_render_svg::math::{MathmlBackend, SvgBackend};

fn tree(source: &str) -> (LayoutNode, FormulaLayout) {
    layout_formula(source, false).expect("formula should lay out")
}

#[test]
fn svg_backend_emits_svg_tags_for_a_fraction() {
    let (root, layout) = tree("\\frac{a}{b}");
    let mut backend = SvgBackend::new(layout.width, layout.height + layout.depth);
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("svg rendering");

    let svg = backend.finish();
    assert!(svg.starts_with("<svg"), "unexpected output: {svg}");
    assert!(svg.ends_with("</svg>"), "unexpected output: {svg}");
    assert!(svg.contains("<text"), "glyphs should be emitted as text: {svg}");
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
    let (root, layout) = tree("\\frac{a}{b}");
    let mut backend = MathmlBackend::new();
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("mathml rendering");

    let mathml = backend.finish();
    assert!(mathml.starts_with("<math"), "unexpected output: {mathml}");
    assert!(mathml.ends_with("</math>"), "unexpected output: {mathml}");
    assert!(mathml.contains("<mrow>"), "unexpected output: {mathml}");
    assert!(mathml.contains("<mi "), "letters should be identifiers: {mathml}");
    assert!(mathml.contains("<mspace "), "fraction rule should be a space: {mathml}");
    assert!(mathml.contains("<mpadded "), "positions should be padded: {mathml}");
}

#[test]
fn mathml_backend_marks_operators() {
    let (root, layout) = tree("a + b");
    let mut backend = MathmlBackend::new();
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("mathml rendering");

    let mathml = backend.finish();
    assert!(mathml.contains("<mo "), "operators should be operators: {mathml}");
}

#[test]
fn mathml_backend_escapes_markup_in_text() {
    let (root, layout) = tree("\\text{<}");
    let mut backend = MathmlBackend::new();
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("mathml rendering");

    let mathml = backend.finish();
    assert!(mathml.contains(">&lt;</mo>"), "markup must be escaped: {mathml}");
}
