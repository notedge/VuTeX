//! Document layout integration tests for display math blocks.

use notedown_ir::{Block, DocumentGraph, DocumentId};
use vutex_layout::{
    LayoutHint, layout,
    math::layout_formula_with_size,
    plan::{LayoutItem, LayoutPage, Path, Rule, TextRun},
};

fn math_graph(content: &str) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Math { content: content.to_string(), language: Some("tex".to_string()) });
    graph
}

fn text_runs(page: &LayoutPage) -> Vec<&TextRun> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

fn rules(page: &LayoutPage) -> Vec<&Rule> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::Rule(rule) => Some(rule),
            _ => None,
        })
        .collect()
}

fn paths(page: &LayoutPage) -> Vec<&Path> {
    page.items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::Path(p) => Some(p),
            _ => None,
        })
        .collect()
}

#[test]
fn simple_formula_emits_left_to_right_glyphs_on_one_baseline() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    assert_eq!(plan.pages.len(), 1);
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let paths = paths(page);

    // Glyphs are emitted as either text runs or paths depending on whether the
    // embedded font supplied an outline. Either way we expect three glyphs.
    assert!(runs.len() + paths.len() >= 3, "expected three visible glyphs as runs or paths: runs={runs:#?} paths={paths:#?}");

    // Ordering and baseline checks: collect all non-rule glyph items.
    let mut glyph_items: Vec<(f64, f64, f64)> = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) => Some((r.x, r.y, 0.0)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y, 0.0)),
            _ => None,
        })
        .collect();
    glyph_items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    for window in glyph_items.windows(2) {
        assert!(window[1].0 > window[0].0, "glyphs must advance left to right: {glyph_items:#?}");
        assert!((window[1].1 - window[0].1).abs() < 1e-9, "glyphs share a baseline: {glyph_items:#?}");
    }

    if let Some(first) = glyph_items.first() {
        assert!(first.0 >= hint.margin.left - 1e-6, "first glyph at {} starts before the content box", first.0);
    }
}

#[test]
fn display_formula_is_centred_in_the_content_column() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let (_, formula) = layout_formula_with_size("x + y", true, hint.base_font_size).expect("formula layout");

    let first = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) if matches!(r.text.as_str(), "x" | "+" | "y") => Some(r.x),
            LayoutItem::Path(p) if !p.d.is_empty() => Some(p.x),
            _ => None,
        })
        .next()
        .expect("at least one glyph must be placed");

    assert!(first > hint.margin.left, "centred formula {} must start inside the margin, not on it", first);

    let expected_offset = (hint.content_width() - formula.width) / 2.0;
    let actual_offset = first - hint.margin.left;
    assert!(
        (actual_offset - expected_offset).abs() < 1.0,
        "left offset {actual_offset} should centre a {} pt formula in {} pt",
        formula.width,
        hint.content_width()
    );
}

#[test]
fn fraction_emits_a_rule_between_numerator_and_denominator() {
    let plan = layout(&math_graph("\\frac{a}{b}"));
    let page = &plan.pages[0];
    let bars = rules(page);

    assert_eq!(bars.len(), 1, "a fraction contributes exactly one bar: {bars:#?}");
    let bar = bars[0];
    assert!(bar.x2 > bar.x1, "fraction bar must have width: {bar:?}");
    assert!(bar.thickness >= 0.4, "fraction bar must stay visible: {bar:?}");

    // Numerator and denominator may be emitted as paths or text runs. Look up
    // both shapes so the test is robust to whichever branch fires.
    let mut glyph_positions: Vec<(f64, f64)> = page
        .items
        .iter()
        .filter_map(|item| match item {
            LayoutItem::TextRun(r) if matches!(r.text.as_str(), "a" | "b") => Some((r.x, r.y)),
            LayoutItem::Path(p) if !p.d.is_empty() => Some((p.x, p.y)),
            _ => None,
        })
        .collect();
    glyph_positions.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

    assert!(glyph_positions.len() >= 2, "numerator and denominator must be present");
    let numerator = glyph_positions[0];
    let denominator = glyph_positions[glyph_positions.len() - 1];
    assert!(numerator.1 < bar.y1, "numerator baseline {} must sit above the bar at {}", numerator.1, bar.y1);
    assert!(denominator.1 > bar.y1, "denominator baseline {} must sit below the bar at {}", denominator.1, bar.y1);
}

#[test]
fn unparseable_formula_falls_back_to_its_raw_source() {
    let source = "\\notacommand{x}";
    let plan = layout(&math_graph(source));
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert!(!runs.is_empty(), "fallback must still paint something");
    let painted: String = runs.iter().map(|run| run.text.as_str()).collect();
    assert_eq!(painted, source, "fallback should render the raw source verbatim: {runs:#?}");
    assert!(rules(page).is_empty(), "text fallback emits no rules");
    assert!(runs.iter().all(|run| run.y <= page.height));
}

#[test]
fn math_blocks_paginate_without_leaving_the_page() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    for _ in 0..80 {
        graph.push_block(Block::Math { content: "\\frac{a}{b}".to_string(), language: None });
    }

    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    assert!(plan.pages.len() > 1, "many math blocks must paginate, got {} page(s)", plan.pages.len());

    for page in &plan.pages {
        assert!(!page.items.is_empty(), "pages are only created when content moves to them");
        for item in &page.items {
            match item {
                LayoutItem::TextRun(run) => {
                    assert!(run.y >= hint.margin.top - 1e-9, "baseline {} above the top margin", run.y);
                    assert!(run.y <= page.height, "baseline {} outside page height {}", run.y, page.height);
                    assert!(run.x <= page.width, "x {} outside page width {}", run.x, page.width);
                }
                LayoutItem::Rule(rule) => {
                    assert!(rule.y1 >= 0.0 && rule.y1 <= page.height, "rule {} outside page height {}", rule.y1, page.height);
                    assert!(rule.x1 >= 0.0 && rule.x2 <= page.width, "rule spans {}..{}", rule.x1, rule.x2);
                }
                LayoutItem::Path(p) => {
                    assert!(p.y >= hint.margin.top - 1e-9, "path baseline {} above the top margin", p.y);
                    assert!(p.y <= page.height + 1e-9, "path baseline {} outside page height {}", p.y, page.height);
                }
                _ => {}
            }
        }
    }

    // Every math block contributes a fraction bar, none was dropped.
    let bars: usize = plan.pages.iter().map(|page| rules(page).len()).sum();
    assert_eq!(bars, 80, "expected one fraction bar per block, got {bars}");
}
