//! Document layout integration tests for inline math inside paragraphs.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{
    LayoutHint,
    document::layout_with_hint,
    hint::Margin,
    layout,
    math::layout_formula_with_size,
    plan::{LayoutItem, LayoutPage, Rule, TextRun},
};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn math(source: &str) -> Inline {
    Inline::InlineMath { content: source.to_string(), language: Some("tex".to_string()) }
}

fn paragraph(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(11));
    graph.push_block(Block::Paragraph { content });
    graph
}

fn followed_by_marker(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(12));
    graph.push_block(Block::Paragraph { content });
    graph.push_block(Block::Paragraph { content: vec![text("Marker")] });
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

fn marker_baseline(page: &LayoutPage) -> f64 {
    text_runs(page).into_iter().find(|run| run.text == "Marker").expect("marker paragraph").y
}

#[test]
fn inline_formula_flows_between_surrounding_text_runs_on_the_text_baseline() {
    let graph = paragraph(vec![text("before "), math("x + y"), text(" after")]);
    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    let before = runs.iter().find(|run| run.text.trim() == "before").expect("leading text");
    let after = runs.iter().find(|run| run.text.trim() == "after").expect("trailing text");
    let glyphs: Vec<&TextRun> = runs.iter().copied().filter(|run| matches!(run.text.as_str(), "x" | "+" | "y")).collect();
    assert_eq!(glyphs.len(), 3, "three formula glyphs: {runs:#?}");

    for glyph in &glyphs {
        assert!(glyph.x > before.x, "glyph {} at {} must follow the leading run", glyph.text, glyph.x);
        assert!(glyph.x < after.x, "glyph {} at {} must precede the trailing run", glyph.text, after.x);
        assert!((glyph.y - before.y).abs() < 1e-9, "glyphs share the text baseline: {glyphs:#?}");
    }
    assert!(after.x > before.x);
}

#[test]
fn inline_fraction_keeps_its_bar_between_the_text_runs_and_grows_the_line() {
    let hint = LayoutHint { line_height: 1.2, ..LayoutHint::a4() };
    let (_, formula) = layout_formula_with_size("\\frac{a}{b}", false, hint.base_font_size).expect("fraction layout");
    assert!(
        formula.height + formula.depth > hint.base_font_size * hint.line_height,
        "fraction extent {} must exceed the plain line box",
        formula.height + formula.depth
    );

    let formula_graph = followed_by_marker(vec![text("The ratio "), math("\\frac{a}{b}"), text(" here")]);
    let plain_graph = followed_by_marker(vec![text("The ratio a/b here")]);
    let formula_plan = layout_with_hint(&formula_graph, &hint);
    let plain_plan = layout_with_hint(&plain_graph, &hint);

    let page = &formula_plan.pages[0];
    let runs = text_runs(page);
    let bars = rules(page);
    assert_eq!(bars.len(), 1, "one fraction bar: {bars:#?}");
    let bar = bars[0];
    let before = runs.iter().find(|run| run.text.trim() == "The ratio").expect("leading text");
    let after = runs.iter().find(|run| run.text.trim() == "here").expect("trailing text");
    assert!(bar.x1 > before.x, "bar {} must sit after the leading run", bar.x1);
    assert!(bar.x2 <= after.x + 1e-9, "bar right {} must end before the trailing run", bar.x2);

    let numerator = runs.iter().find(|run| run.text == "a").expect("numerator");
    let denominator = runs.iter().find(|run| run.text == "b").expect("denominator");
    assert!(numerator.y < bar.y1, "numerator {} must sit above the bar {}", numerator.y, bar.y1);
    assert!(denominator.y > bar.y1, "denominator {} must sit below the bar {}", denominator.y, bar.y1);

    assert!(
        marker_baseline(page) > marker_baseline(&plain_plan.pages[0]),
        "the fraction line must consume more vertical space than a plain paragraph"
    );
}

#[test]
fn formula_that_does_not_fit_moves_to_the_next_line() {
    let hint = LayoutHint::a4().with_page_size(240.0, 400.0).with_margin(Margin::uniform(40.0));
    let graph = paragraph(vec![text(&"a".repeat(28)), math("x + y")]);
    let plan = layout_with_hint(&graph, &hint);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    let filler = runs.iter().find(|run| run.text.starts_with("aaa")).expect("filler run");
    let glyph = runs.iter().find(|run| run.text == "x").expect("formula glyph");
    assert!(glyph.y > filler.y, "formula baseline {} must move below the filler {}", glyph.y, filler.y);
    assert!((glyph.x - hint.margin.left).abs() < 1e-9, "wrapped formula starts at the column left edge");
    assert!(glyph.x + 8.0 <= plan.pages[0].width);
}

#[test]
fn formula_wider_than_the_content_width_is_still_painted_clamped() {
    let source = format!("{}x", "x+".repeat(100));
    let graph = paragraph(vec![math(&source)]);
    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert_eq!(runs.len(), 201, "every formula glyph is painted, none dropped");
    assert!(rules(page).is_empty(), "a plain symbol sequence contributes no rules");
    assert!(runs.iter().all(|run| matches!(run.text.as_str(), "x" | "+")), "oversized formula keeps its glyphs");

    let first = runs.first().expect("first glyph");
    assert!((first.x - hint.margin.left).abs() < 1e-9, "clamped formula starts at the column left edge");
    assert!(runs.iter().all(|run| run.x >= 0.0 && run.x <= page.width), "no glyph may exceed the page width");
    assert!(runs.iter().all(|run| run.x + 0.5 * run.size <= page.width), "glyph advances must stay inside the page");
    let baseline = runs[0].y;
    assert!(runs.iter().all(|run| (run.y - baseline).abs() < 1e-9), "an oversized formula stays on one line");
}

#[test]
fn unparseable_inline_formula_falls_back_to_its_raw_source() {
    let source = "\\notacommand{x}";
    let graph = paragraph(vec![text("See "), math(source), text(" here")]);
    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert_eq!(runs.len(), 1, "fallback joins the surrounding text flow: {runs:#?}");
    assert_eq!(runs[0].text, format!("See {source} here"));
    assert!(rules(page).is_empty(), "text fallback emits no rules");
}

#[test]
fn pure_text_paragraphs_keep_one_run_per_line_at_the_left_margin() {
    let graph = paragraph(vec![text("Pure text stays.")]);
    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "Pure text stays.");
    assert_eq!(runs[0].x, hint.margin.left);
    assert!(runs[0].y > hint.margin.top && runs[0].y < hint.margin.top + runs[0].size);
}
