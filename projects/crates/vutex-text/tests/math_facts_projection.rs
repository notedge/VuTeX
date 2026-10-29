//! `FontMetricsData` must be a projection of `MathFontFacts`, never a second
//! read of the `MATH` table.
//!
//! The cache is a `GlyphKey`-indexed re-keying of the typed facts, so these
//! tests walk the cache back to where every number came from: the constants, a
//! delimiter's prepared designs and assembly recipe, and each glyph's italic
//! correction and accent attachment. If a second reader ever crept back in,
//! these comparisons would drift apart.

use std::collections::HashMap;

use vutex_text::math::{GlyphId, MathFontFacts, StretchAxis, opentype_font_metrics};
use vutex_types::math::{Fixed, FontStyle, GlyphKey, MathConstant};

/// Family label the opentype projection keys its cache entries under.
const FAMILY: &str = "latin-modern-math";

/// Assert two `Fixed` values describe the same em value.
///
/// One `Fixed` step is 1/65536 em, so a tighter tolerance would compare bit
/// patterns rather than the numbers the font published.
fn close(actual: Fixed, expected: Fixed, what: &str) {
    let gap = (actual.to_f64() - expected.to_f64()).abs();
    assert!(gap < 1e-4, "{what}: cache says {actual:?}, facts say {expected:?}");
}

#[test]
fn every_constant_in_the_cache_is_the_one_the_facts_read() {
    let data = opentype_font_metrics().expect("the embedded font parses");
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");

    let published: HashMap<MathConstant, Fixed> = facts.constants().font_constants().collect();
    assert!(!published.is_empty(), "the font publishes constants");
    assert_eq!(data.constants.len(), published.len(), "the cache must carry exactly the font's constants");

    for (constant, expected) in &published {
        let actual = *data.constants.get(constant).unwrap_or_else(|| panic!("`{constant:?}` missing from the cache"));
        close(actual, *expected, &format!("constant `{constant:?}`"));
    }
}

#[test]
fn every_delimiter_projects_the_facts_construction_one_to_one() {
    let data = opentype_font_metrics().expect("the embedded font parses");
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");
    assert!(!data.delimiters.is_empty(), "the projection must carry the font's constructions");

    for (base_key, projected) in &data.delimiters {
        let ch = base_key.char.expect("delimiter entries are keyed by character");
        assert_eq!(base_key.font_family.as_deref(), Some(FAMILY), "`{ch}`: the projected family");
        assert_eq!(base_key.style, FontStyle::Math, "`{ch}`: the projected face");
        assert_eq!(base_key.glyph_id, None, "`{ch}`: the base entry is keyed by character");

        let glyph = facts.glyph_for_char(ch).expect("the cache only projects characters the font maps");
        let source = facts
            .variants()
            .construction(glyph, StretchAxis::Vertical)
            .map(|construction| (StretchAxis::Vertical, construction))
            .or_else(|| {
                facts
                    .variants()
                    .construction(glyph, StretchAxis::Horizontal)
                    .map(|construction| (StretchAxis::Horizontal, construction))
            })
            .unwrap_or_else(|| panic!("`{ch}` has no construction in the facts"));

        let (axis, construction) = source;
        assert_eq!(projected.variants.len(), construction.variants.len(), "`{ch}`: one cache entry per prepared design");

        for (cache_variant, fact) in projected.variants.iter().zip(&construction.variants) {
            assert_eq!(cache_variant.glyph.char, Some(ch), "`{ch}`: a design stays keyed by its character");
            assert_eq!(
                cache_variant.glyph.glyph_id,
                Some(fact.glyph.get()),
                "`{ch}`: the design glyph in the same order the font lists it"
            );

            let metrics = data
                .glyphs
                .get(&cache_variant.glyph)
                .unwrap_or_else(|| panic!("`{ch}` design {:?} carries no metrics", fact.glyph));
            match axis {
                StretchAxis::Vertical => {
                    let advance =
                        fact.advance.unwrap_or_else(|| panic!("`{ch}` design {:?} has no `hmtx` advance", fact.glyph));
                    close(metrics.width, advance, &format!("`{ch}` design {:?} width", fact.glyph));
                    if let Some(height) = fact.height {
                        close(metrics.height, height, &format!("`{ch}` design {:?} ink height", fact.glyph));
                    }
                }
                // A horizontal design advances along the writing direction, so
                // its stacking measurement is its pen advance.
                StretchAxis::Horizontal => {
                    close(metrics.width, fact.stretch_measurement, &format!("`{ch}` design {:?} width", fact.glyph))
                }
            }
        }

        match &construction.assembly {
            Some(assembly) => {
                assert_eq!(
                    projected.components.len(),
                    assembly.parts.len(),
                    "`{ch}`: one component per part of the font's recipe"
                );
                for (component, part) in projected.components.iter().zip(&assembly.parts) {
                    assert_eq!(component.glyph.char, None, "`{ch}`: a part is keyed by glyph");
                    assert_eq!(
                        component.glyph.glyph_id,
                        Some(part.glyph.get()),
                        "`{ch}`: the part glyph in the same order the font lists it"
                    );
                    assert_eq!(component.is_extender, part.is_extender, "`{ch}`: the extender flag");
                    close(component.start_connector, part.start_connector, &format!("`{ch}` part start connector"));
                    close(component.end_connector, part.end_connector, &format!("`{ch}` part end connector"));
                    close(component.full_advance, part.full_advance, &format!("`{ch}` part extent"));
                }
            }
            None => {
                assert!(projected.components.is_empty(), "`{ch}` publishes no recipe, so the cache must carry no components")
            }
        }
    }
}

#[test]
fn every_glyph_math_metric_in_the_cache_is_the_one_the_facts_read() {
    let data = opentype_font_metrics().expect("the embedded font parses");
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");

    let mut checked = 0usize;
    for (key, metrics) in &data.glyphs {
        // Base character entries are keyed by character only: a prepared design
        // carries both a character and its glyph, so it is skipped here. Both
        // faces repeat the same numbers, so either style is compared.
        if key.glyph_id.is_some() {
            continue;
        }
        let Some(ch) = key.char
        else {
            continue;
        };

        let glyph: GlyphId = facts.glyph_for_char(ch).expect("the cache only projects characters the font maps");
        let expected_correction = facts.italic_correction(glyph).unwrap_or(Fixed::ZERO);
        let expected_attachment = facts.top_accent_attachment(glyph).unwrap_or(Fixed::ZERO);
        close(metrics.italic_correction, expected_correction, &format!("`{ch}` italic correction"));
        close(metrics.top_accent_attachment, expected_attachment, &format!("`{ch}` top accent attachment"));
        checked += 1;
    }

    assert!(checked > 100, "the projection must carry the whole character scope, checked {checked}");
}

/// The glyph-keyed entries exist for the same glyphs the facts describe, so no
/// character can be missing a design the font publishes.
#[test]
fn every_projected_design_glyph_is_a_glyph_the_facts_name() {
    let data = opentype_font_metrics().expect("the embedded font parses");
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");

    let mut designs = 0usize;
    for key in data.glyphs.keys().filter(|key| key.glyph_id.is_some()) {
        let glyph = GlyphId::new(key.glyph_id.expect("the filter keeps glyph keys"));
        let named = facts.variants().vertical_constructions().chain(facts.variants().horizontal_constructions()).any(
            |(_, construction)| {
                construction.variants.iter().any(|variant| variant.glyph == glyph)
                    || construction
                        .assembly
                        .as_ref()
                        .is_some_and(|assembly| assembly.parts.iter().any(|part| part.glyph == glyph))
            },
        );
        assert!(named, "glyph {:?} is projected but no construction in the facts names it", glyph);
        designs += 1;
    }
    assert!(designs > 20, "the projection must carry the font's designs, found {designs}");
}

/// The cache keys its base entries by the same characters the facts map, so a
/// glyph-keyed lookup never has to re-resolve a character.
#[test]
fn projected_character_entries_use_the_facts_character_map() {
    let data = opentype_font_metrics().expect("the embedded font parses");
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");

    for key in data.glyphs.keys().filter(|key| key.char.is_some()) {
        let ch = key.char.expect("the filter keeps character keys");
        assert!(facts.glyph_for_char(ch).is_some(), "`{ch}` is keyed in the cache but not in the font's character map");
        assert_eq!(key.font_family.as_deref(), Some(FAMILY), "`{ch}`: the projected family");
    }

    let paren = GlyphKey::from_char('(', Some(FAMILY.to_string()), FontStyle::Math);
    assert!(data.glyphs.contains_key(&paren), "the cache must carry the base character `(`");
}
