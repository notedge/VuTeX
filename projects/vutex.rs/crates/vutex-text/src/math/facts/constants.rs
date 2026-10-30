//! The numeric `MathConstants` of the `MATH` table, with their provenance.
//!
//! Every layout decision in a formula — how far a superscript rises, how much
//! room an accent needs, how deep a radical degree sits — is expressed as one of
//! these constants. The font is the truth source for them, and a font is allowed
//! to publish a value for all of them.
//!
//! A caller still needs to lay out a formula when the font is missing, so this
//! module also answers a synthetic fallback. The difference between the two is
//! never hidden: [`MathConstantValue`] carries the provenance in its variant, so
//! a measured value and a fallback cannot be confused at the call site.

use acorn_ttf::tables::math::MathConstants as TableConstants;

use vutex_types::math::{Fixed, MathConstant};

use super::super::font_metrics::{ALL_SYNTHETIC_CONSTANTS, default_math_constant};

/// One resolved math constant.
///
/// The variants are the provenance: a [`MathConstantValue::FontTable`] number is
/// what the font measured, a [`MathConstantValue::SyntheticFallback`] number is
/// the crate's own stand-in, and [`MathConstantValue::Unavailable`] means there is
/// no number at all — for example a ratio only a real `MATH` table can provide.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MathConstantValue {
    /// The value the font's `MathConstants` table publishes, in em.
    FontTable(Fixed),
    /// A synthetic stand-in used because the font publishes nothing, in em.
    SyntheticFallback(Fixed),
    /// No value: the font publishes nothing and no fallback is defined.
    Unavailable,
}

impl MathConstantValue {
    /// The number, whichever source it came from.
    pub fn value(self) -> Option<Fixed> {
        match self {
            Self::FontTable(value) | Self::SyntheticFallback(value) => Some(value),
            Self::Unavailable => None,
        }
    }

    /// The number only if the font itself published it.
    pub fn font_value(self) -> Option<Fixed> {
        match self {
            Self::FontTable(value) => Some(value),
            Self::SyntheticFallback(_) | Self::Unavailable => None,
        }
    }

    /// Whether the number came from the font's table.
    pub fn is_from_font(self) -> bool {
        matches!(self, Self::FontTable(_))
    }

    /// Whether the number is a synthetic fallback.
    pub fn is_synthetic(self) -> bool {
        matches!(self, Self::SyntheticFallback(_))
    }
}

/// The constants one font publishes, in the specification's order.
///
/// This is a value object: it is built once from a parsed `MATH` table and
/// passed on by reference. An empty view is the font that publishes nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MathConstants {
    values: Vec<(MathConstant, Fixed)>,
}

impl MathConstants {
    /// Read every constant out of a parsed `MathConstants` table.
    ///
    /// `ScriptPercentScaleDown`, `ScriptScriptPercentScaleDown` and
    /// `RadicalDegreeBottomRaisePercent` are percentages in the table, so they
    /// are normalized to a fraction of the em like the rest of the values here.
    /// `DelimitedSubFormulaMinHeight` and `DisplayOperatorMinHeight` are plain
    /// design units, which is the same division by the em.
    pub(crate) fn from_table(table: &TableConstants, one_em: f64) -> Self {
        let units = |value: f32| Fixed::from_f64(value as f64 / one_em);
        let percent = |value: f32| Fixed::from_f64(value as f64 / 100.0);

        let values = vec![
            (MathConstant::ScriptPercentScaleDown, percent(table.script_percent_scale_down)),
            (MathConstant::ScriptScriptPercentScaleDown, percent(table.script_script_percent_scale_down)),
            (MathConstant::DelimitedSubFormulaMinHeight, units(table.delimited_sub_formula_min_height)),
            (MathConstant::DisplayOperatorMinHeight, units(table.display_operator_min_height)),
            (MathConstant::MathLeading, units(table.math_leading)),
            (MathConstant::AxisHeight, units(table.axis_height)),
            (MathConstant::AccentBaseHeight, units(table.accent_base_height)),
            (MathConstant::FlattenedAccentBaseHeight, units(table.flattened_accent_base_height)),
            (MathConstant::SubscriptShiftDown, units(table.subscript_shift_down)),
            (MathConstant::SubscriptTopMax, units(table.subscript_top_max)),
            (MathConstant::SubscriptBaselineDropMin, units(table.subscript_baseline_drop_min)),
            (MathConstant::SuperscriptShiftUp, units(table.superscript_shift_up)),
            (MathConstant::SuperscriptShiftUpCramped, units(table.superscript_shift_up_cramped)),
            (MathConstant::SuperscriptBottomMin, units(table.superscript_bottom_min)),
            (MathConstant::SuperscriptBaselineDropMax, units(table.superscript_baseline_drop_max)),
            (MathConstant::SubSuperscriptGapMin, units(table.sub_superscript_gap_min)),
            (MathConstant::SuperscriptBottomMaxWithSubscript, units(table.superscript_bottom_max_with_subscript)),
            (MathConstant::SpaceAfterScript, units(table.space_after_script)),
            (MathConstant::UpperLimitGapMin, units(table.upper_limit_gap_min)),
            (MathConstant::UpperLimitBaselineRiseMin, units(table.upper_limit_baseline_rise_min)),
            (MathConstant::LowerLimitGapMin, units(table.lower_limit_gap_min)),
            (MathConstant::LowerLimitBaselineDropMin, units(table.lower_limit_baseline_drop_min)),
            (MathConstant::StackTopShiftUp, units(table.stack_top_shift_up)),
            (MathConstant::StackTopDisplayStyleShiftUp, units(table.stack_top_display_style_shift_up)),
            (MathConstant::StackBottomShiftDown, units(table.stack_bottom_shift_down)),
            (MathConstant::StackBottomDisplayStyleShiftDown, units(table.stack_bottom_display_style_shift_down)),
            (MathConstant::StackGapMin, units(table.stack_gap_min)),
            (MathConstant::StackDisplayStyleGapMin, units(table.stack_display_style_gap_min)),
            (MathConstant::StretchStackTopShiftUp, units(table.stretch_stack_top_shift_up)),
            (MathConstant::StretchStackBottomShiftDown, units(table.stretch_stack_bottom_shift_down)),
            (MathConstant::StretchStackGapAboveMin, units(table.stretch_stack_gap_above_min)),
            (MathConstant::StretchStackGapBelowMin, units(table.stretch_stack_gap_below_min)),
            (MathConstant::FractionNumeratorShiftUp, units(table.fraction_numerator_shift_up)),
            (MathConstant::FractionNumeratorDisplayStyleShiftUp, units(table.fraction_numerator_display_style_shift_up)),
            (MathConstant::FractionDenominatorShiftDown, units(table.fraction_denominator_shift_down)),
            (
                MathConstant::FractionDenominatorDisplayStyleShiftDown,
                units(table.fraction_denominator_display_style_shift_down),
            ),
            (MathConstant::FractionNumeratorGapMin, units(table.fraction_numerator_gap_min)),
            (MathConstant::FractionNumDisplayStyleGapMin, units(table.fraction_num_display_style_gap_min)),
            (MathConstant::FractionRuleThickness, units(table.fraction_rule_thickness)),
            (MathConstant::FractionDenominatorGapMin, units(table.fraction_denominator_gap_min)),
            (MathConstant::FractionDenomDisplayStyleGapMin, units(table.fraction_denom_display_style_gap_min)),
            (MathConstant::SkewedFractionHorizontalGap, units(table.skewed_fraction_horizontal_gap)),
            (MathConstant::SkewedFractionVerticalGap, units(table.skewed_fraction_vertical_gap)),
            (MathConstant::OverbarVerticalGap, units(table.overbar_vertical_gap)),
            (MathConstant::OverbarRuleThickness, units(table.overbar_rule_thickness)),
            (MathConstant::OverbarExtraAscender, units(table.overbar_extra_ascender)),
            (MathConstant::UnderbarVerticalGap, units(table.underbar_vertical_gap)),
            (MathConstant::UnderbarRuleThickness, units(table.underbar_rule_thickness)),
            (MathConstant::UnderbarExtraDescender, units(table.underbar_extra_descender)),
            (MathConstant::RadicalVerticalGap, units(table.radical_vertical_gap)),
            (MathConstant::RadicalDisplayStyleVerticalGap, units(table.radical_display_style_vertical_gap)),
            (MathConstant::RadicalRuleThickness, units(table.radical_rule_thickness)),
            (MathConstant::RadicalExtraAscender, units(table.radical_extra_ascender)),
            (MathConstant::RadicalKernBeforeDegree, units(table.radical_kern_before_degree)),
            (MathConstant::RadicalKernAfterDegree, units(table.radical_kern_after_degree)),
            (MathConstant::RadicalDegreeBottomRaisePercent, percent(table.radical_degree_bottom_raise_percent)),
        ];

        Self { values }
    }

    /// Every constant the font publishes, in the specification's order.
    pub fn font_constants(&self) -> impl Iterator<Item = (MathConstant, Fixed)> + '_ {
        self.values.iter().copied()
    }

    /// The value the font publishes for `constant`, without any fallback.
    pub fn font_value(&self, constant: MathConstant) -> Option<Fixed> {
        self.values.iter().find(|(candidate, _)| *candidate == constant).map(|(_, value)| *value)
    }

    /// The value to lay out with, and where it came from.
    ///
    /// Answers [`MathConstantValue::FontTable`] when the font publishes the
    /// constant, [`MathConstantValue::SyntheticFallback`] when the crate defines
    /// a stand-in for it, and [`MathConstantValue::Unavailable`] otherwise.
    pub fn resolve(&self, constant: MathConstant) -> MathConstantValue {
        if let Some(value) = self.font_value(constant) {
            return MathConstantValue::FontTable(value);
        }
        if ALL_SYNTHETIC_CONSTANTS.contains(&constant) {
            return MathConstantValue::SyntheticFallback(default_math_constant(constant));
        }
        MathConstantValue::Unavailable
    }

    /// `UpperLimitGapMin`: minimum gap between an upper limit and its base.
    pub fn upper_limit_gap_min(&self) -> MathConstantValue {
        self.resolve(MathConstant::UpperLimitGapMin)
    }

    /// `UpperLimitBaselineRiseMin`: minimum rise of an upper limit's baseline.
    pub fn upper_limit_baseline_rise_min(&self) -> MathConstantValue {
        self.resolve(MathConstant::UpperLimitBaselineRiseMin)
    }

    /// `LowerLimitGapMin`: minimum gap between a lower limit and its base.
    pub fn lower_limit_gap_min(&self) -> MathConstantValue {
        self.resolve(MathConstant::LowerLimitGapMin)
    }

    /// `LowerLimitBaselineDropMin`: minimum drop of a lower limit's baseline.
    pub fn lower_limit_baseline_drop_min(&self) -> MathConstantValue {
        self.resolve(MathConstant::LowerLimitBaselineDropMin)
    }

    /// `AccentBaseHeight`: height of the tallest base an accent may sit on flat.
    pub fn accent_base_height(&self) -> MathConstantValue {
        self.resolve(MathConstant::AccentBaseHeight)
    }

    /// `FlattenedAccentBaseHeight`: base height above which an accent flattens.
    pub fn flattened_accent_base_height(&self) -> MathConstantValue {
        self.resolve(MathConstant::FlattenedAccentBaseHeight)
    }

    /// `RadicalDegreeBottomRaisePercent`: raise of the degree, as a fraction of
    /// the radical's height.
    pub fn radical_degree_bottom_raise_percent(&self) -> MathConstantValue {
        self.resolve(MathConstant::RadicalDegreeBottomRaisePercent)
    }

    /// `RadicalKernAfterDegree`: kern between the degree and the radical sign.
    pub fn radical_kern_after_degree(&self) -> MathConstantValue {
        self.resolve(MathConstant::RadicalKernAfterDegree)
    }

    /// `SuperscriptShiftUpCramped`: superscript rise in a cramped style.
    pub fn superscript_shift_up_cramped(&self) -> MathConstantValue {
        self.resolve(MathConstant::SuperscriptShiftUpCramped)
    }

    /// `SpaceAfterScript`: space inserted after a script.
    pub fn space_after_script(&self) -> MathConstantValue {
        self.resolve(MathConstant::SpaceAfterScript)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `TableConstants` by writing the same value into every field.
    fn table_with(value: f32) -> TableConstants {
        TableConstants {
            script_percent_scale_down: value,
            script_script_percent_scale_down: value,
            delimited_sub_formula_min_height: value,
            display_operator_min_height: value,
            math_leading: value,
            axis_height: value,
            accent_base_height: value,
            flattened_accent_base_height: value,
            subscript_shift_down: value,
            subscript_top_max: value,
            subscript_baseline_drop_min: value,
            superscript_shift_up: value,
            superscript_shift_up_cramped: value,
            superscript_bottom_min: value,
            superscript_baseline_drop_max: value,
            sub_superscript_gap_min: value,
            superscript_bottom_max_with_subscript: value,
            space_after_script: value,
            upper_limit_gap_min: value,
            upper_limit_baseline_rise_min: value,
            lower_limit_gap_min: value,
            lower_limit_baseline_drop_min: value,
            stack_top_shift_up: value,
            stack_top_display_style_shift_up: value,
            stack_bottom_shift_down: value,
            stack_bottom_display_style_shift_down: value,
            stack_gap_min: value,
            stack_display_style_gap_min: value,
            stretch_stack_top_shift_up: value,
            stretch_stack_bottom_shift_down: value,
            stretch_stack_gap_above_min: value,
            stretch_stack_gap_below_min: value,
            fraction_numerator_shift_up: value,
            fraction_numerator_display_style_shift_up: value,
            fraction_denominator_shift_down: value,
            fraction_denominator_display_style_shift_down: value,
            fraction_numerator_gap_min: value,
            fraction_num_display_style_gap_min: value,
            fraction_rule_thickness: value,
            fraction_denominator_gap_min: value,
            fraction_denom_display_style_gap_min: value,
            skewed_fraction_horizontal_gap: value,
            skewed_fraction_vertical_gap: value,
            overbar_vertical_gap: value,
            overbar_rule_thickness: value,
            overbar_extra_ascender: value,
            underbar_vertical_gap: value,
            underbar_rule_thickness: value,
            underbar_extra_descender: value,
            radical_vertical_gap: value,
            radical_display_style_vertical_gap: value,
            radical_rule_thickness: value,
            radical_extra_ascender: value,
            radical_kern_before_degree: value,
            radical_kern_after_degree: value,
            radical_degree_bottom_raise_percent: value,
        }
    }

    #[test]
    fn a_view_without_a_table_marks_every_value_it_answers_as_synthetic() {
        let empty = MathConstants::default();
        let gap = empty.upper_limit_gap_min();
        assert!(gap.is_synthetic(), "a missing table must not answer an unmarked value");
        assert!(!gap.is_from_font());
        assert_eq!(gap.font_value(), None, "no font value exists behind a fallback");
        assert_eq!(
            gap.value(),
            Some(default_math_constant(MathConstant::UpperLimitGapMin)),
            "the fallback comes from the one synthetic table"
        );
    }

    #[test]
    fn a_constant_with_no_fallback_is_unavailable_rather_than_zero() {
        let empty = MathConstants::default();
        // The script scale ratios only exist when a font publishes them, so a
        // missing table leaves the caller with nothing instead of a fake 1.0.
        let ratio = empty.resolve(MathConstant::ScriptPercentScaleDown);
        assert_eq!(ratio, MathConstantValue::Unavailable);
        assert_eq!(ratio.value(), None);
    }

    #[test]
    fn a_table_value_is_reported_as_the_font_measurement() {
        let view = MathConstants::from_table(&table_with(500.0), 1000.0);
        assert_eq!(view.accent_base_height(), MathConstantValue::FontTable(Fixed::from_f64(0.5)));
        assert!(view.accent_base_height().is_from_font());
        assert!(!view.accent_base_height().is_synthetic());
    }

    #[test]
    fn percentage_constants_are_normalized_to_a_fraction() {
        let view = MathConstants::from_table(&table_with(60.0), 1000.0);
        assert_eq!(
            view.radical_degree_bottom_raise_percent(),
            MathConstantValue::FontTable(Fixed::from_f64(0.6)),
            "a percentage constant must arrive as a fraction of the radical height"
        );
        assert_eq!(view.resolve(MathConstant::ScriptPercentScaleDown), MathConstantValue::FontTable(Fixed::from_f64(0.6)));
    }

    #[test]
    fn every_published_constant_is_present_in_the_view() {
        let view = MathConstants::from_table(&table_with(1.0), 1000.0);
        assert_eq!(view.font_constants().count(), 56, "the table publishes 56 constants");
        for (constant, _) in view.font_constants() {
            assert!(view.font_value(constant).is_some(), "`{constant:?}` must be readable back");
        }
    }
}
