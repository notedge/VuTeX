//! SVG rendering tests: page groups, geometry, and document text.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::layout;
use vutex_presentation::build_display_presentation;
use vutex_render_svg::render_svg;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn document() -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(1));
    graph.push_block(Block::Section { level: 1, title: vec![text("Layout pipeline")], children: Vec::new() });
    graph.push_block(Block::Paragraph { content: vec![text("Rendered from the presentation IR.")] });
    for index in 0..60 {
        let body = format!("Paragraph {index} keeps the page filling with body copy for the renderer.");
        graph.push_block(Block::Paragraph { content: vec![text(&body)] });
    }
    graph
}

#[test]
fn renders_one_group_per_page_with_document_text() {
    let plan = layout(&document());
    let presentation = build_display_presentation(&plan);
    let svg = render_svg(&presentation).expect("svg render");

    assert!(svg.starts_with("<svg"), "output must start with the svg root");
    assert!(svg.trim_end().ends_with("</svg>"), "output must close the svg root");
    assert_eq!(svg.matches("<g data-page=").count(), presentation.pages.len(), "one group per page");
    assert!(presentation.pages.len() > 1, "the sample must paginate");
    assert!(svg.contains("data-page=\"1\""));
    assert!(svg.contains("data-page=\"2\""));
    assert!(svg.contains("Layout pipeline"), "heading text missing from svg");
    assert!(svg.contains("Rendered from the presentation IR."), "paragraph text missing from svg");
    assert!(svg.contains("<text "), "text runs must render as text elements");
    assert!(svg.contains("font-family=\"serif\""));
    assert!(svg.contains("viewBox=\"0 0 "));
}

#[test]
fn escapes_markup_in_text_runs() {
    let mut graph = DocumentGraph::new(DocumentId(2));
    graph.push_block(Block::Paragraph { content: vec![text("A & B < C > D")] });

    let svg = render_svg(&build_display_presentation(&layout(&graph))).expect("svg render");
    assert!(svg.contains("A &amp; B &lt; C &gt; D"));
    assert!(!svg.contains("A & B < C"));
}

#[test]
fn renders_single_page_for_short_documents() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    graph.push_block(Block::Paragraph { content: vec![text("Short note.")] });

    let svg = render_svg(&build_display_presentation(&layout(&graph))).expect("svg render");
    assert_eq!(svg.matches("<g data-page=").count(), 1);
    assert!(svg.contains("Short note."));
}
