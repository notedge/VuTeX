//! Math formula semantic types (ported from RuTeX).

mod error;
mod metrics;
mod semantic;

pub use error::{MathError, MathErrorKind, MathResult};
pub use metrics::{DelimiterComponent, DelimiterConstruction, DelimiterVariant, FontMetricsData, GlyphMetrics, MathConstant};
pub use semantic::{
    Alignment, Fixed, FontStyle, GlyphKey, LineStyle, MathSemanticTree, MathStyle, SemanticNode, SpacingRule, SymbolRole,
};
