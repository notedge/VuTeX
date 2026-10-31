//! End-to-end validation of the real glyph-outline pipeline.
//!
//! Verifies that the chain
//!     `opentype.rs` glyph outline extraction
//!     → `collect.rs` / `layout.rs` path emission
//!     → `render_formula_result` path rendering
//! produces SVG `<path>` elements (never `<text>`) for representative
//! formulas, and that tricky constructs like `Vmatrix` keep their
//! double-bar delimiters.

use vutex_layout::math::{collect_paint, default_base_size, layout_formula_with_result};
use vutex_render_svg::math::render_formula_result;
use vutex_types::math::FormulaLayoutResult;

struct Case {
    name: &'static str,
    source: &'static str,
    display: bool,
    /// Extra structural assertion on the SVG body.
    check: fn(&str),
}

fn render_outline(source: &str, display: bool) -> (String, FormulaLayoutResult) {
    let (result, _, _) =
        layout_formula_with_result(source, display, default_base_size(display)).expect("layout should succeed");
    let out = render_formula_result(&result);
    assert!(out.diagnostics.is_empty(), "the SVG backend found a glyph without an outline: {:?}", out.diagnostics);
    (out.markup, result)
}

/// Count closed, straight-sided subpaths.
///
/// Bar delimiters are emitted as rectangles while glyph outlines are cubic, so a
/// `d` without curve operators is a bar and its `M` count is its stroke count.
fn bar_strokes(svg: &str) -> usize {
    let mut strokes = 0;
    let mut cursor = 0;
    while let Some(index) = svg[cursor..].find("<path d=\"").map(|i| i + cursor) {
        let start = index + "<path d=\"".len();
        let end = start + svg[start..].find('"').expect("path data terminator");
        let d = &svg[start..end];
        if !d.contains('C') && !d.contains('Q') {
            strokes += d.matches('M').count();
        }
        cursor = end;
    }
    strokes
}

fn has_double_bar(svg: &str) {
    // One matrix has two delimiter sides, and `Vmatrix` paints two strokes per
    // side, so the doubled delimiter produces four bar subpaths in total.
    let strokes = bar_strokes(svg);
    assert_eq!(strokes, 4, "expected two Vmatrix bar strokes per side, found {strokes} in:\n{svg}");
}

fn nonzero_size(svg: &str) {
    assert!(svg.contains("scale("), "expected scaled glyph paths");
}

fn all_cases() -> Vec<Case> {
    vec![
        Case { name: "simple", source: "x + y", display: false, check: nonzero_size },
        Case { name: "fraction", source: r"\frac{a}{b}", display: true, check: nonzero_size },
        Case { name: "radical", source: r"\sqrt{x}", display: false, check: nonzero_size },
        Case { name: "limits", source: r"\sum_{i=1}^{n} x_i", display: true, check: nonzero_size },
        Case { name: "vmatrix", source: r"\begin{Vmatrix}a&b\\c&d\end{Vmatrix}", display: true, check: has_double_bar },
    ]
}

#[test]
fn all_five_formulas_render_as_paths() {
    for case in all_cases() {
        let (svg, layout) = render_outline(case.source, case.display);

        // SVG must contain path elements and no text elements.
        assert!(svg.contains("<path"), "{}: missing <path> in SVG", case.name);
        assert!(!svg.contains("<text"), "{}: unexpected <text> fallback in SVG", case.name);

        // Metrics must be positive and finite.
        assert!(layout.metrics.width.to_f64() > 0.0, "{}: non-positive width", case.name);
        assert!(layout.metrics.height.to_f64() >= 0.0, "{}: negative height", case.name);

        (case.check)(&svg);
    }
}

#[test]
fn lowercase_vmatrix_keeps_a_single_bar() {
    let (svg, _) = render_outline(r"\begin{vmatrix}a&b\\c&d\end{vmatrix}", true);
    // One stroke per delimiter side, so a plain `vmatrix` paints two subpaths.
    let strokes = bar_strokes(&svg);
    assert_eq!(strokes, 2, "expected a single vmatrix bar stroke per side, found {strokes} in:\n{svg}");
}

/// Whitespace the source spells as a spacing command paints no glyph, so it must
/// never be reported as a glyph without an outline.
///
/// `\,`, `\:`, `\;`, `\ `, `\quad`, `\qquad` and the spaces inside `\text{}` all
/// reach layout as `SemanticNode::Text`, which TeX sets as a horizontal kern.
/// Turning them into glyphs made the collector warn about a blank glyph the font
/// cannot outline even though no ink was ever meant to be painted.
#[test]
fn spacing_commands_paint_no_glyph_and_raise_no_outline_gap() {
    for source in [r"a\ b", r"a\,b", r"a\:b", r"a\;b", r"a\quad b", r"a\qquad b", r"\text{a b}"] {
        let (result, _, _) = layout_formula_with_result(source, true, default_base_size(true))
            .unwrap_or_else(|error| panic!("layout {source}: {error}"));
        let mut diagnostics = Vec::new();
        let items = collect_paint(&result, 0.0, result.metrics.height.to_f64(), &mut diagnostics);

        assert!(
            diagnostics.iter().all(|d| d.code != "vutex.layout.math.outline_missing"),
            "{source}: whitespace is not a missing glyph: {diagnostics:#?}"
        );
        assert_eq!(items.len(), 2, "{source}: only `a` and `b` paint ink: {items:#?}");
    }

    // The space still advances the pen, it just does so without ink.
    let spaced = layout_formula_with_result(r"a\ b", true, default_base_size(true)).expect("layout").0.metrics.width.to_f64();
    let tight = layout_formula_with_result("ab", true, default_base_size(true)).expect("layout").0.metrics.width.to_f64();
    assert!(spaced > tight, "a spacing command must still advance the pen: {spaced} vs {tight}");

    // A glyph the font really cannot outline keeps its diagnostic.
    let (result, _, _) = layout_formula_with_result(r"\text{中}", true, default_base_size(true)).expect("layout");
    let mut diagnostics = Vec::new();
    let items = collect_paint(&result, 0.0, result.metrics.height.to_f64(), &mut diagnostics);
    assert!(items.is_empty(), "the CJK glyph paints nothing: {items:#?}");
    assert_eq!(diagnostics.len(), 1, "the CJK gap is still reported: {diagnostics:#?}");
    assert_eq!(diagnostics[0].code, "vutex.layout.math.outline_missing");
    assert!(diagnostics[0].message.contains("U+4E2D"), "the report names the code point: {diagnostics:#?}");
}

#[test]
fn outline_positions_are_not_all_at_origin() {
    let (svg, _) = render_outline("x + y", false);
    // Expect multiple distinct translate x positions.
    let mut xs: Vec<f64> = Vec::new();
    for cap in svg.match_indices("translate(") {
        let start = cap.0 + cap.1.len();
        let rest = &svg[start..];
        let num_str: String = rest.chars().take_while(|c| *c != ',' && *c != ')').collect();
        if let Ok(v) = num_str.trim().parse::<f64>() {
            if v.abs() > f64::EPSILON {
                xs.push(v);
            }
        }
    }
    assert!(xs.len() >= 2, "expected at least 2 distinct non-origin x positions, got {xs:?}");
}
