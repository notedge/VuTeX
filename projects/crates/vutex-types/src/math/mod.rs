//! Math formula semantic types (ported from RuTeX).

mod error;
pub mod math_list;
mod metrics;
mod semantic;

pub use error::{MathError, MathErrorKind, MathResult};
pub use math_list::{
    MathList, MathStyleState, Noad, NoadKind, Scripts, normalize_math_list, project_semantic,
};
pub use metrics::{DelimiterComponent, DelimiterConstruction, DelimiterVariant, FontMetricsData, GlyphMetrics, MathConstant};
pub use semantic::{
    Alignment, Fixed, FontStyle, GlyphKey, LineStyle, MathSemanticTree, MathStyle, SemanticNode, SpacingRule, SymbolRole,
};
