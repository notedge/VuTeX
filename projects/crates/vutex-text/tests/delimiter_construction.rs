//! Delimiter constructions must resolve to the font's own glyph data.
//!
//! Layout reads variant and assembly glyph keys straight out of the
//! construction it looks up, so every key it hands back has to resolve inside
//! the same font data the construction came from. A key that only matches
//! synthetic fallback metrics silently paints nothing.

use vutex_text::math::builtin_font_metrics;
use vutex_types::math::{FontStyle, GlyphKey};

const DELIMITERS: &[char] = &['(', ')', '[', ']', '{', '}', '|', '‖'];

#[test]
fn every_delimiter_glyph_has_its_own_metrics_and_outline() {
    let system = builtin_font_metrics();
    for &ch in DELIMITERS {
        let key = GlyphKey::from_char(ch, Some("default".to_string()), FontStyle::Math);
        let construction = system.get_delimiter_construction(&key).expect("construction lookup");
        assert!(!construction.variants.is_empty(), "{ch}: no prepared variants");

        for variant in &construction.variants {
            let metrics = system.get_metrics(&variant.glyph).expect("variant metrics");
            assert!(metrics.width.to_f64() > 0.1, "{ch}: variant {:?} has no advance", variant.glyph.glyph_id);
            assert!(system.get_glyph_path(&variant.glyph).is_ok(), "{ch}: variant {:?} has no outline", variant.glyph.glyph_id);
        }

        for component in &construction.components {
            assert!(
                system.get_glyph_path(&component.glyph).is_ok(),
                "{ch}: assembly part {:?} has no outline",
                component.glyph.glyph_id
            );
        }

        let tallest = construction
            .variants
            .iter()
            .map(|variant| {
                let metrics = system.get_metrics(&variant.glyph).expect("variant metrics");
                metrics.height.to_f64() + metrics.depth.to_f64()
            })
            .fold(0.0, f64::max);
        assert!(tallest > 1.4, "{ch}: the tallest prepared variant only spans {tallest} em");
    }
}
