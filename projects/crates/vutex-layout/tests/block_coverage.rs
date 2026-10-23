//! Layout coverage for non-paragraph blocks and inline images.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, SemanticStatus, TableCell, TableRow};
use vutex_layout::{
    layout,
    plan::{LayoutItem, LayoutPage, TextRun},
};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
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
fn lays_out_quote_code_table_break_and_image() {
    let mut graph = DocumentGraph::new(DocumentId(4));
    graph.push_block(Block::Quote { content: vec![text("Quoted line")] });
    graph.push_block(Block::Code { language: Some("rust".to_string()), content: "fn main() {}".to_string() });
    graph.push_block(Block::Table {
        caption: Some("Table 1".to_string()),
        rows: vec![
            TableRow {
                is_header: true,
                cells: vec![
                    TableCell { content: vec![text("Metric")], colspan: 1, rowspan: 1 },
                    TableCell { content: vec![text("Value")], colspan: 1, rowspan: 1 },
                ],
            },
            TableRow {
                is_header: false,
                cells: vec![
                    TableCell { content: vec![text("Accuracy")], colspan: 1, rowspan: 1 },
                    TableCell { content: vec![text("0.9")], colspan: 1, rowspan: 1 },
                ],
            },
        ],
    });
    graph.push_block(Block::Opaque {
        kind: "thematic_break".to_string(),
        payload_hint: "---".to_string(),
        status: SemanticStatus::Partial,
    });
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Image {
            source: "figure.png".to_string(),
            alt: vec![text("Figure alt")],
            title: None,
            asset: None,
        }],
    });

    let plan = layout(&graph);
    let page = &plan.pages[0];
    let runs = text_runs(page);

    assert!(runs.iter().any(|run| run.text == "Quoted line"));
    assert!(runs.iter().any(|run| run.font_family == "monospace" && run.text == "fn main() {}"));
    assert!(runs.iter().any(|run| run.text == "Metric"));
    assert!(runs.iter().any(|run| run.text == "0.9"));
    assert!(page.items.iter().any(|item| matches!(item, LayoutItem::Rule(_))), "table and thematic break rules");
    assert!(page.items.iter().any(|item| matches!(item, LayoutItem::Rect(rect) if rect.filled)), "code block frame");
    assert!(
        page.items.iter().any(|item| matches!(item, LayoutItem::ImagePlaceholder(image) if image.alt == "Figure alt")),
        "inline images reserve a placeholder"
    );
}
