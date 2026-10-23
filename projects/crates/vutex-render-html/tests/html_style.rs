//! HTML tests for styled text runs in snapshots and reflow output.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::layout;
use vutex_presentation::{build_display_presentation, build_flow_presentation};
use vutex_render_html::{render_html_reflow, render_html_snapshot};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
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
