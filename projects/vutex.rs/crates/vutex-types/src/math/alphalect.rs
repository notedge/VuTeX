//! Math alphabet-to-glyph resolution.
//!
//! TeX math does not typeset a variable with the upright letter the source
//! spells: `x` is the math-italic glyph, `\mathbb{R}` is the double-struck
//! one, and so on. OpenType MATH fonts ship those faces as the Mathematical
//! Alphanumeric Symbols block plus the Letterlike Symbols that Unicode
//! reserves for the letters the block leaves as gaps (italic `h`, the script
//! capitals `B E F H I L M R`, and friends).
//!
//! This module is that contract: authored character + [`FontStyle`] in,
//! codepoint in, or `None` when the style carries no dedicated face for the
//! character (digits, operators, Greek capitals — TeX sets those upright).
//! The font still has the final say: callers must fall back to the authored
//! character when the resolved face does not ship the returned codepoint.

use super::FontStyle;

/// The Mathematical Alphanumeric Symbols codepoint carrying `c` in `style`.
///
/// `Math` and `Italic` both resolve to the math-italic alphabet (TeX's
/// default variable face). `Bold` resolves to the upright bold alphabet,
/// matching LaTeX's `\mathbf`. Greek letters italicize only for `Math` and
/// `Italic`; capitals stay upright everywhere, as in TeX.
pub fn math_alphalect(style: FontStyle, c: char) -> Option<char> {
    match style {
        FontStyle::Math | FontStyle::Italic => italic(c),
        FontStyle::Bold => latin_block(c, 0x1D400, 0x1D41A),
        FontStyle::SansSerif => latin_block(c, 0x1D5A0, 0x1D5BA),
        FontStyle::Monospace => latin_block(c, 0x1D670, 0x1D68A),
        FontStyle::Calligraphic => script_capital(c),
        FontStyle::DoubleStruck => double_struck(c),
        FontStyle::Fraktur => fraktur(c),
        FontStyle::Normal | FontStyle::Strike => None,
    }
}

/// Every letter this contract can redirect, for the font-facts extraction.
pub fn alphalect_sources() -> &'static [char] {
    &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x',
        'y', 'z', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S', 'T', 'U', 'V',
        'W', 'X', 'Y', 'Z', 'α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ', 'λ', 'μ', 'ν', 'ξ', 'ο', 'π', 'ρ', 'ς', 'σ', 'τ',
        'υ', 'φ', 'χ', 'ψ', 'ω', 'ϵ', 'ϑ', 'ϖ', 'ϱ',
    ]
}

/// The styles whose letters can carry a dedicated face.
pub fn alphalect_styles() -> &'static [FontStyle] {
    &[
        FontStyle::Math,
        FontStyle::Italic,
        FontStyle::Bold,
        FontStyle::SansSerif,
        FontStyle::Monospace,
        FontStyle::Calligraphic,
        FontStyle::DoubleStruck,
        FontStyle::Fraktur,
    ]
}

fn cp(base: u32, index: u32) -> Option<char> {
    char::from_u32(base + index)
}

/// Latin letters onto a pair of contiguous 26-codepoint blocks.
fn latin_block(c: char, upper_base: u32, lower_base: u32) -> Option<char> {
    match c {
        'A'..='Z' => cp(upper_base, (c as u32) - ('A' as u32)),
        'a'..='z' => cp(lower_base, (c as u32) - ('a' as u32)),
        _ => None,
    }
}

/// Math italic: the default TeX variable face, Latin and Greek.
///
/// The lowercase block skips `h`, `i` and `j` (Unicode reserves those slots
/// for the letterlike look-alikes). The fonts this kernel ships carry italic
/// `i` and `j` at the unassigned block positions and italic `h` as U+210E,
/// the same substitution unicode-math documents for these faces.
fn italic(c: char) -> Option<char> {
    match c {
        'A'..='Z' => cp(0x1D434, (c as u32) - ('A' as u32)),
        'a'..='g' => cp(0x1D44E, (c as u32) - ('a' as u32)),
        'h' => Some('\u{210E}'),
        'i' => cp(0x1D456, 0),
        'j' => cp(0x1D457, 0),
        'k'..='z' => cp(0x1D458, (c as u32) - ('k' as u32)),
        'α' => cp(0x1D6FC, 0),
        'β' => cp(0x1D6FC, 1),
        'γ' => cp(0x1D6FC, 2),
        'δ' => cp(0x1D6FC, 3),
        'ε' => cp(0x1D6FC, 4),
        'ζ' => cp(0x1D6FC, 5),
        'η' => cp(0x1D6FC, 6),
        'θ' => cp(0x1D6FC, 7),
        'ι' => cp(0x1D6FC, 8),
        'κ' => cp(0x1D6FC, 9),
        'λ' => cp(0x1D6FC, 10),
        'μ' => cp(0x1D6FC, 11),
        'ν' => cp(0x1D6FC, 12),
        'ξ' => cp(0x1D6FC, 13),
        'ο' => cp(0x1D6FC, 14),
        'π' => cp(0x1D6FC, 15),
        'ρ' => cp(0x1D6FC, 16),
        'ς' => cp(0x1D71B, 0),
        'σ' => cp(0x1D6FC, 17),
        'τ' => cp(0x1D6FC, 18),
        'υ' => cp(0x1D6FC, 19),
        'φ' => cp(0x1D6FC, 20),
        'χ' => cp(0x1D6FC, 21),
        'ψ' => cp(0x1D6FC, 22),
        'ω' => cp(0x1D6FC, 23),
        'ϵ' => cp(0x1D716, 0),
        'ϑ' => cp(0x1D717, 0),
        'ϱ' => cp(0x1D71A, 0),
        'ϖ' => cp(0x1D71C, 0),
        _ => None,
    }
}

/// Calligraphic capitals: the script block, with the eight letterlike
/// capitals Unicode leaves out of it (`B E F H I L M R`).
fn script_capital(c: char) -> Option<char> {
    let letterlike = match c {
        'B' => return Some('\u{212C}'),
        'E' => return Some('\u{2130}'),
        'F' => return Some('\u{2131}'),
        'H' => return Some('\u{210B}'),
        'I' => return Some('\u{2110}'),
        'L' => return Some('\u{2112}'),
        'M' => return Some('\u{2133}'),
        'R' => return Some('\u{211B}'),
        'A'..='Z' => 0,
        _ => return None,
    };
    cp(0x1D49C, (c as u32) - ('A' as u32) + letterlike)
}

/// Double-struck letters: the block plus the seven letterlike capitals
/// (`C H N P Q R Z`) it skips. TeX's `\mathbb` is a capitals-only face;
/// the block does assign lowercase, and the shipped fonts carry it.
fn double_struck(c: char) -> Option<char> {
    match c {
        'C' => return Some('\u{2102}'),
        'H' => return Some('\u{210D}'),
        'N' => return Some('\u{2115}'),
        'P' => return Some('\u{2119}'),
        'Q' => return Some('\u{211A}'),
        'R' => return Some('\u{211D}'),
        'Z' => return Some('\u{2124}'),
        'A'..='Z' => {}
        'a'..='z' => return cp(0x1D552, (c as u32) - ('a' as u32)),
        _ => return None,
    }
    cp(0x1D538, (c as u32) - ('A' as u32))
}

/// Fraktur letters: the blocks plus the five letterlike capitals
/// (`C H I R Z`) the uppercase one skips.
fn fraktur(c: char) -> Option<char> {
    match c {
        'C' => return Some('\u{212D}'),
        'H' => return Some('\u{210C}'),
        'I' => return Some('\u{2111}'),
        'R' => return Some('\u{211C}'),
        'Z' => return Some('\u{2128}'),
        'A'..='Z' => {}
        'a'..='z' => return cp(0x1D51E, (c as u32) - ('a' as u32)),
        _ => return None,
    }
    cp(0x1D504, (c as u32) - ('A' as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_variables_resolve_to_italic() {
        assert_eq!(math_alphalect(FontStyle::Math, 'x'), Some('\u{1D465}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'e'), Some('\u{1D452}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'A'), Some('\u{1D434}'));
        assert_eq!(math_alphalect(FontStyle::Italic, 'z'), Some('\u{1D467}'));
    }

    #[test]
    fn italic_gaps_take_the_letterlike_stand_in() {
        assert_eq!(math_alphalect(FontStyle::Math, 'h'), Some('\u{210E}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'i'), Some('\u{1D456}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'j'), Some('\u{1D457}'));
    }

    #[test]
    fn greek_italicizes_lowercase_only() {
        assert_eq!(math_alphalect(FontStyle::Math, 'α'), Some('\u{1D6FC}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'ω'), Some('\u{1D713}'));
        assert_eq!(math_alphalect(FontStyle::Math, 'Γ'), None);
        assert_eq!(math_alphalect(FontStyle::Math, 'ϵ'), Some('\u{1D716}'));
    }

    #[test]
    fn non_letters_never_redirect() {
        for style in alphalect_styles() {
            for c in ['0', '9', '+', '=', '(', '.', ' '] {
                assert_eq!(math_alphalect(*style, c), None, "{style:?} {c:?}");
            }
        }
    }

    #[test]
    fn upright_styles_keep_the_authored_glyph() {
        assert_eq!(math_alphalect(FontStyle::Normal, 'x'), None);
        assert_eq!(math_alphalect(FontStyle::Strike, 'x'), None);
    }

    #[test]
    fn faces_land_in_their_own_blocks() {
        assert_eq!(math_alphalect(FontStyle::Bold, 'A'), Some('\u{1D400}'));
        assert_eq!(math_alphalect(FontStyle::SansSerif, 'a'), Some('\u{1D5BA}'));
        assert_eq!(math_alphalect(FontStyle::Monospace, 'z'), Some('\u{1D6A3}'));
        assert_eq!(math_alphalect(FontStyle::Calligraphic, 'L'), Some('\u{2112}'));
        assert_eq!(math_alphalect(FontStyle::Calligraphic, 'a'), None);
        assert_eq!(math_alphalect(FontStyle::DoubleStruck, 'R'), Some('\u{211D}'));
        assert_eq!(math_alphalect(FontStyle::Fraktur, 'g'), Some('\u{1D524}'));
    }

    #[test]
    fn every_source_resolves_somewhere_under_math() {
        for c in alphalect_sources() {
            assert!(math_alphalect(FontStyle::Math, *c).is_some(), "{c:?}");
        }
    }
}
