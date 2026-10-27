//! Document layout integration tests: block coverage and pagination.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem};
use vutex_layout::{
    LayoutHint, layout,
    plan::{LayoutItem, LayoutPage, TextRun},
};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn sample_graph() -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Section { level: 1, title: vec![text("Layout contract")], children: Vec::new() });
    graph.push_block(Block::Paragraph { content: vec![text("First paragraph.")] });
    graph.push_block(Block::Paragraph { content: vec![text("Second paragraph.")] });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![
            ListItem { content: vec![text("Alpha")], children: Vec::new(), number: None, is_task: false, is_checked: None },
            ListItem { content: vec![text("Beta")], children: Vec::new(), number: None, is_task: false, is_checked: None },
        ],
    });
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
fn lays_out_heading_paragraphs_and_list_on_one_a4_page() {
    let plan = layout(&sample_graph());
    let hint = LayoutHint::a4();

    assert_eq!(plan.pages.len(), 1);
    let page = &plan.pages[0];
    assert_eq!(page.width, hint.page_width);
    assert_eq!(page.height, hint.page_height);

    // One heading line, one line per paragraph, and a marker plus a line per list item.
    let runs = text_runs(page);
    assert_eq!(runs.len(), 7, "line boxes: {runs:#?}");

    assert_eq!(runs[0].text, "Layout contract");
    assert_eq!(runs[0].x, hint.margin.left);
    assert!(runs[0].y > hint.margin.top, "first baseline {} must sit below the top margin", runs[0].y);
    assert!(runs[0].y < hint.margin.top + runs[0].size, "first baseline {} must stay in the first line box", runs[0].y);
    assert_eq!(runs[0].font_family, "serif");

    assert_eq!(runs[1].text, "First paragraph.");
    assert_eq!(runs[2].text, "Second paragraph.");
    assert_eq!(runs[3].text, "•");
    assert_eq!(runs[4].text, "Alpha");
    assert_eq!(runs[5].text, "•");
    assert_eq!(runs[6].text, "Beta");

    // List content is indented past its marker, and later lines move down the page.
    assert!(runs[4].x > runs[3].x);
    assert!(runs[4].y >= runs[3].y);
    assert!(runs[1].y > runs[0].y);
    assert!(runs.iter().all(|run| run.y <= page.height));
}

#[test]
fn wraps_long_paragraphs_inside_the_content_width() {
    let mut graph = DocumentGraph::new(DocumentId(2));
    let body = "Word ".repeat(200);
    graph.push_block(Block::Paragraph { content: vec![text(body.trim())] });

    let plan = layout(&graph);
    let hint = LayoutHint::a4();
    let runs = text_runs(&plan.pages[0]);
    assert!(runs.len() > 1, "long paragraph must wrap onto several lines");
    assert!(runs.iter().all(|run| run.x == hint.margin.left));
    assert!(runs.iter().all(|run| run.y <= hint.page_height));
}

#[test]
fn paginates_long_documents_across_multiple_pages() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    for index in 0..120 {
        let body = format!("Paragraph {index} carries enough words to fill a few lines of the content area.");
        graph.push_block(Block::Paragraph { content: vec![text(&body)] });
    }

    let plan = layout(&graph);
    assert!(plan.pages.len() > 1, "expected pagination, got {} page(s)", plan.pages.len());

    let hint = LayoutHint::a4();
    for page in &plan.pages {
        assert!(!page.items.is_empty(), "pages are only created when content moves to them");
        assert_eq!(page.width, hint.page_width);
        assert_eq!(page.height, hint.page_height);
        for item in &page.items {
            if let LayoutItem::TextRun(run) = item {
                assert!(run.y >= hint.margin.top - 1e-9);
                assert!(run.y <= page.height, "baseline {} outside page height {}", run.y, page.height);
            }
        }
    }

    // Every paragraph contributes at least one line box, none is dropped.
    let lines: usize = plan.pages.iter().map(|page| text_runs(page).len()).sum();
    assert!(lines >= 120, "expected at least one line per paragraph, got {lines}");
}

#[test]
fn honours_explicit_page_size_and_margins() {
    let hint = LayoutHint::a4().with_page_size(320.0, 200.0).with_margin(vutex_layout::hint::Margin::uniform(40.0));
    let plan = vutex_layout::document::layout_with_hint(&sample_graph(), &hint);

    assert!(plan.pages.len() > 1, "a small page must force pagination");
    assert!(plan.pages.iter().all(|page| page.width == 320.0 && page.height == 200.0));
    let first = text_runs(&plan.pages[0]);
    assert_eq!(first[0].x, 40.0);
    assert!(first[0].y > 40.0);
}
