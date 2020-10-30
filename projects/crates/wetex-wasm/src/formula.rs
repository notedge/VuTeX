use vutex_types::{Diagnostic, Severity};

use crate::types::{DisplayMode, FormulaDiagnostic, FormulaMetrics, FormulaRender, RenderOptions};

const UNSUPPORTED_COMMANDS: &[&str] = &["usepackage", "input", "include", "write", "openout"];

/// Measure a formula without emitting a full render payload.
pub fn measure(source: &str, options: &RenderOptions) -> FormulaRender {
    let diagnostics = collect_diagnostics(source);
    let metrics = if has_error(&diagnostics) {
        FormulaMetrics { width: 0.0, height: 0.0, depth: 0.0, baseline: 0.0 }
    }
    else {
        estimate_metrics(source, options.display_mode)
    };

    FormulaRender { metrics, svg: String::new(), mathml: None, diagnostics }
}

/// Render a formula to SVG (and optional MathML in full `render`).
pub fn render_svg(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut result = measure(source, options);
    if !has_error(&result.diagnostics) {
        result.svg = build_placeholder_svg(source, &result.metrics, options.display_mode);
    }
    result
}

/// Render a formula to Presentation MathML (scaffold placeholder).
pub fn render_mathml(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut result = measure(source, options);
    if !has_error(&result.diagnostics) {
        result.mathml = Some(build_placeholder_mathml(source));
    }
    result
}

/// Full render with SVG and MathML.
pub fn render(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut result = render_svg(source, options);
    if !has_error(&result.diagnostics) {
        result.mathml = Some(build_placeholder_mathml(source));
    }
    result
}

fn collect_diagnostics(source: &str) -> Vec<FormulaDiagnostic> {
    let mut diagnostics = Vec::new();

    if source.trim().is_empty() {
        diagnostics.push(FormulaDiagnostic {
            code: "WETEX_EMPTY_SOURCE".into(),
            severity: severity_name(Severity::Error),
            message: "formula source must not be empty".into(),
            start: Some(0),
            end: Some(0),
        });
        return diagnostics;
    }

    for command in UNSUPPORTED_COMMANDS {
        let needle = format!("\\{}", command);
        if let Some(index) = source.find(&needle) {
            diagnostics.push(FormulaDiagnostic {
                code: "WETEX_UNSUPPORTED_COMMAND".into(),
                severity: severity_name(Severity::Error),
                message: format!("unsupported TeX command `\\{}`", command),
                start: Some(index as u32),
                end: Some((index + needle.len()) as u32),
            });
        }
    }

    diagnostics
}

fn has_error(diagnostics: &[FormulaDiagnostic]) -> bool {
    diagnostics.iter().any(|item| item.severity == "error")
}

fn estimate_metrics(source: &str, display_mode: DisplayMode) -> FormulaMetrics {
    let line_count = source.lines().count().max(1) as f64;
    let char_count = source.chars().count().max(1) as f64;
    let width = char_count * 7.2 + 12.0;
    let height = if display_mode == DisplayMode::Block { 20.0 * line_count + 16.0 } else { 20.0 };
    let depth = if display_mode == DisplayMode::Block { 4.0 } else { 2.0 };
    let baseline = height - depth;

    FormulaMetrics { width, height, depth, baseline }
}

fn build_placeholder_svg(source: &str, metrics: &FormulaMetrics, display_mode: DisplayMode) -> String {
    let label = xml_escape(source);
    let y = metrics.baseline;
    let font_size = if display_mode == DisplayMode::Block { 16 } else { 14 };
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" data-wetex="placeholder"><text x="6" y="{y}" font-family="serif" font-size="{font_size}">{label}</text></svg>"#,
        width = metrics.width,
        height = metrics.height,
        y = y,
        font_size = font_size,
        label = label,
    )
}

fn build_placeholder_mathml(source: &str) -> String {
    let label = xml_escape(source);
    format!(
        r#"<math xmlns="http://www.w3.org/1998/Math/MathML" data-wetex="placeholder"><mtext>{label}</mtext></math>"#,
        label = label,
    )
}

fn xml_escape(input: &str) -> String {
    input.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn severity_name(severity: Severity) -> String {
    match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
    .to_string()
}

impl From<Diagnostic> for FormulaDiagnostic {
    fn from(value: Diagnostic) -> Self {
        Self { code: value.code, severity: severity_name(value.severity), message: value.message, start: None, end: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source_is_error() {
        let result = render_svg("", &RenderOptions::default());
        assert!(result.svg.is_empty());
        assert!(has_error(&result.diagnostics));
    }

    #[test]
    fn simple_source_emits_svg() {
        let result = render_svg("x^2 + y^2", &RenderOptions::default());
        assert!(!result.svg.is_empty());
        assert!(result.svg.contains("data-wetex=\"placeholder\""));
        assert!(result.metrics.width > 0.0);
    }

    #[test]
    fn unsupported_command_reports_diagnostic() {
        let result = render_svg("\\usepackage{amsmath}", &RenderOptions::default());
        assert!(has_error(&result.diagnostics));
        assert!(result.diagnostics.iter().any(|item| item.code == "WETEX_UNSUPPORTED_COMMAND"));
    }
}
