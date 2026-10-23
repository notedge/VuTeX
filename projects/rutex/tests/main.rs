use rutex::{parser, layout, font, renderer_svg};

#[test]
fn test_modules_accessible() {
    // This just verifies that the re-exports are working
    let _ = parser::RuTeXError::internal_error("test");
}
