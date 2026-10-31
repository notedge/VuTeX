//! Smoke tests for [`LayoutEngine::layout_formula_result`].
//!
//! 只验证新入口能调通、返回非空 metrics 和非空 paint 树。不做 parity
//! 比对——那是 `math_list_pipeline.rs` 的职责。

use vutex_layout::math::LayoutEngine;
use vutex_text::math::builtin_font_metrics;
use vutex_types::math::{FontStyle, GlyphKey, MathStyle, SemanticNode, SymbolRole};

fn sym(c: char, role: SymbolRole) -> SemanticNode {
    SemanticNode::Symbol { glyph_key: GlyphKey::from_char(c, None, FontStyle::Math), role }
}

fn make_test_engine() -> LayoutEngine {
    LayoutEngine::new(builtin_font_metrics())
}

#[test]
fn simple_formula_produces_nonzero_metrics() {
    let engine = make_test_engine();
    let nodes = vec![sym('x', SymbolRole::Ordinary), sym('+', SymbolRole::Binary), sym('y', SymbolRole::Ordinary)];
    let (result, _tree) = engine.layout_formula_result(&nodes, MathStyle::Display).unwrap();

    // 基本 sanity：metrics 必须全是正数（x + glue + + + glue + y 的宽度肯定大于零）。
    assert!(result.metrics.width > vutex_types::math::Fixed::ZERO, "width must be > 0");
    assert!(result.metrics.height > vutex_types::math::Fixed::ZERO, "height must be > 0");
    // depth 允许为零（x、+、y 都没有 descender）。
    assert!(result.metrics.depth >= vutex_types::math::Fixed::ZERO);

    // ink_bounds 必须至少能覆盖 width。
    assert!(result.metrics.ink_bounds.width >= result.metrics.width, "ink_bounds.width must cover metrics.width");
}

#[test]
fn paint_tree_is_group_rooted() {
    let engine = make_test_engine();
    let nodes = vec![sym('a', SymbolRole::Ordinary), sym('+', SymbolRole::Binary), sym('b', SymbolRole::Ordinary)];
    let (result, _tree) = engine.layout_formula_result(&nodes, MathStyle::Text).unwrap();

    // paint 树根节点是 Group(HBox children lowered)。
    match &result.paint {
        vutex_types::math::PaintNode::Group { children, .. } => {
            assert!(!children.is_empty(), "paint tree group must have children");
        }
        other => panic!("expected PaintNode::Group root, got {other:?}"),
    }
}

#[test]
fn diagnostics_and_fonts_are_default_empty() {
    let engine = make_test_engine();
    let nodes = vec![sym('x', SymbolRole::Ordinary)];
    let (result, _tree) = engine.layout_formula_result(&nodes, MathStyle::Script).unwrap();
    assert!(result.diagnostics.is_empty(), "diagnostics 占位应为空");
    assert!(result.fonts.faces.is_empty(), "fonts 占位应为空");
}
