//! Integration tests for the math parse/layout pipeline.

use vutex_layout::math::{FormulaLayout, layout_formula};

fn layout(source: &str) -> FormulaLayout {
    let (_, metrics) = layout_formula(source, false).expect("formula should lay out");
    metrics
}

#[test]
fn simple_formula_has_nonzero_box() {
    let metrics = layout("x");
    assert!(metrics.width > 0.0, "width should be positive: {metrics:?}");
    assert!(metrics.height > 0.0, "height should be positive: {metrics:?}");
    assert!(metrics.baseline > 0.0, "baseline should be positive: {metrics:?}");
}

#[test]
fn multi_atom_formula_is_wider_than_single_atom() {
    let single = layout("x");
    let wider = layout("x + y");
    assert!(wider.width > single.width, "expected {} > {}", wider.width, single.width);
}

#[test]
fn nested_fraction_grows_monotonically() {
    let inner = layout("\\frac{a}{b}");
    let outer = layout("\\frac{\\frac{a}{b}}{c}");

    assert!(inner.width > 0.0, "inner fraction width: {inner:?}");
    assert!(outer.width > inner.width, "nested fraction should be wider: {} vs {}", outer.width, inner.width);
    assert!(outer.height >= inner.height, "nested fraction should not shrink in height: {} vs {}", outer.height, inner.height);
}

#[test]
fn display_style_is_larger_than_text_style() {
    let (_, inline) = layout_formula("\\frac{a}{b}", false).expect("inline layout");
    let (_, display) = layout_formula("\\frac{a}{b}", true).expect("display layout");
    assert!(display.width > inline.width, "display style should use a larger base size: {} vs {}", display.width, inline.width);
}

#[test]
fn missing_fraction_argument_is_a_parse_error() {
    let error = layout_formula("\\frac{a}", false).expect_err("missing denominator must fail");
    let message = error.to_string();
    assert!(message.contains("Parse Error"), "unexpected diagnostic: {message}");
    assert!(message.contains("missing argument"), "unexpected diagnostic: {message}");
}

#[test]
fn unbalanced_group_is_a_parse_error() {
    let error = layout_formula("{a + b", false).expect_err("unclosed group must fail");
    assert!(error.to_string().contains("Parse Error"));
}

#[test]
fn unknown_command_is_a_parse_error() {
    let error = layout_formula("\\notacommand{x}", false).expect_err("unknown command must fail");
    let message = error.to_string();
    assert!(message.contains("unknown command"), "unexpected diagnostic: {message}");
}

#[test]
fn stray_script_is_a_parse_error() {
    let error = layout_formula("^2", false).expect_err("script without base must fail");
    assert!(error.to_string().contains("without a base"));
}

#[test]
fn delimited_radical_and_superscript_lay_out() {
    let metrics = layout("\\left(\\sqrt[3]{x^2}\\right)");
    assert!(metrics.width > 0.0);
    assert!(metrics.height > 0.0);
}

#[test]
fn matrix_lays_out_with_two_rows() {
    let metrics = layout("\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}");
    assert!(metrics.width > 0.0);
    assert!(metrics.height >= metrics.depth);
}

#[test]
fn spacing_adds_width_around_binary() {
    // TeX inserts thick muskip around binary operators like `+`.  A sequence
    // containing a binary atom must be wider than the trivial concatenation of
    // its operands — the gap is visible in the total HBox width.
    let plain = layout("xy");
    let spaced = layout("x+y");
    // x + y should be wider than x + y (operator glyph only)
    assert!(spaced.width > plain.width, "binary spacing must add width: {} vs {}", spaced.width, plain.width);

    // And it must also be wider than the two operands concatenated
    let two_atoms = layout("ab");
    assert!(spaced.width > two_atoms.width, "binary spacing must exceed plain two-atom width: {} vs {}", spaced.width, two_atoms.width);
}

#[test]
fn spacing_adds_width_around_relation() {
    // Relations (`=`, `<`, etc.) take thick muskip on both sides.
    let lhs = layout("x");
    let eq = layout("=");
    let rhs = layout("y");
    let spaced = layout("x=y");
    let plain_sum = lhs.width + eq.width + rhs.width;
    assert!(spaced.width > plain_sum, "relation spacing must add width: {} vs {}", spaced.width, plain_sum);
}

#[test]
fn spacing_adds_width_around_fraction_inner() {
    // Fractions are Inner atoms — thick muskip between them and neighbours.
    let a = layout("a");
    let b = layout("\\frac{x}{y}");
    let c = layout("c");
    let spaced = layout("a \\frac{x}{y} c");
    let plain_sum = a.width + b.width + c.width;
    assert!(spaced.width > plain_sum, "inner spacing must add width: {} vs {}", spaced.width, plain_sum);
}

#[test]
fn big_delimiter_parses() {
    let metrics = layout("\\big(x\\big)");
    assert!(metrics.width > 0.0, "big delimiter formula should have positive width");
}

#[test]
fn binom_lays_out() {
    let metrics = layout("\\binom{n}{k}");
    assert!(metrics.width > 0.0, "\\binom should have positive width");
    assert!(metrics.height > 0.0, "\\binom should have positive height");
}

#[test]
fn smallmatrix_supported() {
    let metrics = layout("\\begin{smallmatrix}1&2\\\\3&4\\end{smallmatrix}");
    assert!(metrics.width > 0.0, "smallmatrix should have positive width");
    assert!(metrics.height > 0.0, "smallmatrix should have positive height");
}

#[test]
fn bmatrix_supported() {
    let metrics = layout("\\begin{Bmatrix}a&b\\\\c&d\\end{Bmatrix}");
    assert!(metrics.width > 0.0, "Bmatrix should have positive width");
    assert!(metrics.height > 0.0, "Bmatrix should have positive height");
}
