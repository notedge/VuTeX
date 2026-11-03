use serde::{Deserialize, Serialize};

/// Diagnostic severity for VuTeX pipeline stages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

/// Byte range into the formula source a layout element came from.
///
/// Offsets index [`crate::math::FormulaLayoutResult::source`], the authored
/// formula text the layout pass consumed. Spans let diagnostics and glyph
/// runs point back at what the author typed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    /// Inclusive start byte offset.
    pub start: u32,
    /// Exclusive end byte offset.
    pub end: u32,
}

/// A single diagnostic record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    /// Where the diagnostic points in the formula source, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<SourceSpan>,
}
