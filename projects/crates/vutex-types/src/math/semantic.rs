use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MathSemanticTree {
    pub root: SemanticNode,
    // TODO: Add source mapping for debugging
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SemanticNode {
    Sequence(Vec<SemanticNode>),
    HorizontalBox {
        content: Vec<SemanticNode>,
        spacing: SpacingRule,
    },
    Paragraph {
        content: Vec<SemanticNode>,
        width: Fixed,
    },
    VerticalBox {
        content: Vec<SemanticNode>,
        alignment: Alignment,
    },
    Fraction {
        num: Box<SemanticNode>,
        den: Box<SemanticNode>,
        line: LineStyle,
    },
    Radical {
        degree: Option<Box<SemanticNode>>,
        radicand: Box<SemanticNode>,
    },
    Symbol {
        glyph_key: GlyphKey,
        role: SymbolRole,
    },
    /// Plain text atoms with an explicit font face.
    ///
    /// `\text` / `\textrm` emit `FontStyle::Normal`; `\mathbf` / `\mathit` /
    /// `\mathrm` emit their respective faces so the layout engine can render
    /// them differently from math-italic symbols.
    Text {
        text: String,
        font_style: FontStyle,
    },
    Subscript {
        base: Box<SemanticNode>,
        sub: Box<SemanticNode>,
    },
    Superscript {
        base: Box<SemanticNode>,
        sup: Box<SemanticNode>,
    },
    SubSuperscript {
        base: Box<SemanticNode>,
        sub: Box<SemanticNode>,
        sup: Box<SemanticNode>,
    },
    Matrix {
        rows: Vec<Vec<SemanticNode>>,
        row_spacing: Fixed,
        col_spacing: Fixed,
        /// Whether the grid is always set at display style.
        ///
        /// `align` and `align*` are display-level environments, so an enclosing
        /// `\scriptstyle` must not demote their cells. Every other matrix
        /// environment follows the style around it.
        display_style: bool,
        /// Alignment of each column, left to right.
        ///
        /// `matrix` centre-aligns every column, `cases` left-aligns both of its
        /// columns, and `aligned` / `alignedat` / `split` / `align` alternate
        /// right and left at each `&`.  Missing entries (ragged rows) centre by
        /// default.
        column_alignment: Vec<Alignment>,
    },
    Delimited {
        left: Option<GlyphKey>,
        right: Option<GlyphKey>,
        content: Box<SemanticNode>,
    },
    Accent {
        base: Box<SemanticNode>,
        accent: GlyphKey,
    },
    /// Imperative: switch the enclosing layout scope to a different [`MathStyle`].
    ///
    /// Emitted by `\displaystyle`, `\textstyle`, `\scriptstyle`, `\scriptscriptstyle`.
    /// Consumed by the layout engine as a side-effect on subsequent atoms; the
    /// node itself produces no boxes.
    SetMathStyle(MathStyle),
    /// Imperative: toggle limits placement for the next LargeOperator.
    ///
    /// `true` = `\limits` (subscripts above/below the operator),
    /// `false` = `\nolimits` (subscripts beside the operator).
    ///
    /// The layout engine pops this flag when it encounters the next
    /// `LargeOperator` so it does not bleed past one operator.
    LimitsSpec(bool),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SpacingRule {
    None,
    Thin,
    Medium,
    Thick,
    Auto,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LineStyle {
    Solid,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub char: Option<char>,
    pub glyph_id: Option<u16>,
    pub font_family: Option<String>,
    pub style: FontStyle,
}

impl GlyphKey {
    pub fn from_char(c: char, family: Option<String>, style: FontStyle) -> Self {
        Self { char: Some(c), glyph_id: None, font_family: family, style }
    }

    pub fn from_gid(gid: u16, family: Option<String>, style: FontStyle) -> Self {
        Self { char: None, glyph_id: Some(gid), font_family: family, style }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Normal,
    Italic,
    Bold,
    Math,
    SansSerif,
    Monospace,
    /// Authored strikethrough text face.
    ///
    /// A text face, not a math face: it selects the serif body text and the
    /// decoration itself comes from layout geometry, not from the font.
    Strike,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SymbolRole {
    Ordinary,
    Operator,
    Binary,
    Relation,
    Opening,
    Closing,
    Punctuation,
    Inner,
    LargeOperator,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MathStyle {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// Fixed-point number for precise layout calculations.
/// Using I28F4 as suggested in roadmap (or adaptable to TeX's 16.16).
/// Here we use i32 with 16 bits for fractional part (TeX style) for better compatibility.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Fixed(pub i32);

impl Fixed {
    pub const ZERO: Fixed = Fixed(0);
    pub const ONE: Fixed = Fixed(65536);
    pub const SCALE: i32 = 65536;

    pub fn from_f64(f: f64) -> Self {
        Fixed((f * Self::SCALE as f64) as i32)
    }

    pub fn to_f64(self) -> f64 {
        self.0 as f64 / Self::SCALE as f64
    }
}

impl std::ops::Add for Fixed {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Fixed(self.0 + other.0)
    }
}

impl std::ops::Sub for Fixed {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Fixed(self.0 - other.0)
    }
}

impl std::ops::Mul for Fixed {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Fixed(((self.0 as i64 * rhs.0 as i64) / Self::SCALE as i64) as i32)
    }
}

impl std::ops::Div for Fixed {
    type Output = Self;
    fn div(self, rhs: Self) -> Self {
        if rhs.0 == 0 {
            Fixed(0) // Should probably be an error or infinity
        }
        else {
            Fixed(((self.0 as i64 * Self::SCALE as i64) / rhs.0 as i64) as i32)
        }
    }
}

impl std::ops::Mul<f64> for Fixed {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self {
        Fixed((self.0 as f64 * rhs) as i32)
    }
}

impl std::ops::Div<f64> for Fixed {
    type Output = Self;
    fn div(self, rhs: f64) -> Self {
        Fixed((self.0 as f64 / rhs) as i32)
    }
}
