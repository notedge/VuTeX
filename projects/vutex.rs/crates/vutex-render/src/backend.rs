use vutex_presentation::display::DisplayPresentation;
use vutex_types::Diagnostic;

/// Output backend contract. Implementations must not read source languages.
pub trait RenderBackend {
    /// Render a display presentation into the target format bytes.
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic>;
}
