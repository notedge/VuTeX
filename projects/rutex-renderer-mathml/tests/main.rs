use rutex_renderer_mathml::{MathmlBackend, LayoutBackend, Result};

#[test]
fn test_mathml_basic() -> Result<()> {
    let mut backend = MathmlBackend::new();
    backend.render_rect(0.0, 0.0, 100.0, 50.0)?;
    backend.render_text("123", 10.0, 20.0, 12.0, None)?;
    
    let mml = backend.finish();
    assert!(mml.contains(r#"<math"#));
    assert!(mml.contains(r#"<mspace width="100px" height="50px""#));
    assert!(mml.contains(r#"<mtext style="font-size: 12px;">123</mtext>"#));
    Ok(())
}

#[test]
fn test_mathml_groups() -> Result<()> {
    let mut backend = MathmlBackend::new();
    backend.start_group(Some("translate(10, 20)"))?;
    backend.render_text("x", 0.0, 0.0, 10.0, None)?;
    backend.end_group()?;
    
    let mml = backend.finish();
    assert!(mml.contains(r#"<mpadded voffset="20px" loffset="10px">"#));
    assert!(mml.contains(r#"<mi style="font-size: 10px;">x</mi>"#));
    Ok(())
}

#[test]
fn test_mathml_path_rendering() -> Result<()> {
    use rutex_layout::{LayoutNode, Path, Fixed};
    let mut backend = MathmlBackend::new();
    let path_node = LayoutNode::Path(Path {
        d: "M 0 0 L 10 10".to_string(),
        width: Fixed::from_f64(10.0),
        height: Fixed::from_f64(10.0),
        depth: Fixed::from_f64(0.0),
    });

    // Use the centralized render_layout_node
    rutex_layout::render_layout_node(&mut backend, &path_node, 5.0, 5.0)?;
    
    let output = backend.finish();
    assert!(output.contains(r#"<svg xmlns="http://www.w3.org/2000/svg""#));
    assert!(output.contains(r#"<path d="M 0 0 L 10 10""#));
    assert!(output.contains(r#"voffset="5px""#));
    assert!(output.contains(r#"loffset="5px""#));
    Ok(())
}
