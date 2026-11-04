use notedown_ir::{Block, DocumentGraph};
use vutex_input_markdown::{import_source, render_svg};
use vutex_layout::Diagrams;
use vutex_types::Severity;

fn render(source: &str) -> vutex_input_markdown::MarkdownRender {
    render_svg("virtual://markdown-constructs.md", source, &Diagrams::new()).expect("fixture must render")
}

fn assert_clean(rendered: &vutex_input_markdown::MarkdownRender, phrases: &[&str]) {
    assert!(rendered.report.unsupported.is_empty(), "{:?}", rendered.report.unsupported);
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", rendered.diagnostics);
    for phrase in phrases {
        assert!(rendered.svg.contains(phrase), "`{phrase}` must reach the svg");
    }
}

fn find_block(graph: &DocumentGraph, predicate: impl Fn(&Block) -> bool) -> bool {
    graph.blocks.iter().any(|node| predicate(&node.block))
}

#[test]
fn table_keeps_header_and_body_rows() {
    let source = "\
| Name | Score |
| --- | --- |
| Alpha | 1 |
| Beta | 2 |
";
    let imported = import_source("virtual://table.md", source).expect("table must import");
    let table = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .expect("a table block must be lowered");
    assert_eq!(table.len(), 3, "one header row and two body rows");
    assert!(table[0].is_header, "the first row stays the header");

    let rendered = render(source);
    assert_clean(&rendered, &["Name", "Score", "Alpha", "Beta"]);
    assert!(rendered.report.lossy.is_empty(), "{:?}", rendered.report.lossy);
}

#[test]
fn table_alignment_downgrade_is_reported() {
    let source = "\
| Left | Right |
| :--- | ---: |
| Alpha | 1 |
";
    let imported = import_source("virtual://table-alignment.md", source).expect("aligned table must import");
    assert!(
        imported.report.lossy.iter().any(|item| item.code == "import.markdown.table_alignment"),
        "{:?}",
        imported.report.lossy
    );
    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.table_alignment")
        .expect("alignment downgrade must produce a diagnostic");
    assert_eq!(diagnostic.severity, Severity::Warning);

    let rendered = render(source);
    assert_clean(&rendered, &["Left", "Right", "Alpha"]);
}

#[test]
fn quote_content_reaches_the_svg() {
    let source = "> Quoted line\n";
    let imported = import_source("virtual://quote.md", source).expect("quote must import");
    assert!(
        find_block(&imported.graph, |block| matches!(block, Block::Quote { content, .. } if !content.is_empty())),
        "quote content must be lowered"
    );

    let rendered = render(source);
    assert_clean(&rendered, &["Quoted line"]);
}

#[test]
fn nested_list_children_stay_linked() {
    let source = "- Outer\n  - Inner\n";
    let imported = import_source("virtual://nested-list.md", source).expect("nested list must import");
    let list = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::List { ordered: false, items } if items.first().is_some_and(|item| !item.children.is_empty()) => Some(items),
            _ => None,
        })
        .expect("the outer unordered list must be lowered");
    assert_eq!(list.len(), 1, "the outer list keeps one item");
    assert_eq!(list[0].children.len(), 1, "the outer item keeps the inner list");
    assert!(
        find_block(&imported.graph, |block| matches!(block, Block::List { items, .. } if items.len() == 1)),
        "the inner list must be a graph node"
    );

    let rendered = render(source);
    assert_clean(&rendered, &["Outer", "Inner"]);
}

#[test]
fn task_list_markers_survive_import() {
    let source = "- [x] Done\n- [ ] Todo\n";
    let imported = import_source("virtual://task-list.md", source).expect("task list must import");
    let items = imported
        .graph
        .blocks
        .iter()
        .find_map(|node| match &node.block {
            Block::List { items, .. } => Some(items),
            _ => None,
        })
        .expect("a task list must be lowered");
    assert_eq!(items.len(), 2, "two task items");
    assert!(items[0].is_task, "a task marker must be recorded");
    assert_eq!(items[0].is_checked, Some(true));
    assert!(items[1].is_task, "an empty task marker must be recorded");
    assert_eq!(items[1].is_checked, Some(false));

    let rendered = render(source);
    assert_clean(&rendered, &["Done", "Todo"]);
}
