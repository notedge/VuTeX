use serde::{Deserialize, Serialize};

use vutex_compose::math::MathSemantics;
use vutex_layout::math::FontConformance;

/// Inline or block formula layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    /// Inline formula set at text size.
    Inline,
    /// Display formula set at display size with display-style spacing.
    Block,
}

/// Font family selection for math layout.
///
/// Each family requires an OpenType MATH table — watex reads
/// `MathConstants`, `MathVariants`, and `MathKern` from it to drive the
/// TeXbook layout engine. [`FontFamily::XitsMath`] and [`FontFamily::LmMath`]
/// ship as embedded assets; [`FontFamily::StixTwoMath`] reports a
/// `WATEX_UNSUPPORTED_FONT_FAMILY` diagnostic until its asset lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontFamily {
    /// XITS Math — Times-based, same lineage as KaTeX_Main. The embedded
    /// default when `fontFamily` is omitted.
    XitsMath,
    /// Latin Modern Math — Computer Modern's OpenType successor, also embedded.
    LmMath,
    /// STIX Two Math — full MATH table upstream, asset not embedded yet, so
    /// renders fail with a `WATEX_UNSUPPORTED_FONT_FAMILY` diagnostic.
    StixTwoMath,
}

/// Host-facing render options.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderOptions {
    /// Inline or block layout mode.
    pub display_mode: DisplayMode,
    /// Explicit base font size in CSS pixels. When omitted the renderer uses
    /// `default_base_size(display_mode)` — 14px for inline, 16px for display.
    #[serde(default)]
    pub font_size: Option<f64>,
    /// Math font family. Defaults to [`FontFamily::XitsMath`] when omitted —
    /// the builtin default embedded face. [`FontFamily::LmMath`] is also
    /// embedded; [`FontFamily::StixTwoMath`] returns a
    /// `WATEX_UNSUPPORTED_FONT_FAMILY` diagnostic until its font asset ships.
    #[serde(default)]
    pub font_family: Option<FontFamily>,
    /// G5 font conformance mode: `strict` fails the layout when any font
    /// fallback (missing MATH constant, glyph variant, assembly piece, italic
    /// correction or accent attachment) is consumed; the default keeps each
    /// fallback as a countable warning diagnostic.
    #[serde(default)]
    pub font_conformance: Option<ConformanceMode>,
}

/// G5 font conformance mode selected by the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConformanceMode {
    /// Fallbacks are allowed and reported as warning diagnostics.
    AllowFallbacks,
    /// Any consumed font fallback fails the layout.
    Strict,
}

impl ConformanceMode {
    /// The layout engine's mode matching this host selection.
    pub fn engine_mode(self) -> FontConformance {
        match self {
            Self::AllowFallbacks => FontConformance::AllowFallbacks,
            Self::Strict => FontConformance::Strict,
        }
    }
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { display_mode: DisplayMode::Inline, font_size: None, font_family: None, font_conformance: None }
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
    /// Stable diagnostic code, e.g. `WATEX_EMPTY_SOURCE`.
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
    /// HTML fragment, empty unless the HTML backend produced it.
    pub html: String,
    /// Presentation MathML, present only when requested.
    pub mathml: Option<String>,
    /// Structured math semantics the layout consumed, absent when the formula
    /// did not parse.
    ///
    /// This is the same IR snapshot the SVG geometry and the MathML structure
    /// of this payload come from, so hosts can inspect the authored structure
    /// without re-parsing the source.
    pub semantics: Option<MathSemantics>,
    /// Parse or layout diagnostics.
    pub diagnostics: Vec<FormulaDiagnostic>,
}
