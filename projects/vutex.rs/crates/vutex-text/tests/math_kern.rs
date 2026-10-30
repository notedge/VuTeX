//! The glyph-level `MathGlyphInfo` facts as the embedded math font publishes them.
//!
//! XITS Math is the reference — pinned explicitly here, because the builtin
//! default family is a product choice that may move: it ships italic corrections
//! for its slanted letters, top accent attachments and an extended shape
//! coverage, and — unlike the TeX-derived Latin Modern design — a real
//! four-corner `MathKern` subtable. The step semantics of a corner kern are
//! exercised against synthetic tables in the crate's unit tests; here the point
//! is the font facts and how a measured value and an absence stay
//! distinguishable.

use vutex_text::math::{GlyphId, KernCorner, MathFontFamily, MathFontFacts};
use vutex_types::math::Fixed;

/// Absolute tolerance for an em-relative font fact, generous enough for the
/// 1/65536 `Fixed` grid and the font's own rounding.
const EM_TOLERANCE: f64 = 0.002;

fn facts() -> MathFontFacts {
    MathFontFacts::for_family(MathFontFamily::XitsMath).expect("XITS Math must carry a MATH table")
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
    close(facts.italic_correction(glyph(&facts, 'f')), 0.1, "'f' italic correction");

    // The correction coverage is selective: letters the font does not measure
    // stay unmeasured rather than being given an invented number.
    assert_eq!(
        facts.italic_correction(glyph(&facts, 'x')),
        None,
        "'x' must stay unmeasured rather than inventing a correction"
    );
    assert_eq!(facts.italic_correction(glyph(&facts, '[')), None, "'[' must stay unmeasured");

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
    close(facts.top_accent_attachment(glyph(&facts, 'A')), 0.361, "'A' top accent attachment");
    close(facts.top_accent_attachment(glyph(&facts, 'j')), 0.144, "'j' top accent attachment");
    // Digits sit outside the attachment coverage: absence, not zero.
    assert_eq!(facts.top_accent_attachment(glyph(&facts, '1')), None, "'1' must stay unmeasured");

    // Extended shapes stretch to whatever the formula needs, so the font names
    // no single attachment point for them.
    assert_eq!(facts.top_accent_attachment(glyph(&facts, '∫')), None, "'∫' must not claim an accent attachment");
}

#[test]
fn the_extended_shape_coverage_comes_from_the_font() {
    let facts = facts();
    let shapes = facts.extended_shapes();
    assert_eq!(shapes.len(), 293, "the embedded font marks 293 extended shapes");

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
fn the_embedded_font_publishes_corner_kerns_and_the_absence_is_still_visible() {
    let facts = facts();
    let kerns = facts.kerns();
    assert_eq!(kerns.glyphs().count(), 29, "XITS Math carries a four-corner `MathKern` subtable for 29 glyphs");

    // Slanted-sided capitals request clearance from a following script: the
    // top-right corner is measured, and the record exists at all.
    let a = glyph(&facts, 'A');
    assert!(kerns.corner(a, KernCorner::TopRight).is_some(), "`A` must publish a top-right kern");
    assert!(kerns.corners(a).is_some(), "`A` must have a kern record");

    // Absence is still reported as absence, not as a zero kern: a caller that
    // wants to know whether the font measured the corner can tell the two
    // apart even now the table exists.
    for ch in ['f', '(', '∫'] {
        let glyph = glyph(&facts, ch);
        for corner in KernCorner::ALL {
            assert!(kerns.corner(glyph, corner).is_none(), "`{ch}` must publish no {corner:?} kern");
            assert_eq!(kerns.math_kern(glyph, corner, Fixed::from_f64(0.5)), Fixed::ZERO);
        }
        assert!(kerns.corners(glyph).is_none(), "`{ch}` must have no kern record at all");
    }
}
