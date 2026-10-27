//! Cross-backend consistency test for formula layout.
//!
//! Verifies that the same [`LayoutNode`] renders successfully through both SVG
//! and MathML math backends without error-level diagnostics, and that every
//! laid-out formula produces sane box metrics (positive width, non-negative
//! height and depth).

use vutex_layout::math::{
    FormulaLayout, LayoutNode, layout_formula, render_layout_node,
};
use vutex_render_svg::math::{MathmlBackend, SvgBackend};
use vutex_types::math::Fixed;

/// A curated list of representative formulas that exercise every major node
/// kind in the layout engine: plain atoms, fractions, radicals, sub/super-
/// scripts, delimiters, matrices, accents, and large operators.
fn formula_list() -> &'static [&'static str] {
    &[
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
    ]
}

// ——— helpers ——— //

fn layout(source: &str) -> (LayoutNode, FormulaLayout) {
    layout_formula(source, false).unwrap_or_else(|e| panic!("failed to layout {source}: {e}"))
}

fn layout_display(source: &str) -> (LayoutNode, FormulaLayout) {
    layout_formula(source, true).unwrap_or_else(|e| panic!("failed to layout (display) {source}: {e}"))
}

// ——— group 1: box-metric sanity ——— //

/// Every laid-out formula must have strictly positive width and non-negative
/// height/depth on both the node accessors and the produced [`FormulaLayout`].
#[test]
fn all_formulas_layout_with_sane_box_metrics() {
    for formula in formula_list() {
        let (node, layout) = layout(formula);

        // FormulaLayout (f64, pipeline-computed)
        assert!(
            layout.width > 0.0,
            "{formula}: FormulaLayout.width must be positive, got {}",
            layout.width
        );
        assert!(
            layout.height >= 0.0,
            "{formula}: FormulaLayout.height must be non-negative, got {}",
            layout.height
        );
        assert!(
            layout.depth >= 0.0,
            "{formula}: FormulaLayout.depth must be non-negative, got {}",
            layout.depth
        );
        let total_expected = node.height().to_f64() + node.depth().to_f64();
        let total_layout = layout.height + layout.depth;
        assert!(
            (total_expected - total_layout).abs() < 1e-6,
            "{formula}: height + depth must match between LayoutNode ({total_expected}) \
             and FormulaLayout ({total_layout})"
        );

        // LayoutNode accessors (Fixed-based)
        assert!(
            node.width() > Fixed::ZERO,
            "{formula}: LayoutNode::width must be positive, got {:?}",
            node.width()
        );
        assert!(
            node.height() >= Fixed::ZERO,
            "{formula}: LayoutNode::height must be non-negative, got {:?}",
            node.height()
        );
        assert!(
            node.depth() >= Fixed::ZERO,
            "{formula}: LayoutNode::depth must be non-negative, got {:?}",
            node.depth()
        );
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
            assert!(
                m.width > 0.0,
                "{formula} ({label}): width must be positive, got {}",
                m.width
            );
            assert!(
                m.height >= 0.0,
                "{formula} ({label}): height must be non-negative, got {}",
                m.height
            );
            assert!(
                m.depth >= 0.0,
                "{formula} ({label}): depth must be non-negative, got {}",
                m.depth
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
        let (root, layout) = layout(formula);
        let mut backend = SvgBackend::new(layout.width, layout.height + layout.depth);
        render_layout_node(&mut backend, &root, 0.0, layout.height)
            .unwrap_or_else(|e| panic!("svg render failed for {formula}: {e}"));

        // Check warnings BEFORE finish() takes ownership.
        let warnings: Vec<String> = backend.warnings().to_vec();
        let svg = backend.finish();
        assert!(
            svg.starts_with("<svg"),
            "{formula}: svg output must start with <svg, got {svg:?}"
        );
        assert!(
            svg.ends_with("</svg>"),
            "{formula}: svg output must end with </svg>, got {svg:?}"
        );

        assert!(
            warnings.is_empty() || warnings.iter().all(|w| !w.contains("deterministic")),
            "{formula}: unexpected deterministic-path warning from non-strict backend"
        );
    }
}

// ——— group 3: MathML backend renders every formula ——— //

/// The same set of formulas must also render cleanly through the MathML
/// backend — this gives us a real cross-backend comparison: the exact same
/// [`LayoutNode`] is painted by two fundamentally different sinks (geometry
/// vs. semantic tree) and both must succeed at producing well-formed output.
#[test]
fn all_formulas_render_through_mathml_backend() {
    for formula in formula_list() {
        let (root, layout) = layout(formula);
        let mut backend = MathmlBackend::new();
        render_layout_node(&mut backend, &root, 0.0, layout.height)
            .unwrap_or_else(|e| panic!("mathml render failed for {formula}: {e}"));

        let mathml = backend.finish();
        assert!(
            mathml.starts_with("<math"),
            "{formula}: mathml output must start with <math, got {mathml:?}"
        );
        assert!(
            mathml.ends_with("</math>"),
            "{formula}: mathml output must end with </math>, got {mathml:?}"
        );
    }
}

// ——— group 4: SVG vs MathML cross-backend coherence ——— //

/// For every formula, both backends must successfully render the same node —
/// this is the actual cross-backend consistency check.  The two sinks produce
/// completely different wire formats, but they *must* both be able to paint
/// every node type the layout engine can emit.
#[test]
fn svg_and_mathml_backends_both_render_every_formula() {
    for formula in formula_list() {
        let (root, layout) = layout(formula);
        let total_height = layout.height + layout.depth;

        let mut svg_backend = SvgBackend::new(layout.width, total_height);
        render_layout_node(&mut svg_backend, &root, 0.0, layout.height)
            .unwrap_or_else(|e| panic!("svg failed for {formula}: {e}"));
        let svg_ok = svg_backend.finish().starts_with("<svg");

        let mut mathml_backend = MathmlBackend::new();
        render_layout_node(&mut mathml_backend, &root, 0.0, layout.height)
            .unwrap_or_else(|e| panic!("mathml failed for {formula}: {e}"));
        let mathml_ok = mathml_backend.finish().starts_with("<math");

        assert!(
            svg_ok && mathml_ok,
            "{formula}: one backend succeeded but the other did not \
             (svg_ok={svg_ok}, mathml_ok={mathml_ok})"
        );
    }
}
