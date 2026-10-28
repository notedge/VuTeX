use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::math_kern::MathKern;
use super::semantic::{Fixed, GlyphKey};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MathConstant {
    ScriptPercentScaleDown,
    ScriptScriptPercentScaleDown,
    DelimitedSubFormulaMinHeight,
    DisplayOperatorMinHeight,
    MathLeading,
    AxisHeight,
    AccentBaseHeight,
    FlattenedAccentBaseHeight,
    SubscriptShiftDown,
    SubscriptTopMax,
    SubscriptBaselineDropMin,
    SuperscriptShiftUp,
    SuperscriptShiftUpCramped,
    SuperscriptBottomMin,
    SuperscriptBaselineDropMax,
    SubSuperscriptGapMin,
    SuperscriptBottomMaxWithSubscript,
    SpaceAfterScript,
    UpperLimitGapMin,
    UpperLimitBaselineRiseMin,
    LowerLimitGapMin,
    LowerLimitBaselineDropMin,
    StackTopShiftUp,
    StackTopDisplayStyleShiftUp,
    StackBottomShiftDown,
    StackBottomDisplayStyleShiftDown,
    StackGapMin,
    StackDisplayStyleGapMin,
    StretchStackTopShiftUp,
    StretchStackBottomShiftDown,
    StretchStackGapAboveMin,
    StretchStackGapBelowMin,
    FractionNumeratorShiftUp,
    FractionNumeratorDisplayStyleShiftUp,
    FractionDenominatorShiftDown,
    FractionDenominatorDisplayStyleShiftDown,
    FractionNumeratorGapMin,
    FractionNumDisplayStyleGapMin,
    FractionRuleThickness,
    FractionDenominatorGapMin,
    FractionDenomDisplayStyleGapMin,
    SkewedFractionHorizontalGap,
    SkewedFractionVerticalGap,
    OverbarVerticalGap,
    OverbarRuleThickness,
    OverbarExtraAscender,
    UnderbarVerticalGap,
    UnderbarRuleThickness,
    UnderbarExtraDescender,
    RadicalVerticalGap,
    RadicalDisplayStyleVerticalGap,
    RadicalRuleThickness,
    RadicalExtraAscender,
    RadicalKernBeforeDegree,
    RadicalKernAfterDegree,
    RadicalDegreeBottomRaisePercent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GlyphMetrics {
    pub width: Fixed,
    pub height: Fixed,
    pub depth: Fixed,
    pub italic_correction: Fixed,
    /// MathTopAccentAttachment TopRight Y value in em units
    pub top_accent_attachment: Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterVariant {
    pub glyph: GlyphKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterComponent {
    pub glyph: GlyphKey,
    pub is_extender: bool,
    pub start_connector: Fixed,
    pub end_connector: Fixed,
    pub full_advance: Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterConstruction {
    pub variants: Vec<DelimiterVariant>,
    pub components: Vec<DelimiterComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontMetricsData {
    pub family: String,
    pub units_per_em: u16,
    pub constants: std::collections::HashMap<MathConstant, Fixed>,
    pub glyphs: std::collections::HashMap<GlyphKey, GlyphMetrics>,
    pub glyph_paths: std::collections::HashMap<GlyphKey, String>,
    pub delimiters: std::collections::HashMap<GlyphKey, DelimiterConstruction>,
    pub kern_map: std::collections::HashMap<GlyphKey, MathKern>,
}

impl FontMetricsData {
    pub fn new(family: String, units_per_em: u16) -> Self {
        Self {
            family,
            units_per_em,
            constants: std::collections::HashMap::new(),
            glyphs: std::collections::HashMap::new(),
            glyph_paths: std::collections::HashMap::new(),
            delimiters: std::collections::HashMap::new(),
            kern_map: std::collections::HashMap::new(),
        }
    }
}
