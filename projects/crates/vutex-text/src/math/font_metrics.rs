//! Math font metrics cache (sync, metrics-only; ported from RuTeX).

use std::collections::HashMap;

use vutex_types::math::{
    DelimiterConstruction, Fixed, FontMetricsData, FontStyle, GlyphKey, GlyphMetrics, MathConstant,
    MathError, MathResult,
};

#[derive(Debug)]
pub struct FontMetricsSystem {
    metrics_cache: HashMap<String, FontMetricsData>,
}

impl FontMetricsSystem {
    pub fn new_with_metrics(data: FontMetricsData) -> Self {
        let mut system = Self { metrics_cache: HashMap::new() };
        system.add_metrics_data(data);
        system
    }

    pub fn add_metrics_data(&mut self, data: FontMetricsData) {
        self.metrics_cache.insert(data.family.clone(), data);
    }

    pub fn insert_metrics(&mut self, key: GlyphKey, metrics: GlyphMetrics) {
        let family = key.font_family.clone().unwrap_or_else(|| "default".to_string());
        self.metrics_cache
            .entry(family.clone())
            .or_insert_with(|| FontMetricsData::new(family.clone(), 1000));
        self.metrics_cache.get_mut(&family).unwrap().glyphs.insert(key, metrics);
    }

    pub fn get_metrics(&self, key: &GlyphKey) -> MathResult<GlyphMetrics> {
        let family = key.font_family.as_deref().unwrap_or("default");
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(metrics) = data.glyphs.get(key) {
                return Ok(*metrics);
            }
        }
        Ok(default_glyph_metrics(key.char))
    }

    pub fn get_delimiter_construction(&self, _key: &GlyphKey) -> MathResult<DelimiterConstruction> {
        Ok(DelimiterConstruction { variants: vec![], components: vec![] })
    }

    pub fn get_math_constant(&self, family: &str, constant: MathConstant) -> MathResult<Fixed> {
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(val) = data.constants.get(&constant) {
                return Ok(*val);
            }
        }
        Ok(default_math_constant(constant))
    }

    pub fn get_glyph_path(&self, key: &GlyphKey) -> MathResult<String> {
        let family = key.font_family.as_deref().unwrap_or("default");
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(path) = data.glyph_paths.get(key) {
                return Ok(path.clone());
            }
        }
        Err(MathError::font_error(family, "glyph path not in cache"))
    }
}

fn default_glyph_metrics(ch: Option<char>) -> GlyphMetrics {
    let width = Fixed::from_f64(0.5);
    GlyphMetrics {
        width: Fixed::from_f64(0.5),
        height: Fixed::from_f64(0.7),
        depth: Fixed::from_f64(0.2),
        italic_correction: Fixed::ZERO,
    }
}

fn default_math_constant(constant: MathConstant) -> Fixed {
    let v = match constant {
        MathConstant::ScriptPercentScaleDown => 0.7,
        MathConstant::ScriptScriptPercentScaleDown => 0.5,
        MathConstant::AxisHeight => 0.25,
        MathConstant::FractionRuleThickness => 0.06,
        MathConstant::FractionNumeratorShiftUp => 0.35,
        MathConstant::FractionNumeratorDisplayStyleShiftUp => 0.6,
        MathConstant::FractionDenominatorShiftDown => 0.35,
        MathConstant::FractionDenominatorDisplayStyleShiftDown => 0.6,
        MathConstant::FractionNumeratorGapMin => 0.05,
        MathConstant::FractionNumDisplayStyleGapMin => 0.1,
        MathConstant::FractionDenominatorGapMin => 0.05,
        MathConstant::FractionDenomDisplayStyleGapMin => 0.1,
        MathConstant::SubscriptShiftDown => 0.2,
        MathConstant::SubscriptTopMax => 0.4,
        MathConstant::SuperscriptShiftUp => 0.4,
        MathConstant::SuperscriptShiftUpCramped => 0.35,
        MathConstant::SubSuperscriptGapMin => 0.1,
        MathConstant::UpperLimitGapMin => 0.1,
        MathConstant::LowerLimitGapMin => 0.1,
        MathConstant::StackGapMin => 0.1,
        MathConstant::StackDisplayStyleGapMin => 0.3,
        MathConstant::RadicalVerticalGap => 0.06,
        MathConstant::RadicalDisplayStyleVerticalGap => 0.1,
        MathConstant::RadicalRuleThickness => 0.06,
        MathConstant::RadicalExtraAscender => 0.06,
        MathConstant::AccentBaseHeight => 0.45,
        MathConstant::OverbarRuleThickness => 0.06,
        MathConstant::OverbarVerticalGap => 0.1,
        MathConstant::UnderbarRuleThickness => 0.06,
        MathConstant::UnderbarVerticalGap => 0.1,
        MathConstant::SuperscriptBottomMin => 0.1,
        MathConstant::LowerLimitBaselineDropMin => 0.6,
        MathConstant::UpperLimitBaselineRiseMin => 0.3,
        _ => 0.0,
    };
    Fixed::from_f64(v)
}

/// Builtin metrics for WASM and tests without font files.
pub fn builtin_font_metrics() -> FontMetricsSystem {
    let mut data = FontMetricsData::new("default".to_string(), 1000);
    for c in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+-=()[]{}.,;:!?".chars() {
        let key = GlyphKey::from_char(c, Some("default".to_string()), FontStyle::Math);
        data.glyphs.insert(
            key,
            GlyphMetrics {
                width: Fixed::from_f64(0.5),
                height: Fixed::from_f64(0.7),
                depth: Fixed::from_f64(0.2),
                italic_correction: Fixed::ZERO,
            },
        );
    }
    for constant in [
        MathConstant::AxisHeight,
        MathConstant::FractionRuleThickness,
        MathConstant::FractionNumeratorShiftUp,
        MathConstant::FractionDenominatorShiftDown,
        MathConstant::SuperscriptShiftUp,
        MathConstant::SubscriptShiftDown,
    ] {
        data.constants.insert(constant, default_math_constant(constant));
    }
    FontMetricsSystem::new_with_metrics(data)
}
