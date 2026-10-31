//! Formula lowering and layout pipeline.
//!
//! VuTeX does not parse math source. `notedown-formats` lowers Oak's AST into
//! `notedown-ir::math`, and this pipeline maps that semantic tree onto the
//! layout nodes and computes one immutable [`FormulaLayoutResult`].

use notedown_ir::{MathDisplay, MathFormula};
use vutex_text::math::{MathFontFacts, MathFontFamily, builtin_font_metrics_for};
use vutex_types::{
    Diagnostic,
    math::{FormulaLayoutResult, MathError, MathResult, MathSemanticTree, MathStyle},
};

use super::{
    LayoutEngine, LayoutNode,
    ir::{is_tex_formula, semantic_tree},
};

/// Em size used for a formula laid out without an explicit base size.
pub fn default_base_size(display: bool) -> f64 {
    if display { 16.0 } else { 14.0 }
}

/// Lower authored TeX math through Notedown's import layer into the semantic tree.
///
/// The syntax stays in Oak and the semantics in `notedown-ir`; this function is
/// orchestration for hosts that own only a formula string, such as WeTeX.
pub fn lower_formula(source: &str, display: bool) -> MathResult<(MathSemanticTree, Vec<Diagnostic>)> {
    let formula = notedown_formats::lower_tex_formula_source(
        source,
        if display { MathDisplay::Display } else { MathDisplay::Inline },
    );
    lower_formula_ir(&formula)
}

/// Map one `notedown-ir` formula onto the layout semantic tree.
pub fn lower_formula_ir(formula: &MathFormula) -> MathResult<(MathSemanticTree, Vec<Diagnostic>)> {
    if !is_tex_formula(formula) {
        return Err(MathError::parse_error(
            format!("math language `{}` has no VuTeX lowering", formula.language.as_deref().unwrap_or_default()),
            None,
        ));
    }
    Ok(semantic_tree(formula))
}

/// Lay out one `notedown-ir` formula with the builtin default font family.
pub fn layout_formula(formula: &MathFormula, base_size: f64) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let family = MathFontFamily::builtin_default().expect("a builtin math font is embedded");
    layout_formula_for(formula, base_size, family)
}

/// Lay out one `notedown-ir` formula with one explicit builtin font family.
pub fn layout_formula_for(
    formula: &MathFormula,
    base_size: f64,
    family: MathFontFamily,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let (tree, mut diagnostics) = lower_formula_ir(formula)?;
    let style = match formula.display {
        MathDisplay::Display => MathStyle::Display,
        MathDisplay::Inline => MathStyle::Text,
    };
    let font_system = builtin_font_metrics_for(family);
    let engine = LayoutEngine::with_facts(font_system, MathFontFacts::for_family(family)).with_base_size(base_size);
    let nodes: &[vutex_types::math::SemanticNode] = match &tree.root {
        vutex_types::math::SemanticNode::Sequence(items) => items.as_slice(),
        other => std::slice::from_ref(other),
    };
    let (result, root) = engine.layout_formula_result(nodes, style)?;
    diagnostics.extend(result.diagnostics.iter().cloned());
    Ok((result, root, diagnostics))
}

/// Lower and lay out an authored TeX formula string with the builtin default family.
pub fn layout_formula_source(
    source: &str,
    display: bool,
    base_size: f64,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let family = MathFontFamily::builtin_default().expect("a builtin math font is embedded");
    layout_formula_source_for(source, display, base_size, family)
}

/// Lower and lay out an authored TeX formula string with one explicit family.
pub fn layout_formula_source_for(
    source: &str,
    display: bool,
    base_size: f64,
    family: MathFontFamily,
) -> MathResult<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>)> {
    let formula = notedown_formats::lower_tex_formula_source(
        source,
        if display { MathDisplay::Display } else { MathDisplay::Inline },
    );
    layout_formula_for(&formula, base_size, family)
}
