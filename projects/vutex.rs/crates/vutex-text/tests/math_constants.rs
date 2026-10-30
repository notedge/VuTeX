//! The `MathConstants` facts as the embedded math font publishes them.
//!
//! Every value here is checked against XITS Math, and every value also
//! carries where it came from: a number the font measured is
//! `MathConstantValue::FontTable`, while a number this crate invented because the
//! font said nothing is `MathConstantValue::SyntheticFallback`. A caller can
//! therefore audit a layout that fell back without reading the font itself.

use vutex_text::math::{MathConstantValue, MathConstants, MathFontFacts};
use vutex_types::math::MathConstant;

/// Absolute tolerance for an em-relative constant, covering the 1/65536 `Fixed`
/// grid plus the font's own rounding.
const EM_TOLERANCE: f64 = 0.002;

fn facts() -> MathFontFacts {
    MathFontFacts::embedded().expect("the embedded font must carry a MATH table")
}

fn close(value: MathConstantValue, expected: f64, what: &str) {
    assert!(value.is_from_font(), "`{what}` must be measured by the font, not a fallback: {value:?}");
    let actual = value.value().unwrap_or_else(|| panic!("`{what}` must answer a number")).to_f64();
    assert!((actual - expected).abs() < EM_TOLERANCE, "`{what}` is {actual}, expected about {expected}");
}

#[test]
fn the_font_publishes_the_limit_constants() {
    let facts = facts();
    let constants = facts.constants();
    close(constants.upper_limit_gap_min(), 0.15, "UpperLimitGapMin");
    close(constants.upper_limit_baseline_rise_min(), 0.3, "UpperLimitBaselineRiseMin");
    close(constants.lower_limit_gap_min(), 0.15, "LowerLimitGapMin");
    close(constants.lower_limit_baseline_drop_min(), 0.6, "LowerLimitBaselineDropMin");
}

#[test]
fn the_font_publishes_the_accent_constants() {
    let facts = facts();
    let constants = facts.constants();
    close(constants.accent_base_height(), 0.45, "AccentBaseHeight");
    close(constants.flattened_accent_base_height(), 0.662, "FlattenedAccentBaseHeight");
}

#[test]
fn the_font_publishes_the_radical_constants() {
    let facts = facts();
    let constants = facts.constants();
    // The degree raise is a percentage in the table and reaches us as a
    // fraction: 70% of the radical height.
    close(constants.radical_degree_bottom_raise_percent(), 0.7, "RadicalDegreeBottomRaisePercent");
    // The kern after a degree pulls the degree towards the radical sign.
    close(constants.radical_kern_after_degree(), -0.555, "RadicalKernAfterDegree");
}

#[test]
fn the_font_publishes_the_script_constants() {
    let facts = facts();
    let constants = facts.constants();
    close(constants.superscript_shift_up_cramped(), 0.275, "SuperscriptShiftUpCramped");
    close(constants.space_after_script(), 0.041, "SpaceAfterScript");
}

#[test]
fn every_constant_the_font_publishes_resolves_as_a_font_value() {
    let facts = facts();
    let constants = facts.constants();
    let published: Vec<_> = constants.font_constants().collect();
    assert_eq!(published.len(), 56, "the embedded font publishes all 56 `MathConstants`");
    for (constant, value) in published {
        assert_eq!(
            constants.resolve(constant),
            MathConstantValue::FontTable(value),
            "`{constant:?}` must resolve to the value the font publishes"
        );
    }
}

#[test]
fn a_font_that_publishes_nothing_marks_every_answer_as_a_synthetic_fallback() {
    let constants = MathConstants::default();
    for constant in [
        MathConstant::UpperLimitGapMin,
        MathConstant::UpperLimitBaselineRiseMin,
        MathConstant::LowerLimitGapMin,
        MathConstant::LowerLimitBaselineDropMin,
        MathConstant::AccentBaseHeight,
        MathConstant::FlattenedAccentBaseHeight,
        MathConstant::RadicalDegreeBottomRaisePercent,
        MathConstant::RadicalKernAfterDegree,
        MathConstant::SuperscriptShiftUpCramped,
        MathConstant::SpaceAfterScript,
    ] {
        let value = constants.resolve(constant);
        assert!(value.is_synthetic(), "`{constant:?}` must be marked as a fallback: {value:?}");
        assert_eq!(value.font_value(), None, "a fallback must never pass for a measured value");
        assert!(value.value().is_some(), "the fallback still gives the caller a number to lay out with");
    }

    // The script scale ratios have no synthetic story at all: a missing table
    // leaves them unanswered rather than inventing a ratio.
    assert_eq!(constants.resolve(MathConstant::ScriptPercentScaleDown), MathConstantValue::Unavailable);
    assert_eq!(constants.resolve(MathConstant::ScriptScriptPercentScaleDown), MathConstantValue::Unavailable);
    assert!(!constants.resolve(MathConstant::ScriptPercentScaleDown).is_synthetic());
}
