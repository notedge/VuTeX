//! Presentation lowering tests.

use davinci_scene::{Color, DisplayScene, NodeKind, Rect, SceneNode, ShapeStyle, Size};
use notedown_ir::{Attributes, Block, DocumentGraph, DocumentId, Inline, ListItem, RelationEndpoint, TableCell, TableRow};
use vutex_layout::{
    layout,
    plan::{Graphic, LayoutItem, LayoutPage, LayoutPlan},
};
use vutex_presentation::{
    DisplayItem, FlowBlock, FlowListItem, FlowRun, FlowTableRow, build_display_presentation, build_flow_presentation,
    display::DisplayTextRun,
};
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
    FlowRun { text: text.to_string(), style, href: None, title: None }
}

fn link_run(text: &str, style: FontStyle, href: &str) -> FlowRun {
    FlowRun { text: text.to_string(), style, href: Some(href.to_string()), title: None }
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
fn display_lowering_carries_diagram_scenes() {
    let mut scene = DisplayScene::new(Size { width: 200.0, height: 100.0 });
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect { x: 0.0, y: 0.0, width: 200.0, height: 100.0 },
        style: ShapeStyle::stroked(Color::BLACK, 1.0),
    }));
    let plan = LayoutPlan {
        pages: vec![LayoutPage {
            width: 595.0,
            height: 842.0,
            items: vec![LayoutItem::Graphic(Graphic {
                x: 72.0,
                y: 120.0,
                width: 400.0,
                height: 200.0,
                scene: scene.clone(),
                alt: "pipeline".to_string(),
            })],
        }],
    };

    let presentation = build_display_presentation(&plan);

    match &presentation.pages[0].items[0] {
        DisplayItem::Graphic(graphic) => {
            assert_eq!(graphic.alt, "pipeline");
            assert_eq!(graphic.scene, scene);
            assert_eq!((graphic.x, graphic.y, graphic.width, graphic.height), (72.0, 120.0, 400.0, 200.0));
        }
        other => panic!("expected a graphic item, got {other:?}"),
    }
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
            assert_eq!(
                items,
                &vec![
                    FlowListItem { runs: vec![run("First item", FontStyle::Normal)], number: Some(1), children: Vec::new() },
                    FlowListItem { runs: vec![run("Second item", FontStyle::Normal)], number: Some(2), children: Vec::new() },
                ]
            );
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
            items: vec![
                FlowListItem { runs: vec![run("slanted", FontStyle::Italic)], number: None, children: Vec::new() },
                FlowListItem { runs: vec![run("plain", FontStyle::Normal)], number: None, children: Vec::new() },
            ],
        }]
    );
}

#[test]
fn flow_nested_list_stays_inside_parent_item() {
    let mut graph = DocumentGraph::new(DocumentId(11));
    let child = graph.push_block(Block::List {
        ordered: false,
        items: vec![ListItem { content: vec![text("child")], children: Vec::new(), number: None }],
    });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![ListItem { content: vec![text("parent")], children: vec![child], number: None }],
    });

    let presentation = build_flow_presentation(&graph);

    // The child list belongs to the parent item, so the document keeps exactly
    // one top-level list block.
    assert_eq!(presentation.blocks.len(), 1);
    assert_eq!(
        presentation.blocks,
        vec![FlowBlock::List {
            ordered: false,
            items: vec![FlowListItem {
                runs: vec![run("parent", FontStyle::Normal)],
                number: None,
                children: vec![FlowBlock::List {
                    ordered: false,
                    items: vec![FlowListItem {
                        runs: vec![run("child", FontStyle::Normal)],
                        number: None,
                        children: Vec::new(),
                    }],
                }],
            }],
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
fn flow_list_item_numbers_follow_authored_markers() {
    let mut graph = DocumentGraph::new(DocumentId(12));
    graph.push_block(Block::List {
        ordered: true,
        items: vec![
            ListItem { content: vec![text("third")], children: Vec::new(), number: Some(3) },
            ListItem { content: vec![text("ninth")], children: Vec::new(), number: Some(9) },
        ],
    });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![ListItem { content: vec![text("bullet")], children: Vec::new(), number: None }],
    });

    let presentation = build_flow_presentation(&graph);

    match &presentation.blocks[..] {
        [FlowBlock::List { ordered: true, items }, FlowBlock::List { ordered: false, items: bullets }] => {
            assert_eq!(items.iter().map(|item| item.number).collect::<Vec<_>>(), vec![Some(3), Some(9)]);
            assert_eq!(bullets.iter().map(|item| item.number).collect::<Vec<_>>(), vec![None]);
        }
        other => panic!("expected an ordered and an unordered list, got {other:?}"),
    }
}

#[test]
fn flow_external_link_runs_carry_the_authored_title() {
    let mut graph = DocumentGraph::new(DocumentId(14));
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Link {
            target: RelationEndpoint::ExternalUri("https://example.com/guide".to_string()),
            display: vec![text("the "), styled("bold", vec![text("guide")])],
            title: Some("Read the guide".to_string()),
        }],
    });

    let presentation = build_flow_presentation(&graph);

    match &presentation.blocks[0] {
        FlowBlock::Paragraph { runs } => assert_eq!(
            runs,
            &vec![
                FlowRun {
                    text: "the ".to_string(),
                    style: FontStyle::Normal,
                    href: Some("https://example.com/guide".to_string()),
                    title: Some("Read the guide".to_string()),
                },
                FlowRun {
                    text: "guide".to_string(),
                    style: FontStyle::Bold,
                    href: Some("https://example.com/guide".to_string()),
                    title: Some("Read the guide".to_string()),
                },
            ]
        ),
        other => panic!("expected a paragraph block, got {other:?}"),
    }
}

#[test]
fn flow_runs_with_different_titles_do_not_merge() {
    let mut graph = DocumentGraph::new(DocumentId(15));
    graph.push_block(Block::Paragraph {
        content: vec![
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
                display: vec![text("first")],
                title: Some("First hint".to_string()),
            },
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
                display: vec![text("second")],
                title: Some("Second hint".to_string()),
            },
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com".to_string()),
                display: vec![text("third")],
                title: Some(String::new()),
            },
        ],
    });

    let presentation = build_flow_presentation(&graph);

    match &presentation.blocks[0] {
        FlowBlock::Paragraph { runs } => {
            assert_eq!(runs.len(), 3, "a title change must end the previous run: {runs:#?}");
            assert_eq!(runs[0].text, "first");
            assert_eq!(runs[0].title.as_deref(), Some("First hint"));
            assert_eq!(runs[1].text, "second");
            assert_eq!(runs[1].title.as_deref(), Some("Second hint"));
            assert_eq!(runs[2].text, "third");
            assert_eq!(runs[2].title, None, "an empty authored title carries no tooltip");
        }
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

#[test]
fn code_block_lowering_keeps_verbatim_lines_over_a_background() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    graph.push_block(Block::Code {
        language: Some("rust".to_string()),
        content: "fn main() {\n    let value = 42;\n}".to_string(),
    });

    let plan = layout(&graph);
    let presentation = build_display_presentation(&plan);
    let items = &presentation.pages[0].items;

    let background = items
        .iter()
        .position(|item| matches!(item, DisplayItem::Rect(rect) if rect.filled))
        .expect("code block paints a filled background");
    let runs: Vec<&DisplayTextRun> = items
        .iter()
        .filter_map(|item| match item {
            DisplayItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect();

    assert_eq!(
        runs.iter().map(|run| run.text.as_str()).collect::<Vec<_>>(),
        ["fn main() {", "    let value = 42;", "}"],
        "code lines stay verbatim and split by authored newline"
    );
    assert!(runs.iter().all(|run| run.font_family == "monospace"), "code runs use the monospace face");
    let first_run = items.iter().position(|item| matches!(item, DisplayItem::TextRun(_))).expect("code text");
    assert!(background < first_run, "background paints before its lines");
    assert!(
        runs.iter().all(|run| !run.text.contains("rust") && !run.text.contains("```")),
        "fence and language tag never become body text"
    );
}
