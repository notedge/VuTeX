//! Math kerning — TeX's MuGlue supplement to Appendix G spacing.
//!
//! Every glyph in a MATH font defines four corner kerns (bottom-right,
//! top-right, bottom-left, top-left).  When two adjacent atoms are spaced,
//! TeX adds the preceding glyph's bottom-right kern and the following
//! glyph's top-left kern on top of the Appendix G glue.

use serde::{Deserialize, Serialize};

use super::semantic::Fixed;

/// Four-corner kerning values (in mu).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct MathKern {
    pub bottom_right: Fixed,
    pub top_right: Fixed,
    pub bottom_left: Fixed,
    pub top_left: Fixed,
}

impl MathKern {
    pub const ZERO: Self = Self {
        bottom_right: Fixed::ZERO,
        top_right: Fixed::ZERO,
        bottom_left: Fixed::ZERO,
        top_left: Fixed::ZERO,
    };

    /// Combined kern between two adjacent noads.
    ///
    /// TeX adds the preceding glyph's bottom-right kern and the following
    /// glyph's top-left kern on top of the Appendix G glue.
    pub fn combined(left: &Self, right: &Self) -> Fixed {
        left.bottom_right + right.top_left
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_combined_with_zero_is_zero() {
        assert_eq!(MathKern::combined(&MathKern::ZERO, &MathKern::ZERO), Fixed::ZERO);
    }

    #[test]
    fn combined_picks_left_bottom_right_and_right_top_left() {
        let left = MathKern {
            bottom_right: Fixed::from_f64(0.05),
            top_right: Fixed::from_f64(99.0),
            bottom_left: Fixed::from_f64(99.0),
            top_left: Fixed::from_f64(99.0),
        };
        let right = MathKern {
            bottom_right: Fixed::from_f64(99.0),
            top_right: Fixed::from_f64(99.0),
            bottom_left: Fixed::from_f64(99.0),
            top_left: Fixed::from_f64(0.03),
        };
        let combined = MathKern::combined(&left, &right).to_f64();
        assert!((combined - 0.08).abs() < 1e-3, "expected ~0.08, got {combined}");
    }

    #[test]
    fn combined_ignores_other_corners() {
        let left = MathKern {
            bottom_right: Fixed::ZERO,
            top_right: Fixed::from_f64(1.0),
            bottom_left: Fixed::from_f64(1.0),
            top_left: Fixed::from_f64(1.0),
        };
        let right = MathKern {
            bottom_right: Fixed::from_f64(1.0),
            top_right: Fixed::from_f64(1.0),
            bottom_left: Fixed::from_f64(1.0),
            top_left: Fixed::ZERO,
        };
        assert_eq!(MathKern::combined(&left, &right), Fixed::ZERO);
    }

    #[test]
    fn default_is_zero() {
        let k = MathKern::default();
        assert_eq!(MathKern::combined(&k, &MathKern::ZERO), Fixed::ZERO);
    }
}
