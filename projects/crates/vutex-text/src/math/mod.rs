//! Math font metrics.

mod font_metrics;
mod opentype;

pub use font_metrics::{FontMetricsSystem, builtin_font_metrics};
pub use opentype::opentype_font_metrics;
