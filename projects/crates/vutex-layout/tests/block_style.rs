//! Inline emphasis reaches headings, lists, quotes, and table cells.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem, TableCell, TableRow};
use vutex_layout::{
    LayoutHint,
    document::layout_with_hint,
    hint::Margin,
    layout,
    plan::{LayoutItem, LayoutPage, Rule, TextRun},
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

fn graph_with(id: u64, block: Block) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(id));
    graph.push_block(block);
    graph
}

fn cell(content: Vec<Inline>) -> TableCell {
    TableCell { content, colspan: 1, rowspan: 1 }
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

fn run_with<'a>(runs: &[&'a TextRun], value: &str) -> &'a TextRun {
    runs.iter().copied().find(|run| run.text == value).unwrap_or_else(|| panic!("missing run {value:?}: {runs:#?}"))
}

#[test]
fn bold_and_code_runs_survive_inside_a_heading() {
    let graph = graph_with(
        41,
        Block::Section {
            level: 1,
            title: vec![text("One "), styled("bold", vec![text("thick")]), text(" and "), code("x = 1")],
            children: Vec::new(),
        },
    );
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let bold = run_with(&runs, "thick");
    assert_eq!(bold.style, FontStyle::Bold);
    assert_eq!(bold.font_family, "serif");
    let plain = run_with(&runs, "One ");
    assert_eq!(plain.style, FontStyle::Normal);
    assert_eq!(bold.size, plain.size, "emphasis keeps the heading size");
    assert!(bold.size > LayoutHint::a4().base_font_size, "heading text scales above body size");

    let inline_code = run_with(&runs, "x = 1");
    assert_eq!(inline_code.font_family, "monospace");
    assert_eq!(inline_code.style, FontStyle::Normal);
    assert_eq!(inline_code.size, plain.size);
}

#[test]
fn italic_runs_survive_inside_a_list_item() {
    let graph = graph_with(
        42,
        Block::List {
            ordered: false,
            items: vec![ListItem {
                content: vec![text("note "), styled("italic", vec![text("slanted")])],
                children: Vec::new(),
                number: None,
                is_task: false,
                is_checked: None,
            }],
        },
    );
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let italic = run_with(&runs, "slanted");
    assert_eq!(italic.style, FontStyle::Italic);
    assert_eq!(italic.font_family, "serif");
    assert_eq!(italic.size, LayoutHint::a4().base_font_size);
}

#[test]
fn inline_code_runs_survive_inside_a_quote() {
    let graph = graph_with(43, Block::Quote { content: vec![text("run "), code("npm test"), text(" now")] });
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let inline_code = run_with(&runs, "npm test");
    assert_eq!(inline_code.font_family, "monospace");
    assert_eq!(inline_code.style, FontStyle::Normal);
    assert!(runs.iter().filter(|run| run.text != "npm test").all(|run| run.font_family == "serif"));
    assert!(inline_code.x > LayoutHint::a4().margin.left, "quote content keeps its indent");
}

#[test]
fn bold_runs_survive_inside_a_table_cell_without_moving_columns() {
    let hint = LayoutHint::a4();
    let graph = graph_with(
        44,
        Block::Table {
            caption: None,
            rows: vec![
                TableRow { is_header: true, cells: vec![cell(vec![text("Header")]), cell(vec![text("Value")])] },
                TableRow {
                    is_header: false,
                    cells: vec![cell(vec![styled("bold", vec![text("strong cell")])]), cell(vec![text("plain")])],
                },
            ],
        },
    );
    let plan = layout_with_hint(&graph, &hint);
    let runs = text_runs(&plan.pages[0]);

    let bold = run_with(&runs, "strong cell");
    assert_eq!(bold.style, FontStyle::Bold);
    assert_eq!(bold.font_family, "serif");

    let plain = run_with(&runs, "plain");
    assert_eq!(plain.style, FontStyle::Normal);
    assert!((plain.y - bold.y).abs() < 1e-9, "both cells of one row share their first baseline");

    // Styled cell runs keep the shared column grid: the second column starts
    // exactly one column width after the first.
    let column_width = hint.content_width() / 2.0;
    assert!((plain.x - bold.x - column_width).abs() < 1e-9, "columns: {} vs {}", bold.x, plain.x);
}

#[test]
fn unstyled_block_content_stays_normal_serif() {
    let mut graph = DocumentGraph::new(DocumentId(45));
    graph.push_block(Block::Section { level: 2, title: vec![text("Plain heading")], children: Vec::new() });
    graph.push_block(Block::List {
        ordered: true,
        items: vec![ListItem {
            content: vec![text("plain item")],
            children: Vec::new(),
            number: Some(1),
            is_task: false,
            is_checked: None,
        }],
    });
    graph.push_block(Block::Quote { content: vec![text("plain quote")] });
    graph.push_block(Block::Table {
        caption: None,
        rows: vec![TableRow { is_header: false, cells: vec![cell(vec![text("plain cell")])] }],
    });

    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);
    for value in ["Plain heading", "1.", "plain item", "plain quote", "plain cell"] {
        let run = run_with(&runs, value);
        assert_eq!(run.style, FontStyle::Normal, "run {value:?}");
        assert_eq!(run.font_family, "serif", "run {value:?}");
    }
    assert!(runs.iter().all(|run| run.style == FontStyle::Normal && run.font_family == "serif"), "{runs:#?}");
}

#[test]
fn list_marker_shares_the_first_content_baseline() {
    let graph = graph_with(
        46,
        Block::List {
            ordered: false,
            items: vec![ListItem {
                content: vec![text("Alpha "), styled("bold", vec![text("bold")])],
                children: Vec::new(),
                number: None,
                is_task: false,
                is_checked: None,
            }],
        },
    );
    let plan = layout(&graph);
    let runs = text_runs(&plan.pages[0]);

    let marker = run_with(&runs, "•");
    let first = run_with(&runs, "Alpha ");
    let bold = run_with(&runs, "bold");
    assert!((marker.y - first.y).abs() < 1e-9, "marker {} and content {} must share a baseline", marker.y, first.y);
    assert!((bold.y - first.y).abs() < 1e-9, "styled runs stay on the first content line");
    assert!(first.x > marker.x, "content starts past the marker");
}

#[test]
fn wrapped_list_content_keeps_the_content_indent() {
    let hint = LayoutHint::a4().with_page_size(320.0, 600.0).with_margin(Margin::uniform(40.0));
    let body = "word ".repeat(40);
    let graph = graph_with(
        47,
        Block::List {
            ordered: false,
            items: vec![ListItem {
                content: vec![text(body.trim())],
                children: Vec::new(),
                number: None,
                is_task: false,
                is_checked: None,
            }],
        },
    );
    let plan = layout_with_hint(&graph, &hint);
    let runs = text_runs(&plan.pages[0]);

    let content: Vec<&TextRun> = runs.iter().copied().filter(|run| run.text != "•").collect();
    assert!(content.len() > 1, "the long item must wrap: {content:#?}");
    let indent = content[0].x;
    assert!(content.iter().all(|run| (run.x - indent).abs() < 1e-9), "continuation lines keep the indent: {content:#?}");
    let marker = run_with(&runs, "•");
    assert!(marker.x < indent);
}

#[test]
fn table_row_height_follows_the_tallest_wrapped_cell() {
    let hint = LayoutHint::a4().with_page_size(320.0, 600.0).with_margin(Margin::uniform(40.0));
    let long = "word ".repeat(30);
    let graph = graph_with(
        48,
        Block::Table {
            caption: None,
            rows: vec![
                TableRow { is_header: false, cells: vec![cell(vec![text(long.trim())]), cell(vec![text("short")])] },
                TableRow { is_header: false, cells: vec![cell(vec![text("next")]), cell(vec![text("value")])] },
            ],
        },
    );
    let plan = layout_with_hint(&graph, &hint);
    let runs = text_runs(&plan.pages[0]);

    let next = run_with(&runs, "next");
    let first_row: Vec<&TextRun> = runs.iter().copied().filter(|run| run.y < next.y).collect();
    let first_y = first_row.iter().map(|run| run.y).fold(f64::MAX, f64::min);
    let last_y = first_row.iter().map(|run| run.y).fold(f64::MIN, f64::max);

    let short = run_with(&runs, "short");
    assert_eq!(short.y, first_y, "a short cell starts on the first row line");
    assert!(last_y > first_y, "the wrapped cell grows the row: {first_row:#?}");
    assert!(next.y > last_y, "the next row starts below the wrapped lines");
}

#[test]
fn tall_inline_formula_grows_the_quote_rule() {
    let hint = LayoutHint::a4();
    let graph = graph_with(
        49,
        Block::Quote {
            content: vec![
                text("before "),
                Inline::InlineMath {
                    content: "\\frac{\\frac{a}{b}}{\\frac{c}{d}}".to_string(),
                    language: Some("latex".to_string()),
                },
                text(" after"),
            ],
        },
    );
    let plan = layout_with_hint(&graph, &hint);
    let rule = plan.pages[0]
        .items
        .iter()
        .find_map(|item| match item {
            LayoutItem::Rule(Rule { y1, y2, .. }) => Some((*y1, *y2)),
            _ => None,
        })
        .expect("quote rule");
    let baseline = run_with(&text_runs(&plan.pages[0]), "before ").y;
    let line_box = hint.base_font_size * hint.line_height;

    assert!(
        baseline - rule.0 > 0.8 * hint.base_font_size,
        "the raised formula pushes the text baseline down: {}",
        baseline - rule.0
    );
    assert!(rule.1 - rule.0 > line_box, "the rule covers the raised line: {} vs {line_box}", rule.1 - rule.0);
}
