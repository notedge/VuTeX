//! Inline emphasis layout tests: styled spans reach positioned text runs.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{
    LayoutHint,
    document::layout_with_hint,
    hint::Margin,
    layout,
    plan::{LayoutItem, LayoutPage, TextRun},
};
use vutex_types::math::FontStyle;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn code(value: &str) -> Inline {
    Inline::InlineCode { text: value.to_string() }
}

fn paragraph(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(21));
    graph.push_block(Block::Paragraph { content });
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

#[test]
fn bold_span_becomes_its_own_text_run() {
    let graph = paragraph(vec![text("Start "), styled("bold", vec![text("thick")]), text(" end")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "a style change must split the run: {runs:#?}");
    assert_eq!(runs[0].text, "Start ");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[1].text, "thick");
    assert_eq!(runs[1].style, FontStyle::Bold);
    assert_eq!(runs[1].font_family, "serif");
    assert_eq!(runs[2].text, " end");
    assert_eq!(runs[2].style, FontStyle::Normal);

    // Styled runs keep flowing on the same baseline, left to right.
    assert!(runs[1].x > runs[0].x);
    assert!(runs[2].x > runs[1].x);
    assert!(runs.iter().all(|run| (run.y - runs[0].y).abs() < 1e-9));
}

#[test]
fn strong_and_em_aliases_map_to_the_same_faces() {
    let graph = paragraph(vec![styled("strong", vec![text("heavy")]), text(" "), styled("em", vec![text("slanted")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "{runs:#?}");
    assert_eq!(runs[0].text, "heavy");
    assert_eq!(runs[0].style, FontStyle::Bold);
    assert_eq!(runs[1].text, " ");
    assert_eq!(runs[1].style, FontStyle::Normal);
    assert_eq!(runs[2].text, "slanted");
    assert_eq!(runs[2].style, FontStyle::Italic);
}

#[test]
fn inline_code_uses_the_monospace_family() {
    let graph = paragraph(vec![text("run "), code("npm test"), text(" now")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 3, "{runs:#?}");
    let code_run = runs.iter().find(|run| run.text == "npm test").expect("inline code run");
    assert_eq!(code_run.font_family, "monospace");
    assert_eq!(code_run.style, FontStyle::Normal);
    assert!(runs.iter().filter(|run| run.text != "npm test").all(|run| run.font_family == "serif"));
}

#[test]
fn mono_style_spans_also_use_the_monospace_family() {
    let graph = paragraph(vec![styled("code", vec![text("verbatim")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "verbatim");
    assert_eq!(runs[0].font_family, "monospace");
    assert_eq!(runs[0].style, FontStyle::Monospace);
}

#[test]
fn unknown_style_names_stay_body_text() {
    let graph = paragraph(vec![styled("highlight", vec![text("plain")])]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1, "{runs:#?}");
    assert_eq!(runs[0].text, "plain");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[0].font_family, "serif");
}

#[test]
fn styled_runs_still_wrap_inside_the_content_width() {
    let hint = LayoutHint::a4().with_page_size(320.0, 600.0).with_margin(Margin::uniform(40.0));
    let body = "word ".repeat(40);
    let graph = paragraph(vec![text("Lead "), styled("bold", vec![text(body.trim())]), text(" tail")]);
    let plan = layout_with_hint(&graph, &hint);
    let runs = text_runs(&plan.pages[0]);

    let bold: Vec<&TextRun> = runs.iter().copied().filter(|run| run.style == FontStyle::Bold).collect();
    assert!(bold.len() > 1, "the styled span must wrap onto several lines: {bold:#?}");
    let baselines: std::collections::HashSet<String> = bold.iter().map(|run| format!("{:.3}", run.y)).collect();
    assert!(baselines.len() > 1, "wrapped styled runs must sit on distinct baselines: {bold:#?}");

    assert!(runs.iter().all(|run| run.x >= hint.margin.left - 1e-9), "runs start inside the left margin");
    assert!(
        runs.iter().all(|run| run.x <= hint.page_width - hint.margin.right + 1e-9),
        "runs never start past the right margin: {runs:#?}"
    );

    // The unstyled lead and tail keep their own runs.
    assert!(runs.iter().any(|run| run.style == FontStyle::Normal && run.text.starts_with("Lead")));
    assert!(runs.iter().any(|run| run.style == FontStyle::Normal && run.text.contains("tail")));
}

#[test]
fn unstyled_paragraphs_keep_one_run_per_line() {
    let graph = paragraph(vec![text("Pure text stays.")]);
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].text, "Pure text stays.");
    assert_eq!(runs[0].style, FontStyle::Normal);
    assert_eq!(runs[0].font_family, "serif");
}
