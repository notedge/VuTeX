//! Document layout integration tests for display math blocks.

use notedown_ir::{Block, DocumentGraph, DocumentId};
use vutex_layout::{
    LayoutHint, layout,
    math::layout_formula_with_size,
    plan::{LayoutItem, LayoutPage, Rule, TextRun},
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

#[test]
fn simple_formula_emits_left_to_right_glyphs_on_one_baseline() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    assert_eq!(plan.pages.len(), 1);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert!(runs.len() >= 3, "expected one run per visible glyph: {runs:#?}");
    assert_eq!(runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(), ["x", "+", "y"]);

    for window in runs.windows(2) {
        assert!(window[1].x > window[0].x, "glyphs must advance left to right: {window:#?}");
        assert!((window[1].y - window[0].y).abs() < 1e-9, "glyphs share a baseline: {window:#?}");
    }

    assert!(runs[0].x >= hint.margin.left, "first glyph {} starts before the content box", runs[0].x);
    assert!(runs.iter().all(|run| run.x <= hint.page_width - hint.margin.right));
    assert!(runs.iter().all(|run| run.y >= hint.margin.top && run.y <= hint.page_height - hint.margin.bottom));
    assert!(runs.iter().all(|run| run.size > 0.0));
    assert!(
        runs.iter().all(|run| run.font_family == "serif"),
        "math glyphs must paint with a usable family, got {:?}",
        runs.iter().map(|run| run.font_family.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn display_formula_is_centred_in_the_content_column() {
    let plan = layout(&math_graph("x + y"));
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let (_, formula) = layout_formula_with_size("x + y", true, hint.base_font_size).expect("formula layout");

    assert!(runs[0].x > hint.margin.left, "centred formula {} must start inside the margin, not on it", runs[0].x);

    let expected_offset = (hint.content_width() - formula.width) / 2.0;
    let actual_offset = runs[0].x - hint.margin.left;
    assert!(
        (actual_offset - expected_offset).abs() < 1.0,
        "left offset {actual_offset} should centre a {} pt formula in {} pt",
        formula.width,
        hint.content_width()
    );

    // The glyph advance is 0.5 em for the builtin metrics, so the last run's
    // right edge must land on the formula box's right edge.
    let last = runs.last().expect("at least one glyph");
    let right_edge = last.x + 0.5 * last.size;
    let box_right = runs[0].x + formula.width;
    assert!((right_edge - box_right).abs() < 1.0, "formula right edge {right_edge} vs box {box_right}");
    assert!(box_right <= hint.page_width - hint.margin.right, "formula must stay inside the content width");
}

#[test]
fn fraction_emits_a_rule_between_numerator_and_denominator() {
    let plan = layout(&math_graph("\\frac{a}{b}"));
    let page = &plan.pages[0];
    let runs = text_runs(page);
    let bars = rules(page);

    assert_eq!(bars.len(), 1, "a fraction contributes exactly one bar: {bars:#?}");
    let bar = bars[0];
    assert!(bar.x2 > bar.x1, "fraction bar must have width: {bar:?}");
    assert!(bar.thickness >= 0.4, "fraction bar must stay visible: {bar:?}");

    let mut texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    texts.sort_unstable();
    assert_eq!(texts, ["a", "b"], "runs: {runs:#?}");

    let numerator = runs.iter().find(|run| run.text == "a").expect("numerator run");
    let denominator = runs.iter().find(|run| run.text == "b").expect("denominator run");
    assert!(numerator.y < bar.y1, "numerator baseline {} must sit above the bar at {}", numerator.y, bar.y1);
    assert!(denominator.y > bar.y1, "denominator baseline {} must sit below the bar at {}", denominator.y, bar.y1);
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
                _ => {}
            }
        }
    }

    // Every math block contributes a fraction bar, none was dropped.
    let bars: usize = plan.pages.iter().map(|page| rules(page).len()).sum();
    assert_eq!(bars, 80, "expected one fraction bar per block, got {bars}");
}
