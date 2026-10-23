use notedown_ir::DocumentGraph;

use crate::plan::LayoutPlan;

/// Compute the layout plan for a Notedown document graph.
///
/// The document layout pass walks the Notedown block tree, measures inline
/// content through `vutex-text`, fills pages, and materialises positioned
/// items.  This module owns that pass; the crate root only re-exports it.
pub fn layout(graph: &DocumentGraph) -> LayoutPlan {
    let _ = graph; // block / inline / relation traversal lands here in the real pass
    LayoutPlan { pages: Vec::new() }
}
