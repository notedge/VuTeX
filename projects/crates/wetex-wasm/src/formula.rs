use vutex_layout::math::{FormulaLayout, layout_formula};
use vutex_render_svg::math::{MathmlBackend, SvgBackend};
use vutex_types::{Diagnostic, Severity, math::MathError};

use crate::types::{DisplayMode, FormulaDiagnostic, FormulaMetrics, FormulaRender, RenderOptions};

const UNSUPPORTED_COMMANDS: &[&str] = &["usepackage", "input", "include", "write", "openout"];

/// Measure a formula without emitting a full render payload.
pub fn measure(source: &str, options: &RenderOptions) -> FormulaRender {
    let diagnostics = collect_diagnostics(source);
    if has_error(&diagnostics) {
        return FormulaRender { metrics: FormulaMetrics::default(), svg: String::new(), mathml: None, diagnostics };
    }

    match layout_formula(source, options.display_mode == DisplayMode::Block) {
        Ok((_, layout)) => FormulaRender { metrics: layout_to_metrics(&layout), svg: String::new(), mathml: None, diagnostics },
        Err(err) => FormulaRender {
            metrics: FormulaMetrics::default(),
            svg: String::new(),
            mathml: None,
            diagnostics: vec![math_error_to_diagnostic(&err)],
        },
    }
}

/// Render a formula to SVG.
pub fn render_svg(source: &str, options: &RenderOptions) -> FormulaRender {
    let diagnostics = collect_diagnostics(source);
    if has_error(&diagnostics) {
        return FormulaRender { metrics: FormulaMetrics::default(), svg: String::new(), mathml: None, diagnostics };
    }

    match render_formula_svg_inner(
        source,
        options.display_mode == DisplayMode::Block,
        options.outline.unwrap_or(false),
    ) {
        Ok((svg, layout)) => FormulaRender { metrics: layout_to_metrics(&layout), svg, mathml: None, diagnostics },
        Err(err) => FormulaRender {
            metrics: FormulaMetrics::default(),
            svg: String::new(),
            mathml: None,
            diagnostics: vec![math_error_to_diagnostic(&err)],
        },
    }
}

/// Render a formula to Presentation MathML.
pub fn render_mathml(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut result = measure(source, options);
    if has_error(&result.diagnostics) {
        return result;
    }

    match render_formula_mathml_inner(source, options.display_mode == DisplayMode::Block) {
        Ok((mathml, _)) => {
            result.mathml = Some(mathml);
            result
        }
        Err(err) => {
            result.diagnostics.push(math_error_to_diagnostic(&err));
            result
        }
    }
}

/// Full render with SVG and MathML.
pub fn render(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut result = render_svg(source, options);
    if !has_error(&result.diagnostics) {
        match render_formula_mathml_inner(source, options.display_mode == DisplayMode::Block) {
            Ok((mathml, _)) => result.mathml = Some(mathml),
            Err(err) => result.diagnostics.push(math_error_to_diagnostic(&err)),
        }
    }
    result
}

fn render_formula_svg_inner(source: &str, display: bool, outline: bool) -> Result<(String, FormulaLayout), MathError> {
    use vutex_layout::math::render_layout_node;
    let (root, layout) = layout_formula(source, display)?;
    let svg_height = layout.height + layout.depth;
    let mut backend = if outline {
        SvgBackend::new_deterministic(layout.width, svg_height)
    } else {
        SvgBackend::new(layout.width, svg_height)
    };
    render_layout_node(&mut backend, &root, 0.0, layout.height)?;
    Ok((backend.finish(), layout))
}

fn render_formula_mathml_inner(source: &str, display: bool) -> Result<(String, FormulaLayout), MathError> {
    use vutex_layout::math::render_layout_node;
    let (root, layout) = layout_formula(source, display)?;
    let mut backend = MathmlBackend::new();
    render_layout_node(&mut backend, &root, 0.0, layout.height)?;
    Ok((backend.finish(), layout))
}

fn layout_to_metrics(layout: &FormulaLayout) -> FormulaMetrics {
    FormulaMetrics { width: layout.width, height: layout.height, depth: layout.depth, baseline: layout.baseline }
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

fn math_error_to_diagnostic(err: &MathError) -> FormulaDiagnostic {
    FormulaDiagnostic {
        code: "WETEX_MATH_ERROR".into(),
        severity: severity_name(Severity::Error),
        message: err.to_string(),
        start: None,
        end: None,
    }
}

fn has_error(diagnostics: &[FormulaDiagnostic]) -> bool {
    diagnostics.iter().any(|item| item.severity == "error")
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
