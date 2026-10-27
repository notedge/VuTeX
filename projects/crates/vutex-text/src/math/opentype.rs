//! OpenType MATH font metrics extraction.
//!
//! Embeds a real OpenType MATH font (Latin Modern Math) and extracts MATH table
//! constants plus glyph advance widths, bounding boxes, SVG outline paths,
//! and MATH Variants delimiter variants at build time via `include_bytes!`.
//! Falls back gracefully to synthetic metrics if parsing fails or the font
//! lacks a MATH table.

use ttf_parser::{Face, GlyphId, OutlineBuilder, math};

use vutex_types::math::{
    DelimiterComponent, DelimiterConstruction, DelimiterVariant, Fixed, FontMetricsData, FontStyle, GlyphKey, GlyphMetrics,
    MathConstant,
};

/// Embedded OpenType MATH font: Latin Modern Math Regular.
///
/// Licensed under the GUST font license (analogous to OFL). Downloaded from
/// `https://www.ctan.org/tex-archive/fonts/lm-math/opentype/latinmodern-math.otf`.
const MATH_FONT_BYTES: &[u8] = include_bytes!("../../assets/LatinModernMath-Regular.otf");

// ---------------------------------------------------------------------------
// SVG path OutlineBuilder
// ---------------------------------------------------------------------------

/// An [`OutlineBuilder`] that serializes outline segments to an SVG path
/// string with coordinates normalized to a 0–1 em box.
pub(crate) struct SvgPathBuilder {
    path: String,
    /// Divisor applied to every (x, y) coordinate — typically `units_per_em`.
    scale: f32,
}

impl SvgPathBuilder {
    /// Creates a new builder. Coordinates will be divided by `scale`.
    pub fn new(scale: f32) -> Self {
        SvgPathBuilder { path: String::with_capacity(128), scale }
    }

    /// Returns the accumulated SVG path string.
    pub fn finish(self) -> String {
        self.path
    }

    #[inline]
    fn emit_move(&mut self, x: f32, y: f32) {
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("M {x:.4},{y:.4} ", x = nx, y = ny));
    }

    #[inline]
    fn emit_line(&mut self, x: f32, y: f32) {
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("L {x:.4},{y:.4} ", x = nx, y = ny));
    }

    #[inline]
    fn emit_quad(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let nx1 = x1 / self.scale;
        let ny1 = y1 / self.scale;
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!("Q {x1:.4},{y1:.4} {x:.4},{y:.4} ", x1 = nx1, y1 = ny1, x = nx, y = ny));
    }

    #[inline]
    fn emit_curve(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let nx1 = x1 / self.scale;
        let ny1 = y1 / self.scale;
        let nx2 = x2 / self.scale;
        let ny2 = y2 / self.scale;
        let nx = x / self.scale;
        let ny = y / self.scale;
        self.path.push_str(&format!(
            "C {x1:.4},{y1:.4} {x2:.4},{y2:.4} {x:.4},{y:.4} ",
            x1 = nx1,
            y1 = ny1,
            x2 = nx2,
            y2 = ny2,
            x = nx,
            y = ny
        ));
    }
}

impl OutlineBuilder for SvgPathBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.emit_move(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.emit_line(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.emit_quad(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.emit_curve(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.path.push('Z');
        self.path.push(' ');
    }
}

// ---------------------------------------------------------------------------
// Character scope used for glyph outlines & delimiter variants.
// ---------------------------------------------------------------------------

/// Characters whose glyph outlines and MATH variants we extract from the font.
const TARGET_CHARS: &[char] = &[
    // Latin lower
    'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y',
    'z', // Latin upper
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X',
    'Y', 'Z', // Digits
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', // Punctuation & operators
    '+', '-', '=', '(', ')', '[', ']', '{', '}', '.', ',', ';', ':', '!', '?', // Greek lower (α–ω, skipping none)
    'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
    // Greek upper (Γ–Ω)
    'Γ', 'Δ', 'Θ', 'Λ', 'Ξ', 'Π', 'Σ', 'Φ', 'Ψ', 'Ω', // Math operators & delimiters
    '±', '×', '÷', '≤', '≥', '≠', '≈', '→', '∞', '√', '∑', '∏', '∫', '|', '‖', '⟨', '⟩', '/',
];

/// Delimiter characters whose MATH Variants constructions we pull from the font.
const DELIMITER_CHARS: &[char] = &['(', ')', '[', ']', '{', '}', '|', '‖', '⟨', '⟩', '/'];

// ---------------------------------------------------------------------------
// Top-level extraction
// ---------------------------------------------------------------------------

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

    let one_em = units_per_em as f64;

    // ---- MATH constants (font units → em-relative Fixed) ----
    extract_math_constants(&constants, &mut data, one_em);

    // ---- Glyph advance widths, bounding boxes, AND outline paths ----
    extract_glyph_metrics_and_outlines(&face, &mut data, one_em);

    // ---- MATH Variants for delimiter stretching ----
    if let Some(variants) = math_table.variants {
        extract_delimiter_variants(&face, variants, &mut data, one_em);
    }

    Some(data)
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

fn extract_math_constants(constants: &math::Constants<'_>, data: &mut FontMetricsData, one_em: f64) {
    // Percentage constants → store as fraction (e.g. 70 → 0.7)
    data.constants
        .insert(MathConstant::ScriptPercentScaleDown, Fixed::from_f64(constants.script_percent_scale_down() as f64 / 100.0));
    data.constants.insert(
        MathConstant::ScriptScriptPercentScaleDown,
        Fixed::from_f64(constants.script_script_percent_scale_down() as f64 / 100.0),
    );
    data.constants.insert(
        MathConstant::DelimitedSubFormulaMinHeight,
        Fixed::from_f64(constants.delimited_sub_formula_min_height() as f64 / one_em),
    );
    data.constants.insert(
        MathConstant::DisplayOperatorMinHeight,
        Fixed::from_f64(constants.display_operator_min_height() as f64 / one_em),
    );

    // All the MathValue-based constants.
    let m = |mv: math::MathValue| Fixed::from_f64(mv.value as f64 / one_em);

    data.constants.insert(MathConstant::MathLeading, m(constants.math_leading()));
    data.constants.insert(MathConstant::AxisHeight, m(constants.axis_height()));
    data.constants.insert(MathConstant::AccentBaseHeight, m(constants.accent_base_height()));
    data.constants.insert(MathConstant::FlattenedAccentBaseHeight, m(constants.flattened_accent_base_height()));
    data.constants.insert(MathConstant::SubscriptShiftDown, m(constants.subscript_shift_down()));
    data.constants.insert(MathConstant::SubscriptTopMax, m(constants.subscript_top_max()));
    data.constants.insert(MathConstant::SubscriptBaselineDropMin, m(constants.subscript_baseline_drop_min()));
    data.constants.insert(MathConstant::SuperscriptShiftUp, m(constants.superscript_shift_up()));
    data.constants.insert(MathConstant::SuperscriptShiftUpCramped, m(constants.superscript_shift_up_cramped()));
    data.constants.insert(MathConstant::SuperscriptBottomMin, m(constants.superscript_bottom_min()));
    data.constants.insert(MathConstant::SuperscriptBaselineDropMax, m(constants.superscript_baseline_drop_max()));
    data.constants.insert(MathConstant::SubSuperscriptGapMin, m(constants.sub_superscript_gap_min()));
    data.constants
        .insert(MathConstant::SuperscriptBottomMaxWithSubscript, m(constants.superscript_bottom_max_with_subscript()));
    data.constants.insert(MathConstant::SpaceAfterScript, m(constants.space_after_script()));
    data.constants.insert(MathConstant::UpperLimitGapMin, m(constants.upper_limit_gap_min()));
    data.constants.insert(MathConstant::UpperLimitBaselineRiseMin, m(constants.upper_limit_baseline_rise_min()));
    data.constants.insert(MathConstant::LowerLimitGapMin, m(constants.lower_limit_gap_min()));
    data.constants.insert(MathConstant::LowerLimitBaselineDropMin, m(constants.lower_limit_baseline_drop_min()));
    data.constants.insert(MathConstant::StackTopShiftUp, m(constants.stack_top_shift_up()));
    data.constants.insert(MathConstant::StackTopDisplayStyleShiftUp, m(constants.stack_top_display_style_shift_up()));
    data.constants.insert(MathConstant::StackBottomShiftDown, m(constants.stack_bottom_shift_down()));
    data.constants.insert(MathConstant::StackBottomDisplayStyleShiftDown, m(constants.stack_bottom_display_style_shift_down()));
    data.constants.insert(MathConstant::StackGapMin, m(constants.stack_gap_min()));
    data.constants.insert(MathConstant::StackDisplayStyleGapMin, m(constants.stack_display_style_gap_min()));
    data.constants.insert(MathConstant::StretchStackTopShiftUp, m(constants.stretch_stack_top_shift_up()));
    data.constants.insert(MathConstant::StretchStackBottomShiftDown, m(constants.stretch_stack_bottom_shift_down()));
    data.constants.insert(MathConstant::StretchStackGapAboveMin, m(constants.stretch_stack_gap_above_min()));
    data.constants.insert(MathConstant::StretchStackGapBelowMin, m(constants.stretch_stack_gap_below_min()));
    data.constants.insert(MathConstant::FractionNumeratorShiftUp, m(constants.fraction_numerator_shift_up()));
    data.constants
        .insert(MathConstant::FractionNumeratorDisplayStyleShiftUp, m(constants.fraction_numerator_display_style_shift_up()));
    data.constants.insert(MathConstant::FractionDenominatorShiftDown, m(constants.fraction_denominator_shift_down()));
    data.constants.insert(
        MathConstant::FractionDenominatorDisplayStyleShiftDown,
        m(constants.fraction_denominator_display_style_shift_down()),
    );
    data.constants.insert(MathConstant::FractionNumeratorGapMin, m(constants.fraction_numerator_gap_min()));
    data.constants.insert(MathConstant::FractionNumDisplayStyleGapMin, m(constants.fraction_num_display_style_gap_min()));
    data.constants.insert(MathConstant::FractionRuleThickness, m(constants.fraction_rule_thickness()));
    data.constants.insert(MathConstant::FractionDenominatorGapMin, m(constants.fraction_denominator_gap_min()));
    data.constants.insert(MathConstant::FractionDenomDisplayStyleGapMin, m(constants.fraction_denom_display_style_gap_min()));
    data.constants.insert(MathConstant::SkewedFractionHorizontalGap, m(constants.skewed_fraction_horizontal_gap()));
    data.constants.insert(MathConstant::SkewedFractionVerticalGap, m(constants.skewed_fraction_vertical_gap()));
    data.constants.insert(MathConstant::OverbarVerticalGap, m(constants.overbar_vertical_gap()));
    data.constants.insert(MathConstant::OverbarRuleThickness, m(constants.overbar_rule_thickness()));
    data.constants.insert(MathConstant::OverbarExtraAscender, m(constants.overbar_extra_ascender()));
    data.constants.insert(MathConstant::UnderbarVerticalGap, m(constants.underbar_vertical_gap()));
    data.constants.insert(MathConstant::UnderbarRuleThickness, m(constants.underbar_rule_thickness()));
    data.constants.insert(MathConstant::UnderbarExtraDescender, m(constants.underbar_extra_descender()));
    data.constants.insert(MathConstant::RadicalVerticalGap, m(constants.radical_vertical_gap()));
    data.constants.insert(MathConstant::RadicalDisplayStyleVerticalGap, m(constants.radical_display_style_vertical_gap()));
    data.constants.insert(MathConstant::RadicalRuleThickness, m(constants.radical_rule_thickness()));
    data.constants.insert(MathConstant::RadicalExtraAscender, m(constants.radical_extra_ascender()));
    data.constants.insert(MathConstant::RadicalKernBeforeDegree, m(constants.radical_kern_before_degree()));
    data.constants.insert(MathConstant::RadicalKernAfterDegree, m(constants.radical_kern_after_degree()));
}

// ---------------------------------------------------------------------------
// Glyph metrics + outline paths
// ---------------------------------------------------------------------------

fn extract_glyph_metrics_and_outlines(face: &Face<'_>, data: &mut FontMetricsData, one_em: f64) {
    let family = Some("latin-modern-math".to_string());

    for &ch in TARGET_CHARS {
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => continue,
        };

        let advance = face.glyph_hor_advance(gid).unwrap_or(0);
        let bbox = face.glyph_bounding_box(gid);

        let (height_emu, depth_emu) = match bbox {
            Some(rect) => {
                let h = rect.y_max as f64 / one_em;
                let d = (-rect.y_min) as f64 / one_em;
                (h.max(0.0), d.max(0.0))
            }
            None => (0.6, 0.2),
        };

        let width_emu = advance as f64 / one_em;

        let metrics = GlyphMetrics {
            width: Fixed::from_f64(width_emu.max(0.01)),
            height: Fixed::from_f64(height_emu),
            depth: Fixed::from_f64(depth_emu),
            italic_correction: Fixed::ZERO,
        };

        let math_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        data.glyphs.insert(math_key, metrics);
        let normal_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
        data.glyphs.insert(normal_key.clone(), metrics);

        // ---- Outline path (em-normalized) ----
        let mut builder = SvgPathBuilder::new(one_em as f32);
        let had_outline = face.outline_glyph(gid, &mut builder).is_some();
        if had_outline {
            let path = builder.finish();
            if !path.is_empty() {
                // Store under both Math and Normal styles so any consumer finds it.
                let math_path_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
                data.glyph_paths.insert(math_path_key, path.clone());
                data.glyph_paths.insert(normal_key, path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// MATH Variants delimiter extraction
// ---------------------------------------------------------------------------

fn extract_delimiter_variants(face: &Face<'_>, variants: math::Variants<'_>, data: &mut FontMetricsData, one_em: f64) {
    let family = Some("latin-modern-math".to_string());

    for &ch in DELIMITER_CHARS {
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => continue,
        };

        // Try vertical constructions first (covers nearly all delimiters),
        // then horizontal as a fallback for a few horizontal stretchers.
        let construction = variants.vertical_constructions.get(gid).or_else(|| variants.horizontal_constructions.get(gid));

        let base_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let mut construction_variants: Vec<DelimiterVariant> = Vec::new();
        let mut construction_components: Vec<DelimiterComponent> = Vec::new();

        if let Some(construction) = construction {
            // --- Prepared variants ---
            for (i, v) in construction.variants.into_iter().enumerate() {
                let variant_glyph_id = v.variant_glyph;
                let advance_emu = v.advance_measurement as f64 / one_em;

                // Build a GlyphKey that records both the char and the variant glyph id.
                let variant_key = GlyphKey {
                    char: Some(ch),
                    glyph_id: Some(variant_glyph_id.0),
                    font_family: family.clone(),
                    style: FontStyle::Math,
                };

                // Also synthesize metrics for this variant glyph so that callers
                // can ask the system for height/depth of the stretched variant.
                let var_advance = face.glyph_hor_advance(variant_glyph_id).unwrap_or(v.advance_measurement);
                let var_bbox = face.glyph_bounding_box(variant_glyph_id);
                let (vh, vd) = match var_bbox {
                    Some(rect) => {
                        let h = rect.y_max as f64 / one_em;
                        let d = (-rect.y_min) as f64 / one_em;
                        (h.max(0.0), d.max(0.0))
                    }
                    None => (0.6 + i as f64 * 0.2, 0.2 + i as f64 * 0.1),
                };
                let var_width = var_advance as f64 / one_em;
                data.glyphs.insert(
                    variant_key.clone(),
                    GlyphMetrics {
                        width: Fixed::from_f64(var_width.max(advance_emu).max(0.01)),
                        height: Fixed::from_f64(vh),
                        depth: Fixed::from_f64(vd),
                        italic_correction: Fixed::ZERO,
                    },
                );

                construction_variants.push(DelimiterVariant { glyph: variant_key });
            }

            // --- Assembly components (for fallback piecewise stretching) ---
            if let Some(assembly) = construction.assembly {
                for part in assembly.parts.into_iter() {
                    let part_key = GlyphKey::from_gid(part.glyph_id.0, family.clone(), FontStyle::Math);
                    construction_components.push(DelimiterComponent {
                        glyph: part_key,
                        is_extender: part.part_flags.extender(),
                        start_connector: Fixed::from_f64(part.start_connector_length as f64 / one_em),
                        end_connector: Fixed::from_f64(part.end_connector_length as f64 / one_em),
                        full_advance: Fixed::from_f64(part.full_advance as f64 / one_em),
                    });
                }
            }
        }

        // If we found variants from the font, use them. Otherwise leave the
        // default empty Vec — the caller in `font_metrics::builtin_font_metrics`
        // will fill synthetic variants as fallback.
        if !construction_variants.is_empty() || !construction_components.is_empty() {
            data.delimiters.insert(
                base_key,
                DelimiterConstruction { variants: construction_variants, components: construction_components },
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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

        let ah = data.constants[&MathConstant::AxisHeight].to_f64();
        assert!((0.15..0.35).contains(&ah), "AxisHeight={ah} out of range");

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

    #[test]
    fn opentype_has_outline_paths_for_common_glyphs() {
        let data = opentype_font_metrics().expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        // Latin 'A' should definitely have an outline.
        let key_a = GlyphKey::from_char('A', family.clone(), FontStyle::Math);
        let path_a = data.glyph_paths.get(&key_a).expect("glyph 'A' should have an SVG path");
        assert!(path_a.contains('M'), "path should start with a move");
        assert!(path_a.contains('L') || path_a.contains('C') || path_a.contains('Q'), "path should have drawing segments");
        // Radical sign too.
        let key_sqrt = GlyphKey::from_char('√', family, FontStyle::Math);
        let path_sqrt = data.glyph_paths.get(&key_sqrt).expect("glyph '√' should have an SVG path");
        assert!(!path_sqrt.is_empty(), "√ path should not be empty");
    }

    #[test]
    fn opentype_has_delimiter_variants() {
        let data = opentype_font_metrics().expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        let key_paren = GlyphKey::from_char('(', family, FontStyle::Math);
        let c = data.delimiters.get(&key_paren).expect("'(' should have a construction");
        assert!(!c.variants.is_empty(), "'(' should have variants");
        // Latin Modern Math provides multiple size variants per delimiter.
        assert!(c.variants.len() >= 3, "'(' should have at least 3 variants, got {}", c.variants.len());
    }

    #[test]
    fn svg_path_builder_normalizes_coords() {
        let mut builder = SvgPathBuilder::new(1000.0);
        // Emit a square of 500 font units → should become 0.5 em.
        builder.move_to(0.0, 0.0);
        builder.line_to(500.0, 0.0);
        builder.line_to(500.0, 500.0);
        builder.line_to(0.0, 500.0);
        builder.close();
        let path = builder.finish();
        assert!(path.contains("M 0.0000,0.0000"));
        assert!(path.contains("L 0.5000,0.0000"));
        assert!(path.contains("L 0.5000,0.5000"));
        assert!(path.contains("Z"));
    }
}
