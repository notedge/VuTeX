//! Formula parse and layout pipeline.

use vutex_text::math::builtin_font_metrics;
use vutex_types::math::{MathResult, MathStyle};

use super::{LayoutEngine, parser::parse_math};

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

/// Parse TeX math and produce a layout tree plus metrics.
///
/// `display` selects display style (larger base size, display-style shifts) instead
/// of inline text style.
pub fn layout_formula(source: &str, display: bool) -> MathResult<(super::LayoutNode, FormulaLayout)> {
    let style = if display { MathStyle::Display } else { MathStyle::Text };
    let base_size = if display { 16.0 } else { 14.0 };
    let tree = parse_math(source)?;
    let font_system = builtin_font_metrics();
    let engine = LayoutEngine::new(font_system).with_base_size(base_size);
    let root = engine.layout_node(&tree.root, style)?;
    let width = root.width().to_f64();
    let height = root.height().to_f64();
    let depth = root.depth().to_f64();
    Ok((root, FormulaLayout { width, height, depth, baseline: height }))
}
