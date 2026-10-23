//! Display lowering tests for styled text runs.

use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{
    layout,
    plan::{LayoutItem, TextRun},
};
use vutex_presentation::{DisplayItem, build_display_presentation, display::DisplayTextRun};
use vutex_types::math::FontStyle;

fn text(value: &str) -> Inline {
    Inline::Text { text: value.to_string() }
}

fn styled(style: &str, children: Vec<Inline>) -> Inline {
    Inline::Styled { style: style.to_string(), children }
}

fn sample_graph() -> DocumentGraph {
    let mut graph = DocumentGraph::new(DocumentId(31));
    graph.push_block(Block::Paragraph {
        content: vec![
            text("Lead "),
            styled("bold", vec![text("heavy")]),
            text(" and "),
            styled("italic", vec![text("slanted")]),
            text(" and "),
            Inline::InlineCode { text: "code".to_string() },
        ],
    });
    graph
}

fn layout_runs(graph: &DocumentGraph) -> Vec<TextRun> {
    layout(graph)
        .pages
        .into_iter()
        .flat_map(|page| page.items)
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

fn display_runs(graph: &DocumentGraph) -> Vec<DisplayTextRun> {
    build_display_presentation(&layout(graph))
        .pages
        .into_iter()
        .flat_map(|page| page.items)
        .filter_map(|item| match item {
            DisplayItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect()
}

#[test]
fn lowering_preserves_style_for_text_runs() {
    let graph = sample_graph();
    let layout = layout_runs(&graph);
    let display = display_runs(&graph);

    assert_eq!(display.len(), layout.len(), "lowering keeps every text run");
    for (lowered, source) in display.iter().zip(&layout) {
        assert_eq!(lowered.x, source.x);
        assert_eq!(lowered.y, source.y);
        assert_eq!(lowered.size, source.size);
        assert_eq!(lowered.font_family, source.font_family);
        assert_eq!(lowered.text, source.text);
        assert_eq!(lowered.style, source.style, "style must survive lowering for `{}`", source.text);
    }
}

#[test]
fn lowering_keeps_bold_italic_and_code_styles() {
    let display = display_runs(&sample_graph());

    let bold = display.iter().find(|run| run.text == "heavy").expect("bold run");
    assert_eq!(bold.style, FontStyle::Bold);
    let italic = display.iter().find(|run| run.text == "slanted").expect("italic run");
    assert_eq!(italic.style, FontStyle::Italic);
    let code = display.iter().find(|run| run.text == "code").expect("code run");
    assert_eq!(code.style, FontStyle::Normal);
    assert_eq!(code.font_family, "monospace");

    assert!(display.iter().any(|run| run.text.starts_with("Lead") && run.style == FontStyle::Normal));
}
