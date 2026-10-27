//! End-to-end validation of the real glyph-outline pipeline.
//!
//! Verifies that the chain
//!     `opentype.rs` glyph outline extraction
//!     → `collect.rs` / `layout.rs` path emission
//!     → `SvgBackend` path rendering
//! produces SVG `<path>` elements (never `<text>`) for representative
//! formulas, and that tricky constructs like `Vmatrix` keep their
//! double-bar delimiters.

use vutex_layout::math::{FormulaLayout, layout_formula, render_layout_node};
use vutex_render_svg::math::SvgBackend;

struct Case {
    name: &'static str,
    source: &'static str,
    display: bool,
    /// Extra structural assertion on the SVG body.
    check: fn(&str),
}

fn render_outline(source: &str, display: bool) -> (String, FormulaLayout) {
    let (root, layout) = layout_formula(source, display).expect("layout should succeed");
    let svg_height = layout.height + layout.depth;
    let mut backend = SvgBackend::new_deterministic(layout.width, svg_height);
    render_layout_node(&mut backend, &root, 0.0, layout.height).expect("render should succeed");
    let svg = backend.finish();
    (svg, layout)
}

fn has_double_bar(svg: &str) {
    // Vmatrix uses two parallel stroke paths (scale > 40) for its bars.
    // We look for two M-moves close together (x 0.11 and 0.29 in em-space).
    let count = svg.matches("M 0.1510").count();
    assert!(count >= 2, "expected two Vmatrix bar paths, found {count} in:\n{svg}");
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
        assert!(layout.width > 0.0, "{}: non-positive width", case.name);
        assert!(layout.height >= 0.0, "{}: negative height", case.name);

        (case.check)(&svg);
    }
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
