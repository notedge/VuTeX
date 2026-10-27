//! Math font metrics cache (sync, metrics-only; ported from RuTeX).

use std::collections::HashMap;

use vutex_types::math::{
    DelimiterConstruction, DelimiterVariant, Fixed, FontMetricsData, FontStyle, GlyphKey, GlyphMetrics, MathConstant,
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
        self.metrics_cache.entry(family.clone()).or_insert_with(|| FontMetricsData::new(family.clone(), 1000));
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

    pub fn get_delimiter_construction(&self, key: &GlyphKey) -> MathResult<DelimiterConstruction> {
        let family = key.font_family.as_deref().unwrap_or("default");
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(construction) = data.delimiters.get(key) {
                return Ok(construction.clone());
            }
        }
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
        MathConstant::DelimitedSubFormulaMinHeight => 1.0,
        MathConstant::DisplayOperatorMinHeight => 1.0,
        MathConstant::MathLeading => 0.08,
        MathConstant::FlattenedAccentBaseHeight => 0.6,
        MathConstant::SubscriptBaselineDropMin => 0.2,
        MathConstant::SuperscriptBottomMaxWithSubscript => 0.5,
        MathConstant::SuperscriptBaselineDropMax => 0.4,
        MathConstant::SpaceAfterScript => 0.02,
        MathConstant::StackTopShiftUp => 0.35,
        MathConstant::StackTopDisplayStyleShiftUp => 0.6,
        MathConstant::StackBottomShiftDown => 0.35,
        MathConstant::StackBottomDisplayStyleShiftDown => 0.6,
        MathConstant::StretchStackTopShiftUp => 0.25,
        MathConstant::StretchStackBottomShiftDown => 0.25,
        MathConstant::StretchStackGapAboveMin => 0.1,
        MathConstant::StretchStackGapBelowMin => 0.1,
        MathConstant::SkewedFractionHorizontalGap => 0.2,
        MathConstant::SkewedFractionVerticalGap => 0.1,
        MathConstant::OverbarExtraAscender => 0.06,
        MathConstant::UnderbarExtraDescender => 0.06,
        MathConstant::RadicalKernBeforeDegree => 0.5,
        MathConstant::RadicalKernAfterDegree => -0.1,
        MathConstant::RadicalDegreeBottomRaisePercent => 0.6,
        _ => 0.0,
    };
    Fixed::from_f64(v)
}

/// Complete list of math constants seeded on the synthetic fallback path.
/// The opentype path ignores this (it seeds all constants from the MATH table).
static ALL_SYNTHETIC_CONSTANTS: &[MathConstant] = &[
    MathConstant::ScriptPercentScaleDown,
    MathConstant::ScriptScriptPercentScaleDown,
    MathConstant::DelimitedSubFormulaMinHeight,
    MathConstant::DisplayOperatorMinHeight,
    MathConstant::MathLeading,
    MathConstant::AxisHeight,
    MathConstant::AccentBaseHeight,
    MathConstant::FlattenedAccentBaseHeight,
    MathConstant::SubscriptShiftDown,
    MathConstant::SubscriptTopMax,
    MathConstant::SubscriptBaselineDropMin,
    MathConstant::SuperscriptShiftUp,
    MathConstant::SuperscriptShiftUpCramped,
    MathConstant::SuperscriptBottomMin,
    MathConstant::SuperscriptBaselineDropMax,
    MathConstant::SubSuperscriptGapMin,
    MathConstant::SuperscriptBottomMaxWithSubscript,
    MathConstant::SpaceAfterScript,
    MathConstant::UpperLimitGapMin,
    MathConstant::UpperLimitBaselineRiseMin,
    MathConstant::LowerLimitGapMin,
    MathConstant::LowerLimitBaselineDropMin,
    MathConstant::StackTopShiftUp,
    MathConstant::StackTopDisplayStyleShiftUp,
    MathConstant::StackBottomShiftDown,
    MathConstant::StackBottomDisplayStyleShiftDown,
    MathConstant::StackGapMin,
    MathConstant::StackDisplayStyleGapMin,
    MathConstant::StretchStackTopShiftUp,
    MathConstant::StretchStackBottomShiftDown,
    MathConstant::StretchStackGapAboveMin,
    MathConstant::StretchStackGapBelowMin,
    MathConstant::FractionNumeratorShiftUp,
    MathConstant::FractionNumeratorDisplayStyleShiftUp,
    MathConstant::FractionDenominatorShiftDown,
    MathConstant::FractionDenominatorDisplayStyleShiftDown,
    MathConstant::FractionNumeratorGapMin,
    MathConstant::FractionNumDisplayStyleGapMin,
    MathConstant::FractionRuleThickness,
    MathConstant::FractionDenominatorGapMin,
    MathConstant::FractionDenomDisplayStyleGapMin,
    MathConstant::SkewedFractionHorizontalGap,
    MathConstant::SkewedFractionVerticalGap,
    MathConstant::OverbarVerticalGap,
    MathConstant::OverbarRuleThickness,
    MathConstant::OverbarExtraAscender,
    MathConstant::UnderbarVerticalGap,
    MathConstant::UnderbarRuleThickness,
    MathConstant::UnderbarExtraDescender,
    MathConstant::RadicalVerticalGap,
    MathConstant::RadicalDisplayStyleVerticalGap,
    MathConstant::RadicalRuleThickness,
    MathConstant::RadicalExtraAscender,
    MathConstant::RadicalKernBeforeDegree,
    MathConstant::RadicalKernAfterDegree,
    MathConstant::RadicalDegreeBottomRaisePercent,
];

/// Rewrite every [`GlyphKey`]'s `font_family` inside a map to `new_family`.
fn rekey_family<V>(map: HashMap<GlyphKey, V>, new_family: &str) -> HashMap<GlyphKey, V> {
    map.into_iter()
        .map(|(mut k, v)| {
            k.font_family = Some(new_family.to_string());
            (k, v)
        })
        .collect()
}

/// Builtin metrics for WASM and tests without font files.
///
/// Prefers the embedded OpenType MATH font (Latin Modern Math) and falls back
/// to synthetic metrics if the embedded font cannot be parsed. On the OTF
/// path we also extract real glyph outline paths and MATH Variants delimiter
/// constructions, re-keyed under "default". Synthetic variants and paths are
/// still seeded for delimiter/path characters that the font did not provide.
pub fn builtin_font_metrics() -> FontMetricsSystem {
    let family_str = "default".to_string();
    let mut data = if let Some(opentype_data) = super::opentype::opentype_font_metrics() {
        // Opentype data comes with its own family name ("latin-modern-math");
        // we re-key everything under "default" so downstream code that
        // hardcodes that name keeps working. Font units per em remains from
        // the OTF.
        FontMetricsData {
            family: family_str.clone(),
            units_per_em: opentype_data.units_per_em,
            constants: opentype_data.constants,
            glyphs: rekey_family(opentype_data.glyphs, &family_str),
            glyph_paths: rekey_family(opentype_data.glyph_paths, &family_str),
            delimiters: rekey_family(opentype_data.delimiters, &family_str),
        }
    }
    else {
        FontMetricsData::new(family_str.clone(), 1000)
    };
    let family = Some(family_str.clone());

    // ---- Synthetic delimiter variants (fallback for any char the OTF missed) ----
    let delimiter_chars: &[char] = &['(', ')', '[', ']', '{', '}', '|', '‖', '⟨', '⟩', '/'];
    let variant_sizes: &[(f64, f64)] = &[
        (0.45, 0.25), // small: total ~0.7em
        (0.65, 0.35), // medium: total ~1.0em
        (1.00, 0.50), // large: total ~1.5em
    ];

    for &ch in delimiter_chars {
        let base_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        if data.delimiters.contains_key(&base_key) {
            continue; // OTF already gave us real variants — skip synthetic fallback
        }
        let mut variants = Vec::new();
        for (i, &(h, d)) in variant_sizes.iter().enumerate() {
            let variant_key = GlyphKey {
                char: Some(ch),
                glyph_id: Some(1000 + i as u16),
                font_family: family.clone(),
                style: FontStyle::Math,
            };
            data.glyphs.insert(
                variant_key.clone(),
                GlyphMetrics {
                    width: Fixed::from_f64(0.6),
                    height: Fixed::from_f64(h),
                    depth: Fixed::from_f64(d),
                    italic_correction: Fixed::ZERO,
                },
            );
            variants.push(DelimiterVariant { glyph: variant_key });
        }
        data.delimiters.insert(base_key, DelimiterConstruction { variants, components: vec![] });
    }

    // ---- Synthetic SVG path glyphs (fallback for chars the OTF missed) ----
    let path_chars: &[(char, &str, f64, f64)] = &[
        ('√', "M 0.05,0.45 L 0.25,0.85 L 0.55,0.15", 0.55, 0.95),
        ('±', "M 0.1,0.35 L 0.9,0.35 M 0.5,0.15 L 0.5,0.55 M 0.1,0.65 L 0.9,0.65", 1.0, 0.7),
        ('×', "M 0.15,0.15 L 0.85,0.85 M 0.85,0.15 L 0.15,0.85", 0.9, 0.9),
        ('÷', "M 0.1,0.35 L 0.9,0.35 M 0.5,0.15 L 0.5,0.5 M 0.5,0.6 L 0.5,0.85 M 0.1,0.72 L 0.9,0.72", 1.0, 0.85),
        (
            '∞',
            "M 0.15,0.5 C 0.15,0.3 0.4,0.3 0.4,0.5 C 0.4,0.7 0.6,0.7 0.6,0.5 C 0.6,0.3 0.85,0.3 0.85,0.5 C 0.85,0.7 0.6,0.7 0.6,0.5",
            0.95,
            0.8,
        ),
    ];
    for &(ch, path, w, h) in path_chars {
        // Only insert if the OTF didn't already provide a path for this char.
        let normal_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
        let math_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let has_path =
            data.glyph_paths.contains_key(&normal_key) || data.glyph_paths.contains_key(&math_key);
        if !has_path {
            data.glyph_paths.insert(normal_key.clone(), path.to_string());
            data.glyph_paths.insert(math_key, path.to_string());
        }
        data.glyphs.entry(normal_key.clone()).or_insert_with(|| GlyphMetrics {
            width: Fixed::from_f64(w),
            height: Fixed::from_f64(h),
            depth: Fixed::ZERO,
            italic_correction: Fixed::ZERO,
        });
    }

    // If we're on the synthetic path, also seed all_constants. On the opentype
    // path constants are already populated, so this is a no-op.
    if family_str == "default" {
        for &constant in ALL_SYNTHETIC_CONSTANTS {
            data.constants.entry(constant).or_insert_with(|| default_math_constant(constant));
        }
    }

    FontMetricsSystem::new_with_metrics(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(ch: char) -> GlyphKey {
        GlyphKey::from_char(ch, Some("default".to_string()), FontStyle::Math)
    }

    #[test]
    fn delimiter_construction_has_variants_for_paren() {
        let sys = builtin_font_metrics();
        let c = sys.get_delimiter_construction(&key('(')).unwrap();
        assert!(!c.variants.is_empty(), "expected non-empty variants for '('");
        assert!(c.variants.len() >= 3, "expected at least 3 variants for '('");
    }

    #[test]
    fn delimiter_construction_has_variants_for_bar() {
        let sys = builtin_font_metrics();
        let c = sys.get_delimiter_construction(&key('|')).unwrap();
        assert!(!c.variants.is_empty(), "expected non-empty variants for '|'");
    }

    #[test]
    fn delimiter_variants_have_distinct_metrics() {
        let sys = builtin_font_metrics();
        let c = sys.get_delimiter_construction(&key('[')).unwrap();
        let heights: Vec<f64> = c
            .variants
            .iter()
            .map(|v| {
                let m = sys.get_metrics(&v.glyph).unwrap();
                (m.height + m.depth).to_f64()
            })
            .collect();
        assert!(heights.len() >= 3, "expected at least 3 variants, got {}", heights.len());
        assert!(heights[0] > 0.0, "variant 0 should have positive total height");
        assert!(heights.windows(2).all(|w| w[0] <= w[1]), "variant heights should be non-decreasing");
    }

    #[test]
    fn math_constants_are_nonzero() {
        let sys = builtin_font_metrics();
        assert!(sys.get_math_constant("default", MathConstant::AxisHeight).unwrap().to_f64() > 0.0);
        assert!(sys.get_math_constant("default", MathConstant::FractionRuleThickness).unwrap().to_f64() > 0.0);
        assert!(sys.get_math_constant("default", MathConstant::RadicalRuleThickness).unwrap().to_f64() > 0.0);
    }

    #[test]
    fn delimited_sub_formula_min_height_is_set() {
        let sys = builtin_font_metrics();
        let v = sys.get_math_constant("default", MathConstant::DelimitedSubFormulaMinHeight).unwrap();
        // In the synthetic fallback this was hardcoded to 1.0, but the real
        // OpenType MATH font may use a different (still positive) value.
        assert!(v.to_f64() > 0.0, "DelimitedSubFormulaMinHeight should be positive");
    }
}
