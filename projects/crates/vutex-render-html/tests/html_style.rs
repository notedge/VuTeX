//! HTML tests for styled text runs in snapshots and reflow output.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem, TableCell, TableRow};
use vutex_layout::layout;
use vutex_presentation::{build_display_presentation, build_flow_presentation};
use vutex_render_html::{render_html_reflow, render_html_snapshot};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn cell(content: Vec<Inline>) -> TableCell {
    TableCell { content, colspan: 1, rowspan: 1 }
}

fn graph(content: Vec<Inline>) -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(51));
    graph.push_block(Block::Paragraph { content });
    graph
}

fn snapshot(content: Vec<Inline>) -> String {
    render_html_snapshot(&build_display_presentation(&layout(&graph(content)))).expect("snapshot html")
}

#[test]
fn snapshot_text_keeps_fixed_geometry_and_adds_style_declarations() {
    let html = snapshot(vec![text("Lead "), styled("bold", vec![text("heavy")]), styled("italic", vec![text("slanted")])]);

    assert!(html.contains("class=\"vutex-text\" data-vutex-item=\"text\""), "{html}");
    assert!(html.contains("style=\"left:"), "positioning must stay inline: {html}");
    assert!(html.contains(";font-weight:700"), "{html}");
    assert!(html.contains(";font-style:italic"), "{html}");
    assert_eq!(html.matches("font-weight:700").count(), 1, "only the bold run carries a weight: {html}");
    assert_eq!(html.matches("font-style:italic").count(), 1, "only the italic run carries a style: {html}");
}

#[test]
fn snapshot_unstyled_runs_keep_their_original_style_attribute() {
    let html = snapshot(vec![text("Plain body")]);

    assert!(html.contains("<div class=\"vutex-text\" data-vutex-item=\"text\" style=\"left:"), "{html}");
    assert!(html.contains(";font-family:serif\">Plain body</div>"), "no extra declarations: {html}");
    assert!(!html.contains("font-weight"), "{html}");
    assert!(!html.contains("font-style"), "{html}");
}

#[test]
fn reflow_paragraphs_render_styled_tags() {
    let mut graph = DocumentGraph::new(DocumentId(52));
    graph.push_block(Block::Paragraph {
        content: vec![
            text("Plain "),
            styled("bold", vec![text("bold")]),
            text(" and "),
            styled("italic", vec![text("italic")]),
            text(" and "),
            Inline::InlineCode { text: "mono".to_string() },
            text("."),
        ],
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(
        html.contains(
            "<p data-vutex-block=\"paragraph\">Plain <strong>bold</strong> and <em>italic</em> and <code>mono</code>.</p>"
        ),
        "{html}"
    );
    assert_eq!(html.matches("<strong>").count(), 1, "adjacent same-style runs are merged: {html}");
}

#[test]
fn reflow_merges_adjacent_runs_of_the_same_style() {
    let mut graph = DocumentGraph::new(DocumentId(53));
    graph.push_block(Block::Paragraph {
        content: vec![styled("strong", vec![text("one "), styled("bold", vec![text("two")])])],
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<p data-vutex-block=\"paragraph\"><strong>one two</strong></p>"), "{html}");
    assert_eq!(html.matches("<strong>").count(), 1, "{html}");
}

#[test]
fn reflow_escapes_text_inside_styled_runs() {
    let mut graph = DocumentGraph::new(DocumentId(54));
    graph.push_block(Block::Paragraph { content: vec![styled("bold", vec![text("A & B < C")])] });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<strong>A &amp; B &lt; C</strong>"), "{html}");
}

#[test]
fn reflow_strike_wraps_text_in_del_and_escapes_it() {
    let mut graph = DocumentGraph::new(DocumentId(60));
    graph.push_block(Block::Paragraph { content: vec![text("Say "), styled("strike", vec![text("A & B < C")]), text(" now")] });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("Say <del>A &amp; B &lt; C</del> now"), "{html}");
    assert_eq!(html.matches("<del>").count(), 1, "{html}");
}

#[test]
fn reflow_heading_emits_strong_inline() {
    let mut graph = DocumentGraph::new(DocumentId(55));
    graph.push_block(Block::Section {
        level: 2,
        title: vec![text("Plain "), styled("bold", vec![text("heavy")]), text(" title")],
        children: Vec::new(),
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<h2 data-vutex-block=\"heading\">Plain <strong>heavy</strong> title</h2>"), "{html}");
}

#[test]
fn reflow_list_items_emit_italic_inline() {
    let mut graph = DocumentGraph::new(DocumentId(56));
    graph.push_block(Block::List {
        ordered: false,
        items: vec![
            ListItem {
                content: vec![text("plain "), styled("italic", vec![text("slanted")])],
                children: Vec::new(),
                number: None, is_task: false, is_checked: None
            },
            ListItem { content: vec![text("second")], children: Vec::new(), number: None, is_task: false, is_checked: None },
        ],
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<li>plain <em>slanted</em></li>"), "{html}");
    assert!(html.contains("<li>second</li>"), "{html}");
}

#[test]
fn reflow_quote_emits_code_inline() {
    let mut graph = DocumentGraph::new(DocumentId(57));
    graph.push_block(Block::Quote {
        content: vec![text("Run "), Inline::InlineCode { text: "vutex".to_string() }, text(" first")],
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<blockquote data-vutex-block=\"quote\"><p>Run <code>vutex</code> first</p></blockquote>"), "{html}");
}

#[test]
fn reflow_table_cells_emit_strong_and_escape_text() {
    let mut graph = DocumentGraph::new(DocumentId(58));
    graph.push_block(Block::Table {
        caption: Some("Scores".to_string()),
        rows: vec![
            TableRow { is_header: true, cells: vec![cell(vec![styled("bold", vec![text("Metric")])])] },
            TableRow {
                is_header: false,
                cells: vec![cell(vec![styled("bold", vec![text("A & B")])]), cell(vec![text("plain")])],
            },
        ],
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<thead>"), "{html}");
    assert!(html.contains("<th><strong>Metric</strong></th>"), "{html}");
    assert!(html.contains("<tbody>"), "{html}");
    assert!(html.contains("<td><strong>A &amp; B</strong></td>"), "{html}");
    assert!(html.contains("<td>plain</td>"), "{html}");
    assert!(html.contains("<caption>Scores</caption>"), "{html}");
}

#[test]
fn reflow_heading_escapes_text_inside_styled_runs() {
    let mut graph = DocumentGraph::new(DocumentId(59));
    graph.push_block(Block::Section {
        level: 1,
        title: vec![styled("bold", vec![text("<tag> & text")])],
        children: Vec::new(),
    });
    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<h1 data-vutex-block=\"heading\"><strong>&lt;tag&gt; &amp; text</strong></h1>"), "{html}");
}
