//! Formula parse and layout pipeline.

use vutex_text::math::builtin_font_metrics;
use vutex_types::{
    Diagnostic,
    math::{MathResult, MathStyle},
};

use super::{LayoutEngine, parser::parse_math_with_diagnostics};

/// Layout metrics for a rendered formula, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormulaLayout {
    /// Advance width.
    pub width: f64,
    /// Extent above the baseline.
    pub height: f64,
    /// Extent below the baseline.
    pub depth: f64,
    /// Baseline offset used when placing the formula.
    pub baseline: f64,
}

/// Em size used for a formula laid out without an explicit base size.
pub fn default_base_size(display: bool) -> f64 {
    if display { 16.0 } else { 14.0 }
}

/// Parse TeX math and produce a layout tree plus metrics.
///
/// `display` selects display style (larger base size, display-style shifts) instead
/// of inline text style.
pub fn layout_formula(source: &str, display: bool) -> MathResult<(super::LayoutNode, FormulaLayout)> {
    let base_size = default_base_size(display);
    layout_formula_with_size(source, display, base_size)
}

/// Parse TeX math and produce a layout tree plus metrics at an explicit base size.
///
/// `base_size` is the em size in pixels used for the outer style; script styles
/// scale down from it. Document layout uses the body size so formulas follow the
/// surrounding text.
pub fn layout_formula_with_size(source: &str, display: bool, base_size: f64) -> MathResult<(super::LayoutNode, FormulaLayout)> {
    layout_formula_with_diagnostics(source, display, base_size).map(|(root, layout, _)| (root, layout))
}

/// Parse TeX math, produce its layout, and report the math-layer warnings.
///
/// The warnings cover source the grammar accepts but the layout can only partly
/// honour, such as a matrix body whose column count disagrees with the
/// environment's declared column pairs. They are [`vutex_types::Severity::Warning`]
/// records with a `vutex.layout.math.*` code and never replace the rendered
/// geometry.
pub fn layout_formula_with_diagnostics(
    source: &str,
    display: bool,
    base_size: f64,
) -> MathResult<(super::LayoutNode, FormulaLayout, Vec<Diagnostic>)> {
    let style = if display { MathStyle::Display } else { MathStyle::Text };
    let (tree, diagnostics) = parse_math_with_diagnostics(source)?;
    let font_system = builtin_font_metrics();
    let engine = LayoutEngine::new(font_system).with_base_size(base_size);
    let root = engine.layout_node(&tree.root, style)?;
    let width = root.width().to_f64();
    let height = root.height().to_f64();
    let depth = root.depth().to_f64();
    Ok((root, FormulaLayout { width, height, depth, baseline: height }, diagnostics))
}
