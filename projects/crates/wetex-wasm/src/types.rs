use serde::{Deserialize, Serialize};

/// Inline or block formula layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Inline,
    Block,
}

/// Host-facing render options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    pub display_mode: DisplayMode,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { display_mode: DisplayMode::Inline }
    }
}

/// Formula bounding metrics in CSS pixels (scaffold contract).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaMetrics {
    pub width: f64,
    pub height: f64,
    pub depth: f64,
    pub baseline: f64,
}

/// Structured diagnostic for formula parsing or layout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaDiagnostic {
    pub code: String,
    pub severity: String,
    pub message: String,
    pub start: Option<u32>,
    pub end: Option<u32>,
}

/// Combined render payload returned to JavaScript hosts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormulaRender {
    pub metrics: FormulaMetrics,
    pub svg: String,
    pub mathml: Option<String>,
    pub diagnostics: Vec<FormulaDiagnostic>,
}
