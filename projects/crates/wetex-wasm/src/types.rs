use serde::{Deserialize, Serialize};

/// Inline or block formula layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// Inline formula set at text size.
    Inline,
    /// Display formula set at display size with display-style spacing.
    Block,
}

/// Host-facing render options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    /// Inline or block layout mode.
    pub display_mode: DisplayMode,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { display_mode: DisplayMode::Inline }
    }
}

/// Formula bounding metrics in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaMetrics {
    /// Advance width.
    pub width: f64,
    /// Extent above the baseline.
    pub height: f64,
    /// Extent below the baseline.
    pub depth: f64,
    /// Baseline offset.
    pub baseline: f64,
}

impl Default for FormulaMetrics {
    fn default() -> Self {
        Self { width: 0.0, height: 0.0, depth: 0.0, baseline: 0.0 }
    }
}

/// Structured diagnostic for formula parsing or layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaDiagnostic {
    /// Stable diagnostic code, e.g. `WETEX_EMPTY_SOURCE`.
    pub code: String,
    /// Severity name: `info`, `warning`, or `error`.
    pub severity: String,
    /// Human-readable message.
    pub message: String,
    /// Optional start offset in the source.
    pub start: Option<u32>,
    /// Optional end offset in the source.
    pub end: Option<u32>,
}

/// Combined render payload returned to JavaScript hosts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaRender {
    /// Formula bounding metrics.
    pub metrics: FormulaMetrics,
    /// SVG document, empty when only measuring.
    pub svg: String,
    /// Presentation MathML, present only when requested.
    pub mathml: Option<String>,
    /// Parse or layout diagnostics.
    pub diagnostics: Vec<FormulaDiagnostic>,
}
