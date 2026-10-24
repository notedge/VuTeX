//! Diagram scenes placed by the document layout pass.

use davinci_scene::{Color, DisplayScene, NodeKind, Point, Rect, SceneNode, ShapeStyle, Size};
use notedown_ir::{Block, DocumentGraph, DocumentId, SemanticStatus};
use vutex_layout::{
    Diagrams, LayoutHint,
    document::layout_with_diagrams,
    layout,
    plan::{Graphic, LayoutItem},
};

fn text_scene(width: f64, height: f64) -> DisplayScene {
    let mut scene = DisplayScene::new(Size { width, height });
    scene.push(SceneNode::new(NodeKind::Rect {
        rect: Rect { x: 0.0, y: 0.0, width, height },
        style: ShapeStyle::stroked(Color::BLACK, 1.0),
    }));
    scene.push(SceneNode::new(NodeKind::Text {
        position: Point { x: 8.0, y: 24.0 },
        text: "diagram".to_string(),
        style: Default::default(),
    }));
    scene
}

fn diagram_block(graph: &mut DocumentGraph) -> notedown_ir::NodeId {
    graph.push_block(Block::Opaque {
        kind: "diagram".to_string(),
        payload_hint: "mermaid".to_string(),
        status: SemanticStatus::Partial,
    })
}

fn mermaid_code_block(graph: &mut DocumentGraph, source: &str) -> notedown_ir::NodeId {
    graph.push_block(Block::Code { language: Some("mermaid".to_string()), content: source.to_string() })
}

fn graphics(plan: &vutex_layout::LayoutPlan) -> Vec<&Graphic> {
    plan.pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            LayoutItem::Graphic(graphic) => Some(graphic),
            _ => None,
        })
        .collect()
}

#[test]
fn places_registered_scene_at_its_intrinsic_aspect_ratio() {
    let mut graph = DocumentGraph::new(DocumentId(7));
    let id = diagram_block(&mut graph);
    let mut diagrams = Diagrams::new();
    diagrams.insert(id, text_scene(400.0, 300.0), "flow chart");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    let placed = &graphics(&plan)[0];

    assert_eq!(placed.alt, "flow chart");
    assert!((placed.width / placed.height - 400.0 / 300.0).abs() < 1e-9, "aspect ratio is preserved");
    assert!(placed.width <= 400.0, "scenes are never enlarged past their intrinsic size");
    assert!(placed.scene.nodes.len() == 2);
}

#[test]
fn scales_wide_scene_down_to_the_content_column() {
    let mut graph = DocumentGraph::new(DocumentId(8));
    let id = diagram_block(&mut graph);
    let mut diagrams = Diagrams::new();
    diagrams.insert(id, text_scene(1200.0, 400.0), "wide flow chart");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    let placed = &graphics(&plan)[0];
    let hint = LayoutHint::a4();
    let content_width = hint.page_width - hint.margin.left - hint.margin.right;

    assert!(placed.width <= content_width + 1e-9, "{} <= {}", placed.width, content_width);
    assert!((placed.width / placed.height - 3.0).abs() < 1e-9);
}

#[test]
fn drops_diagram_block_without_a_registered_scene() {
    let mut graph = DocumentGraph::new(DocumentId(9));
    diagram_block(&mut graph);

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &Diagrams::new());
    assert!(graphics(&plan).is_empty());
    assert!(layout(&graph).pages[0].items.is_empty());
}

#[test]
fn drops_scene_without_a_positive_canvas() {
    let mut graph = DocumentGraph::new(DocumentId(10));
    let id = diagram_block(&mut graph);
    let mut diagrams = Diagrams::new();
    diagrams.insert(id, text_scene(0.0, 0.0), "empty");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    assert!(graphics(&plan).is_empty());
}

#[test]
fn places_a_registered_scene_for_a_mermaid_code_block() {
    let mut graph = DocumentGraph::new(DocumentId(11));
    let id = mermaid_code_block(&mut graph, "flowchart TD\n  A[Start] --> B[Stop]\n");
    let mut diagrams = Diagrams::new();
    diagrams.insert(id, text_scene(400.0, 300.0), "flow chart");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &diagrams);
    let placed = &graphics(&plan)[0];

    assert_eq!(placed.alt, "flow chart");
    assert!(placed.scene.nodes.iter().any(|node| matches!(&node.kind, NodeKind::Text { text, .. } if text == "diagram")));
}

#[test]
fn keeps_mermaid_fence_text_when_no_scene_is_registered() {
    let mut graph = DocumentGraph::new(DocumentId(12));
    mermaid_code_block(&mut graph, "flowchart TD\n  A[Start] --> B[Stop]\n");

    let plan = layout_with_diagrams(&graph, &LayoutHint::a4(), &Diagrams::new());
    assert!(graphics(&plan).is_empty());
    let text: String = plan
        .pages
        .iter()
        .flat_map(|page| page.items.iter())
        .filter_map(|item| match item {
            LayoutItem::TextRun(run) => Some(run.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("flowchart TD"), "{text}");
    assert!(text.contains("A[Start] --> B[Stop]"), "{text}");
}
