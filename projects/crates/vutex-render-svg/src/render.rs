use vutex_presentation::display::DisplayPresentation;
use vutex_render::RenderBackend;
use vutex_types::Diagnostic;

/// SVG renderer stub. Consumes only display presentation IR.
pub fn render_svg(presentation: &DisplayPresentation) -> Result<String, Diagnostic> {
    let backend = SvgBackend;
    backend.render_display(presentation).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

struct SvgBackend;

impl RenderBackend for SvgBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        let width = presentation.pages.first().map(|p| p.width).unwrap_or(0.0);
        let height = presentation.pages.first().map(|p| p.height).unwrap_or(0.0);
        Ok(format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"/>"#,
            width = width,
            height = height,
        )
        .into_bytes())
    }
}
