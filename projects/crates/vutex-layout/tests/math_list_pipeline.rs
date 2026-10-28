//! Dual-pipeline parity smoke tests.
//!
//! Compare glue count / width between the legacy
//!   `SemanticNode → layout_sequence → spacing_between(demote) → LayoutNode`
//! path and the new
//!   `SemanticNode → project_semantic → normalize_math_list → tex_skip_between_noad → LayoutNode`
//! path.
//!
//! `normalize_math_list` now runs demotion and consecutive-Atom merge in one
//! pass, and `tex_skip_between_noad` consumes the already-resolved
//! [`NoadKind`] directly (no re-demote).  As long as the normalizer stays
//! faithful to TeX Appendix G, the two paths should emit identical glue
//! sequences for formulas where merge_atoms_pass does not shrink the list.

use vutex_layout::math::{LayoutEngine, LayoutNode};
use vutex_text::math::builtin_font_metrics;
use vutex_types::math::{FontStyle, GlyphKey, LineStyle, MathStyle, SemanticNode, SymbolRole};

/// Build a symbol atom for a single char.
fn sym(c: char, role: SymbolRole) -> SemanticNode {
    SemanticNode::Symbol {
        glyph_key: GlyphKey::from_char(c, None, FontStyle::Math),
        role,
    }
}

/// Glue widths extracted from every HBox leaf in layout order.
fn glue_widths(node: &LayoutNode) -> Vec<f64> {
    match node {
        LayoutNode::HBox(hb) => hb
            .children
            .iter()
            .filter_map(|c| match c {
                LayoutNode::Glue(g) => Some(g.width.to_f64()),
                _ => None,
            })
            .collect(),
        _ => panic!("expected HBox root, got {node:?}"),
    }
}

fn compare_pipelines(nodes: &[SemanticNode], label: &str) {
    let engine = LayoutEngine::new(builtin_font_metrics());
    let style = MathStyle::Display;

    let legacy = engine.layout_node(&nodes[0], style).unwrap_or_else(|e| {
        panic!("{label}: legacy path must succeed: {e}");
    });
    let normalized = engine.layout_math_list(nodes, style).unwrap_or_else(|e| {
        panic!("{label}: normalized path must succeed: {e}");
    });

    let legacy_glues = glue_widths(&legacy);
    let normalized_glues = glue_widths(&normalized);

    assert_eq!(
        legacy_glues.len(),
        normalized_glues.len(),
        "{label}: glue count mismatch\nlegacy:    {legacy_glues:?}\nnormalized: {normalized_glues:?}"
    );
    for (i, (a, b)) in legacy_glues.iter().zip(normalized_glues.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-10,
            "{label}: glue[{i}] width mismatch: legacy={a} normalized={b}\nlegacy:    {legacy_glues:?}\nnormalized: {normalized_glues:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Cases where NO demotion fires AND merge_atoms_pass does not shrink the list.
// These should be glue-identical across both pipelines.
// ---------------------------------------------------------------------------

#[test]
fn bin_between_atoms_keeps_medium_glue() {
    // `a + b`: Ord Bin Ord — no demote, 1 glue (medium)
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('+', SymbolRole::Binary),
        sym('b', SymbolRole::Ordinary),
    ])];
    compare_pipelines(&nodes, "a + b");
}

#[test]
fn relation_between_atoms_keeps_thick_glue() {
    // `a = b`: Ord Rel Ord — no demote (Rel stays Rel between two Ords), 1 glue (thick)
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('=', SymbolRole::Relation),
        sym('b', SymbolRole::Ordinary),
    ])];
    compare_pipelines(&nodes, "a = b");
}

#[test]
fn operator_between_atoms_keeps_medium_glue() {
    // `a ∑ b`: Ord Op Ord — Op stays Op, 1 glue (medium each side)
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('∑', SymbolRole::LargeOperator),
        sym('b', SymbolRole::Ordinary),
    ])];
    compare_pipelines(&nodes, "a ∑ b");
}

#[test]
fn three_operators_take_glue_on_each_gap() {
    // `a + b = c`: Ord Bin Ord Rel Ord — Bin stays Bin (no demote), Rel stays Rel, 2 glues
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('+', SymbolRole::Binary),
        sym('b', SymbolRole::Ordinary),
        sym('=', SymbolRole::Relation),
        sym('c', SymbolRole::Ordinary),
    ])];
    compare_pipelines(&nodes, "a + b = c");
}

#[test]
fn inner_fraction_takes_thick_glue_on_both_sides() {
    // `a + \frac{b}{c}`: Ord Bin Inner — Bin stays Bin (no demote before Inner, Bin after Atom)
    // spacing: Bin→Inner = Medium, Ord→Bin = Medium.  2 glues total.
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('+', SymbolRole::Binary),
        SemanticNode::Fraction {
            num: Box::new(sym('b', SymbolRole::Ordinary)),
            den: Box::new(sym('c', SymbolRole::Ordinary)),
            line: LineStyle::Solid,
        },
    ])];
    compare_pipelines(&nodes, "a + frac{b}{c}");
}

// ---------------------------------------------------------------------------
// Case where merge_atoms_pass would shrink the list — intentionally ignored
// because the two pipelines are *not* expected to be equal here.  Keeping
// the test as a marker so future work can decide whether the layout engine
// should demote *before* merge (as TeX does) or *after*.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "merge_atoms_pass collapses consecutive Atoms; legacy path does not — pipelines intentionally diverge here"]
fn consecutive_atoms_merge_changes_glue_count() {
    // `a b + c`: Ord Ord Bin Ord → normalized collapses first two into one Atom
    // legacy: 3 glues (NoGlue, Medium, NoGlue)
    // normalized: 2 glues (Medium, NoGlue) — merged first two atoms
    // We skip this test; it documents the known delta.
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('a', SymbolRole::Ordinary),
        sym('b', SymbolRole::Ordinary),
        sym('+', SymbolRole::Binary),
        sym('c', SymbolRole::Ordinary),
    ])];
    let engine = LayoutEngine::new(builtin_font_metrics());
    let legacy = engine.layout_node(&nodes[0], MathStyle::Display).unwrap();
    let normalized = engine.layout_math_list(nodes.as_slice(), MathStyle::Display).unwrap();
    let lg = glue_widths(&legacy);
    let ng = glue_widths(&normalized);
    assert_ne!(lg.len(), ng.len(), "this test expects different glue counts! legacy={lg:?} normalized={ng:?}");
}
