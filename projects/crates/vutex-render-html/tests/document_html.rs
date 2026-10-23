//! HTML rendering tests: semantic reflow markup and snapshot geometry.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline, ListItem};
use vutex_layout::layout;
use vutex_presentation::{build_display_presentation, build_flow_presentation};
use vutex_render_html::{render_html_reflow, render_html_snapshot};

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
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
