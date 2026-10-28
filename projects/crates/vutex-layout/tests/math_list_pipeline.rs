//! Math-list pipeline parity and glue-shape smoke tests.
//!
//! Every math list reaches the layout engine through the normalized pipeline
//!   `SemanticNode → project_semantic → normalize_math_list → tex_skip_between_noad → LayoutNode`
//! driven by `LayoutEngine::layout_math_list`, which `layout_node` on a
//! `Sequence` delegates to.
//!
//! `normalize_math_list` runs demotion and consecutive-Atom merge in one pass,
//! and `tex_skip_between_noad` consumes the already-resolved `NoadKind`
//! directly (no re-demote).  These tests pin the glue the two public entry
//! points emit for the Appendix G cases where no atom merge shrinks the list,
//! so re-introducing a second, role-based demotion path anywhere in `layout`
//! shows up as a glue mismatch here.

use vutex_layout::math::{LayoutEngine, LayoutNode};
use vutex_text::math::builtin_font_metrics;
use vutex_types::math::{
    FontStyle, GlyphKey, LimitsPolicy, LineStyle, MathStyle, SemanticNode, SymbolRole, normalize_math_list,
    project_semantic,
};

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
// Both entry points must agree on the glue sequence here.
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
fn inner_fraction_next_to_binary_keeps_medium_glue() {
    // `a + \frac{b}{c}`: Ord Bin Inner — Bin stays Bin (no demote before Inner, Bin after Atom)
    // Appendix G: Ord→Bin = 2 (Medium), Bin→Inner = 2 (Medium).  2 glues total.
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
// `\limits` / `\nolimits` ownership
//
// The policy belongs to the atom it follows.  It travels through the noad that
// atom produced and into `layout_subsup` as an argument, so neither projection
// nor layout can hand it to a different operator.
// ---------------------------------------------------------------------------

/// Project and normalize a math list in text style, the context where an
/// accidental `\limits` shows up as stacked instead of side-mounted scripts.
fn text_noads(nodes: &[SemanticNode]) -> vutex_types::math::MathList {
    normalize_math_list(project_semantic(nodes, MathStyle::Text))
}

/// A `\limits` authored after an ordinary atom is consumed by that atom: it is
/// not stored on it and it does not reach the operator that follows.
#[test]
fn limits_after_an_ordinary_atom_do_not_reach_a_later_operator() {
    let nodes = vec![
        SemanticNode::LimitsSpec(LimitsPolicy::Limits),
        SemanticNode::Subscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sub: Box::new(sym('1', SymbolRole::Ordinary)),
        },
        SemanticNode::Superscript {
            base: Box::new(sym('∑', SymbolRole::LargeOperator)),
            sup: Box::new(sym('2', SymbolRole::Ordinary)),
        },
        sym('y', SymbolRole::Ordinary),
    ];
    let list = text_noads(&nodes);
    assert_eq!(list.len(), 3);
    assert_eq!(list.0[0].limits, None, "`x` is not an operator and must not carry the policy");
    assert_eq!(
        list.0[1].limits, None,
        "a policy authored after `x` must not be attached to the following `∑`"
    );
    assert_eq!(list.0[1].kind, vutex_types::math::NoadKind::Op);
}

/// Consecutive `\limits` then `\nolimits` on one operator: the last policy
/// wins, and the policy stays on that operator.
#[test]
fn consecutive_limits_keep_the_last_policy() {
    let nodes = vec![
        SemanticNode::LimitsSpec(LimitsPolicy::Limits),
        SemanticNode::LimitsSpec(LimitsPolicy::NoLimits),
        SemanticNode::SubSuperscript {
            base: Box::new(sym('∑', SymbolRole::LargeOperator)),
            sub: Box::new(sym('1', SymbolRole::Ordinary)),
            sup: Box::new(sym('2', SymbolRole::Ordinary)),
        },
        sym('y', SymbolRole::Ordinary),
    ];
    let list = text_noads(&nodes);
    assert_eq!(list.len(), 2);
    assert_eq!(
        list.0[0].limits,
        Some(LimitsPolicy::NoLimits),
        "the last `\\limits` / `\\nolimits` before the operator wins"
    );
    assert_eq!(list.0[1].limits, None, "the policy must not reach the next atom");
}

/// A policy authored inside a group stays inside it: the operator after the
/// group keeps TeX's default placement.
#[test]
fn limits_inside_a_group_stay_with_that_operator() {
    let group = SemanticNode::Sequence(vec![
        SemanticNode::LimitsSpec(LimitsPolicy::Limits),
        SemanticNode::Superscript {
            base: Box::new(sym('∑', SymbolRole::LargeOperator)),
            sup: Box::new(sym('2', SymbolRole::Ordinary)),
        },
    ]);
    let nodes = vec![
        group,
        SemanticNode::Subscript {
            base: Box::new(sym('∫', SymbolRole::LargeOperator)),
            sub: Box::new(sym('0', SymbolRole::Ordinary)),
        },
    ];
    let list = text_noads(&nodes);
    assert_eq!(list.len(), 2);
    assert_eq!(list.0[0].limits, Some(LimitsPolicy::Limits), "the grouped `∑` keeps its own `\\limits`");
    assert_eq!(list.0[1].limits, None, "the grouped policy must not escape to `∫`");
}

/// Layout is a pure function of its arguments: the same math list laid out
/// twice on one engine produces the same tree, and laying out an unrelated
/// formula in between changes nothing.
#[test]
fn repeated_layout_of_one_math_list_is_identical() {
    let engine = LayoutEngine::new(builtin_font_metrics());
    let nodes = vec![SemanticNode::Sequence(vec![
        sym('x', SymbolRole::Ordinary),
        SemanticNode::LimitsSpec(LimitsPolicy::Limits),
        SemanticNode::Subscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sub: Box::new(sym('1', SymbolRole::Ordinary)),
        },
        SemanticNode::Superscript {
            base: Box::new(sym('∑', SymbolRole::LargeOperator)),
            sup: Box::new(sym('2', SymbolRole::Ordinary)),
        },
    ])];

    let first = engine.layout_math_list(&nodes, MathStyle::Text).expect("first layout");
    let second = engine.layout_math_list(&nodes, MathStyle::Text).expect("second layout");
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "the same math list must lay out identically every time"
    );

    let unrelated = vec![SemanticNode::SubSuperscript {
        base: Box::new(sym('∫', SymbolRole::LargeOperator)),
        sub: Box::new(sym('0', SymbolRole::Ordinary)),
        sup: Box::new(sym('1', SymbolRole::Ordinary)),
    }];
    engine.layout_math_list(&unrelated, MathStyle::Text).expect("unrelated layout");

    let third = engine.layout_math_list(&nodes, MathStyle::Text).expect("layout after an unrelated formula");
    assert_eq!(
        format!("{first:?}"),
        format!("{third:?}"),
        "no layout may leave state behind for the next one"
    );
}
