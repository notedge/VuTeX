#![deny(missing_debug_implementations)]
#![warn(missing_docs, rustdoc::missing_crate_level_docs)]
#![doc = include_str!("../readme.md")]

use notedown_ir::DocumentGraph;

pub mod hint;
// pub mod math; // migrate-rutex.py scaffold — Result unwrap bugs (91) — re-enable after fix
pub mod plan;

pub use hint::LayoutHint;
pub use plan::{LayoutPage, LayoutPlan};

/// Compute the layout plan for a Notedown document graph.
///
/// VuTeX layouts directly from `notedown-ir::DocumentGraph` — no intermediate
/// projection into a VuTeX-owned document IR.  Source spans, relations,
/// assets and coverage/loss all stay attached to the Notedown graph so the
/// layout stage never drops provenance.
///
/// Returns a scalar empty plan as a scaffold — the real layout pass iterates
/// the graph's block tree and materialises pages.
pub fn layout(graph: &DocumentGraph) -> LayoutPlan {
    let _ = graph; // block / inline / relation traversal lands here in the real pass
    LayoutPlan { pages: Vec::new() }
}
