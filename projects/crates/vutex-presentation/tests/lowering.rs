//! Presentation lowering tests.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem};
use vutex_layout::layout;
use vutex_presentation::{DisplayItem, FlowBlock, build_display_presentation, build_flow_presentation};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn sample_graph() -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Section { level: 1, title: vec![text("Flow heading")], children: Vec::new() });
    graph.push_block(Block::Paragraph { content: vec![text("Flow paragraph.")] });
    graph.push_block(Block::List {
        ordered: true,
        items: vec![
            ListItem { content: vec![text("First item")], children: Vec::new(), number: Some(1) },
            ListItem { content: vec![text("Second item")], children: Vec::new(), number: Some(2) },
        ],
    });
    graph
}

#[test]
fn display_lowering_preserves_geometry_and_items() {
    let plan = layout(&sample_graph());
    let presentation = build_display_presentation(&plan);

    assert_eq!(presentation.pages.len(), plan.pages.len());
    for (display, layout_page) in presentation.pages.iter().zip(&plan.pages) {
        assert_eq!(display.width, layout_page.width);
        assert_eq!(display.height, layout_page.height);
        assert_eq!(display.items.len(), layout_page.items.len());
    }

    let first = &presentation.pages[0];
    assert!(matches!(first.items.first(), Some(DisplayItem::TextRun(run)) if run.text == "Flow heading"));
}

#[test]
fn flow_lowering_keeps_block_semantics() {
    let presentation = build_flow_presentation(&sample_graph());

    assert_eq!(presentation.root_tag, "article");
    assert_eq!(presentation.blocks.len(), 3);
    assert!(matches!(&presentation.blocks[0], FlowBlock::Heading { level: 1, text } if text == "Flow heading"));
    assert!(matches!(&presentation.blocks[1], FlowBlock::Paragraph { text } if text == "Flow paragraph."));
    match &presentation.blocks[2] {
        FlowBlock::List { ordered, items } => {
            assert!(ordered);
            assert_eq!(items, &vec!["First item".to_string(), "Second item".to_string()]);
        }
        other => panic!("expected a list block, got {other:?}"),
    }
}
