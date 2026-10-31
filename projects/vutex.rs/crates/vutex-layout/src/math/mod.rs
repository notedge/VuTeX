//! Math formula lowering from `notedown-ir`, layout, and line breaking.

mod collect;
mod ir;
mod knuth_plass;
mod layout;
mod pipeline;

pub use collect::collect_paint;
pub use ir::{column_alignments, is_tex_formula, semantic_tree};
pub use layout::{Glue, Glyph, HBox, LayoutEngine, LayoutNode, Path, VBox};
pub use pipeline::{
    default_base_size, layout_formula, layout_formula_for, layout_formula_source, layout_formula_source_for, lower_formula, lower_formula_ir,
};
