//! Math formula semantic types (ported from RuTeX).

mod alphalect;
mod error;
pub mod formula_layout;
pub mod math_list;
mod metrics;
pub mod operator;
mod semantic;

pub use alphalect::{alphalect_sources, alphalect_styles, math_alphalect};
pub use error::{MathError, MathErrorKind, MathResult};
pub use formula_layout::{
    BBox, FontAxis, FontFaceSnapshot, FontMetricsSnapshot, FormulaLayoutResult, FormulaMetrics, PaintNode,
};
pub use math_list::{
    MathGlue, MathItem, MathList, MathStyleState, Noad, NoadKind, Scripts, normalize_math_list, project_semantic,
    project_semantic_with_state,
};
pub use metrics::{DelimiterComponent, DelimiterConstruction, DelimiterVariant, FontMetricsData, GlyphMetrics, MathConstant};
pub use operator::{LimitsPolicy, OperatorClass};
pub use semantic::{
    AccentPosition, Alignment, AlignmentAnchors, EnvironmentContext, EnvironmentKind, EnvironmentRow, Fixed, FontStyle,
    GlyphKey, LineStyle, MathEnvironment, MathSemNode, MathSemanticTree, MathSemantics, MathStyle, MathVariant, Numbering,
    RowPlacement, SemanticNode, SpacingRule, SymbolRole,
};
