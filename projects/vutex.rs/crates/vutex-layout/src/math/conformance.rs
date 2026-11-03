//! G5 font conformance: the fallback ledger and the gate mode.
//!
//! Layout can always answer: the metrics cache fills anything the font omitted
//! with synthetic stand-ins so a formula still lays out when a font fails to
//! parse or ships no `MATH` table. Gate G5 forbids those stand-ins from ever
//! passing as font data. Every fallback the layout consumes is recorded here
//! with its provenance, and [`FontConformance`] decides whether the record
//! stays a countable warning (production mode) or becomes a hard layout
//! failure (conformance mode).

use vutex_types::math::MathConstant;

/// How strictly layout treats font fallbacks (gate G5).
///
/// Production mode keeps layout working: every fallback is reported as a
/// warning diagnostic on the layout result, so a host can count substitutions
/// without breaking rendering. Conformance mode fails the layout the moment a
/// fallback is consumed, which is the shape the `quality-gates` CI job runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontConformance {
    /// Fallbacks are allowed and counted as warning diagnostics.
    #[default]
    AllowFallbacks,
    /// Any consumed fallback fails the layout with a structured diagnostic.
    Strict,
}

/// One font fallback the layout consumed.
///
/// The variants are the G5 categories: a missing `MATH` constant, a missing
/// glyph variant or assembly piece, a missing italic correction or accent
/// attachment, and a glyph the font does not ship at all. Each maps to one
/// structured diagnostic code via [`FontFallback::code`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FontFallback {
    /// Layout consumed a `MATH` constant the font does not publish.
    MissingConstant(MathConstant),
    /// A delimiter or large operator needed a size variant the font ships no
    /// construction for.
    MissingGlyphVariant(char),
    /// A delimiter stretch needed an assembly piece the cache could not draw.
    MissingAssemblyPiece(char),
    /// An italic or math-italic glyph was laid out without a published italic
    /// correction.
    MissingItalicCorrection(char),
    /// An accent was centred because the base glyph publishes no top accent
    /// attachment.
    MissingAccentAttachment(char),
    /// Layout consumed a glyph the font does not ship at all.
    MissingGlyph(char),
}

impl FontFallback {
    /// The structured diagnostic code naming this fallback class.
    pub fn code(&self) -> &'static str {
        match self {
            Self::MissingConstant(_) => "vutex.math.conformance.missing_constant",
            Self::MissingGlyphVariant(_) => "vutex.math.conformance.missing_glyph_variant",
            Self::MissingAssemblyPiece(_) => "vutex.math.conformance.missing_assembly_piece",
            Self::MissingItalicCorrection(_) => "vutex.math.conformance.missing_italic_correction",
            Self::MissingAccentAttachment(_) => "vutex.math.conformance.missing_accent_attachment",
            Self::MissingGlyph(_) => "vutex.math.conformance.missing_glyph",
        }
    }

    /// A human-readable description of the single missing fact.
    pub fn subject(&self) -> String {
        match self {
            Self::MissingConstant(constant) => format!("MATH constant `{constant:?}`"),
            Self::MissingGlyphVariant(ch) => format!("glyph variant for '{ch}' (U+{:04X})", *ch as u32),
            Self::MissingAssemblyPiece(ch) => format!("assembly piece for '{ch}' (U+{:04X})", *ch as u32),
            Self::MissingItalicCorrection(ch) => format!("italic correction for '{ch}' (U+{:04X})", *ch as u32),
            Self::MissingAccentAttachment(ch) => format!("top accent attachment for '{ch}' (U+{:04X})", *ch as u32),
            Self::MissingGlyph(ch) => format!("glyph for '{ch}' (U+{:04X})", *ch as u32),
        }
    }
}

impl std::fmt::Display for FontFallback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.code(), self.subject())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fallback_class_owns_one_structured_code() {
        let cases = [
            (FontFallback::MissingConstant(MathConstant::AxisHeight), "vutex.math.conformance.missing_constant"),
            (FontFallback::MissingGlyphVariant('('), "vutex.math.conformance.missing_glyph_variant"),
            (FontFallback::MissingAssemblyPiece('('), "vutex.math.conformance.missing_assembly_piece"),
            (FontFallback::MissingItalicCorrection('x'), "vutex.math.conformance.missing_italic_correction"),
            (FontFallback::MissingAccentAttachment('x'), "vutex.math.conformance.missing_accent_attachment"),
            (FontFallback::MissingGlyph('☃'), "vutex.math.conformance.missing_glyph"),
        ];
        for (fallback, code) in cases {
            assert_eq!(fallback.code(), code, "`{fallback}` must keep its structured code");
            assert!(!fallback.subject().is_empty(), "`{fallback}` must name what is missing");
            assert!(fallback.to_string().contains(code), "the display form carries the code");
        }
    }

    #[test]
    fn production_mode_is_the_default() {
        assert_eq!(FontConformance::default(), FontConformance::AllowFallbacks);
    }
}
