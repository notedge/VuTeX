//! Math formula types for the layout engine (ported from RuTeX).

mod alphalect;
mod error;
pub mod formula_layout;
pub mod math_list;
mod metrics;
pub mod operator;
mod formula;

pub use alphalect::{alphalect_sources, alphalect_styles, math_alphalect};
pub use error::{MathError, MathErrorKind, MathResult};
pub use formula_layout::{
    BBox, FontAxis, FontFaceSnapshot, FontMetricsSnapshot, FormulaLayoutResult, FormulaMetrics, PaintNode,
};
pub use math_list::{
    MathGlue, MathItem, MathList, MathStyleState, Noad, NoadKind, Scripts, normalize_math_list, project_formula,
    project_formula_with_state,
};
pub use metrics::{DelimiterComponent, DelimiterConstruction, DelimiterVariant, FontMetricsData, GlyphMetrics, MathConstant};
pub use operator::{LimitsPolicy, OperatorClass};
pub use formula::{
    AccentPosition, Alignment, AlignmentAnchors, EnvironmentContext, EnvironmentKind, EnvironmentRow, Fixed, FontStyle,
    GlyphKey, LineStyle, MathEnvironment, MathSemNode, FormulaTree, MathSemantics, MathStyle, MathVariant, Numbering,
    RowPlacement, FormulaNode, SpacingRule, SymbolRole,
};
