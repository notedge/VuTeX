use rutex_renderer_image::{ImageBackend, LayoutBackend, Result};

#[test]
fn test_image_basic() -> Result<()> {
    let mut backend = ImageBackend::new(100.0, 50.0, 96.0);
    backend.render_rect(0.0, 0.0, 100.0, 50.0)?;
    
    let png = backend.finish()?;
    assert!(!png.is_empty());
    Ok(())
}
