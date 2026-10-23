#[tokio::test]
async fn test_basic_render() {
    // This test will fail if no font file is found at the specified path,
    // but it serves as an integration test for the API structure.
    let tex = "x^2 + y^2 = z^2";
    let font_path = "assets/fonts";
    
    let result = rutex::render(tex, font_path).await;
    
    match result {
        Ok(svg) => {
            println!("Successfully rendered SVG: {} bytes", svg.len());
            std::fs::write("test_output.svg", svg).unwrap();
        }
        Err(e) => {
            panic!("Render failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_complex_render() {
    let tex = r"\frac{\alpha + \beta}{\gamma} = \sum_{i=0}^n x_i^2";
    let font_path = "assets/fonts";
    
    let result = rutex::render(tex, font_path).await;
    
    match result {
        Ok(svg) => {
            println!("Successfully rendered complex SVG: {} bytes", svg.len());
            std::fs::write("test_complex_output.svg", svg).unwrap();
        }
        Err(e) => {
            panic!("Complex render failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_matrix_render() {
    let tex = r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}";
    let font_path = "assets/fonts";
    
    let result = rutex::render(tex, font_path).await;
    
    match result {
        Ok(svg) => {
            println!("Successfully rendered matrix SVG: {} bytes", svg.len());
            std::fs::write("test_matrix_output.svg", svg).unwrap();
        }
        Err(e) => {
            panic!("Matrix render failed: {:?}", e);
        }
    }
}
