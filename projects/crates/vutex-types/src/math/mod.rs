//! Math formula semantic types (ported from RuTeX).

mod error;
pub mod formula_layout;
pub mod math_kern;
pub mod math_list;
mod metrics;
pub mod operator;
mod semantic;

pub use error::{MathError, MathErrorKind, MathResult};
pub use formula_layout::{
    BBox, FontAxis, FontFaceSnapshot, FontMetricsSnapshot, FormulaLayoutResult, FormulaMetrics, PaintNode,
};
pub use math_kern::MathKern;
pub use math_list::{
    MathGlue, MathItem, MathList, MathStyleState, Noad, NoadKind, Scripts, normalize_math_list, project_semantic,
    project_semantic_with_state,
};
pub use metrics::{DelimiterComponent, DelimiterConstruction, DelimiterVariant, FontMetricsData, GlyphMetrics, MathConstant};
pub use operator::{LimitsPolicy, OperatorClass};
pub use semantic::{
    AccentPosition, Alignment, Fixed, FontStyle, GlyphKey, LineStyle, MathSemNode, MathSemanticTree, MathSemantics, MathStyle,
    SemanticNode, SpacingRule, SymbolRole,
};
