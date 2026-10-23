pub use rutex_types::{RuTeXError, Result, MathStyle};
pub use rutex_parser as parser;
pub use rutex_layout as layout;
pub use rutex_font as font;
pub use rutex_renderer_svg as renderer_svg;
// pub use rutex_renderer_mathml as renderer_mathml;

use std::sync::Arc;
use font::{FontMetricsSystem, FileFontLoader};
use layout::{LayoutEngine, render_layout_node};
use renderer_svg::SvgBackend;
// use renderer_mathml::MathmlBackend;

pub async fn render(tex: &str, font_path: &str) -> Result<String> {
    // 1. Parse
    let tree = parser::parse(tex)?;
    
    // 2. Initialize Font System
    let loader = Arc::new(FileFontLoader::new(font_path));
    let font_system = FontMetricsSystem::new(loader);
    
    // 3. Layout
    let engine = LayoutEngine::new(font_system).with_base_size(16.0);
    let layout_root = engine.layout_node(&tree.root, MathStyle::Display).await?;
    
    // 4. Render to SVG
    let width = layout_root.width().to_f64();
    let height = layout_root.height().to_f64();
    let depth = layout_root.depth().to_f64();
    
    let mut backend = SvgBackend::new(width, height + depth);
    render_layout_node(&mut backend, &layout_root, 0.0, height)?;
    
    Ok(backend.finish())
}

/// Pre-calculate all math constants and glyph metrics for AOT.
pub async fn precompute_metrics(font_path: &str, family: &str, chars: &[char]) -> Result<font::FontMetricsData> {
    let loader = Arc::new(FileFontLoader::new(font_path));
    let font_system = FontMetricsSystem::new(loader);
    font_system.export_metrics(family, chars).await
}

/// Render using pre-computed metrics (AOT mode). 
/// This does NOT require the full font file or ttf-parser at runtime.
pub async fn render_with_metrics(tex: &str, metrics: font::FontMetricsData) -> Result<String> {
    let tree = parser::parse(tex)?;
    let font_system = FontMetricsSystem::new_with_metrics(metrics);
    let engine = LayoutEngine::new(font_system).with_base_size(16.0);
    let layout_root = engine.layout_node(&tree.root, MathStyle::Display).await?;
    
    let width = layout_root.width().to_f64();
    let height = layout_root.height().to_f64();
    let depth = layout_root.depth().to_f64();
    
    let mut backend = SvgBackend::new(width, height + depth);
    render_layout_node(&mut backend, &layout_root, 0.0, height)?;
    
    Ok(backend.finish())
}

/*
pub async fn render_to_mathml(tex: &str, font_path: &str) -> Result<String> {
    // 1. Parse
    let tree = parser::parse(tex)?;
    
    // 2. Initialize Font System
    let loader = Arc::new(FileFontLoader::new(font_path));
    let font_system = FontMetricsSystem::new(loader);
    
    // 3. Layout
    let engine = LayoutEngine::new(font_system).with_base_size(16.0);
    let layout_root = engine.layout_node(&tree.root, MathStyle::Display).await?;
    
    // 4. Render to MathML
    let mut backend = MathmlBackend::new();
    let height = layout_root.height().to_f64();
    render_layout_node(&mut backend, &layout_root, 0.0, height)?;
    
    Ok(backend.finish())
}
*/
