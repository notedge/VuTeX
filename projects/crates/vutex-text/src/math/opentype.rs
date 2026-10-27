//! OpenType MATH font metrics extraction.
//!
//! Embeds a real OpenType MATH font (Latin Modern Math) and extracts MATH table
//! constants plus glyph advance widths and bounding boxes at build time via
//! `include_bytes!`. Falls back gracefully to synthetic metrics if parsing fails
//! or the font lacks a MATH table.

use ttf_parser::{math, Face, GlyphId};

use vutex_types::math::{FontMetricsData, FontStyle, Fixed, GlyphKey, GlyphMetrics, MathConstant};

/// Embedded OpenType MATH font: Latin Modern Math Regular.
///
/// Licensed under the GUST font license (analogous to OFL). Downloaded from
/// `https://www.ctan.org/tex-archive/fonts/lm-math/opentype/latinmodern-math.otf`.
const MATH_FONT_BYTES: &[u8] = include_bytes!("../../assets/LatinModernMath-Regular.otf");

/// Try to build a [`FontMetricsData`] from the embedded OpenType MATH font.
///
/// Returns `Some(data)` if the font parsed successfully and contains a MATH
/// table with usable constants. Returns `None` for any failure (parse error,
/// missing MATH table, missing constants). The caller should then use the
/// synthetic fallback in [`super::font_metrics::builtin_font_metrics`].
pub fn opentype_font_metrics() -> Option<FontMetricsData> {
    let face = Face::parse(MATH_FONT_BYTES, 0).ok()?;
    let units_per_em = face.units_per_em();

    let tables = face.tables();
    let math_table = tables.math.as_ref()?;
    let constants = math_table.constants?;

    let mut data = FontMetricsData::new("latin-modern-math".to_string(), units_per_em);

    // ---- MATH constants (font units → em-relative Fixed) ----
    let one_em = units_per_em as f64;

    // Percentage constants → store as fraction (e.g. 70 → 0.7)
    data.constants.insert(
        MathConstant::ScriptPercentScaleDown,
        Fixed::from_f64(constants.script_percent_scale_down() as f64 / 100.0),
    );
    data.constants.insert(
        MathConstant::ScriptScriptPercentScaleDown,
        Fixed::from_f64(constants.script_script_percent_scale_down() as f64 / 100.0),
    );
    // delimited_sub_formula_min_height & display_operator_min_height are u16 font units
    data.constants.insert(
        MathConstant::DelimitedSubFormulaMinHeight,
        Fixed::from_f64(constants.delimited_sub_formula_min_height() as f64 / one_em),
    );
    data.constants.insert(
        MathConstant::DisplayOperatorMinHeight,
        Fixed::from_f64(constants.display_operator_min_height() as f64 / one_em),
    );
    // radical_degree_bottom_raise_percent is i16 percent
    // (we don't have a direct mapping but store it as-is if needed; skip for now)

    // All the MathValue-based constants.
    let m = |mv: math::MathValue| Fixed::from_f64(mv.value as f64 / one_em);

    data.constants.insert(MathConstant::MathLeading, m(constants.math_leading()));
    data.constants.insert(MathConstant::AxisHeight, m(constants.axis_height()));
    data.constants.insert(MathConstant::AccentBaseHeight, m(constants.accent_base_height()));
    data.constants.insert(
        MathConstant::FlattenedAccentBaseHeight,
        m(constants.flattened_accent_base_height()),
    );
    data.constants.insert(MathConstant::SubscriptShiftDown, m(constants.subscript_shift_down()));
    data.constants.insert(MathConstant::SubscriptTopMax, m(constants.subscript_top_max()));
    data.constants.insert(
        MathConstant::SubscriptBaselineDropMin,
        m(constants.subscript_baseline_drop_min()),
    );
    data.constants.insert(MathConstant::SuperscriptShiftUp, m(constants.superscript_shift_up()));
    data.constants.insert(
        MathConstant::SuperscriptShiftUpCramped,
        m(constants.superscript_shift_up_cramped()),
    );
    data.constants.insert(MathConstant::SuperscriptBottomMin, m(constants.superscript_bottom_min()));
    data.constants.insert(
        MathConstant::SuperscriptBaselineDropMax,
        m(constants.superscript_baseline_drop_max()),
    );
    data.constants.insert(
        MathConstant::SubSuperscriptGapMin,
        m(constants.sub_superscript_gap_min()),
    );
    data.constants.insert(
        MathConstant::SuperscriptBottomMaxWithSubscript,
        m(constants.superscript_bottom_max_with_subscript()),
    );
    data.constants.insert(MathConstant::SpaceAfterScript, m(constants.space_after_script()));
    data.constants.insert(MathConstant::UpperLimitGapMin, m(constants.upper_limit_gap_min()));
    data.constants.insert(
        MathConstant::UpperLimitBaselineRiseMin,
        m(constants.upper_limit_baseline_rise_min()),
    );
    data.constants.insert(MathConstant::LowerLimitGapMin, m(constants.lower_limit_gap_min()));
    data.constants.insert(
        MathConstant::LowerLimitBaselineDropMin,
        m(constants.lower_limit_baseline_drop_min()),
    );
    data.constants.insert(MathConstant::StackTopShiftUp, m(constants.stack_top_shift_up()));
    data.constants.insert(
        MathConstant::StackTopDisplayStyleShiftUp,
        m(constants.stack_top_display_style_shift_up()),
    );
    data.constants.insert(
        MathConstant::StackBottomShiftDown,
        m(constants.stack_bottom_shift_down()),
    );
    data.constants.insert(
        MathConstant::StackBottomDisplayStyleShiftDown,
        m(constants.stack_bottom_display_style_shift_down()),
    );
    data.constants.insert(MathConstant::StackGapMin, m(constants.stack_gap_min()));
    data.constants.insert(
        MathConstant::StackDisplayStyleGapMin,
        m(constants.stack_display_style_gap_min()),
    );
    data.constants.insert(
        MathConstant::StretchStackTopShiftUp,
        m(constants.stretch_stack_top_shift_up()),
    );
    data.constants.insert(
        MathConstant::StretchStackBottomShiftDown,
        m(constants.stretch_stack_bottom_shift_down()),
    );
    data.constants.insert(
        MathConstant::StretchStackGapAboveMin,
        m(constants.stretch_stack_gap_above_min()),
    );
    data.constants.insert(
        MathConstant::StretchStackGapBelowMin,
        m(constants.stretch_stack_gap_below_min()),
    );
    data.constants.insert(
        MathConstant::FractionNumeratorShiftUp,
        m(constants.fraction_numerator_shift_up()),
    );
    data.constants.insert(
        MathConstant::FractionNumeratorDisplayStyleShiftUp,
        m(constants.fraction_numerator_display_style_shift_up()),
    );
    data.constants.insert(
        MathConstant::FractionDenominatorShiftDown,
        m(constants.fraction_denominator_shift_down()),
    );
    data.constants.insert(
        MathConstant::FractionDenominatorDisplayStyleShiftDown,
        m(constants.fraction_denominator_display_style_shift_down()),
    );
    data.constants.insert(
        MathConstant::FractionNumeratorGapMin,
        m(constants.fraction_numerator_gap_min()),
    );
    data.constants.insert(
        MathConstant::FractionNumDisplayStyleGapMin,
        m(constants.fraction_num_display_style_gap_min()),
    );
    data.constants.insert(MathConstant::FractionRuleThickness, m(constants.fraction_rule_thickness()));
    data.constants.insert(
        MathConstant::FractionDenominatorGapMin,
        m(constants.fraction_denominator_gap_min()),
    );
    data.constants.insert(
        MathConstant::FractionDenomDisplayStyleGapMin,
        m(constants.fraction_denom_display_style_gap_min()),
    );
    data.constants.insert(
        MathConstant::SkewedFractionHorizontalGap,
        m(constants.skewed_fraction_horizontal_gap()),
    );
    data.constants.insert(
        MathConstant::SkewedFractionVerticalGap,
        m(constants.skewed_fraction_vertical_gap()),
    );
    data.constants.insert(MathConstant::OverbarVerticalGap, m(constants.overbar_vertical_gap()));
    data.constants.insert(MathConstant::OverbarRuleThickness, m(constants.overbar_rule_thickness()));
    data.constants.insert(
        MathConstant::OverbarExtraAscender,
        m(constants.overbar_extra_ascender()),
    );
    data.constants.insert(MathConstant::UnderbarVerticalGap, m(constants.underbar_vertical_gap()));
    data.constants.insert(MathConstant::UnderbarRuleThickness, m(constants.underbar_rule_thickness()));
    data.constants.insert(
        MathConstant::UnderbarExtraDescender,
        m(constants.underbar_extra_descender()),
    );
    data.constants.insert(MathConstant::RadicalVerticalGap, m(constants.radical_vertical_gap()));
    data.constants.insert(
        MathConstant::RadicalDisplayStyleVerticalGap,
        m(constants.radical_display_style_vertical_gap()),
    );
    data.constants.insert(MathConstant::RadicalRuleThickness, m(constants.radical_rule_thickness()));
    data.constants.insert(MathConstant::RadicalExtraAscender, m(constants.radical_extra_ascender()));
    data.constants.insert(
        MathConstant::RadicalKernBeforeDegree,
        m(constants.radical_kern_before_degree()),
    );
    data.constants.insert(
        MathConstant::RadicalKernAfterDegree,
        m(constants.radical_kern_after_degree()),
    );

    // ---- Glyph advance widths and bounding boxes ----
    //
    // Character set: Latin, digits, punctuation, Greek (lower + upper),
    // common math operators and delimiters.
    let family = Some("latin-modern-math".to_string());
    let chars: &[char] = &[
        // Latin lower
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm',
        'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
        // Latin upper
        'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M',
        'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
        // Digits
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
        // Punctuation & operators
        '+', '-', '=', '(', ')', '[', ']', '{', '}', '.', ',', ';', ':', '!', '?',
        // Greek lower (α–ω, skipping none)
        'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ',
        'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
        // Greek upper (Γ–Ω)
        'Γ', 'Δ', 'Θ', 'Λ', 'Ξ', 'Π', 'Σ', 'Φ', 'Ψ', 'Ω',
        // Math operators & delimiters
        '±', '×', '÷', '≤', '≥', '≠', '≈', '→', '∞', '√', '∑', '∏', '∫',
        '|', '‖', '⟨', '⟩', '/',
    ];

    for &ch in chars {
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => continue, // font doesn't have this glyph; skip
        };

        let advance = face.glyph_hor_advance(gid).unwrap_or(0);
        let bbox = face.glyph_bounding_box(gid);

        let (height_emu, depth_emu) = match bbox {
            Some(rect) => {
                // y_max is ascender (above baseline, positive);
                // y_min is descender (below baseline, negative).
                let h = rect.y_max as f64 / one_em;
                let d = (-rect.y_min) as f64 / one_em;
                (h.max(0.0), d.max(0.0))
            }
            None => (0.6, 0.2), // fallback if bbox unavailable
        };

        let width_emu = advance as f64 / one_em;

        let metrics = GlyphMetrics {
            width: Fixed::from_f64(width_emu.max(0.01)), // never zero
            height: Fixed::from_f64(height_emu),
            depth: Fixed::from_f64(depth_emu),
            italic_correction: Fixed::ZERO, // italic correction omitted for now
        };

        // Insert for both Math and Normal styles since we're building a math font.
        let math_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        data.glyphs.insert(math_key, metrics);
        let normal_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
        data.glyphs.insert(normal_key, metrics);
    }

    Some(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use vutex_types::math::MathConstant;

    #[test]
    fn opentype_parses_and_has_constants() {
        let data = opentype_font_metrics().expect("font should parse and expose MATH table");
        assert!(data.constants.contains_key(&MathConstant::AxisHeight));
        assert!(data.constants.contains_key(&MathConstant::FractionRuleThickness));
        assert!(data.constants.contains_key(&MathConstant::SuperscriptShiftUp));

        // AxisHeight for Latin Modern Math is around 0.255 em.
        let ah = data.constants[&MathConstant::AxisHeight].to_f64();
        assert!((0.15..0.35).contains(&ah), "AxisHeight={ah} out of range");

        // FractionRuleThickness should be small, around 0.04 em.
        let frt = data.constants[&MathConstant::FractionRuleThickness].to_f64();
        assert!((0.01..0.12).contains(&frt), "FractionRuleThickness={frt} out of range");
    }

    #[test]
    fn opentype_has_glyph_metrics() {
        let data = opentype_font_metrics().expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        let key = GlyphKey::from_char('x', family, FontStyle::Math);
        let m = data.glyphs.get(&key).expect("glyph 'x' should exist");
        assert!(m.width.to_f64() > 0.3, "width of 'x' should be > 0.3 em");
        assert!(m.height.to_f64() > 0.3, "height of 'x' should be > 0.3 em");
    }
}
