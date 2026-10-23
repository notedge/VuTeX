//! SVG rendering tests: page groups, geometry, and document text.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::layout;
use vutex_presentation::{
    build_display_presentation,
    display::{DisplayItem, DisplayPage, DisplayPresentation, DisplayTextRun},
};
use vutex_render_svg::render_svg;
use vutex_types::math::FontStyle;

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

fn link_presentation(href: &str) -> DisplayPresentation {
    titled_link_presentation(href, None)
}

fn titled_link_presentation(href: &str, title: Option<&str>) -> DisplayPresentation {
    DisplayPresentation {
        pages: vec![DisplayPage {
            width: 595.0,
            height: 842.0,
            items: vec![DisplayItem::TextRun(DisplayTextRun {
                x: 72.0,
                y: 100.0,
                size: 12.0,
                width: 60.0,
                font_family: "serif".to_string(),
                style: FontStyle::Normal,
                text: "example link".to_string(),
                href: Some(href.to_string()),
                title: title.map(str::to_string),
            })],
        }],
    }
}

#[test]
fn wraps_link_text_runs_in_anchors() {
    let svg = render_svg(&link_presentation("https://example.com")).expect("svg render");

    assert!(svg.contains("<a href=\"https://example.com\"><text "), "{svg}");
    assert!(svg.contains("</text></a>"), "{svg}");
    assert!(svg.contains("example link"), "{svg}");
}

#[test]
fn writes_link_titles_inside_the_anchor() {
    let svg = render_svg(&titled_link_presentation("https://example.com", Some("A & B"))).expect("svg render");

    assert!(svg.contains("<a href=\"https://example.com\"><title>A &amp; B</title><text "), "{svg}");
    assert!(svg.contains("</text></a>"), "{svg}");
}

#[test]
fn drops_unsafe_link_destinations_but_keeps_the_text() {
    let svg = render_svg(&link_presentation("javascript:alert(1)")).expect("svg render");

    assert!(!svg.contains("javascript:"), "{svg}");
    assert!(!svg.contains("<a "), "{svg}");
    assert!(svg.contains("<text "), "{svg}");
    assert!(svg.contains("example link"), "{svg}");
}

#[test]
fn renders_single_page_for_short_documents() {
    let mut graph = DocumentGraph::new(DocumentId(3));
    graph.push_block(Block::Paragraph { content: vec![text("Short note.")] });

    let svg = render_svg(&build_display_presentation(&layout(&graph))).expect("svg render");
    assert_eq!(svg.matches("<g data-page=").count(), 1);
    assert!(svg.contains("Short note."));
}
