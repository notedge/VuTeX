//! `MathVariants` facts must come from the embedded font, not from a guess.
//!
//! Every expectation below is a fact of `XITSMath-Regular.otf`: its
//! `MathVariants` coverage, its prepared variant records and its `GlyphAssembly`
//! recipes. A size the font does not publish must be reported as missing rather
//! than replaced with a scaled-up copy of the base glyph.

use vutex_text::math::{GlyphId, MathFontFacts, StretchAxis, StretchPlan};
use vutex_types::math::Fixed;

fn em(value: f64) -> Fixed {
    Fixed::from_f64(value)
}

fn close(actual: Fixed, expected: f64, what: &str) {
    let actual = actual.to_f64();
    assert!((actual - expected).abs() < 1e-3, "{what}: expected ~{expected}, got {actual}");
}

#[test]
fn the_font_publishes_a_construction_per_covered_glyph() {
    let facts = MathFontFacts::embedded().expect("the embedded font carries a MATH table");
    let variants = facts.variants();

    assert_eq!(facts.units_per_em(), 1000, "XITS Math is a 1000 unit font");
    close(variants.min_connector_overlap, 0.05, "MinConnectorOverlap");

    // Coverage-driven extraction: the counts are the font's own coverage
    // lengths, so they do not follow from any character list here.
    assert_eq!(variants.vertical_constructions().count(), 168, "vertical VertGlyphCoverage length");
    assert_eq!(variants.horizontal_constructions().count(), 41, "horizontal HorizGlyphCoverage length");

    for ch in ['(', ')', '[', ']', '{', '}', '|', '√', '∑', '∫', '⟨'] {
        let glyph = facts.glyph_for_char(ch).unwrap_or_else(|| panic!("the font maps `{ch}`"));
        assert!(variants.construction(glyph, StretchAxis::Vertical).is_some(), "`{ch}` must carry a vertical construction");
    }
    let x = facts.glyph_for_char('x').expect("the font maps `x`");
    assert!(variants.construction(x, StretchAxis::Vertical).is_none(), "`x` is not a stretchable glyph");
}

#[test]
fn parenthesis_variants_and_assembly_are_read_from_the_font() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();
    let paren = facts.glyph_for_char('(').expect("the font maps `(`");
    assert_eq!(paren, GlyphId::new(41), "'(' is glyph 41 in this font");
    let construction = variants.construction(paren, StretchAxis::Vertical).expect("'(' grows");

    assert_eq!(construction.variants.len(), 5, "'(' publishes five prepared designs");
    let measures: Vec<f64> = construction.variants.iter().map(|variant| variant.stretch_measurement.to_f64()).collect();
    assert!(measures.windows(2).all(|pair| pair[0] < pair[1]), "prepared designs grow in order: {measures:?}");
    close(construction.variants[0].stretch_measurement, 0.854, "'(' smallest prepared design");
    close(construction.variants.last().unwrap().stretch_measurement, 3.076, "'(' tallest prepared design");

    // A vertical variant reports the ink it lifts above the baseline, and its
    // pen advance comes from `hmtx` instead of the stacking measurement.
    let base = &construction.variants[0];
    close(base.height.expect("the font publishes a bounding box for '('"), 0.676, "'(' smallest design height");
    close(base.advance.expect("the font publishes an advance for '('"), 0.333, "'(' pen advance");
    let tallest = construction.variants.last().unwrap();
    close(tallest.height.expect("tallest '(' design height"), 2.566, "tallest '(' height");

    let assembly = construction.assembly.as_ref().expect("'(' publishes an assembly");
    assert_eq!(assembly.parts.len(), 3, "'(' is a hook, an extender and a hook");
    let parts = &assembly.parts;
    assert!(!parts[0].is_extender && parts[1].is_extender && !parts[2].is_extender, "only the middle part repeats");
    close(parts[0].full_advance, 1.005, "hook extent");
    close(parts[1].full_advance, 1.01, "extender extent");
    close(parts[1].start_connector, 0.15, "extender connector");
    close(parts[0].start_connector, 0.0, "the first part starts flush");
    close(parts[2].end_connector, 0.0, "the last part ends flush");
}

#[test]
fn stretch_takes_the_smallest_sufficient_variant_then_the_assembly() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();
    let paren = facts.glyph_for_char('(').expect("the font maps `(`");

    // The smallest prepared design reaches 0.854 em; 1.0 em needs the next one.
    match variants.stretch(paren, StretchAxis::Vertical, em(1.0)).expect("a design reaches 1 em") {
        StretchPlan::Variant(variant) => {
            assert_eq!(variant.glyph, GlyphId::new(3617), "the smallest design above 1 em");
            close(variant.stretch_measurement, 1.231, "next larger '(' design");
        }
        other => panic!("expected a prepared variant, got {other:?}"),
    }

    // Nothing prepared reaches 3.5 em, so the assembly repeats its extender once.
    match variants.stretch(paren, StretchAxis::Vertical, em(3.5)).expect("the assembly reaches 3.5 em") {
        StretchPlan::Assembly(plan) => {
            assert_eq!(plan.instances.len(), 4, "two hooks and one extra extender");
            close(plan.overlap, 0.05, "the plan keeps the font's connector overlap");
            close(plan.extent, 3.88, "assembled '(' extent");
            assert!(plan.extent >= em(3.5), "an assembly must reach the target");
        }
        other => panic!("expected an assembly, got {other:?}"),
    }
}

#[test]
fn a_brace_grows_through_both_of_its_extenders() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();
    let brace = facts.glyph_for_char('{').expect("the font maps `{`");
    let construction = variants.construction(brace, StretchAxis::Vertical).expect("the brace grows");
    let assembly = construction.assembly.as_ref().expect("the brace publishes an assembly");

    // XITS Math's brace recipe: hook, extender, middle, extender, hook.
    assert_eq!(assembly.parts.len(), 5, "`{{` is a five part recipe");
    assert_eq!(assembly.parts.iter().filter(|part| part.is_extender).count(), 2, "`{{` repeats two extenders");

    match variants.stretch(brace, StretchAxis::Vertical, em(5.0)).expect("the recipe reaches 5 em") {
        StretchPlan::Assembly(plan) => {
            assert_eq!(plan.instances.len(), 7, "one extra round grows both extenders");
            close(plan.extent, 6.76, "assembled `{{` extent");
        }
        other => panic!("expected an assembly, got {other:?}"),
    }
}

#[test]
fn horizontal_constructions_cover_the_wide_forms_the_font_ships() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();

    // XITS Math does not stretch `=` at all: a construction must not be
    // invented for an uncovered glyph.
    let equals = facts.glyph_for_char('=').expect("the font maps `=`");
    assert!(variants.construction(equals, StretchAxis::Horizontal).is_none(), "'=' is not horizontally covered");

    // The arrow publishes no prepared variant: its construction is an assembly
    // only, so a variant-driven extractor would miss it entirely.
    let arrow = facts.glyph_for_char('→').expect("the font maps `→`");
    let construction = variants.construction(arrow, StretchAxis::Horizontal).expect("'→' stretches sideways");
    assert!(construction.variants.is_empty(), "'→' publishes no prepared variant");
    assert_eq!(construction.assembly.as_ref().map(|assembly| assembly.parts.len()), Some(2), "'→' is a two part recipe");

    // The wide top brace has six prepared designs and a two-extender recipe.
    // XITS publishes no italic correction for these glyphs, and the extractor
    // must not invent one.
    let top_brace = facts.glyph_for_char('⏞').expect("the font maps U+23DE");
    let construction = variants.construction(top_brace, StretchAxis::Horizontal).expect("U+23DE stretches sideways");
    assert_eq!(construction.variants.len(), 6, "U+23DE publishes six prepared designs");
    assert!(
        construction.variants.iter().all(|variant| variant.italic_correction.is_none()),
        "U+23DE designs carry no italic correction"
    );

    // A horizontal stretch advances along the writing direction, so the
    // arrow's assembly is much longer than its single prepared design.
    match variants.stretch(arrow, StretchAxis::Horizontal, em(3.5)).expect("the arrow stretches to 3.5 em") {
        StretchPlan::Assembly(plan) => {
            assert_eq!(plan.instances.len(), 12, "'→' needs ten extra extenders at 3.5 em");
            close(plan.extent, 3.701, "assembled arrow extent");
        }
        other => panic!("expected an assembly, got {other:?}"),
    }
}

#[test]
fn a_size_the_font_does_not_publish_is_reported_as_missing() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();

    // `∑` publishes two prepared designs and no assembly, so 2 em is simply not
    // available: no synthetic scaling may stand in for it.
    let sum = facts.glyph_for_char('∑').expect("the font maps `∑`");
    let construction = variants.construction(sum, StretchAxis::Vertical).expect("'∑' has display versions");
    assert_eq!(construction.variants.len(), 2, "'∑' publishes two designs");
    assert!(construction.assembly.is_none(), "'∑' publishes no assembly");
    assert!(variants.stretch(sum, StretchAxis::Vertical, em(2.0)).is_none(), "'∑' cannot reach 2 em");
    assert!(variants.stretch(sum, StretchAxis::Vertical, em(1.0)).is_some(), "'∑' reaches 1 em");

    let x = facts.glyph_for_char('x').expect("the font maps `x`");
    assert!(variants.stretch(x, StretchAxis::Vertical, em(1.0)).is_none(), "an ordinary letter never stretches");
    assert!(facts.glyph_for_char('\u{10FFFF}').is_none(), "an unmapped character has no glyph id");
}

#[test]
fn vertical_integral_variants_keep_their_math_italic_correction() {
    let facts = MathFontFacts::embedded().expect("MATH table");
    let variants = facts.variants();
    let integral = facts.glyph_for_char('∫').expect("the font maps `∫`");
    let construction = variants.construction(integral, StretchAxis::Vertical).expect("'∫' has display versions");
    assert_eq!(construction.variants.len(), 2, "'∫' publishes two designs");
    close(construction.variants[0].italic_correction.expect("base italic correction"), 0.18, "base '∫' italic correction");
    close(
        construction.variants[1].italic_correction.expect("display italic correction"),
        0.55,
        "display '∫' italic correction",
    );
}
