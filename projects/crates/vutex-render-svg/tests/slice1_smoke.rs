use vutex_layout::{LayoutPage, LayoutPlan};
use vutex_presentation::build_display_presentation;
use vutex_render_svg::render_svg;

#[test]
fn notedown_slice1_smoke_svg_from_presentation_only() {
    let plan = LayoutPlan { pages: vec![LayoutPage { width: 595.0, height: 842.0, items: Vec::new() }] };
    let presentation = build_display_presentation(&plan);
    let svg = render_svg(&presentation).expect("svg render");
    assert!(svg.contains("<svg"));
}
