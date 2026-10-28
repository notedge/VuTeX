//! OpenType MATH font metrics extraction.
//!
//! Embeds a real OpenType MATH font (Latin Modern Math) and extracts MATH table
//! constants plus glyph advance widths, bounding boxes, SVG outline paths,
//! and MATH Variants delimiter variants at build time via `include_bytes!`.
//! Falls back gracefully to synthetic metrics if parsing fails or the font
//! lacks a MATH table.

use acorn_ttf::{FontFace, OutlineBuilder, tables::math::PartRecord};

use vutex_types::math::{
    DelimiterComponent, DelimiterConstruction, DelimiterVariant, Fixed, FontMetricsData, FontStyle, GlyphKey,
    GlyphMetrics, MathConstant,
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

/// Non-ASCII symbols whose glyph outlines and MATH variant coverage we extract.
///
/// Printable ASCII (`!` through `~`) is always extracted, so relations such as
/// `<` and `>` never fall back to a host font. This list adds the symbols the
/// math parser can emit beyond ASCII.
const TARGET_SYMBOLS: &[char] = &[
    'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'σ', 'τ', 'υ', 'φ', 'χ', 'ψ', 'ω',
    'Γ', 'Δ', 'Θ', 'Λ', 'Ξ', 'Π', 'Σ', 'Φ', 'Ψ', 'Ω', '±', '×', '÷', '≤', '≥', '≠', '≈', '≡', '≅', '∼', '≃', '≪', '≫', '∓',
    '→', '←', '↑', '↓', '↔', '⇒', '⇐', '⇔', '↦', '∞', '√', '∑', '∏', '∫', '∮', '∂', '∇', '∅', '∈', '∉', '∋', '⊂', '⊃', '⊆',
    '⊇', '∪', '∩', '∧', '∨', '¬', '∀', '∃', '⋅', '∘', '∗', '†', '‡', '…', '⋯', '′', '″', '⊥', '∥', '|', '‖', '⟨', '⟩', 'ℝ',
    'ℕ', 'ℤ', 'ℚ', 'ℂ', 'ℓ', 'ℏ', 'ℜ', 'ℑ', 'ℵ',
];

/// Accent characters the math parser stacks over a base.
///
/// `\hat` uses `ˆ`, `\bar` uses `¯`, `\tilde` uses `˜`, `\dot` uses `˙` and
/// `\ddot` uses `¨`, so those five must reach the outline and metric tables.
/// `\vec` already resolves through `→` in [`TARGET_SYMBOLS`] and `\acute`,
/// `\grave` through printable ASCII (ASCII `` ` ``), so only the modifier
/// letters the parser can emit beyond ASCII are listed here. The extra marks
/// (`˘` breve, `ˇ` caron, `˚` ring, U+20D7 combining vector arrow) cover the
/// remaining TeX accent spellings, so an accent never silently degrades to a
/// host-font `<text>` run when the font does ship the mark.
const ACCENT_SYMBOLS: &[char] = &['ˆ', '¯', '˜', '˙', '¨', '˘', 'ˇ', '˚', '\u{20D7}'];

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
    let mut face = FontFace::load(MATH_FONT_BYTES).ok()?;
    let units_per_em = face.units_per_em();

    // Clone MATH table data up front so we can drop the mutable borrow on
    // `face` and pass it on to the next extraction function.
    let math_table = face.math()?.clone();

    let mut data = FontMetricsData::new("latin-modern-math".to_string(), units_per_em);

    let one_em = units_per_em as f64;

    // ---- MATH constants (font units → em-relative Fixed) ----
    extract_math_constants(&math_table.constants, &mut data, one_em);

    // ---- Glyph advance widths bounding boxes outline paths MATH glyph info ----
    extract_glyph_metrics_and_outlines(&mut face, &math_table.glyph_info, &mut data, one_em);

    // ---- MATH Variants for delimiter stretching ----
    extract_delimiter_variants(&mut face, &math_table.variants, &mut data, one_em);

    Some(data)
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

fn extract_math_constants(constants: &acorn_ttf::tables::math::MathConstants, data: &mut FontMetricsData, one_em: f64) {
    // Percentage constants → store as fraction (e.g. 70 → 0.7)
    data.constants.insert(
        MathConstant::ScriptPercentScaleDown,
        Fixed::from_f64(constants.script_percent_scale_down as f64 / 100.0),
    );
    data.constants.insert(
        MathConstant::ScriptScriptPercentScaleDown,
        Fixed::from_f64(constants.script_script_percent_scale_down as f64 / 100.0),
    );
    data.constants.insert(
        MathConstant::DelimitedSubFormulaMinHeight,
        Fixed::from_f64(constants.delimited_sub_formula_min_height as f64 / one_em),
    );
    data.constants.insert(
        MathConstant::DisplayOperatorMinHeight,
        Fixed::from_f64(constants.display_operator_min_height as f64 / one_em),
    );

    // All the MathValue-based constants — acorn-ttf stores them as raw f32 font units.
    let m = |v: f32| Fixed::from_f64(v as f64 / one_em);

    data.constants.insert(MathConstant::MathLeading, m(constants.math_leading));
    data.constants.insert(MathConstant::AxisHeight, m(constants.axis_height));
    data.constants.insert(MathConstant::AccentBaseHeight, m(constants.accent_base_height));
    data.constants.insert(MathConstant::FlattenedAccentBaseHeight, m(constants.flattened_accent_base_height));
    data.constants.insert(MathConstant::SubscriptShiftDown, m(constants.subscript_shift_down));
    data.constants.insert(MathConstant::SubscriptTopMax, m(constants.subscript_top_max));
    data.constants.insert(MathConstant::SubscriptBaselineDropMin, m(constants.subscript_baseline_drop_min));
    data.constants.insert(MathConstant::SuperscriptShiftUp, m(constants.superscript_shift_up));
    data.constants.insert(MathConstant::SuperscriptShiftUpCramped, m(constants.superscript_shift_up_cramped));
    data.constants.insert(MathConstant::SuperscriptBottomMin, m(constants.superscript_bottom_min));
    data.constants.insert(MathConstant::SuperscriptBaselineDropMax, m(constants.superscript_baseline_drop_max));
    data.constants.insert(MathConstant::SubSuperscriptGapMin, m(constants.sub_superscript_gap_min));
    data.constants.insert(
        MathConstant::SuperscriptBottomMaxWithSubscript,
        m(constants.superscript_bottom_max_with_subscript),
    );
    data.constants.insert(MathConstant::SpaceAfterScript, m(constants.space_after_script));
    data.constants.insert(MathConstant::UpperLimitGapMin, m(constants.upper_limit_gap_min));
    data.constants.insert(MathConstant::UpperLimitBaselineRiseMin, m(constants.upper_limit_baseline_rise_min));
    data.constants.insert(MathConstant::LowerLimitGapMin, m(constants.lower_limit_gap_min));
    data.constants.insert(MathConstant::LowerLimitBaselineDropMin, m(constants.lower_limit_baseline_drop_min));
    data.constants.insert(MathConstant::StackTopShiftUp, m(constants.stack_top_shift_up));
    data.constants.insert(MathConstant::StackTopDisplayStyleShiftUp, m(constants.stack_top_display_style_shift_up));
    data.constants.insert(MathConstant::StackBottomShiftDown, m(constants.stack_bottom_shift_down));
    data.constants.insert(
        MathConstant::StackBottomDisplayStyleShiftDown,
        m(constants.stack_bottom_display_style_shift_down),
    );
    data.constants.insert(MathConstant::StackGapMin, m(constants.stack_gap_min));
    data.constants.insert(MathConstant::StackDisplayStyleGapMin, m(constants.stack_display_style_gap_min));
    data.constants.insert(MathConstant::StretchStackTopShiftUp, m(constants.stretch_stack_top_shift_up));
    data.constants.insert(MathConstant::StretchStackBottomShiftDown, m(constants.stretch_stack_bottom_shift_down));
    data.constants.insert(MathConstant::StretchStackGapAboveMin, m(constants.stretch_stack_gap_above_min));
    data.constants.insert(MathConstant::StretchStackGapBelowMin, m(constants.stretch_stack_gap_below_min));
    data.constants.insert(MathConstant::FractionNumeratorShiftUp, m(constants.fraction_numerator_shift_up));
    data.constants.insert(
        MathConstant::FractionNumeratorDisplayStyleShiftUp,
        m(constants.fraction_numerator_display_style_shift_up),
    );
    data.constants.insert(MathConstant::FractionDenominatorShiftDown, m(constants.fraction_denominator_shift_down));
    data.constants.insert(
        MathConstant::FractionDenominatorDisplayStyleShiftDown,
        m(constants.fraction_denominator_display_style_shift_down),
    );
    data.constants.insert(MathConstant::FractionNumeratorGapMin, m(constants.fraction_numerator_gap_min));
    data.constants.insert(MathConstant::FractionNumDisplayStyleGapMin, m(constants.fraction_num_display_style_gap_min));
    data.constants.insert(MathConstant::FractionRuleThickness, m(constants.fraction_rule_thickness));
    data.constants.insert(MathConstant::FractionDenominatorGapMin, m(constants.fraction_denominator_gap_min));
    data.constants.insert(MathConstant::FractionDenomDisplayStyleGapMin, m(constants.fraction_denom_display_style_gap_min));
    data.constants.insert(MathConstant::SkewedFractionHorizontalGap, m(constants.skewed_fraction_horizontal_gap));
    data.constants.insert(MathConstant::SkewedFractionVerticalGap, m(constants.skewed_fraction_vertical_gap));
    data.constants.insert(MathConstant::OverbarVerticalGap, m(constants.overbar_vertical_gap));
    data.constants.insert(MathConstant::OverbarRuleThickness, m(constants.overbar_rule_thickness));
    data.constants.insert(MathConstant::OverbarExtraAscender, m(constants.overbar_extra_ascender));
    data.constants.insert(MathConstant::UnderbarVerticalGap, m(constants.underbar_vertical_gap));
    data.constants.insert(MathConstant::UnderbarRuleThickness, m(constants.underbar_rule_thickness));
    data.constants.insert(MathConstant::UnderbarExtraDescender, m(constants.underbar_extra_descender));
    data.constants.insert(MathConstant::RadicalVerticalGap, m(constants.radical_vertical_gap));
    data.constants.insert(MathConstant::RadicalDisplayStyleVerticalGap, m(constants.radical_display_style_vertical_gap));
    data.constants.insert(MathConstant::RadicalRuleThickness, m(constants.radical_rule_thickness));
    data.constants.insert(MathConstant::RadicalExtraAscender, m(constants.radical_extra_ascender));
    data.constants.insert(MathConstant::RadicalKernBeforeDegree, m(constants.radical_kern_before_degree));
    data.constants.insert(MathConstant::RadicalKernAfterDegree, m(constants.radical_kern_after_degree));

    // This is the only MATH constant stored as a percentage rather than in
    // design units: 60 means 60%. Normalize it to the 0–1 fraction the layout
    // and `default_math_constant` use.
    data.constants.insert(
        MathConstant::RadicalDegreeBottomRaisePercent,
        Fixed::from_f64(constants.radical_degree_bottom_raise_percent as f64 / 100.0),
    );
}

// ---------------------------------------------------------------------------
// Glyph metrics + outline paths
// ---------------------------------------------------------------------------

fn extract_glyph_metrics_and_outlines(
    face: &mut FontFace,
    math_glyph_info: &acorn_ttf::tables::math::MathGlyphInfo,
    data: &mut FontMetricsData,
    one_em: f64,
) {
    let family = Some("latin-modern-math".to_string());

    let italic_corrections = &math_glyph_info.italic_correction;
    let top_accent_attachments = &math_glyph_info.top_accent_attachment;

    for ch in ('!'..='~').chain(TARGET_SYMBOLS.iter().copied()).chain(ACCENT_SYMBOLS.iter().copied()) {
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => continue,
        };

        let advance = face.glyph_hor_advance(gid).unwrap_or(0);
        let bbox = face.glyph_bounding_box(gid);

        let (height_emu, depth_emu) = match bbox {
            Some(b) => {
                let h = b.y_max as f64 / one_em;
                let d = (-b.y_min) as f64 / one_em;
                (h.max(0.0), d.max(0.0))
            }
            None => (0.6, 0.2),
        };

        let width_emu = advance as f64 / one_em;

        // Real italic correction from MathGlyphInfo, in em units.
        let italic_correction = italic_corrections
            .get(&gid)
            .map(|mv| Fixed::from_f64(*mv as f64 / one_em))
            .unwrap_or(Fixed::ZERO);

        // MathTopAccentAttachment TopRight Y value, in em units.
        let top_accent_attachment = top_accent_attachments
            .get(&gid)
            .map(|mv| Fixed::from_f64(*mv as f64 / one_em))
            .unwrap_or(Fixed::ZERO);

        let metrics = GlyphMetrics {
            width: Fixed::from_f64(width_emu.max(0.01)),
            height: Fixed::from_f64(height_emu),
            depth: Fixed::from_f64(depth_emu),
            italic_correction,
            top_accent_attachment,
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

fn extract_delimiter_variants(
    face: &mut FontFace,
    variants: &acorn_ttf::tables::math::MathVariants,
    data: &mut FontMetricsData,
    one_em: f64,
) {
    let family = Some("latin-modern-math".to_string());

    for &ch in DELIMITER_CHARS {
        let gid = match face.glyph_index(ch) {
            Some(g) => g,
            None => continue,
        };

        // Try vertical constructions first (covers nearly all delimiters),
        // then horizontal as a fallback for a few horizontal stretchers.
        //
        // The construction's `advance_measurement` is measured along the
        // stretching direction. For a vertical construction it is a height, so
        // it must never be mixed into the horizontal pen advance: doing that
        // gives `(` a width as tall as its tall variant and `\left(` then
        // leaves exactly that much blank space before the body. Horizontal
        // constructions advance horizontally, where the measurement *is* the
        // advance.
        let is_vertical_construction = variants.vert_constructions.contains_key(&gid);
        let construction = if is_vertical_construction {
            variants.vert_constructions.get(&gid)
        }
        else {
            variants.horiz_constructions.get(&gid)
        };

        let base_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let mut construction_variants: Vec<DelimiterVariant> = Vec::new();
        let mut construction_components: Vec<DelimiterComponent> = Vec::new();

        if let Some(construction) = construction {
            for (i, v) in construction.variants.iter().enumerate() {
                let variant_glyph_id = v.variant_glyph;

                let variant_key = GlyphKey {
                    char: Some(ch),
                    glyph_id: Some(variant_glyph_id),
                    font_family: family.clone(),
                    style: FontStyle::Math,
                };

                // Also synthesize metrics for this variant glyph so that callers
                // can ask the system for height/depth of the stretched variant.
                let var_advance = if is_vertical_construction {
                    face.glyph_hor_advance(variant_glyph_id).unwrap_or(0) as f64
                }
                else {
                    v.advance_measurement as f64
                };
                let var_bbox = face.glyph_bounding_box(variant_glyph_id);
                let (vh, vd) = match var_bbox {
                    Some(b) => {
                        let h = b.y_max as f64 / one_em;
                        let d = (-b.y_min) as f64 / one_em;
                        (h.max(0.0), d.max(0.0))
                    }
                    None => (0.6 + i as f64 * 0.2, 0.2 + i as f64 * 0.1),
                };
                let var_width = var_advance as f64 / one_em;
                data.glyphs.insert(
                    variant_key.clone(),
                    GlyphMetrics {
                        width: Fixed::from_f64(var_width.max(0.01)),
                        height: Fixed::from_f64(vh),
                        depth: Fixed::from_f64(vd),
                        italic_correction: Fixed::ZERO,
                        top_accent_attachment: Fixed::ZERO,
                    },
                );
                // The variant keeps its own outline so stretching never falls
                // back to the base glyph shape.
                if let Some(path) = glyph_outline(face, variant_glyph_id, one_em) {
                    data.glyph_paths.insert(variant_key.clone(), path);
                }

                construction_variants.push(DelimiterVariant { glyph: variant_key });
            }

            // Piecewise assembly parts are the only way to build delimiters
            // taller than the largest prepared variant.
            if let Some(assembly) = &construction.assembly {
                for part in &assembly.parts {
                    let part_key = GlyphKey::from_gid(part.glyph_id, family.clone(), FontStyle::Math);
                    if let Some(path) = glyph_outline(face, part.glyph_id, one_em) {
                        data.glyph_paths.insert(part_key.clone(), path);
                    }
                    if let Some(bbox) = face.glyph_bounding_box(part.glyph_id) {
                        // Same rule as variants: `full_advance` measures the
                        // stacking direction (kept in the component table),
                        // while a vertical part's horizontal advance is `hmtx`.
                        let part_advance = if is_vertical_construction {
                            face.glyph_hor_advance(part.glyph_id).unwrap_or(0) as f64
                        }
                        else {
                            part.full_advance as f64
                        } / one_em;
                        data.glyphs.insert(
                            part_key.clone(),
                            GlyphMetrics {
                                width: Fixed::from_f64(part_advance.max(0.01)),
                                height: Fixed::from_f64((bbox.y_max as f64 / one_em).max(0.0)),
                                depth: Fixed::from_f64((-bbox.y_min as f64 / one_em).max(0.0)),
                                italic_correction: Fixed::ZERO,
                                top_accent_attachment: Fixed::ZERO,
                            },
                        );
                    }
                    construction_components.push(DelimiterComponent {
                        glyph: part_key,
                        is_extender: part.part_flags & PartRecord::EXTENDER != 0,
                        start_connector: Fixed::from_f64(part.start_connector_length as f64 / one_em),
                        end_connector: Fixed::from_f64(part.end_connector_length as f64 / one_em),
                        full_advance: Fixed::from_f64(part.full_advance as f64 / one_em),
                    });
                }
            }
        }

        if !construction_variants.is_empty() || !construction_components.is_empty() {
            data.delimiters.insert(
                base_key,
                DelimiterConstruction { variants: construction_variants, components: construction_components },
            );
        }
    }
}

/// Extract an em-normalized SVG outline path for one glyph.
fn glyph_outline(face: &mut FontFace, gid: u16, one_em: f64) -> Option<String> {
    let mut builder = SvgPathBuilder::new(one_em as f32);
    face.outline_glyph(gid, &mut builder)?;
    let path = builder.finish();
    (!path.is_empty()).then_some(path)
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
        let key_a = GlyphKey::from_char('A', family.clone(), FontStyle::Math);
        let path_a = data.glyph_paths.get(&key_a).expect("glyph 'A' should have an SVG path");
        assert!(path_a.contains('M'), "path should start with a move");
        assert!(path_a.contains('L') || path_a.contains('C') || path_a.contains('Q'), "path should have drawing segments");
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
        assert!(c.variants.len() >= 3, "'(' should have at least 3 variants, got {}", c.variants.len());
    }

    #[test]
    fn opentype_covers_every_accent_the_parser_emits() {
        let data = opentype_font_metrics().expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        // `\hat`, `\bar`, `\vec`, `\tilde`, `\dot` and `\ddot`, in that order.
        for ch in ['ˆ', '¯', '→', '˜', '˙', '¨'] {
            let normal = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
            let metrics = data.glyphs.get(&normal).unwrap_or_else(|| panic!("'{ch}' should carry metrics"));
            assert!(metrics.width.to_f64() > 0.2, "'{ch}' advance {} is too narrow", metrics.width.to_f64());
            assert!(metrics.height.to_f64() > 0.4, "'{ch}' ink tops out at {} em", metrics.height.to_f64());
            let path = data.glyph_paths.get(&normal).unwrap_or_else(|| panic!("'{ch}' should carry an outline"));
            assert!(path.contains('M'), "'{ch}' outline should start with a move: {path}");
        }
    }

    /// Vertical delimiter variants advance by their `hmtx` width, not by the
    /// height they span in the stacking direction.
    ///
    /// Oracle: the font is re-parsed here and every reported width is compared
    /// against `glyph_hor_advance`. The MATH `advance_measurement` of at least
    /// one variant is also required to exceed that advance, so this test keeps
    /// failing if a vertical measurement is ever mixed back into the width.
    #[test]
    fn vertical_delimiter_variants_advance_by_their_hmtx_width() {
        let data = opentype_font_metrics().expect("font should parse");
        let mut face = FontFace::load(MATH_FONT_BYTES).expect("font should load");
        let one_em = face.units_per_em() as f64;
        let math = face.math().cloned().expect("MATH table");
        let family = Some("latin-modern-math".to_string());

        let mut widest_measurement_gap = 0.0f64;
        for ch in ['(', ')', '[', ']', '{', '}', '|'] {
            let gid = face.glyph_index(ch).expect("delimiter should exist in the font");
            let construction = math.variants.vert_constructions.get(&gid).expect("vertical construction");
            let key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
            let reported = data.delimiters.get(&key).expect("extracted construction");
            assert_eq!(
                reported.variants.len(),
                construction.variants.len(),
                "`{ch}`: every prepared variant must be extracted"
            );

            for (source, extracted) in construction.variants.iter().zip(&reported.variants) {
                let hmtx = face.glyph_hor_advance(source.variant_glyph).expect("variant hmtx") as f64 / one_em;
                let metrics = data.glyphs.get(&extracted.glyph).expect("variant metrics");
                let width = metrics.width.to_f64();
                assert!(
                    (width - hmtx).abs() < 1e-4,
                    "`{ch}` variant {:?}: width {width} must equal the hmtx advance {hmtx}",
                    source.variant_glyph
                );
                widest_measurement_gap = widest_measurement_gap.max(source.advance_measurement as f64 / one_em - hmtx);
            }

            if let Some(assembly) = &construction.assembly {
                for (source, component) in assembly.parts.iter().zip(&reported.components) {
                    let hmtx = face.glyph_hor_advance(source.glyph_id).expect("part hmtx") as f64 / one_em;
                    let metrics = data.glyphs.get(&component.glyph).expect("part metrics");
                    let width = metrics.width.to_f64();
                    assert!(
                        (width - hmtx).abs() < 1e-4,
                        "`{ch}` assembly part {:?}: width {width} must equal the hmtx advance {hmtx}",
                        source.glyph_id
                    );
                    widest_measurement_gap = widest_measurement_gap.max(source.full_advance as f64 / one_em - hmtx);
                }
            }
        }

        assert!(
            widest_measurement_gap > 1.0,
            "no variant's stacking advance exceeds its hmtx width by an em, so this test proves nothing"
        );
    }

    #[test]
    fn svg_path_builder_normalizes_coords() {
        let mut builder = SvgPathBuilder::new(1000.0);
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
