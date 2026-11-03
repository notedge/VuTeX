//! Source spans land on the paint tree and the diagnostics.
//!
//! Gate 2 of the layout-result contract: every painted glyph points back at
//! the authored formula source, and diagnostics that degrade a fragment
//! carry the fragment's span. The assertions slice the result's own
//! `source`, so a wrong offset fails the test instead of looking plausible.
//! Glyph chars are matched through their span slice, not the painted
//! codepoint: the alphalect maps authored letters onto math-alphabet
//! codepoints, so the char a glyph paints is not the char the author typed.

use vutex_layout::math::{layout_formula_source, lower_formula};
use vutex_types::SourceSpan;
use vutex_types::math::{FormulaLayoutResult, PaintNode};

/// Glyph spans in paint order (depth-first over the paint tree).
fn collect_spans(result: &FormulaLayoutResult) -> Vec<Option<SourceSpan>> {
    let mut out = Vec::new();
    walk(&result.paint, &mut out);
    out
}

fn walk(node: &PaintNode, out: &mut Vec<Option<SourceSpan>>) {
    match node {
        PaintNode::Group { children, .. } => {
            for child in children {
                walk(child, out);
            }
        }
        PaintNode::GlyphRun { spans, .. } => {
            out.extend(spans.iter().copied());
        }
        PaintNode::Rect { .. } | PaintNode::VectorPath { .. } => {}
    }
}

fn slice_span(result: &FormulaLayoutResult, span: SourceSpan) -> String {
    result.source.get(span.start as usize..span.end as usize).unwrap_or("").to_string()
}

/// The span of the first painted glyph whose source slice is `text`.
fn span_of_slice(result: &FormulaLayoutResult, text: &str) -> SourceSpan {
    collect_spans(result)
        .into_iter()
        .flatten()
        .find(|span| slice_span(result, *span) == text)
        .unwrap_or_else(|| panic!("no painted glyph points at `{text}` in `{}`", result.source))
}

#[test]
fn glyph_spans_point_back_at_the_formula_source() {
    let (result, _root, _diagnostics) = layout_formula_source("x + \\alpha", false, 16.0).expect("layout");
    assert_eq!(result.source, "x + \\alpha");
    let slices: Vec<String> = collect_spans(&result).into_iter().flatten().map(|span| slice_span(&result, span)).collect();
    assert_eq!(slices, vec!["x", "+", "\\alpha"], "every authored atom paints one glyph under its own span");
}

#[test]
fn accent_glyph_and_base_keep_their_own_spans() {
    let (result, _root, _diagnostics) = layout_formula_source("\\hat{x}", false, 16.0).expect("layout");
    let base = span_of_slice(&result, "x");
    assert_eq!((base.start, base.end), (5, 6));
    let accent = span_of_slice(&result, "\\hat{x}");
    assert_eq!((accent.start, accent.end), (0, 7), "the accent glyph points at the whole command");
}

#[test]
fn script_glyphs_carry_their_own_spans() {
    let (result, _root, _diagnostics) = layout_formula_source("x^2", false, 16.0).expect("layout");
    let base = span_of_slice(&result, "x");
    assert_eq!((base.start, base.end), (0, 1));
    let sup = span_of_slice(&result, "2");
    assert_eq!((sup.start, sup.end), (2, 3));
}

#[test]
fn starred_alignedat_cells_keep_spans_through_layout() {
    let source = "\\begin{alignedat*}{2} a &= b \\end{alignedat*}";
    let (result, _root, _diagnostics) = layout_formula_source(source, true, 16.0).expect("layout");
    let a = span_of_slice(&result, "a");
    assert_eq!((a.start, a.end), (22, 23));
    let relation = span_of_slice(&result, "=");
    assert_eq!((relation.start, relation.end), (25, 26));
    let b = span_of_slice(&result, "b");
    assert_eq!((b.start, b.end), (27, 28));
}

#[test]
fn unsupported_fragment_keeps_its_span_on_text_and_diagnostic() {
    let (tree, diagnostics) = lower_formula("\\foo", false).expect("layout");
    let _ = tree;
    let diagnostic = diagnostics.iter().find(|d| d.code == "vutex.math.unsupported_expression").expect("a diagnostic");
    let span = diagnostic.span.expect("the diagnostic points at the fragment");
    assert_eq!("\\foo".get(span.start as usize..span.end as usize), Some("\\foo"));
    let (result, _root, _diagnostics) = layout_formula_source("\\foo", false, 16.0).expect("layout");
    // The fallback text paints under the same span.
    let slices: Vec<String> = collect_spans(&result).into_iter().flatten().map(|span| slice_span(&result, span)).collect();
    assert!(!slices.is_empty(), "the unsupported source stays visible");
    assert!(slices.iter().all(|slice| slice == "\\foo"), "every fallback glyph points at the fragment: {slices:?}");
}

#[test]
fn engine_only_layout_carries_no_source() {
    // The engine entry point sees layout nodes only; the source text is
    // filled in by the pipeline that owns the formula.
    use vutex_layout::math::LayoutEngine;
    use vutex_text::math::builtin_font_metrics;
    use vutex_types::math::{FontStyle, FormulaNode, GlyphKey, MathStyle, SymbolRole};
    let engine = LayoutEngine::new(builtin_font_metrics()).with_base_size(16.0);
    let nodes = vec![FormulaNode::Symbol {
        glyph_key: GlyphKey::from_char('x', None, FontStyle::Math),
        role: SymbolRole::Ordinary,
        span: Some(SourceSpan { start: 0, end: 1 }),
    }];
    let (result, _root) = engine.layout_formula_result(&nodes, MathStyle::Text).expect("layout");
    assert!(result.source.is_empty(), "the engine does not invent a source text");
    let spans = collect_spans(&result);
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0], Some(SourceSpan { start: 0, end: 1 }), "the glyph keeps the span it was authored with");
}
