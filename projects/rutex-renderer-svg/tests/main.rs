use rutex_renderer_svg::{SvgBackend, LayoutBackend, render_layout_node, Result};
use std::sync::Arc;
use async_trait::async_trait;
use rutex_types::{Fixed, SemanticNode, GlyphKey, SymbolRole, FontStyle, MathStyle};
use rutex_layout::{LayoutEngine, LayoutNode, Path};
use rutex_font::{FontMetricsSystem, FontLoader};

struct MockLoader;
#[async_trait]
impl FontLoader for MockLoader {
    async fn load_font_data(&self, _family: &str) -> Result<Arc<Vec<u8>>> {
        Ok(Arc::new(vec![]))
    }
}

#[tokio::test]
async fn test_full_pipeline() -> Result<()> {
    let loader = Arc::new(MockLoader);
    let font_system = FontMetricsSystem::new(loader);
    
    // Setup some mock metrics
    let key_x = GlyphKey { char: 'x', font_family: None, style: FontStyle::Normal };
    font_system.insert_metrics(key_x.clone(), rutex_font::GlyphMetrics {
        width: Fixed::from_f64(10.0),
        height: Fixed::from_f64(8.0),
        depth: Fixed::from_f64(2.0),
        italic_correction: Fixed::ZERO,
    });

    let engine = LayoutEngine::new(font_system);
    
    // Create a simple semantic node: x
    let node = SemanticNode::Symbol {
        glyph_key: key_x,
        role: SymbolRole::Ordinary,
    };

    // 1. Layout
    let layout = engine.layout_node(&node, MathStyle::Text).await?;
    
    // 2. Render to SVG
    let mut backend = SvgBackend::new(100.0, 100.0);
    render_layout_node(&mut backend, &layout, 10.0, 50.0)?;
    
    let svg = backend.finish();
    
    // 3. Verify
    assert!(svg.contains(r#"width="100""#));
    assert!(svg.contains(r#"height="100""#));
    assert!(svg.contains("x"));
    assert!(svg.contains(r#"fill="currentColor""#));
    
    Ok(())
}

#[test]
fn test_svg_basic() -> Result<()> {
    let mut backend = SvgBackend::new(100.0, 50.0);
    backend.render_rect(0.0, 0.0, 100.0, 50.0)?;
    backend.render_text("Hello", 10.0, 20.0, 12.0, Some("Arial"))?;
    
    let svg = backend.finish();
    assert!(svg.contains(r#"width="100" height="50""#));
    assert!(svg.contains(r#"<rect x="0" y="0" width="100" height="50""#));
    assert!(svg.contains(r#"<text x="10" y="20" font-size="12" font-family="Arial""#));
    assert!(svg.contains("Hello"));
    Ok(())
}

#[test]
fn test_groups() -> Result<()> {
    let mut backend = SvgBackend::new(100.0, 100.0);
    backend.start_group(Some("translate(10, 10)"))?;
    backend.render_rect(0.0, 0.0, 10.0, 10.0)?;
    backend.end_group()?;
    
    let svg = backend.finish();
    assert!(svg.contains(r#"<g transform="translate(10, 10)">"#));
    assert!(svg.contains("</g>"));
    Ok(())
}

#[test]
fn test_unclosed_group() -> Result<()> {
    let mut backend = SvgBackend::new(100.0, 100.0);
    backend.start_group(None)?;
    backend.render_rect(0.0, 0.0, 10.0, 10.0)?;
    // Not calling end_group
    
    let svg = backend.finish();
    assert!(svg.contains("<g>"));
    assert!(svg.contains("</g>")); // finish() should close it
    Ok(())
}

#[test]
fn test_svg_path_rendering() -> Result<()> {
    let mut backend = SvgBackend::new(100.0, 100.0);
    let path_node = LayoutNode::Path(Path {
        d: "M 0 0 L 10 10".to_string(),
        width: Fixed::from_f64(10.0),
        height: Fixed::from_f64(10.0),
        depth: Fixed::from_f64(0.0),
    });

    // Use the centralized render_layout_node
    render_layout_node(&mut backend, &path_node, 5.0, 5.0)?;

    let output = backend.finish();
    assert!(output.contains(r#"<path d="M 0 0 L 10 10" transform="translate(5, 5)"#));
    Ok(())
}
