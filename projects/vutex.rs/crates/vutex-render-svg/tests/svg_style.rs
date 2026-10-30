//! SVG tests for styled text runs.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::layout;
use vutex_presentation::build_display_presentation;
use vutex_render_svg::render_svg;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn render(content: Vec<Inline>) -> String {
    let mut graph = DocumentGraph::new(DocumentId(41));
    graph.push_block(Block::Paragraph { content });
    render_svg(&build_display_presentation(&layout(&graph))).expect("svg render")
}

#[test]
fn bold_runs_carry_a_weight_attribute() {
    let svg = render(vec![text("Lead "), styled("bold", vec![text("heavy")]), text(" tail")]);

    assert_eq!(svg.matches("font-weight=\"700\"").count(), 1, "one bold run: {svg}");
    assert!(!svg.contains("font-style=\"italic\""));
    assert!(svg.contains(">heavy</text>"));
}

#[test]
fn italic_runs_carry_a_style_attribute() {
    let svg = render(vec![text("Lead "), styled("italic", vec![text("slanted")])]);

    assert_eq!(svg.matches("font-style=\"italic\"").count(), 1, "one italic run: {svg}");
    assert!(!svg.contains("font-weight="));
}

#[test]
fn monospace_runs_keep_their_family_without_extra_attributes() {
    let svg = render(vec![Inline::InlineCode { text: "code".to_string() }]);

    assert!(svg.contains("font-family=\"monospace\""), "inline code keeps the mono family: {svg}");
    assert!(!svg.contains("font-weight="));
    assert!(!svg.contains("font-style="));
}

#[test]
fn unstyled_runs_keep_the_plain_attribute_set() {
    let svg = render(vec![text("Plain body")]);

    assert!(svg.contains("<text "), "{svg}");
    assert!(svg.contains("font-family=\"serif\""), "{svg}");
    assert!(!svg.contains("font-weight="), "no weight attribute for body text");
    assert!(!svg.contains("font-style="), "no style attribute for body text");
    assert!(svg.contains("xml:space=\"preserve\""), "existing attributes stay in place");
}
