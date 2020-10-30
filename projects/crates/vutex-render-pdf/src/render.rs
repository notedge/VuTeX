use vutex_presentation::display::DisplayPresentation;
use vutex_render::RenderBackend;
use vutex_types::{Diagnostic, Severity};

/// PDF renderer stub. Consumes only display presentation IR.
pub fn render_pdf(presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
    let backend = PdfBackend;
    backend.render_display(presentation)
}

struct PdfBackend;

impl RenderBackend for PdfBackend {
    fn render_display(&self, presentation: &DisplayPresentation) -> Result<Vec<u8>, Diagnostic> {
        let _ = presentation;
        Err(Diagnostic {
            code: "vutex.render.pdf.unimplemented".into(),
            severity: Severity::Error,
            message: "PDF backend is not implemented yet".into(),
        })
    }
}
