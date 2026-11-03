//! Snapshot HTML must paint diagram scenes, not reserve empty boxes.
//!
//! The production chains are `notedown-ir` diagram block → `davinci-diagram`
//! scene lowering → `vutex-layout` placement → display presentation. Only the
//! final sink differs: this file asserts the HTML snapshot backend embeds the
//! scene's own SVG inside the placed box for both Mermaid and DOT.

use davinci_diagram::{dot_to_scene, mermaid_to_scene};
use notedown_ir::{Block, DocumentGraph, DocumentId, Inline};
use vutex_layout::{Diagrams, LayoutHint, layout_with_diagrams};
use vutex_presentation::{build_display_presentation, display::DisplayPresentation};
use vutex_render_html::render_html_snapshot;

const MERMAID: &str = "flowchart LR\n    A[TeX source] --> B[SVG output]\n";

const DOT: &str =
    "digraph pipeline {\n  rankdir=LR;\n  Fetch [shape=box]\n  Store [shape=box]\n  Fetch -> Store [label=\"persist\"]\n}\n";

/// One page carrying a lowered Mermaid diagram followed by a paragraph.
fn diagram_presentation() -> DisplayPresentation {
    let mut graph = DocumentGraph::new(DocumentId(7));
    let node = graph.push_block(Block::Diagram { language: "mermaid".into(), source: MERMAID.into() });
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "after diagram".into() }] });

    let lowered = mermaid_to_scene(MERMAID).expect("the mermaid source must lower");
    let mut diagrams = Diagrams::new();
    diagrams.insert(node, lowered.scene, "Mermaid diagram");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    build_display_presentation(&plan)
}

/// One page carrying a lowered DOT graph followed by a paragraph.
fn dot_presentation() -> DisplayPresentation {
    let mut graph = DocumentGraph::new(DocumentId(8));
    let node = graph.push_block(Block::Diagram { language: "dot".into(), source: DOT.into() });
    graph.push_block(Block::Paragraph { content: vec![Inline::Text { text: "after diagram".into() }] });

    let lowered = dot_to_scene(DOT).expect("the dot source must lower");
    let mut diagrams = Diagrams::new();
    diagrams.insert(node, lowered.scene, "DOT graph");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    build_display_presentation(&plan)
}

/// The graphic box of the snapshot.
fn graphic_box(html: &str) -> &str {
    let after = html.split("data-vutex-item=\"graphic\"").nth(1).expect("snapshot must carry a graphic item");
    let end = after.find("</div>").expect("the graphic box must close");
    &after[..end]
}

#[test]
fn snapshot_html_embeds_the_diagram_scene_as_svg() {
    for (language, alt, presentation) in
        [("mermaid", "Mermaid diagram", diagram_presentation()), ("dot", "DOT graph", dot_presentation())]
    {
        let html = render_html_snapshot(&presentation).expect("snapshot html");
        assert!(
            html.contains(&format!("data-diagram=\"{alt}\"")),
            "{language}: the graphic box must carry the diagram label: {html}"
        );
        let box_markup = graphic_box(&html);
        assert!(box_markup.contains("<svg"), "{language}: the graphic box must embed the scene svg, got: {box_markup}");
        assert!(box_markup.contains("<path"), "{language}: the embedded scene must paint vector geometry");
        assert!(
            box_markup.contains("width:100%;height:100%"),
            "{language}: the embedded scene must stretch onto the placed box: {box_markup}"
        );
    }
}

#[test]
fn snapshot_html_keeps_the_diagram_box_geometry() {
    for presentation in [diagram_presentation(), dot_presentation()] {
        let html = render_html_snapshot(&presentation).expect("snapshot html");
        let box_markup = graphic_box(&html);
        let style = box_markup.split("style=\"").nth(1).expect("graphic style").split('"').next().expect("style end");
        for key in ["left:", "top:", "width:", "height:"] {
            assert!(style.contains(key), "graphic box keeps fixed geometry `{key}`: {style}");
        }
        for part in style.split(';') {
            if let Some(value) = part.strip_prefix("width:").or_else(|| part.strip_prefix("height:")) {
                let extent: f64 = value.trim_end_matches("pt").parse().expect("dimension number");
                assert!(extent > 0.0, "graphic box must have a positive extent: {style}");
            }
        }
    }
}
