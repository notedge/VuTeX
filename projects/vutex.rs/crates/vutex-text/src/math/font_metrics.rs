//! Math font metrics cache (sync, metrics-only; ported from RuTeX).

use std::collections::HashMap;

use super::opentype::MathFontFamily;

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

    /// Whether the font facts actually carry a glyph for `key`.
    ///
    /// Layout uses this to confirm a math-alphabet redirect ships a real face
    /// before swapping the authored character for its dedicated codepoint;
    /// `get_metrics` cannot answer the question because it falls back to
    /// synthetic defaults on a miss.
    pub fn has_glyph(&self, key: &GlyphKey) -> bool {
        let family = key.font_family.as_deref().unwrap_or("default");
        let normalized = GlyphKey { font_family: Some(family.to_string()), ..key.clone() };
        self.metrics_cache.get(family).is_some_and(|data| data.glyphs.contains_key(&normalized))
    }

    pub fn get_metrics(&self, key: &GlyphKey) -> MathResult<GlyphMetrics> {
        let family = key.font_family.as_deref().unwrap_or("default");
        // Normalize key: layout passes None family, but rekey_family stores
        // Some("default"). A None key must match the Some("default") entry.
        let normalized = GlyphKey { font_family: Some(family.to_string()), ..key.clone() };
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(metrics) = data.glyphs.get(&normalized) {
                return Ok(*metrics);
            }
            if let Some(metrics) = emphasis_metrics(data, &normalized) {
                return Ok(metrics);
            }
        }
        Ok(default_glyph_metrics(key.char))
    }

    pub fn get_delimiter_construction(&self, key: &GlyphKey) -> MathResult<DelimiterConstruction> {
        let family = key.font_family.as_deref().unwrap_or("default");
        if let Some(data) = self.metrics_cache.get(family) {
            // Delimiter constructions are style-independent, but callers build
            // keys with the surrounding math style while the font records them
            // under the math face. Try the requested style first, then the
            // faces the font actually uses.
            for style in [key.style, FontStyle::Math, FontStyle::Normal] {
                let candidate = GlyphKey { font_family: Some(family.to_string()), style, ..key.clone() };
                if let Some(construction) = data.delimiters.get(&candidate) {
                    if !construction.variants.is_empty() || !construction.components.is_empty() {
                        return Ok(construction.clone());
                    }
                }
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

    /// A MATH constant exactly as the font publishes it.
    ///
    /// Unlike [`Self::get_math_constant`], which answers a synthetic value for
    /// anything the font omitted, this returns `None` when the font carries no
    /// such constant. Callers that must not mistake a fallback for a measured
    /// value use this lookup and supply their own fallback.
    pub fn font_math_constant(&self, family: &str, constant: MathConstant) -> Option<Fixed> {
        self.metrics_cache.get(family)?.constants.get(&constant).copied()
    }

    /// The font's own outline for `key`.
    ///
    /// An emphasis face is never synthesized from the upright outline: when the
    /// font carries no bold or italic design for the character, the miss is
    /// reported as an explicit diagnostic naming the face and codepoint.
    pub fn get_glyph_path(&self, key: &GlyphKey) -> MathResult<String> {
        let family = key.font_family.as_deref().unwrap_or("default");
        let normalized = GlyphKey { font_family: Some(family.to_string()), ..key.clone() };
        if let Some(data) = self.metrics_cache.get(family) {
            if let Some(path) = data.glyph_paths.get(&normalized) {
                return Ok(path.clone());
            }
        }
        let emphasis = match key.style {
            FontStyle::Bold => Some("bold"),
            FontStyle::Italic => Some("italic"),
            _ => None,
        };
        if let (Some(face), Some(ch)) = (emphasis, key.char) {
            return Err(MathError::font_error(
                family,
                format!(
                    "font ships no {face} face for '{ch}' (U+{:04X}); synthesized emphasis outlines are forbidden",
                    ch as u32
                ),
            ));
        }
        Err(MathError::font_error(family, "glyph path not in cache"))
    }
}

/// Slant assumed when widening the advance of a derived italic face.
const ITALIC_SHEAR: f64 = 0.21;
/// Extra advance a derived bold face adds for its heavier stroke.
const BOLD_OFFSET: f64 = 0.014;

/// Look up the upright key a derived emphasis face is built from.
fn upright_key(key: &GlyphKey, style: FontStyle) -> GlyphKey {
    GlyphKey {
        char: key.char,
        glyph_id: None,
        font_family: key.font_family.clone().or_else(|| Some("default".to_string())),
        style,
    }
}

/// Metrics for a derived bold or italic face.
///
/// The embedded math font carries a single upright design per character, so
/// emphasis faces reuse its extents: bold widens the advance by the stroke
/// thickness it adds, italic widens it by the slant at the glyph's own height.
fn emphasis_metrics(data: &FontMetricsData, key: &GlyphKey) -> Option<GlyphMetrics> {
    if !matches!(key.style, FontStyle::Bold | FontStyle::Italic) {
        return None;
    }
    let normal = upright_key(key, FontStyle::Normal);
    let upright = data.glyphs.get(&normal).or_else(|| data.glyphs.get(&upright_key(key, FontStyle::Math)))?;
    let mut metrics = *upright;
    match key.style {
        FontStyle::Italic => {
            // Keep the real MATH italic correction of the source glyph: the
            // derived face stands for one the font does not ship, so the
            // correction the OpenType MATH table reports for its upright design
            // is still the closest real value (`f` carries 0.079 em).
            metrics.width = metrics.width + Fixed::from_f64(ITALIC_SHEAR * metrics.height.to_f64());
        }
        _ => {
            metrics.width = metrics.width + Fixed::from_f64(BOLD_OFFSET * 2.0);
        }
    }
    Some(metrics)
}

fn default_glyph_metrics(ch: Option<char>) -> GlyphMetrics {
    let width = Fixed::from_f64(0.5);
    GlyphMetrics {
        width: Fixed::from_f64(0.5),
        height: Fixed::from_f64(0.7),
        depth: Fixed::from_f64(0.2),
        // Synthetic fixture: the embedded font has no entry for this glyph, so
        // these MATH values are placeholders rather than measured ones. Every
        // accent the math parser emits (`ˆ`, `¯`, `˜`, `˙`, `¨` and `→`) is
        // measured from the font, so only genuinely absent characters — CJK
        // text inside `\text{}`, for example — reach this fallback.
        italic_correction: Fixed::ZERO,
        top_accent_attachment: Fixed::ZERO,
    }
}

pub(crate) fn default_math_constant(constant: MathConstant) -> Fixed {
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
///
/// `ScriptPercentScaleDown` and `ScriptScriptPercentScaleDown` are deliberately
/// absent: they exist only when a font's MATH table published them, so a caller
/// can tell a measured ratio from the synthetic one with
/// [`FontMetricsSystem::font_math_constant`]. The synthetic ratio is still
/// available through [`FontMetricsSystem::get_math_constant`].
pub(crate) static ALL_SYNTHETIC_CONSTANTS: &[MathConstant] = &[
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

/// Stroke thickness of a bar delimiter, in em units.
const BAR_STROKE: f64 = 0.06;

/// Rectangle outlines for a bar delimiter in em units, y growing upward.
///
/// Each entry in `offsets` is the left edge of one vertical stroke; the
/// outline spans `-depth..height` so a centred delimiter lines up with the math
/// axis exactly like a font outline would.
fn bar_path(offsets: &[f64], height: f64, depth: f64) -> String {
    let bottom = -depth;
    let mut path = String::new();
    for &x in offsets {
        let right = x + BAR_STROKE;
        path.push_str(&format!(
            "M {x:.4},{bottom:.4} L {right:.4},{bottom:.4} L {right:.4},{height:.4} L {x:.4},{height:.4} Z "
        ));
    }
    path
}

/// Rewrite every [`GlyphKey`]'s `font_family` inside a map to `new_family`.
fn rekey_family<V>(map: HashMap<GlyphKey, V>, new_family: &str) -> HashMap<GlyphKey, V> {
    map.into_iter()
        .map(|(mut k, v)| {
            k.font_family = Some(new_family.to_string());
            (k, v)
        })
        .collect()
}

/// Re-key a delimiter map, including the glyph keys nested in every construction.
///
/// Layout reads the variant and assembly glyph keys straight out of the
/// construction it looks up, so a construction whose inner keys still name the
/// source font resolves to synthetic extents and paints nothing.
fn rekey_delimiters(
    map: HashMap<GlyphKey, DelimiterConstruction>,
    new_family: &str,
) -> HashMap<GlyphKey, DelimiterConstruction> {
    let family = Some(new_family.to_string());
    map.into_iter()
        .map(|(mut key, mut construction)| {
            key.font_family = family.clone();
            for variant in &mut construction.variants {
                variant.glyph.font_family = family.clone();
            }
            for component in &mut construction.components {
                component.glyph.font_family = family.clone();
            }
            (key, construction)
        })
        .collect()
}

/// Builtin metrics for WASM and tests without font files.
///
/// Prefers XITS Math (Times-like, KaTeX-compatible) when embedded, falls back
/// to Latin Modern Math, and then to synthetic metrics if neither parses. On
/// the OTF path we also extract real glyph outline paths and MATH Variants
/// delimiter constructions, re-keyed under "default". Synthetic variants and
/// paths are still seeded for delimiter/path characters that the font did not
/// provide.
pub fn builtin_font_metrics() -> FontMetricsSystem {
    // Prefer XITS Math (KaTeX lineage), fall back to LM Math (Computer Modern).
    // The same list gates `MathFontFacts::embedded`, so the engine's facts and
    // this cache always describe one font.
    let opentype_data =
        MathFontFamily::BUILTIN_PREFERENCE.into_iter().find_map(|fam| super::opentype::opentype_font_metrics(fam));
    builtin_font_metrics_from(opentype_data)
}

/// Builtin metrics resolved from exactly one family.
///
/// A family whose asset is not embedded (or whose font file cannot be parsed)
/// yields the pure synthetic cache, the same shape
/// [`builtin_font_metrics`] falls back to. Pair with
/// [`MathFontFacts::for_family`](super::facts::MathFontFacts::for_family) so
/// the metrics cache and the `MATH` facts describe one font.
pub fn builtin_font_metrics_for(family: MathFontFamily) -> FontMetricsSystem {
    builtin_font_metrics_from(super::opentype::opentype_font_metrics(family))
}

/// Assemble the builtin metrics cache around one family's projected data.
///
/// `opentype_data` is the chosen font's metrics or `None` for the pure
/// synthetic path; everything the font publishes is re-keyed under "default".
fn builtin_font_metrics_from(opentype_data: Option<FontMetricsData>) -> FontMetricsSystem {
    let family_str = "default".to_string();

    let mut data = if let Some(opentype_data) = opentype_data {
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
            delimiters: rekey_delimiters(opentype_data.delimiters, &family_str),
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
                    // Synthetic fixture: this variant only exists because the
                    // font shipped no construction for the delimiter.
                    italic_correction: Fixed::ZERO,
                    top_accent_attachment: Fixed::ZERO,
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
        let has_path = data.glyph_paths.contains_key(&normal_key) || data.glyph_paths.contains_key(&math_key);
        if !has_path {
            data.glyph_paths.insert(normal_key.clone(), path.to_string());
            data.glyph_paths.insert(math_key, path.to_string());
        }
        data.glyphs.entry(normal_key.clone()).or_insert_with(|| GlyphMetrics {
            width: Fixed::from_f64(w),
            height: Fixed::from_f64(h),
            depth: Fixed::ZERO,
            // Synthetic fixture: hand-written outline, no MATH table behind it.
            italic_correction: Fixed::ZERO,
            top_accent_attachment: Fixed::ZERO,
        });
    }

    // ---- Vertical bar delimiters (single vs double) ----
    // Bars are geometry, not letterforms: emit exact rectangles so `vmatrix`
    // (one stroke) and `Vmatrix` (two strokes) stay distinct at every size and
    // a stretched bar keeps its thickness instead of scaling sideways.
    let bar_variants: &[(f64, f64)] = &[(0.70, 0.30), (1.10, 0.40), (1.60, 0.50), (2.20, 0.60), (3.00, 0.80)];
    let bar_specs: &[(char, f64, &[f64])] = &[('|', 0.28, &[0.11]), ('‖', 0.40, &[0.10, 0.24])];
    for &(ch, advance, offsets) in bar_specs {
        let base_metrics = GlyphMetrics {
            width: Fixed::from_f64(advance),
            height: Fixed::from_f64(bar_variants[0].0),
            depth: Fixed::from_f64(bar_variants[0].1),
            // Synthetic fixture: bars are geometry, not font glyphs.
            italic_correction: Fixed::ZERO,
            top_accent_attachment: Fixed::ZERO,
        };
        let base_path = bar_path(offsets, bar_variants[0].0, bar_variants[0].1);
        let mut variants = Vec::new();
        for (index, &(height, depth)) in bar_variants.iter().enumerate() {
            let variant_key = GlyphKey {
                char: Some(ch),
                glyph_id: Some(2000 + index as u16),
                font_family: family.clone(),
                style: FontStyle::Math,
            };
            data.glyphs.insert(
                variant_key.clone(),
                GlyphMetrics {
                    width: Fixed::from_f64(advance),
                    height: Fixed::from_f64(height),
                    depth: Fixed::from_f64(depth),
                    // Synthetic fixture: rectangles drawn by `bar_path`.
                    italic_correction: Fixed::ZERO,
                    top_accent_attachment: Fixed::ZERO,
                },
            );
            data.glyph_paths.insert(variant_key.clone(), bar_path(offsets, height, depth));
            variants.push(DelimiterVariant { glyph: variant_key });
        }
        for style in [FontStyle::Normal, FontStyle::Math] {
            let base_key = GlyphKey::from_char(ch, family.clone(), style);
            data.glyphs.insert(base_key.clone(), base_metrics);
            data.glyph_paths.insert(base_key.clone(), base_path.clone());
            data.delimiters.insert(base_key, DelimiterConstruction { variants: variants.clone(), components: vec![] });
        }
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
    fn real_font_italic_correction_reaches_derived_italic_face() {
        let sys = builtin_font_metrics();
        let math_f = sys.get_metrics(&key('f')).unwrap();
        assert!(
            math_f.italic_correction.to_f64() > 0.05,
            "expected the MATH italic correction of 'f', got {}",
            math_f.italic_correction.to_f64()
        );

        let italic_key = GlyphKey::from_char('f', Some("default".to_string()), FontStyle::Italic);
        let italic_f = sys.get_metrics(&italic_key).unwrap();
        assert_eq!(italic_f.italic_correction, math_f.italic_correction, "derived italic face must keep the real correction");
        assert_eq!(italic_f.top_accent_attachment, math_f.top_accent_attachment);
        assert!(italic_f.top_accent_attachment.to_f64() > 0.0, "top accent attachment must come from the MATH table");
    }

    #[test]
    fn accent_glyphs_resolve_under_the_default_family() {
        let sys = builtin_font_metrics();
        for ch in ['ˆ', '¯', '˜', '˙', '¨'] {
            let m = sys.get_metrics(&key(ch)).unwrap();
            assert!(m.width.to_f64() > 0.2, "'{ch}' advance {} is a synthetic placeholder", m.width.to_f64());
            assert!(m.height.to_f64() > 0.4, "'{ch}' height {} is a synthetic placeholder", m.height.to_f64());
            let path = sys.get_glyph_path(&key(ch)).expect("accent outline must come from the font");
            assert!(path.contains('M'), "'{ch}' outline should start with a move: {path}");
        }
        // The vector arrow is the widest accent: `\vec` scales nothing, so the
        // arrow it stamps over a base comes straight from the font.
        let arrow = sys.get_metrics(&key('→')).unwrap();
        assert!(arrow.width.to_f64() > 0.9, "'→' advance {} is a synthetic placeholder", arrow.width.to_f64());
    }

    #[test]
    fn math_constants_are_nonzero() {
        let sys = builtin_font_metrics();
        assert!(sys.get_math_constant("default", MathConstant::AxisHeight).unwrap().to_f64() > 0.0);
        assert!(sys.get_math_constant("default", MathConstant::FractionRuleThickness).unwrap().to_f64() > 0.0);
        assert!(sys.get_math_constant("default", MathConstant::RadicalRuleThickness).unwrap().to_f64() > 0.0);
    }

    #[test]
    fn embedded_font_publishes_the_script_scale_ratios() {
        let sys = builtin_font_metrics();
        for constant in [MathConstant::ScriptPercentScaleDown, MathConstant::ScriptScriptPercentScaleDown] {
            let ratio = sys
                .font_math_constant("default", constant)
                .unwrap_or_else(|| panic!("the embedded math font must publish `{constant:?}`"))
                .to_f64();
            assert!(ratio > 0.0 && ratio < 1.0, "`{constant:?}` must be a fraction of the base em, got {ratio}");
        }
    }

    #[test]
    fn synthetic_metrics_publish_no_font_ratio() {
        let sys = FontMetricsSystem::new_with_metrics(FontMetricsData::new("default".to_string(), 1000));
        for constant in [MathConstant::ScriptPercentScaleDown, MathConstant::ScriptScriptPercentScaleDown] {
            assert_eq!(sys.font_math_constant("default", constant), None, "`{constant:?}` must stay unmeasured");
            // The general lookup still answers a value, from the synthetic table.
            assert!(sys.get_math_constant("default", constant).unwrap().to_f64() > 0.0);
        }
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
