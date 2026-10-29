//! Typed, auditable access to the OpenType `MATH` font facts.
//!
//! The `MATH` table is the single truth source for everything a formula layout
//! needs from a font: the numeric layout constants, the glyph-level math
//! metrics (italic correction, accent attachment, four-corner kerns, extended
//! shapes) and the size variants plus assemblies that grow delimiters and large
//! operators.
//!
//! [`MathFontFacts`] reads that table once and hands the results out as value
//! objects. Two rules hold throughout:
//!
//! - Every number is read from the font. A fact the font does not publish is
//!   `None` (or a marked synthetic fallback in [`constants`]) — never a
//!   silently invented value.
//! - Nothing is cached in shared mutable state. Parsing returns a value the
//!   caller owns and passes on.

use std::collections::BTreeMap;

use acorn_ttf::{
    FontFace,
    tables::math::{MathGlyphConstruction, MathGlyphInfo, MathTable, PartRecord},
};

use vutex_types::math::{Fixed, FontStyle, GlyphKey};

mod constants;
mod kern;
mod variants;

pub use constants::{MathConstantValue, MathConstants};
pub use kern::{CornerKern, GlyphKerns, KernCorner, KernStep, MathKernInfo};
pub use variants::{
    AssemblyPart, AssemblyPlan, GlyphAssembly, GlyphConstruction, MathVariant, MathVariants, StretchAxis, StretchPlan,
};

use super::opentype::MATH_FONT_BYTES;

/// A glyph index inside one font.
///
/// `MATH` coverage tables and `hmtx`/bounds are all glyph-indexed, so the facts
/// are addressed by [`GlyphId`] rather than by character. Use
/// [`MathFontFacts::glyph_for_char`] to resolve a character first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphId(u16);

impl GlyphId {
    /// Wrap a raw glyph index.
    pub const fn new(raw: u16) -> Self {
        Self(raw)
    }

    /// The raw glyph index.
    pub const fn get(self) -> u16 {
        self.0
    }

    /// The [`GlyphKey`] that addresses this glyph in `family`'s math face.
    pub fn key(self, family: &str) -> GlyphKey {
        GlyphKey::from_gid(self.0, Some(family.to_string()), FontStyle::Math)
    }
}

/// The per-glyph attachment metrics of one glyph.
///
/// Both values are optional because `MathGlyphInfo` covers only the glyphs the
/// font chose to publish: `None` means the font carries no value for this glyph,
/// which callers must treat as "unmeasured" rather than as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MathGlyphFacts {
    italic_correction: Option<Fixed>,
    top_accent_attachment: Option<Fixed>,
}

impl MathGlyphFacts {
    /// The glyph's italic correction, if the font publishes one.
    pub fn italic_correction(self) -> Option<Fixed> {
        self.italic_correction
    }

    /// The horizontal point a top accent attaches at, if the font publishes one.
    pub fn top_accent_attachment(self) -> Option<Fixed> {
        self.top_accent_attachment
    }

    /// Whether the font publishes anything at all for this glyph.
    pub fn is_empty(self) -> bool {
        self.italic_correction.is_none() && self.top_accent_attachment.is_none()
    }
}

/// The `MATH` facts of one font.
#[derive(Debug, Clone)]
pub struct MathFontFacts {
    units_per_em: u16,
    char_glyphs: BTreeMap<char, GlyphId>,
    variants: MathVariants,
    constants: MathConstants,
    kerns: MathKernInfo,
    glyph_math: BTreeMap<GlyphId, MathGlyphFacts>,
    extended_shapes: Vec<GlyphId>,
}

impl MathFontFacts {
    /// Read the `MATH` facts out of a font file.
    ///
    /// Returns `None` when the font cannot be parsed or carries no `MATH`
    /// table: a font without the table has no math facts to answer with.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut face = FontFace::load(bytes).ok()?;
        let units_per_em = face.units_per_em();
        let math = face.math().cloned()?;
        let one_em = units_per_em as f64;

        // Collect the character map before the mutable glyph queries below, so
        // no borrow of the face is carried across them.
        let char_glyphs: BTreeMap<char, GlyphId> = face
            .cmap()
            .map(|cmap| cmap.iter().collect::<Vec<(u32, u16)>>())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(codepoint, glyph)| char::from_u32(codepoint).map(|ch| (ch, GlyphId::new(glyph))))
            .collect();

        let variants = extract_variants(&mut face, &math, one_em);
        let constants = MathConstants::from_table(&math.constants, one_em);
        let kerns = MathKernInfo::from_glyph_info(&math.glyph_info, one_em);
        let glyph_math = extract_glyph_math(&math.glyph_info, one_em);
        let extended_shapes = extract_extended_shapes(&math.glyph_info);

        Some(Self { units_per_em, char_glyphs, variants, constants, kerns, glyph_math, extended_shapes })
    }

    /// Read the `MATH` facts of the font embedded with this crate.
    pub fn embedded() -> Option<Self> {
        Self::from_bytes(MATH_FONT_BYTES)
    }

    /// Font units per em, the divisor behind every em-relative value here.
    pub fn units_per_em(&self) -> u16 {
        self.units_per_em
    }

    /// The glyph the font maps `ch` to, if its character map covers it.
    pub fn glyph_for_char(&self, ch: char) -> Option<GlyphId> {
        self.char_glyphs.get(&ch).copied()
    }

    /// The size variants and assemblies of this font.
    pub fn variants(&self) -> &MathVariants {
        &self.variants
    }

    /// The four-corner kerns of this font.
    pub fn kerns(&self) -> &MathKernInfo {
        &self.kerns
    }

    /// The numeric layout constants of this font.
    pub fn constants(&self) -> &MathConstants {
        &self.constants
    }

    /// The attachment metrics the font publishes for `glyph`.
    ///
    /// Answers `None` when the font publishes neither an italic correction nor
    /// a top accent attachment for the glyph.
    pub fn glyph_math(&self, glyph: GlyphId) -> Option<MathGlyphFacts> {
        self.glyph_math.get(&glyph).copied()
    }

    /// The italic correction of `glyph`, if the font publishes one.
    pub fn italic_correction(&self, glyph: GlyphId) -> Option<Fixed> {
        self.glyph_math(glyph)?.italic_correction()
    }

    /// The top accent attachment point of `glyph`, if the font publishes one.
    pub fn top_accent_attachment(&self, glyph: GlyphId) -> Option<Fixed> {
        self.glyph_math(glyph)?.top_accent_attachment()
    }

    /// Whether the font lists `glyph` in its extended shape coverage.
    ///
    /// Extended shapes are the glyphs whose height and depth an accent must not
    /// be raised past; the font publishes the set, so a glyph absent from it is
    /// simply not marked, not "unknown".
    pub fn is_extended_shape(&self, glyph: GlyphId) -> bool {
        self.extended_shapes.binary_search(&glyph).is_ok()
    }

    /// Every glyph the font marks as an extended shape, in glyph order.
    pub fn extended_shapes(&self) -> &[GlyphId] {
        &self.extended_shapes
    }
}

/// Read the size variants and assemblies out of the font's `MathVariants`.
fn extract_variants(face: &mut FontFace, math: &MathTable, one_em: f64) -> MathVariants {
    let table = &math.variants;
    MathVariants::new(
        Fixed::from_f64(table.min_connector_overlap as f64 / one_em),
        extract_constructions(face, math, &table.vert_coverages, &table.vert_constructions, StretchAxis::Vertical, one_em),
        extract_constructions(face, math, &table.horiz_coverages, &table.horiz_constructions, StretchAxis::Horizontal, one_em),
    )
}

/// Turn every covered glyph's construction into a [`GlyphConstruction`].
///
/// The coverage table decides what the font claims to support, so only covered
/// glyphs get an entry, in glyph order.
fn extract_constructions(
    face: &mut FontFace,
    math: &MathTable,
    coverage: &[u16],
    constructions: &std::collections::HashMap<u16, MathGlyphConstruction>,
    axis: StretchAxis,
    one_em: f64,
) -> BTreeMap<GlyphId, GlyphConstruction> {
    let mut out = BTreeMap::new();
    for &glyph in coverage {
        let Some(construction) = constructions.get(&glyph)
        else {
            continue;
        };
        out.insert(GlyphId::new(glyph), glyph_construction(face, math, construction, axis, one_em));
    }
    out
}

/// Read one construction: its prepared variants and its assembly recipe.
fn glyph_construction(
    face: &mut FontFace,
    math: &MathTable,
    construction: &MathGlyphConstruction,
    axis: StretchAxis,
    one_em: f64,
) -> GlyphConstruction {
    let variants = construction
        .variants
        .iter()
        .map(|record| {
            let glyph = record.variant_glyph;
            MathVariant {
                glyph: GlyphId::new(glyph),
                advance: face.glyph_hor_advance(glyph).map(|advance| Fixed::from_f64(advance as f64 / one_em)),
                stretch_measurement: Fixed::from_f64(record.advance_measurement as f64 / one_em),
                height: match axis {
                    // Ink above the baseline is what a vertical variant grows;
                    // ink below the baseline is not part of that extent.
                    StretchAxis::Vertical => {
                        face.glyph_bounding_box(glyph).map(|bbox| Fixed::from_f64((bbox.y_max as f64 / one_em).max(0.0)))
                    }
                    StretchAxis::Horizontal => None,
                },
                italic_correction: math
                    .glyph_info
                    .italic_correction
                    .get(&glyph)
                    .map(|correction| Fixed::from_f64(*correction as f64 / one_em)),
            }
        })
        .collect();

    let assembly = construction.assembly.as_ref().map(|assembly| GlyphAssembly {
        italic_correction: Fixed::from_f64(assembly.italics_correction as f64 / one_em),
        parts: assembly
            .parts
            .iter()
            .map(|part| AssemblyPart {
                glyph: GlyphId::new(part.glyph_id),
                start_connector: Fixed::from_f64(part.start_connector_length as f64 / one_em),
                end_connector: Fixed::from_f64(part.end_connector_length as f64 / one_em),
                full_advance: Fixed::from_f64(part.full_advance as f64 / one_em),
                is_extender: part.part_flags & PartRecord::EXTENDER != 0,
            })
            .collect(),
    });

    GlyphConstruction { variants, assembly }
}

/// Read the per-glyph attachment metrics out of the font's `MathGlyphInfo`.
///
/// Both sub-info tables are glyph-indexed, so a glyph keeps its value only when
/// the corresponding table covers it. Glyphs covered by neither are left out.
fn extract_glyph_math(info: &MathGlyphInfo, one_em: f64) -> BTreeMap<GlyphId, MathGlyphFacts> {
    let mut glyphs: BTreeMap<GlyphId, MathGlyphFacts> = BTreeMap::new();
    for (&glyph, &correction) in &info.italic_correction {
        glyphs.entry(GlyphId::new(glyph)).or_default().italic_correction = Some(Fixed::from_f64(correction as f64 / one_em));
    }
    for (&glyph, &attachment) in &info.top_accent_attachment {
        glyphs.entry(GlyphId::new(glyph)).or_default().top_accent_attachment =
            Some(Fixed::from_f64(attachment as f64 / one_em));
    }
    glyphs
}

/// The glyph order used by [`MathFontFacts::is_extended_shape`]'s binary search.
fn extract_extended_shapes(info: &MathGlyphInfo) -> Vec<GlyphId> {
    let mut shapes: Vec<GlyphId> = info.extended_shapes.iter().map(|&glyph| GlyphId::new(glyph)).collect();
    shapes.sort_unstable();
    shapes.dedup();
    shapes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_for_char_resolves_through_the_font_character_map() {
        let facts = MathFontFacts::embedded().expect("the embedded font must carry a MATH table");
        assert_eq!(facts.units_per_em(), 1000, "Latin Modern Math is a 1000 unit font");
        let paren = facts.glyph_for_char('(').expect("the font maps '('");
        assert_eq!(facts.glyph_for_char('('), Some(paren), "the mapping is stable");
        assert_eq!(facts.glyph_for_char('\u{10FFFF}'), None, "unmapped characters must not invent a glyph");
    }

    #[test]
    fn font_without_a_math_table_reports_nothing() {
        assert!(MathFontFacts::from_bytes(b"not a font").is_none(), "unparsable bytes must not answer with facts");
        assert!(MathFontFacts::from_bytes(&[]).is_none(), "empty input must not answer with facts");
    }
}
