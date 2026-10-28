//! TeX's operator classes and the `\limits` / `\nolimits` placement policy.
//!
//! TeX splits large operators into two families.  The `op` class (`\sum`,
//! `\prod`, `\coprod`) stacks its scripts above and below the operator in
//! display style, while the `intop` class (`\int`, `\iint`, `\oint`) keeps
//! them beside the operator unless `\limits` explicitly asks for stacking.
//! Named functions such as `\sin`, `\log` and `\lim` are operators for
//! spacing purposes but never stack, and every other atom is ordinary.
//!
//! The classification and the placement rule live here so the layout engine
//! never has to inspect a glyph character, and so the arithmetic that TeX
//! hard-codes in its operator dictionary has exactly one home.

use serde::{Deserialize, Serialize};

use super::semantic::{MathStyle, SemanticNode, SymbolRole};

/// TeX's class of an operator atom, used for script placement.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum OperatorClass {
    /// The `op` class: `\sum`, `\prod`, `\coprod`.
    Op,
    /// The `intop` class: `\int`, `\iint`, `\oint`.
    IntOp,
    /// An upright named function: `\sin`, `\log`, `\lim`.
    NamedOp,
    /// Any other atom, including variables, binary operators and relations.
    Ord,
}

/// How an operator places its sub- and superscripts.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LimitsPolicy {
    /// TeX's default for the operator class and the current math style.
    #[default]
    Auto,
    /// `\limits`: stack the scripts above and below the operator.
    Limits,
    /// `\nolimits`: keep the scripts beside the operator.
    NoLimits,
}

impl LimitsPolicy {
    /// The policy an authored `\limits` (`true`) or `\nolimits` (`false`)
    /// selects.
    pub const fn from_flag(flag: bool) -> Self {
        if flag { Self::Limits } else { Self::NoLimits }
    }
}

impl OperatorClass {
    /// TeX's operator dictionary, keyed by the operator's code point.
    ///
    /// Every large operator the math parser accepts is listed here once: the
    /// `op` class stacks by default in display style, the `intop` class does
    /// not.  Anything else is ordinary.
    pub fn of_char(c: Option<char>) -> Self {
        match c {
            Some('∑' | '∏' | '∐') => Self::Op,
            Some('∫' | '∬' | '∮') => Self::IntOp,
            _ => Self::Ord,
        }
    }

    /// Classify the nucleus of an atom.
    ///
    /// Only a large-operator symbol can stack by default; text atoms are the
    /// upright named functions, and every other node behaves like an ordinary
    /// atom for placement purposes.
    pub fn of_node(node: &SemanticNode) -> Self {
        match node {
            SemanticNode::Symbol { glyph_key, role } => match role {
                SymbolRole::LargeOperator => Self::of_char(glyph_key.char),
                SymbolRole::Operator => Self::NamedOp,
                _ => Self::Ord,
            },
            SemanticNode::Text { .. } => Self::NamedOp,
            _ => Self::Ord,
        }
    }

    /// True for the two classes TeX draws as a large operator.
    pub fn is_large_operator(self) -> bool {
        matches!(self, Self::Op | Self::IntOp)
    }

    /// Whether the scripts go above and below the operator.
    ///
    /// `Auto` follows TeX: only the `op` class stacks, and only in display
    /// style.  An explicit policy still cannot make a non-operator stack —
    /// `\limits` applies to large operators alone.
    pub fn places_limits(self, policy: LimitsPolicy, style: MathStyle) -> bool {
        match policy {
            LimitsPolicy::Limits => self.is_large_operator(),
            LimitsPolicy::NoLimits => false,
            LimitsPolicy::Auto => matches!(self, Self::Op) && style == MathStyle::Display,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn large_op(c: char) -> SemanticNode {
        SemanticNode::Symbol {
            glyph_key: super::super::semantic::GlyphKey::from_char(c, None, super::super::semantic::FontStyle::Math),
            role: SymbolRole::LargeOperator,
        }
    }

    #[test]
    fn sum_class_stacks_in_display_only() {
        let class = OperatorClass::of_node(&large_op('∑'));
        assert_eq!(class, OperatorClass::Op);
        assert!(class.places_limits(LimitsPolicy::Auto, MathStyle::Display));
        assert!(!class.places_limits(LimitsPolicy::Auto, MathStyle::Text));
    }

    #[test]
    fn integrals_never_stack_by_default() {
        for c in ['∫', '∬', '∮'] {
            let class = OperatorClass::of_node(&large_op(c));
            assert_eq!(class, OperatorClass::IntOp, "`{c}` must be an `intop`");
            assert!(!class.places_limits(LimitsPolicy::Auto, MathStyle::Display));
            assert!(class.places_limits(LimitsPolicy::Limits, MathStyle::Display));
            assert!(class.places_limits(LimitsPolicy::Limits, MathStyle::Text));
            assert!(!class.places_limits(LimitsPolicy::NoLimits, MathStyle::Display));
        }
    }

    #[test]
    fn ordinary_atoms_never_stack() {
        let class = OperatorClass::of_node(&SemanticNode::Text {
            text: "sin".to_string(),
            font_style: super::super::semantic::FontStyle::Normal,
        });
        assert_eq!(class, OperatorClass::NamedOp);
        assert!(!class.places_limits(LimitsPolicy::Limits, MathStyle::Display));
        assert!(!OperatorClass::Ord.places_limits(LimitsPolicy::Limits, MathStyle::Display));
    }
}
