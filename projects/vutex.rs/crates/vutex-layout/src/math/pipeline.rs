//! Formula parse and layout pipeline.

use vutex_text::math::{MathFontFacts, MathFontFamily, builtin_font_metrics, builtin_font_metrics_for};
use vutex_types::{
    Diagnostic,
    math::{FormulaLayoutResult, MathResult, MathSemanticTree, MathStyle, SemanticNode},
};

use super::{LayoutEngine, LayoutNode, parser::parse_math_with_diagnostics};

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

/// Parse TeX math into its semantic tree without laying it out.
///
/// Structured backends (Presentation MathML) render the derived
/// [`MathSemantics`](vutex_types::math::MathSemantics) tree directly instead of
/// reconstructing formula structure from laid-out geometry.
pub fn parse_formula(source: &str) -> MathResult<MathSemanticTree> {
    parse_math_with_diagnostics(source).map(|(tree, _)| tree)
}

/// Parse TeX math once and lay it out into the immutable formula result.
///
/// Returns the [`FormulaLayoutResult`] every backend consumes together with the
/// box tree the legacy SVG backend walks and the math-layer warnings.  Both
/// trees come from the same parse, so a renderer that needs geometry and a
/// renderer that needs structure cannot describe different formulas: the
/// result carries the very semantics node list that was laid out.
pub fn layout_formula_with_result(
    source: &str,
    display: bool,
    base_size: f64,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let family = MathFontFamily::builtin_default().expect("a builtin math font is embedded");
    layout_formula_with_result_for(source, display, base_size, family)
}

/// Parse TeX math once and lay it out with one explicit builtin font family.
///
/// The metrics cache and the `MATH` facts are both resolved from `family`, so
/// constants, glyph facts and delimiter assemblies all describe that one font.
/// A family whose asset is not embedded falls back to the synthetic metrics,
/// matching [`builtin_font_metrics`]'s behavior for an unparsable font.
pub fn layout_formula_with_result_for(
    source: &str,
    display: bool,
    base_size: f64,
    family: MathFontFamily,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let style = if display { MathStyle::Display } else { MathStyle::Text };
    let (tree, mut diagnostics) = parse_math_with_diagnostics(source)?;
    let font_system = builtin_font_metrics_for(family);
    let engine = LayoutEngine::with_facts(font_system, MathFontFacts::for_family(family)).with_base_size(base_size);
    let root = engine.layout_node(&tree.root, style)?;
    let nodes: &[SemanticNode] = match &tree.root {
        // The parser always roots a formula in a sequence, and a sequence is a
        // style scope that projects to its children, so the layout consumes the
        // siblings directly instead of one redundant wrapper node.
        SemanticNode::Sequence(items) => items.as_slice(),
        other => std::slice::from_ref(other),
    };
    let result = engine.layout_formula_result(nodes, style)?;
    diagnostics.extend(result.diagnostics.iter().cloned());
    Ok((result, root, diagnostics))
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
/// environment's declared column pairs, and a math font that publishes no
/// script scale ratios. They are [`vutex_types::Severity::Warning`] records with
/// a `vutex.layout.math.*` code and never replace the rendered geometry.
pub fn layout_formula_with_diagnostics(
    source: &str,
    display: bool,
    base_size: f64,
) -> MathResult<(super::LayoutNode, FormulaLayout, Vec<Diagnostic>)> {
    let style = if display { MathStyle::Display } else { MathStyle::Text };
    let (tree, mut diagnostics) = parse_math_with_diagnostics(source)?;
    let font_system = builtin_font_metrics();
    let engine = LayoutEngine::new(font_system).with_base_size(base_size);
    let root = engine.layout_node(&tree.root, style)?;
    diagnostics.extend(engine.script_scale_diagnostics());
    let width = root.width().to_f64();
    let height = root.height().to_f64();
    let depth = root.depth().to_f64();
    Ok((root, FormulaLayout { width, height, depth, baseline: height }, diagnostics))
}
