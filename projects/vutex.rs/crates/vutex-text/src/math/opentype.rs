//! OpenType MATH font metrics projection.
//!
//! Embeds a real OpenType MATH font family and projects its typed
//! [`MathFontFacts`] into the [`FontMetricsData`] cache the layout engine reads:
//! the MATH constants, glyph advances and ink extents with SVG outline paths,
//! and the MATH Variants constructions delimiters and large operators grow
//! from.
//!
//! The `MATH` table has exactly one reader — `MathFontFacts` — so this module
//! only re-keys its facts into the cache's [`GlyphKey`]-indexed shape. Glyph
//! geometry comes from the same [`FontFace`]: the outline paths of everything
//! the cache can paint, and the ink boxes plus `hmtx` advances it stores per
//! glyph. Values the font does not publish are explicit stand-ins, collected in
//! the `Synthesized fallbacks` section below.
//!
//! Falls back gracefully to synthetic metrics in
//! [`super::font_metrics::builtin_font_metrics`] when the font cannot be
//! parsed or carries no MATH table.

use acorn_ttf::{FontFace, OutlineBuilder};

use vutex_types::math::{
    DelimiterComponent, DelimiterConstruction, DelimiterVariant, Fixed, FontMetricsData, FontStyle, GlyphKey, GlyphMetrics,
    alphalect_sources, alphalect_styles, math_alphalect,
};

use super::facts::{MathFontFacts, StretchAxis};

/// Selectable OpenType MATH font families.
///
/// Each variant must carry both `bytes` (the raw OTF payload) and a stable
/// family label so glyphs the cache holds under one family can be re-keyed to
/// another without re-parsing. A missing font (bytes = `None`) causes
/// [`opentype_font_metrics`] to return `None` and the caller falls back to
/// synthetic metrics — the default-family gate in [`super::font_metrics`]
/// chooses the first present font at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFontFamily {
    /// Times-like, same lineage as KaTeX_Main; the modern default face.
    XitsMath,
    /// STIX Two Math — full MATH table upstream, asset not embedded yet.
    StixTwoMath,
    /// Computer Modern OpenType successor; kept embedded for compatibility.
    LmMath,
}

impl MathFontFamily {
    /// Stable family label used to key glyphs in [`FontMetricsData`].
    pub const fn label(self) -> &'static str {
        match self {
            Self::XitsMath => "xits-math",
            Self::StixTwoMath => "stix-two-math",
            Self::LmMath => "latin-modern-math",
        }
    }

    /// Raw OTF bytes for this family, or `None` when the font is not embedded.
    pub fn bytes(self) -> Option<&'static [u8]> {
        match self {
            Self::LmMath => Some(LM_MATH_BYTES),
            Self::XitsMath => Some(XITS_MATH_BYTES),
            // STIX Two Math asset pending — falls back to synthetic metrics.
            Self::StixTwoMath => None,
        }
    }

    /// Embedded families in preference order.
    ///
    /// The metrics cache and the font facts must resolve the default face
    /// through this list so they always describe the same font.
    pub const BUILTIN_PREFERENCE: [Self; 2] = [Self::LmMath, Self::XitsMath];

    /// The first embedded family, i.e. the builtin default face.
    pub fn builtin_default() -> Option<Self> {
        Self::BUILTIN_PREFERENCE.into_iter().find(|family| family.bytes().is_some())
    }
}

/// Embedded OpenType MATH font: Latin Modern Math Regular.
///
/// Licensed under the GUST font license (analogous to OFL). Downloaded from
/// `https://www.ctan.org/tex-archive/fonts/lm-math/opentype/latinmodern-math.otf`.
pub(crate) const LM_MATH_BYTES: &[u8] = include_bytes!("../../assets/LatinModernMath-Regular.otf");

/// Embedded OpenType MATH font: XITS Math Regular.
///
/// Licensed under the SIL Open Font License 1.1. Downloaded from
/// `https://cdn.jsdelivr.net/gh/alif-type/xits@master/XITSMath-Regular.otf`
/// (upstream: <https://github.com/alif-type/xits>).
pub(crate) const XITS_MATH_BYTES: &[u8] = include_bytes!("../../assets/XITSMath-Regular.otf");

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

/// The dedicated math-alphabet codepoints the alphalect contract can redirect
/// an authored letter to (math italic, bold, script, double-struck, fraktur,
/// sans-serif, monospace).
///
/// Layout swaps a variable's key onto these codepoints only when the font
/// actually ships them, so they must be inside the glyph scope the projection
/// extracts — otherwise the real italic face would never be found and every
/// variable would fall back to the upright cmap entry of the authored letter.
fn alphalect_targets() -> Vec<char> {
    let mut targets: Vec<char> = alphalect_styles()
        .iter()
        .flat_map(|style| alphalect_sources().iter().map(move |c| math_alphalect(*style, *c)))
        .flatten()
        .collect();
    targets.sort_unstable();
    targets.dedup();
    targets
}

/// Delimiter characters whose MATH Variants constructions we pull from the font.
const DELIMITER_CHARS: &[char] = &['(', ')', '[', ']', '{', '}', '|', '‖', '⟨', '⟩', '/'];

/// Large operators whose display-style size variants we pull from the font.
///
/// MATH keeps them in the same `MathVariants` construction table delimiters
/// stretch from: `\sum`, `\prod`, `\coprod`, `\int`, `\iint` and `\oint` grow
/// from their text-size design to a taller display design rather than being
/// scaled. The list is exactly the characters the parser emits as
/// `SymbolRole::LargeOperator`.
const LARGE_OPERATOR_CHARS: &[char] = &['∑', '∏', '∐', '∫', '∬', '∮'];

// ---------------------------------------------------------------------------
// Synthesized fallbacks
// ---------------------------------------------------------------------------
//
// `FontMetricsData` has no way to say "the font publishes nothing", so a
// handful of numbers below are this crate's own stand-ins. They are all defined
// here so every synthesized value the cache can hold is visible in one place
// instead of scattered through the projection.

/// Narrowest advance the cache reports, in em.
///
/// A glyph the font gives no advance is laid out as a hair of ink rather than
/// as a zero-width box.
const MINIMUM_ADVANCE: f64 = 0.01;

/// Ink box `(height, depth)` assumed for a glyph whose design publishes no
/// bounds, in em.
const SYNTHETIC_INK: (f64, f64) = (0.6, 0.2);

/// Ink box assumed for the `index`-th prepared design of a construction when
/// the font publishes no bounds for it, in em.
///
/// The construction already says the designs grow, so each successive one is
/// assumed a step larger than the previous.
fn synthesized_variant_ink(index: usize) -> (f64, f64) {
    (SYNTHETIC_INK.0 + index as f64 * 0.2, SYNTHETIC_INK.1 + index as f64 * 0.1)
}

/// The MATH metric the cache reports for a glyph the font's `MathGlyphInfo`
/// does not cover.
///
/// The glyph is stored with a zero correction and a zero accent attachment so
/// the cache keeps one shape per glyph. Which glyphs the font actually covers
/// stays visible through `MathFontFacts::glyph_math`.
const UNCOVERED_MATH_METRIC: Fixed = Fixed::ZERO;

// ---------------------------------------------------------------------------
// Font geometry
// ---------------------------------------------------------------------------

/// Pen advance of `glyph` in em, from `hmtx`.
///
/// A glyph with no `hmtx` record has no advance: it answers zero here and the
/// callers clamp it to [`MINIMUM_ADVANCE`].
fn advance_em(face: &mut FontFace, glyph: u16, one_em: f64) -> f64 {
    face.glyph_hor_advance(glyph).unwrap_or(0) as f64 / one_em
}

/// Ink box `(height, depth)` of `glyph` in em, from its font bounds.
///
/// `None` when the font publishes no bounds, so every caller picks its own
/// documented stand-in.
fn ink_from_bounds(face: &mut FontFace, glyph: u16, one_em: f64) -> Option<(f64, f64)> {
    let bounds = face.glyph_bounding_box(glyph)?;
    Some(((bounds.y_max as f64 / one_em).max(0.0), ((-bounds.y_min) as f64 / one_em).max(0.0)))
}

/// The cache's width for a pen advance, clamped to [`MINIMUM_ADVANCE`].
fn width_from_advance(advance_em: f64) -> Fixed {
    Fixed::from_f64(advance_em.max(MINIMUM_ADVANCE))
}

// ---------------------------------------------------------------------------
// Top-level projection
// ---------------------------------------------------------------------------

/// Try to build a [`FontMetricsData`] from the embedded OpenType MATH font.
///
/// Returns `Some(data)` if the requested family has an embedded font that
/// parsed successfully and carries a MATH table. Returns `None` otherwise
/// (family not embedded, parse error, missing MATH table). The caller should
/// then use the synthetic fallback in
/// [`super::font_metrics::builtin_font_metrics`] or try a smaller family.
pub fn opentype_font_metrics(family: MathFontFamily) -> Option<FontMetricsData> {
    let bytes = family.bytes()?;
    let mut face = FontFace::load(bytes).ok()?;
    let facts = MathFontFacts::from_face(&mut face)?;
    let family_label = family.label();
    let one_em = facts.units_per_em() as f64;

    let mut data = FontMetricsData::new(family_label.to_string(), facts.units_per_em());
    data.constants.extend(facts.constants().font_constants());

    extract_glyph_metrics_and_outlines(&mut face, &facts, &mut data, one_em, family_label);
    extract_delimiter_constructions(&mut face, &facts, &mut data, one_em, family_label);

    Some(data)
}

// ---------------------------------------------------------------------------
// Glyph metrics + outline paths
// ---------------------------------------------------------------------------

/// Project the metrics and outline of every character in the product's glyph
/// scope.
///
/// The math metrics come from the facts' glyph coverage: a glyph the font does
/// not cover is stored with [`UNCOVERED_MATH_METRIC`] rather than a guess.
fn extract_glyph_metrics_and_outlines(
    face: &mut FontFace,
    facts: &MathFontFacts,
    data: &mut FontMetricsData,
    one_em: f64,
    family: &str,
) {
    let family = Some(family.to_string());

    for ch in ('!'..='~').chain(TARGET_SYMBOLS.iter().copied()).chain(ACCENT_SYMBOLS.iter().copied()).chain(alphalect_targets())
    {
        let Some(glyph) = facts.glyph_for_char(ch)
        else {
            continue;
        };
        let gid = glyph.get();

        let advance = advance_em(face, gid, one_em);
        let ink = ink_from_bounds(face, gid, one_em).unwrap_or(SYNTHETIC_INK);
        let math = facts.glyph_math(glyph);

        let metrics = GlyphMetrics {
            width: width_from_advance(advance),
            height: Fixed::from_f64(ink.0),
            depth: Fixed::from_f64(ink.1),
            italic_correction: math.and_then(|math| math.italic_correction()).unwrap_or(UNCOVERED_MATH_METRIC),
            top_accent_attachment: math.and_then(|math| math.top_accent_attachment()).unwrap_or(UNCOVERED_MATH_METRIC),
        };

        // The same metrics serve the math face and the upright face, so accent
        // marks resolve without a second lookup.
        let math_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let normal_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Normal);
        data.glyphs.insert(math_key.clone(), metrics);
        data.glyphs.insert(normal_key.clone(), metrics);

        if let Some(path) = glyph_outline(face, gid, one_em) {
            data.glyph_paths.insert(math_key, path.clone());
            data.glyph_paths.insert(normal_key, path);
        }
    }
}

// ---------------------------------------------------------------------------
// MATH Variants projection
// ---------------------------------------------------------------------------

/// Project the growing designs of every delimiter and large operator into the
/// cache: their prepared variants, their assembly parts and the metrics and
/// outlines needed to draw either.
///
/// Which designs exist, in which order, with which connector lengths comes
/// from the facts. Only glyph geometry — `hmtx` advances, ink boxes and
/// outlines — is read from the face.
fn extract_delimiter_constructions(
    face: &mut FontFace,
    facts: &MathFontFacts,
    data: &mut FontMetricsData,
    one_em: f64,
    family: &str,
) {
    let family = Some(family.to_string());

    for &ch in DELIMITER_CHARS.iter().chain(LARGE_OPERATOR_CHARS) {
        let Some(glyph) = facts.glyph_for_char(ch)
        else {
            continue;
        };

        // Try vertical constructions first (covers nearly all delimiters),
        // then horizontal as a fallback for a few horizontal stretchers.
        //
        // The construction's `stretch_measurement` is measured along the
        // stretching direction. For a vertical construction it is a height, so
        // it must never be mixed into the horizontal pen advance: doing that
        // gives `(` a width as tall as its tall variant and `\left(` then
        // leaves exactly that much blank space before the body. Horizontal
        // constructions advance horizontally, where the measurement *is* the
        // advance.
        let vertical = facts.variants().construction(glyph, StretchAxis::Vertical).map(|c| (StretchAxis::Vertical, c));
        let Some((axis, construction)) = vertical
            .or_else(|| facts.variants().construction(glyph, StretchAxis::Horizontal).map(|c| (StretchAxis::Horizontal, c)))
        else {
            continue;
        };
        let is_vertical_construction = axis == StretchAxis::Vertical;

        let base_key = GlyphKey::from_char(ch, family.clone(), FontStyle::Math);
        let mut construction_variants: Vec<DelimiterVariant> = Vec::new();
        let mut construction_components: Vec<DelimiterComponent> = Vec::new();

        for (index, variant) in construction.variants.iter().enumerate() {
            let variant_glyph_id = variant.glyph.get();

            let variant_key = GlyphKey {
                char: Some(ch),
                glyph_id: Some(variant_glyph_id),
                font_family: family.clone(),
                style: FontStyle::Math,
            };

            // Variant metrics so callers can ask for the ink and the width of a
            // stretched design.
            let variant_advance = if is_vertical_construction {
                advance_em(face, variant_glyph_id, one_em)
            }
            else {
                variant.stretch_measurement.to_f64()
            };
            let ink = ink_from_bounds(face, variant_glyph_id, one_em).unwrap_or_else(|| synthesized_variant_ink(index));
            data.glyphs.insert(
                variant_key.clone(),
                GlyphMetrics {
                    width: width_from_advance(variant_advance),
                    height: Fixed::from_f64(ink.0),
                    depth: Fixed::from_f64(ink.1),
                    italic_correction: UNCOVERED_MATH_METRIC,
                    top_accent_attachment: UNCOVERED_MATH_METRIC,
                },
            );
            // The variant keeps its own outline so stretching never falls back
            // to the base glyph shape.
            if let Some(path) = glyph_outline(face, variant_glyph_id, one_em) {
                data.glyph_paths.insert(variant_key.clone(), path);
            }

            construction_variants.push(DelimiterVariant { glyph: variant_key });
        }

        // Piecewise assembly parts are the only way to build delimiters taller
        // than the largest prepared variant.
        if let Some(assembly) = &construction.assembly {
            for part in &assembly.parts {
                let part_glyph_id = part.glyph.get();
                let part_key = GlyphKey::from_gid(part_glyph_id, family.clone(), FontStyle::Math);
                if let Some(path) = glyph_outline(face, part_glyph_id, one_em) {
                    data.glyph_paths.insert(part_key.clone(), path);
                }
                // A part the font publishes no bounds for carries no ink
                // metrics here, while its connectors below stay exact.
                if let Some(ink) = ink_from_bounds(face, part_glyph_id, one_em) {
                    // Same rule as variants: `full_advance` measures the
                    // stacking direction (kept in the component table), while a
                    // vertical part's horizontal advance is `hmtx`.
                    let part_advance = if is_vertical_construction {
                        advance_em(face, part_glyph_id, one_em)
                    }
                    else {
                        part.full_advance.to_f64()
                    };
                    data.glyphs.insert(
                        part_key.clone(),
                        GlyphMetrics {
                            width: width_from_advance(part_advance),
                            height: Fixed::from_f64(ink.0),
                            depth: Fixed::from_f64(ink.1),
                            italic_correction: UNCOVERED_MATH_METRIC,
                            top_accent_attachment: UNCOVERED_MATH_METRIC,
                        },
                    );
                }
                construction_components.push(DelimiterComponent {
                    glyph: part_key,
                    is_extender: part.is_extender,
                    start_connector: part.start_connector,
                    end_connector: part.end_connector,
                    full_advance: part.full_advance,
                });
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
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse and expose MATH table");
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
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        let key = GlyphKey::from_char('x', family, FontStyle::Math);
        let m = data.glyphs.get(&key).expect("glyph 'x' should exist");
        assert!(m.width.to_f64() > 0.3, "width of 'x' should be > 0.3 em");
        assert!(m.height.to_f64() > 0.3, "height of 'x' should be > 0.3 em");
    }

    #[test]
    fn opentype_has_outline_paths_for_common_glyphs() {
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse");
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
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse");
        let family = Some("latin-modern-math".to_string());
        let key_paren = GlyphKey::from_char('(', family, FontStyle::Math);
        let c = data.delimiters.get(&key_paren).expect("'(' should have a construction");
        assert!(!c.variants.is_empty(), "'(' should have variants");
        assert!(c.variants.len() >= 3, "'(' should have at least 3 variants, got {}", c.variants.len());
    }

    #[test]
    fn opentype_covers_every_accent_the_parser_emits() {
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse");
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
        let data = opentype_font_metrics(MathFontFamily::LmMath).expect("font should parse");
        let mut face = FontFace::load(LM_MATH_BYTES).expect("font should load");
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
