//! Formula parse and layout pipeline.

use vutex_text::math::{MathFontFacts, MathFontFamily, builtin_font_metrics_for};
use vutex_types::{
    Diagnostic,
    math::{FormulaLayoutResult, MathResult, MathSemanticTree, MathStyle, SemanticNode},
};

use super::{LayoutEngine, LayoutNode, parser::parse_math_with_diagnostics};

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
/// box tree and the math-layer warnings.  Both
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
/// A family whose asset is not embedded falls back to the synthetic metrics
/// used for an unparsable font.
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
    let nodes: &[SemanticNode] = match &tree.root {
        // The parser always roots a formula in a sequence, and a sequence is a
        // style scope that projects to its children, so the layout consumes the
        // siblings directly instead of one redundant wrapper node.
        SemanticNode::Sequence(items) => items.as_slice(),
        other => std::slice::from_ref(other),
    };
    // One layout pass produces both the immutable result and the box tree;
    // laying the same semantic tree twice could let the two halves of a
    // formula drift apart.
    let (result, root) = engine.layout_formula_result(nodes, style)?;
    diagnostics.extend(result.diagnostics.iter().cloned());
    Ok((result, root, diagnostics))
}
