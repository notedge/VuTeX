//! HTML rendering tests: semantic reflow markup and snapshot geometry.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem, NodeId, RelationEndpoint};
use vutex_layout::layout;
use vutex_presentation::{build_display_presentation, build_flow_presentation};
use vutex_render_html::{render_html_reflow, render_html_snapshot};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn sample_graph() -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Section { level: 1, title: vec![text("Layout pipeline")], children: Vec::new() });
    graph.push_block(Block::Paragraph { content: vec![text("Rendered from the presentation IR.")] });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![
            ListItem { content: vec![text("Alpha")], children: Vec::new(), number: None },
            ListItem { content: vec![text("Beta")], children: Vec::new(), number: None },
        ],
    });
    graph.push_block(Block::Code { language: Some("rust".to_string()), content: "fn main() {}".to_string() });
    graph
}

#[test]
fn reflow_html_uses_semantic_tags_and_keeps_text() {
    let html = render_html_reflow(&build_flow_presentation(&sample_graph())).expect("reflow html");

    assert!(html.starts_with("<!doctype html>"));
    assert!(html.contains("<article class=\"vutex-flow\" data-vutex=\"flow\">"));
    assert!(html.contains("<h1 data-vutex-block=\"heading\">Layout pipeline</h1>"));
    assert!(html.contains("<p data-vutex-block=\"paragraph\">Rendered from the presentation IR.</p>"));
    assert!(html.contains("<ul data-vutex-block=\"list\">"));
    assert!(html.contains("<li>Alpha</li>"));
    assert!(html.contains("<li>Beta</li>"));
    assert!(html.contains("<pre data-vutex-block=\"code\"><code class=\"language-rust\">fn main() {}</code></pre>"));
    assert!(html.contains("</article>"));
}

#[test]
fn snapshot_html_keeps_page_geometry_and_text() {
    let plan = layout(&sample_graph());
    assert_eq!(plan.pages.len(), 1);
    let html = render_html_snapshot(&build_display_presentation(&plan)).expect("snapshot html");

    assert!(html.contains("data-vutex-page=\"1\""));
    assert!(html.contains(&format!("width:{:.3}pt;height:{:.3}pt", plan.pages[0].width, plan.pages[0].height)));
    assert!(html.contains("<div class=\"vutex-text\" data-vutex-item=\"text\""));
    assert!(html.contains("Layout pipeline"));
    assert!(html.contains("Rendered from the presentation IR."));
}

#[test]
fn reflow_html_rejects_non_element_root_tags() {
    let presentation = vutex_presentation::FlowPresentation { root_tag: "article onclick".to_string(), blocks: Vec::new() };
    assert!(render_html_reflow(&presentation).is_err());
}

#[test]
fn reflow_html_nests_child_list_inside_parent_item() {
    let mut graph = DocumentGraph::new(DocumentId(5));
    let child = graph.push_block(Block::List {
        ordered: false,
        items: vec![ListItem { content: vec![text("child")], children: Vec::new(), number: None }],
    });
    graph.push_block(Block::List {
        ordered: false,
        items: vec![ListItem { content: vec![text("parent")], children: vec![child], number: None }],
    });

    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    // Line breaks are layout, not structure, so compare the compact shape.
    let compact = html.replace('\n', "");
    assert!(
        compact.contains("<ul data-vutex-block=\"list\"><li>parent<ul data-vutex-block=\"list\"><li>child</li></ul></li></ul>"),
        "{html}"
    );
}

#[test]
fn reflow_html_wraps_link_runs_in_anchors() {
    let mut graph = DocumentGraph::new(DocumentId(2));
    graph.push_block(Block::Paragraph {
        content: vec![
            text("Read "),
            Inline::Link {
                target: RelationEndpoint::ExternalUri("https://example.com/guide?q=a&lang=zh".to_string()),
                display: vec![styled("bold", vec![text("the guide")])],
                title: None,
            },
            text("."),
        ],
    });

    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains(
        "<p data-vutex-block=\"paragraph\">Read \
         <a href=\"https://example.com/guide?q=a&amp;lang=zh\"><strong>the guide</strong></a>.</p>"
    ));
}

#[test]
fn reflow_html_omits_href_for_internal_links() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Link {
            target: RelationEndpoint::Node(NodeId(1)),
            display: vec![text("Section one")],
            title: None,
        }],
    });

    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<p data-vutex-block=\"paragraph\">Section one</p>"));
    assert!(!html.contains("href="));
}

#[test]
fn reflow_html_drops_unsafe_link_destinations() {
    let mut graph = DocumentGraph::new(DocumentId(4));
    graph.push_block(Block::Paragraph {
        content: vec![Inline::Link {
            target: RelationEndpoint::ExternalUri("javascript:alert(1)".to_string()),
            display: vec![text("click")],
            title: None,
        }],
    });

    let html = render_html_reflow(&build_flow_presentation(&graph)).expect("reflow html");

    assert!(html.contains("<p data-vutex-block=\"paragraph\">click</p>"));
    assert!(!html.contains("href="));
}
