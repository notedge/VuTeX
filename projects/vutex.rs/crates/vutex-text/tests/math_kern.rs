//! The glyph-level `MathGlyphInfo` facts as the embedded math font publishes them.
//!
//! Latin Modern Math is the reference: it ships italic corrections, top accent
//! attachments and an extended shape coverage, and — as a TeX-derived design —
//! publishes no four-corner `MathKern` subtable at all. The step semantics of a
//! corner kern are exercised against synthetic tables in the crate's unit
//! tests; here the point is the font facts and how an absence is expressed.

use vutex_text::math::{GlyphId, KernCorner, MathFontFacts};
use vutex_types::math::Fixed;

/// Absolute tolerance for an em-relative font fact, generous enough for the
/// 1/65536 `Fixed` grid and the font's own rounding.
const EM_TOLERANCE: f64 = 0.002;

fn facts() -> MathFontFacts {
    MathFontFacts::embedded().expect("the embedded font must carry a MATH table")
}

fn glyph(facts: &MathFontFacts, ch: char) -> GlyphId {
    facts.glyph_for_char(ch).unwrap_or_else(|| panic!("the font must map `{ch}`"))
}

fn close(actual: Option<Fixed>, expected: f64, what: &str) {
    let actual = actual.unwrap_or_else(|| panic!("the font must publish `{what}`")).to_f64();
    assert!((actual - expected).abs() < EM_TOLERANCE, "`{what}` is {actual}, expected about {expected}");
}

#[test]
fn italic_correction_is_read_from_the_fonts_glyph_info() {
    let facts = facts();
    // 'f' is the classic slanted letter: the correction is what keeps its
    // overhanging top from crashing into the following script.
    close(facts.italic_correction(glyph(&facts, 'f')), 0.079, "'f' italic correction");
    close(facts.italic_correction(glyph(&facts, 'x')), 0.016, "'x' italic correction");
    close(facts.italic_correction(glyph(&facts, '[')), 0.006, "'[' italic correction");

    // A parenthesis carries no italic correction: the value is absent, which is
    // a different answer from "zero".
    assert_eq!(
        facts.italic_correction(glyph(&facts, '(')),
        None,
        "'(' must stay unmeasured rather than inventing a correction"
    );
    assert_eq!(facts.glyph_math(glyph(&facts, '(')), None, "a glyph outside the coverage has no math facts");
}

#[test]
fn top_accent_attachment_is_published_for_the_glyphs_an_accent_lands_on() {
    let facts = facts();
    // A capital is the widest base an accent has to centre over.
    close(facts.top_accent_attachment(glyph(&facts, 'A')), 0.375, "'A' top accent attachment");
    close(facts.top_accent_attachment(glyph(&facts, '1')), 0.257, "'1' top accent attachment");
    close(facts.top_accent_attachment(glyph(&facts, 'j')), 0.156, "'j' top accent attachment");

    // Extended shapes stretch to whatever the formula needs, so the font names
    // no single attachment point for them.
    assert_eq!(facts.top_accent_attachment(glyph(&facts, '∫')), None, "'∫' must not claim an accent attachment");
}

#[test]
fn the_extended_shape_coverage_comes_from_the_font() {
    let facts = facts();
    let shapes = facts.extended_shapes();
    assert_eq!(shapes.len(), 250, "the embedded font marks 250 extended shapes");

    // The coverage is handed out in glyph order with duplicates removed, which
    // is what makes the membership test a binary search.
    assert!(shapes.windows(2).all(|w| w[0] < w[1]), "extended shapes must be strictly ascending");
    assert!(shapes.iter().all(|&shape| facts.is_extended_shape(shape)), "every listed shape must answer true");

    // Big operators and their integral forms are the archetypal extended
    // shapes: an accent over them has to grow instead of sitting at a fixed
    // height. Ordinary delimiters are not extended shapes.
    for ch in ['∑', '∏', '∫', '∬'] {
        assert!(facts.is_extended_shape(glyph(&facts, ch)), "`{ch}` is a tall operator and must be an extended shape");
    }
    for ch in ['(', ')', 'A', 'x'] {
        assert!(!facts.is_extended_shape(glyph(&facts, ch)), "`{ch}` is not an extended shape");
    }
}

#[test]
fn the_embedded_font_publishes_no_corner_kern_and_the_absence_is_visible() {
    let facts = facts();
    let kerns = facts.kerns();
    assert_eq!(kerns.glyphs().count(), 0, "Latin Modern Math carries no four-corner `MathKern` subtable");

    // Absence is reported as absence, not as a zero kern: a caller that wants
    // to know whether the font measured the corner can tell the two apart.
    for ch in ['f', '(', 'A', '∫'] {
        let glyph = glyph(&facts, ch);
        for corner in KernCorner::ALL {
            assert!(kerns.corner(glyph, corner).is_none(), "`{ch}` must publish no {corner:?} kern");
            assert_eq!(kerns.math_kern(glyph, corner, Fixed::from_f64(0.5)), Fixed::ZERO);
        }
        assert!(kerns.corners(glyph).is_none(), "`{ch}` must have no kern record at all");
    }
}
