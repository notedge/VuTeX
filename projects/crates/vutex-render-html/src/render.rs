use vutex_presentation::{display::DisplayPresentation, flow::FlowPresentation};
use vutex_types::{Diagnostic, Severity};

/// Snapshot HTML maps fixed layout geometry into controlled markup.
pub fn render_html_snapshot(_presentation: &DisplayPresentation) -> Result<String, Diagnostic> {
    Err(not_implemented("snapshot"))
}

/// Reflow HTML maps semantic flow presentation into responsive markup.
pub fn render_html_reflow(_presentation: &FlowPresentation) -> Result<String, Diagnostic> {
    Err(not_implemented("reflow"))
}

fn not_implemented(mode: &str) -> Diagnostic {
    Diagnostic {
        code: format!("vutex.render.html.{mode}.unimplemented"),
        severity: Severity::Error,
        message: format!("HTML {mode} backend is not implemented yet"),
    }
}
