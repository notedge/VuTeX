//! Presentation lowering tests.

use notedown_ir::{Attributes, Block, DocumentGraph, DocumentId, Inline, ListItem, RelationEndpoint, TableCell, TableRow};
use vutex_layout::layout;
use vutex_presentation::{DisplayItem, FlowBlock, FlowRun, FlowTableRow, build_display_presentation, build_flow_presentation};
use vutex_types::math::FontStyle;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn cell(content: Vec<Inline>) -> TableCell {
    TableCell { content, colspan: 1, rowspan: 1 }
}

fn run(text: &str, style: FontStyle) -> FlowRun {
    FlowRun { text: text.to_string(), style, href: None }
}

fn link_run(text: &str, style: FontStyle, href: &str) -> FlowRun {
    FlowRun { text: text.to_string(), style, href: Some(href.to_string()) }
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
    assert!(
        matches!(&presentation.blocks[0], FlowBlock::Heading { level: 1, runs } if runs == &vec![run("Flow heading", FontStyle::Normal)])
    );
    assert!(matches!(
        &presentation.blocks[1],
        FlowBlock::Paragraph { runs } if runs.len() == 1 && runs[0].text == "Flow paragraph."
    ));
    match &presentation.blocks[2] {
        FlowBlock::List { ordered, items } => {
            assert!(ordered);
            assert_eq!(items, &vec![vec![run("First item", FontStyle::Normal)], vec![run("Second item", FontStyle::Normal)]]);
        }
        other => panic!("expected a list block, got {other:?}"),
    }
}

#[test]
fn flow_heading_runs_preserve_bold() {
    let mut graph = DocumentGraph::new(DocumentId(2));
    graph.push_block(Block::Section {
        level: 2,
        title: vec![text("Plain "), styled("bold", vec![text("heavy")])],
        children: Vec::new(),
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::Heading { level: 2, runs: vec![run("Plain ", FontStyle::Normal), run("heavy", FontStyle::Bold)] }]
    );
}

#[test]
fn flow_list_item_runs_preserve_italic_per_item() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    graph.push_block(Block::List {
        ordered: false,
        items: vec![
            ListItem { content: vec![styled("italic", vec![text("slanted")])], children: Vec::new(), number: None },
            ListItem { content: vec![text("plain")], children: Vec::new(), number: None },
        ],
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::List {
            ordered: false,
            items: vec![vec![run("slanted", FontStyle::Italic)], vec![run("plain", FontStyle::Normal)]],
        }]
    );
}

#[test]
fn flow_quote_runs_keep_inline_code_as_monospace() {
    let mut graph = DocumentGraph::new(DocumentId(4));
    graph.push_block(Block::Quote {
        content: vec![text("Use "), Inline::InlineCode { text: "vutex".to_string() }, text(" here.")],
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::Quote {
            runs: vec![run("Use ", FontStyle::Normal), run("vutex", FontStyle::Monospace), run(" here.", FontStyle::Normal),],
        }]
    );
}

#[test]
fn flow_table_cell_runs_preserve_bold_and_header_role() {
    let mut graph = DocumentGraph::new(DocumentId(5));
    graph.push_block(Block::Table {
        caption: Some("Scores".to_string()),
        rows: vec![
            TableRow { is_header: true, cells: vec![cell(vec![styled("bold", vec![text("Metric")])])] },
            TableRow {
                is_header: false,
                cells: vec![cell(vec![text("Accuracy")]), cell(vec![styled("bold", vec![text("0.9")])])],
            },
        ],
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::Table {
            caption: Some("Scores".to_string()),
            rows: vec![
                FlowTableRow { is_header: true, cells: vec![vec![run("Metric", FontStyle::Bold)]] },
                FlowTableRow {
                    is_header: false,
                    cells: vec![vec![run("Accuracy", FontStyle::Normal)], vec![run("0.9", FontStyle::Bold)]],
                },
            ],
        }]
    );
}

#[test]
fn flow_heading_keeps_image_alt_text() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    graph.push_block(Block::Section {
        level: 1,
        title: vec![Inline::Image { source: "logo.png".to_string(), alt: vec![text("Logo")], title: None, asset: None }],
        children: Vec::new(),
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(presentation.blocks, vec![FlowBlock::Heading { level: 1, runs: vec![run("Logo", FontStyle::Normal)] }]);
}

#[test]
fn flow_adjacent_same_style_text_merges() {
    let mut graph = DocumentGraph::new(DocumentId(6));
    graph.push_block(Block::Section {
        level: 1,
        title: vec![styled("strong", vec![text("one "), styled("bold", vec![text("two")])])],
        children: Vec::new(),
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(presentation.blocks, vec![FlowBlock::Heading { level: 1, runs: vec![run("one two", FontStyle::Bold)] }]);
}

#[test]
fn flow_wrapped_children_keep_their_styles() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    graph.push_block(Block::Paragraph {
        content: vec![
            Inline::Attributed { attributes: Attributes::default(), children: vec![styled("bold", vec![text("kept")])] },
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
                display: vec![text(" see "), styled("italic", vec![text("note")])],
                title: None,
            },
            Inline::Note { label: "1".to_string(), display: Some(vec![styled("bold", vec![text("foot")])]) },
        ],
    });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::Paragraph {
            runs: vec![
                run("kept", FontStyle::Bold),
                link_run(" see ", FontStyle::Normal, "https://example.com"),
                link_run("note", FontStyle::Italic, "https://example.com"),
                run("foot", FontStyle::Bold),
            ],
        }]
    );
}

#[test]
fn flow_external_link_runs_carry_href_and_keep_authored_style() {
    let mut graph = DocumentGraph::new(DocumentId(10));
    graph.push_block(Block::Paragraph {
        content: vec![
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com/guide".to_string()),
                display: vec![text("guide")],
                title: None,
            },
            text(" then "),
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com/bold".to_string()),
                display: vec![styled("bold", vec![text("bold link")])],
                title: None,
            },
        ],
    });

    let presentation = build_flow_presentation(&graph);

    match &presentation.blocks[0] {
        FlowBlock::Paragraph { runs } => assert_eq!(
            runs,
            &vec![
                link_run("guide", FontStyle::Normal, "https://example.com/guide"),
                run(" then ", FontStyle::Normal),
                link_run("bold link", FontStyle::Bold, "https://example.com/bold"),
            ]
        ),
        other => panic!("expected a paragraph block, got {other:?}"),
    }
}

#[test]
fn flow_strike_span_lowers_to_strike_run() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    graph.push_block(Block::Paragraph { content: vec![text("keep "), styled("strike", vec![text("gone")]), text(" later")] });

    let presentation = build_flow_presentation(&graph);

    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::Paragraph {
            runs: vec![run("keep ", FontStyle::Normal), run("gone", FontStyle::Strike), run(" later", FontStyle::Normal)],
        }]
    );
}
