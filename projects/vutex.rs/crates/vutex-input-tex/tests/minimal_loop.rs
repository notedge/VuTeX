//! Closed loop: a minimal TeX subset reaches document semantics, layout,
//! presentation, and an SVG artifact.

use notedown_ir::Block;
use vutex_input_tex::{ConversionStatus, import_source};
use vutex_layout::{
    Diagrams, LayoutHint,
    document::layout_with_diagnostics,
    plan::{LayoutItem, TextRun},
};
use vutex_presentation::{DisplayItem, build_display_presentation};
use vutex_render_svg::render_svg;

const SAMPLE: &str = r"\documentclass{article}
\begin{document}
\section{Introduction}
Inline math $E = mc^2$ and \textbf{bold} plus \emph{italic} text.
\begin{itemize}
\item First item
\item Second item with $x^2$
\end{itemize}
\[
\frac{a}{b}
\]
\begin{verbatim}
let x = 1
\end{verbatim}
\end{document}
";

#[test]
fn minimal_tex_document_reaches_an_svg_artifact() {
    let document = import_source("minimal.tex", SAMPLE).expect("the minimal subset must import");
    let graph = &document.graph;

    assert_eq!(document.report.count(ConversionStatus::Parsed), 1);

    let sections = graph.blocks.iter().filter(|node| matches!(node.block, Block::Section { .. })).count();
    let paragraphs = graph.blocks.iter().filter(|node| matches!(node.block, Block::Paragraph { .. })).count();
    let lists = graph.blocks.iter().filter(|node| matches!(node.block, Block::List { .. })).count();
    let math = graph.blocks.iter().filter(|node| matches!(node.block, Block::Math { .. })).count();
    let code = graph.blocks.iter().filter(|node| matches!(node.block, Block::Code { .. })).count();
    assert_eq!((sections, paragraphs, lists, math, code), (1, 1, 1, 1, 1), "graph blocks: {:#?}", graph.blocks);

    for node in &graph.blocks {
        if let Block::Math { formula } = &node.block {
            assert_eq!(formula.language.as_deref(), Some("latex"), "display math must select the LaTeX profile");
        }
    }
    let title = graph.blocks.iter().find_map(|node| match &node.block {
        Block::Section { title, .. } => Some(title),
        _ => None,
    });
    assert!(
        title.is_some_and(|title| title.iter().any(|inline| inline.plain_text().contains("Introduction"))),
        "the section title must reach document semantics"
    );

    let (plan, diagnostics) = layout_with_diagnostics(graph, &LayoutHint::a4(), &Diagrams::default());
    assert!(!plan.pages.is_empty(), "layout must emit at least one page");
    assert!(
        !diagnostics.iter().any(|d| d.code == "vutex.layout.math.unsupported_language"),
        "the profile must not hand a non-TeX formula to layout: {diagnostics:#?}"
    );
    assert!(
        document.report.count(ConversionStatus::Unsupported) == 0,
        "the minimal subset must not report unsupported constructs: {:#?}",
        document.report.entries
    );

    let text_runs: Vec<&TextRun> = plan
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run),
            _ => None,
        })
        .collect();
    assert!(
        text_runs.iter().any(|run| run.text.contains("Introduction")),
        "the heading must be laid out: {:#?}",
        text_runs.iter().map(|run| &run.text).collect::<Vec<_>>()
    );
    assert!(
        text_runs.iter().any(|run| run.text.contains("First item")),
        "list content must be laid out: {:#?}",
        text_runs.iter().map(|run| &run.text).collect::<Vec<_>>()
    );
    let rules = plan
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter(|item| matches!(item, LayoutItem::Rule(_)))
        .count();
    assert!(rules >= 1, "the display fraction must paint a rule, got {rules}");

    let display = build_display_presentation(&plan);
    assert_eq!(display.pages.len(), plan.pages.len(), "display pages must mirror layout pages");
    let painted: usize = display.pages.iter().map(|page| page.items.len()).sum();
    assert!(painted > 20, "layout must position text and formula primitives, got {painted} painted items");
    assert!(
        display.pages.iter().flat_map(|page| page.items.iter()).any(|item| matches!(item, DisplayItem::TextRun(_))),
        "the display presentation must carry text runs"
    );

    let svg = render_svg(&display).expect("the SVG backend must render the presentation");
    assert!(svg.starts_with("<svg"), "rendered artifact must be SVG: {}", svg.chars().take(40).collect::<String>());
    assert!(svg.contains("Introduction"), "the artifact must carry the heading text");
    assert!(svg.len() > 1024, "rendered artifact must carry real content, got {} bytes", svg.len());

    // The preamble has no document semantics, so it must be reported as opaque
    // instead of disappearing from the report.
    let opaque: Vec<_> = document
        .report
        .entries
        .iter()
        .filter(|entry| entry.status == ConversionStatus::Opaque)
        .collect();
    assert_eq!(opaque.len(), 1, "only the preamble stays opaque: {:#?}", document.report.entries);
    assert!(opaque[0].span.is_some(), "the preamble must be located in the source");
    // The document model states its own coverage gap once per import.
    let coverage: Vec<_> = document
        .report
        .entries
        .iter()
        .filter(|entry| entry.code == "import.tex.partial_coverage")
        .collect();
    assert_eq!(coverage.len(), 1, "the coverage statement is forwarded once");
    assert_eq!(coverage[0].status, ConversionStatus::Lossy);

    println!(
        "tex minimal loop: pages={} text_runs={} rules={} painted={} svg_bytes={} blocks={} opaque={} lossy={}",
        plan.pages.len(),
        text_runs.len(),
        rules,
        painted,
        svg.len(),
        graph.blocks.len(),
        document.report.count(ConversionStatus::Opaque),
        document.report.count(ConversionStatus::Lossy),
    );
}
