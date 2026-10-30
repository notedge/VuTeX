use vutex_layout::math::{
    LayoutNode, collect_layout_with_diagnostics, default_base_size, layout_formula_with_result_for, render_layout_node,
};
use vutex_render_svg::math::{SvgBackend, render_mathml_result};
use vutex_text::math::MathFontFamily;
use vutex_types::{
    Diagnostic, Severity,
    math::{FormulaLayoutResult, MathError},
};

use crate::html::HtmlBackend;
use crate::types::{DisplayMode, FontFamily, FormulaDiagnostic, FormulaMetrics, FormulaRender, RenderOptions};

const UNSUPPORTED_COMMANDS: &[&str] = &["usepackage", "input", "include", "write", "openout"];

/// Measure a formula without emitting a full render payload.
pub fn measure(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut diagnostics = collect_diagnostics(source);
    if let Some(diagnostic) = font_family_diagnostic(options) {
        diagnostics.push(diagnostic);
    }
    if has_error(&diagnostics) {
        return failed(diagnostics);
    }

    match layout_formula(source, options) {
        Ok((result, root, math_diagnostics)) => {
            diagnostics.extend(math_diagnostics.into_iter().map(FormulaDiagnostic::from));
            FormulaRender {
                metrics: node_metrics(&root),
                svg: String::new(),
                html: String::new(),
                mathml: None,
                semantics: Some(result.semantics),
                diagnostics,
            }
        }
        Err(err) => failed(vec![math_error_to_diagnostic(&err)]),
    }
}

/// Render a formula to SVG.
///
/// Strict path-only output is the default: a glyph the math font ships no
/// outline for fails the render instead of being painted as a host-font
/// `<text>` run. Callers opt into that fallback with `outline: false`, which
/// also reports a `vutex.layout.math.outline_missing` warning so a host-font
/// dependency is never silent.
pub fn render_svg(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut diagnostics = collect_diagnostics(source);
    if let Some(diagnostic) = font_family_diagnostic(options) {
        diagnostics.push(diagnostic);
    }
    if has_error(&diagnostics) {
        return failed(diagnostics);
    }

    match layout_formula(source, options) {
        Ok((result, root, math_diagnostics)) => {
            diagnostics.extend(math_diagnostics.into_iter().map(FormulaDiagnostic::from));
            match render_svg_markup(&root, options.outline.unwrap_or(true)) {
                Ok((svg, outline_diagnostics)) => {
                    diagnostics.extend(outline_diagnostics.into_iter().map(FormulaDiagnostic::from));
                    FormulaRender {
                        metrics: node_metrics(&root),
                        svg,
                        html: String::new(),
                        mathml: None,
                        semantics: Some(result.semantics),
                        diagnostics,
                    }
                }
                Err(err) => {
                    diagnostics.push(math_error_to_diagnostic(&err));
                    failed(diagnostics)
                }
            }
        }
        Err(err) => failed(vec![math_error_to_diagnostic(&err)]),
    }
}

/// Render a formula to Presentation MathML.
///
/// The structure comes from [`FormulaLayoutResult::semantics`], the semantic
/// tree the layout pass itself consumed, so the source is never parsed twice.
pub fn render_mathml(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut diagnostics = collect_diagnostics(source);
    if let Some(diagnostic) = font_family_diagnostic(options) {
        diagnostics.push(diagnostic);
    }
    if has_error(&diagnostics) {
        return failed(diagnostics);
    }

    match layout_formula(source, options) {
        Ok((result, root, math_diagnostics)) => {
            diagnostics.extend(math_diagnostics.into_iter().map(FormulaDiagnostic::from));
            FormulaRender {
                metrics: node_metrics(&root),
                svg: String::new(),
                html: String::new(),
                mathml: Some(render_mathml_result(&result)),
                semantics: Some(result.semantics),
                diagnostics,
            }
        }
        Err(err) => failed(vec![math_error_to_diagnostic(&err)]),
    }
}

/// Render a formula to an HTML fragment.
///
/// The fragment is one inline-block container holding absolutely positioned
/// text runs and outline paths, laid out by the same box tree the SVG backend
/// paints, so the two agree on geometry while the HTML flows with host text
/// and stays selectable wherever the selected face reaches the glyph.
pub fn render_html(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut diagnostics = collect_diagnostics(source);
    if let Some(diagnostic) = font_family_diagnostic(options) {
        diagnostics.push(diagnostic);
    }
    if has_error(&diagnostics) {
        return failed(diagnostics);
    }

    match layout_formula(source, options) {
        Ok((result, root, math_diagnostics)) => {
            diagnostics.extend(math_diagnostics.into_iter().map(FormulaDiagnostic::from));
            let family = embedded_family(options.font_family).expect("entries reject unsupported families before layout");
            FormulaRender {
                metrics: node_metrics(&root),
                svg: String::new(),
                html: HtmlBackend::new(family).render(&root),
                mathml: None,
                semantics: Some(result.semantics),
                diagnostics,
            }
        }
        Err(err) => failed(vec![math_error_to_diagnostic(&err)]),
    }
}

/// Full render with SVG and MathML taken from one laid-out formula.
pub fn render(source: &str, options: &RenderOptions) -> FormulaRender {
    let mut diagnostics = collect_diagnostics(source);
    if let Some(diagnostic) = font_family_diagnostic(options) {
        diagnostics.push(diagnostic);
    }
    if has_error(&diagnostics) {
        return failed(diagnostics);
    }

    match layout_formula(source, options) {
        Ok((result, root, math_diagnostics)) => {
            diagnostics.extend(math_diagnostics.into_iter().map(FormulaDiagnostic::from));
            match render_svg_markup(&root, options.outline.unwrap_or(true)) {
                Ok((svg, outline_diagnostics)) => {
                    diagnostics.extend(outline_diagnostics.into_iter().map(FormulaDiagnostic::from));
                    let family =
                        embedded_family(options.font_family).expect("entries reject unsupported families before layout");
                    FormulaRender {
                        metrics: node_metrics(&root),
                        svg,
                        html: HtmlBackend::new(family).render(&root),
                        mathml: Some(render_mathml_result(&result)),
                        semantics: Some(result.semantics),
                        diagnostics,
                    }
                }
                Err(err) => {
                    diagnostics.push(math_error_to_diagnostic(&err));
                    failed(diagnostics)
                }
            }
        }
        Err(err) => failed(vec![math_error_to_diagnostic(&err)]),
    }
}

/// Parse and lay out one formula into the single immutable result plus the box
/// tree the SVG backend walks.
///
/// Every entry point funnels through this call, so the SVG geometry, the MathML
/// structure and the metrics reported to the host all describe the same formula.
fn layout_formula(
    source: &str,
    options: &RenderOptions,
) -> Result<(FormulaLayoutResult, LayoutNode, Vec<Diagnostic>), MathError> {
    let display = options.display_mode == DisplayMode::Block;
    let base = options.font_size.unwrap_or_else(|| default_base_size(display));
    let family = embedded_family(options.font_family).expect("entries reject unsupported families before layout");
    layout_formula_with_result_for(source, display, base, family)
}

/// Map the host's `fontFamily` choice onto an embedded layout family.
///
/// An omitted choice resolves to the builtin default face. `None` means the
/// requested family ships no embedded asset yet; the public entries report
/// that with `WETEX_UNSUPPORTED_FONT_FAMILY` before layout runs.
fn embedded_family(family: Option<FontFamily>) -> Option<MathFontFamily> {
    match family {
        None => MathFontFamily::builtin_default(),
        Some(FontFamily::LmMath) => Some(MathFontFamily::LmMath),
        Some(FontFamily::XitsMath) => Some(MathFontFamily::XitsMath),
        Some(FontFamily::StixTwoMath) => None,
    }
}

/// The error a host sees when it asks for a family whose asset is not embedded.
fn font_family_diagnostic(options: &RenderOptions) -> Option<FormulaDiagnostic> {
    let unsupported = match options.font_family {
        Some(FontFamily::StixTwoMath) => "stix-two-math",
        _ => return None,
    };
    Some(FormulaDiagnostic {
        code: "WETEX_UNSUPPORTED_FONT_FAMILY".into(),
        severity: severity_name(Severity::Error),
        message: format!("font family `{unsupported}` is not embedded yet"),
        start: None,
        end: None,
    })
}

/// Render the SVG markup of one laid-out formula.
///
/// Returns the markup plus the warnings raised while walking the box tree, such
/// as a glyph that has no outline and fell back to a host font.
fn render_svg_markup(root: &LayoutNode, outline: bool) -> Result<(String, Vec<Diagnostic>), MathError> {
    let width = root.width().to_f64();
    let baseline = root.height().to_f64();
    let height = baseline + root.depth().to_f64();
    let mut backend = if outline { SvgBackend::new_deterministic(width, height) } else { SvgBackend::new(width, height) };
    render_layout_node(&mut backend, root, 0.0, baseline)?;
    let mut diagnostics = Vec::new();
    collect_layout_with_diagnostics(root, 0.0, baseline, &mut diagnostics);
    Ok((backend.finish(), diagnostics))
}

/// Formula metrics derived from the laid-out box tree.
fn node_metrics(root: &LayoutNode) -> FormulaMetrics {
    let height = root.height().to_f64();
    FormulaMetrics { width: root.width().to_f64(), height, depth: root.depth().to_f64(), baseline: height }
}

/// An empty payload carrying only the diagnostics of a failed render.
fn failed(diagnostics: Vec<FormulaDiagnostic>) -> FormulaRender {
    FormulaRender {
        metrics: FormulaMetrics::default(),
        svg: String::new(),
        html: String::new(),
        mathml: None,
        semantics: None,
        diagnostics,
    }
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
