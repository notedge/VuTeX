wit_bindgen::generate!({
    world: "rutex-renderer",
    path: "wit/rutex.wit",
});

use rutex_parser as parser;
use rutex_layout as layout;
use rutex_renderer_svg::SvgBackend;
use rutex_font::{FontMetricsSystem, FontMetricsData};
use rutex_types::MathStyle;
use futures::executor::block_on;

struct Component;

impl Guest for Component {
    fn render(expr: String, _opts: rutex::katex::types::Options) -> String {
        // 1. Parse
        let tree = match parser::parse(&expr) {
            Ok(t) => t,
            Err(e) => return format!("Parse Error: {:?}", e),
        };

        // 2. Initialize Font System with empty metrics for now
        // In a real scenario, we would pass metrics from JS or embed them.
        let metrics = FontMetricsData::new("default".to_string(), 1000);
        let font_system = FontMetricsSystem::new_with_metrics(metrics);

        // 3. Layout
        let engine = layout::LayoutEngine::new(font_system).with_base_size(16.0);
        let layout_root = match block_on(engine.layout_node(&tree.root, MathStyle::Display)) {
            Ok(node) => node,
            Err(e) => return format!("Layout Error: {:?}", e),
        };

        // 4. Render to SVG
        let width = layout_root.width().to_f64();
        let height = layout_root.height().to_f64();
        let depth = layout_root.depth().to_f64();

        let mut backend = SvgBackend::new(width, height + depth);
        if let Err(e) = layout::render_layout_node(&mut backend, &layout_root, 0.0, height) {
            return format!("Render Error: {:?}", e);
        }

        backend.finish()
    }
}

export!(Component);
