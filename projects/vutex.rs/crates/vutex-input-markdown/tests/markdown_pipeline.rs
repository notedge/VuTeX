use notedown_ir::Block;
use vutex_input_markdown::{codes, import_source, render_svg};
use vutex_layout::Diagrams;
use vutex_types::Severity;

const SOURCE: &str = "\
# Markdown to SVG

Inline math $a^2 + b^2$ stays in the paragraph.

$$
E = mc^2
$$

```mermaid
graph TD
  A[Start] --> B[End]
```
";

fn diagram_scenes(imported: &vutex_input_markdown::MarkdownDocument) -> Diagrams {
    let diagram = imported
        .graph
        .blocks
        .iter()
        .find(|node| matches!(&node.block, Block::Diagram { language, .. } if language == "mermaid"))
        .expect("the mermaid fence must lower to a diagram block");
    let Block::Diagram { source, .. } = &diagram.block
    else {
        unreachable!("the node was selected as a diagram block");
    };
    let lowered = davinci_diagram::mermaid_to_scene(source).expect("mermaid fence must compile");
    assert!(lowered.diagnostics.is_empty(), "{:?}", lowered.diagnostics);

    let mut diagrams = Diagrams::new();
    diagrams.insert(diagram.id, lowered.scene, "Mermaid diagram");
    diagrams
}

#[test]
fn markdown_reaches_the_rendered_svg() {
    let imported = import_source("virtual://markdown-to-svg.md", SOURCE).expect("fixture must import");
    assert!(imported.report.unsupported.is_empty(), "{:?}", imported.report.unsupported);
    assert!(imported.report.lossy.is_empty(), "{:?}", imported.report.lossy);
    assert!(imported.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", imported.diagnostics);

    let diagrams = diagram_scenes(&imported);
    let rendered = render_svg("virtual://markdown-to-svg.md", SOURCE, &diagrams).expect("render must succeed");

    assert!(rendered.pages >= 1, "{}", rendered.pages);
    assert!(rendered.items >= 1, "{}", rendered.items);
    assert!(rendered.diagnostics.iter().all(|diagnostic| diagnostic.severity != Severity::Error), "{:?}", rendered.diagnostics);
    assert!(rendered.svg.starts_with("<svg"), "{}", &rendered.svg[..rendered.svg.len().min(80)]);
    assert!(rendered.svg.contains("Markdown to SVG"), "heading text must reach the svg");
    assert!(rendered.svg.contains("data-diagram=\"Mermaid diagram\""), "diagram must reach the svg");
    assert!(rendered.svg.contains("<path"), "math glyph outlines must reach the svg");
}

#[test]
fn lossy_markdown_is_reported_with_a_structured_code() {
    let source = "Term\n: definition\n";
    let imported = import_source("virtual://definition-list.md", source).expect("lossy constructs are not fatal");

    let item = imported
        .report
        .lossy
        .iter()
        .find(|item| item.code == "import.markdown.definition_list")
        .expect("definition list downgrade must be reported");
    assert!(!item.detail.is_empty());

    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "import.markdown.definition_list")
        .expect("definition list downgrade must produce a diagnostic");
    assert_eq!(diagnostic.severity, Severity::Warning);
}

#[test]
fn opaque_blocks_are_reported_instead_of_dropped() {
    let source = "Above\n\n---\n\nBelow\n";
    let imported = import_source("virtual://thematic-break.md", source).expect("thematic breaks are not fatal");

    assert!(imported.report.opaque.iter().any(|item| item.detail.contains("thematic_break")), "{:?}", imported.report.opaque);
    let diagnostic = imported
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == codes::OPAQUE_BLOCK)
        .expect("opaque block must produce a diagnostic");
    assert_eq!(diagnostic.severity, Severity::Info);
}
