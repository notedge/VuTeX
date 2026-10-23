use std::fmt;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuTeXErrorKind {
    ParseError {
        message: String,
        position: Option<usize>,
    },
    LayoutError(String),
    FontError {
        glyph: String,
        message: String,
    },
    BackendError(String),
    InternalError(String),
    IO(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuTeXError {
    pub kind: Box<RuTeXErrorKind>,
}

impl RuTeXError {
    pub fn new(kind: RuTeXErrorKind) -> Self {
        Self {
            kind: Box::new(kind),
        }
    }

    pub fn parse_error(message: impl Into<String>, position: Option<usize>) -> Self {
        Self::new(RuTeXErrorKind::ParseError {
            message: message.into(),
            position,
        })
    }

    pub fn layout_error(message: impl Into<String>) -> Self {
        Self::new(RuTeXErrorKind::LayoutError(message.into()))
    }

    pub fn font_error(glyph: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(RuTeXErrorKind::FontError {
            glyph: glyph.into(),
            message: message.into(),
        })
    }

    pub fn backend_error(message: impl Into<String>) -> Self {
        Self::new(RuTeXErrorKind::BackendError(message.into()))
    }

    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new(RuTeXErrorKind::InternalError(message.into()))
    }

    pub fn io_error(err: impl std::fmt::Display) -> Self {
        Self::new(RuTeXErrorKind::IO(err.to_string()))
    }
}

impl From<std::io::Error> for RuTeXError {
    fn from(err: std::io::Error) -> Self {
        Self::io_error(err)
    }
}

impl fmt::Display for RuTeXError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &*self.kind {
            RuTeXErrorKind::ParseError { message, position } => {
                if let Some(pos) = position {
                    write!(f, "Parse Error at position {}: {}", pos, message)
                } else {
                    write!(f, "Parse Error: {}", message)
                }
            }
            RuTeXErrorKind::LayoutError(msg) => write!(f, "Layout Error: {}", msg),
            RuTeXErrorKind::FontError { glyph, message } => {
                write!(f, "Font Error (Glyph: {}): {}", glyph, message)
            }
            RuTeXErrorKind::BackendError(msg) => write!(f, "Backend Error: {}", msg),
            RuTeXErrorKind::InternalError(msg) => write!(f, "Internal Error: {}", msg),
            RuTeXErrorKind::IO(msg) => write!(f, "IO Error: {}", msg),
        }
    }
}

impl std::error::Error for RuTeXError {}

pub type Result<T> = std::result::Result<T, RuTeXError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MathConstant {
    ScriptPercentScaleDown,
    ScriptScriptPercentScaleDown,
    DelimitedSubFormulaMinHeight,
    DisplayOperatorMinHeight,
    MathLeading,
    AxisHeight,
    AccentBaseHeight,
    FlattenedAccentBaseHeight,
    SubscriptShiftDown,
    SubscriptTopMax,
    SubscriptBaselineDropMin,
    SuperscriptShiftUp,
    SuperscriptShiftUpCramped,
    SuperscriptBottomMin,
    SuperscriptBaselineDropMax,
    SubSuperscriptGapMin,
    SuperscriptBottomMaxWithSubscript,
    SpaceAfterScript,
    UpperLimitGapMin,
    UpperLimitBaselineRiseMin,
    LowerLimitGapMin,
    LowerLimitBaselineDropMin,
    StackTopShiftUp,
    StackTopDisplayStyleShiftUp,
    StackBottomShiftDown,
    StackBottomDisplayStyleShiftDown,
    StackGapMin,
    StackDisplayStyleGapMin,
    StretchStackTopShiftUp,
    StretchStackBottomShiftDown,
    StretchStackGapAboveMin,
    StretchStackGapBelowMin,
    FractionNumeratorShiftUp,
    FractionNumeratorDisplayStyleShiftUp,
    FractionDenominatorShiftDown,
    FractionDenominatorDisplayStyleShiftDown,
    FractionNumeratorGapMin,
    FractionNumDisplayStyleGapMin,
    FractionRuleThickness,
    FractionDenominatorGapMin,
    FractionDenomDisplayStyleGapMin,
    SkewedFractionHorizontalGap,
    SkewedFractionVerticalGap,
    OverbarVerticalGap,
    OverbarRuleThickness,
    OverbarExtraAscender,
    UnderbarVerticalGap,
    UnderbarRuleThickness,
    UnderbarExtraDescender,
    RadicalVerticalGap,
    RadicalDisplayStyleVerticalGap,
    RadicalRuleThickness,
    RadicalExtraAscender,
    RadicalKernBeforeDegree,
    RadicalKernAfterDegree,
    RadicalDegreeBottomRaisePercent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GlyphMetrics {
    pub width: Fixed,
    pub height: Fixed,
    pub depth: Fixed,
    pub italic_correction: Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterVariant {
    pub glyph: GlyphKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterComponent {
    pub glyph: GlyphKey,
    pub is_extender: bool,
    pub start_connector: Fixed,
    pub end_connector: Fixed,
    pub full_advance: Fixed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelimiterConstruction {
    pub variants: Vec<DelimiterVariant>,
    pub components: Vec<DelimiterComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FontMetricsData {
    pub family: String,
    pub units_per_em: u16,
    pub constants: std::collections::HashMap<MathConstant, Fixed>,
    pub glyphs: std::collections::HashMap<GlyphKey, GlyphMetrics>,
    pub glyph_paths: std::collections::HashMap<GlyphKey, String>,
    pub delimiters: std::collections::HashMap<GlyphKey, DelimiterConstruction>,
}

impl FontMetricsData {
    pub fn new(family: String, units_per_em: u16) -> Self {
        Self {
            family,
            units_per_em,
            constants: std::collections::HashMap::new(),
            glyphs: std::collections::HashMap::new(),
            glyph_paths: std::collections::HashMap::new(),
            delimiters: std::collections::HashMap::new(),
        }
    }
}

// --- Math Semantic Tree ---

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
    Text(String),
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
        alignment: Alignment,
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
        Self {
            char: Some(c),
            glyph_id: None,
            font_family: family,
            style,
        }
    }

    pub fn from_gid(gid: u16, family: Option<String>, style: FontStyle) -> Self {
        Self {
            char: None,
            glyph_id: Some(gid),
            font_family: family,
            style,
        }
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
    fn add(self, other: Self) -> Self { Fixed(self.0 + other.0) }
}

impl std::ops::Sub for Fixed {
    type Output = Self;
    fn sub(self, other: Self) -> Self { Fixed(self.0 - other.0) }
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
        } else {
            Fixed(((self.0 as i64 * Self::SCALE as i64) / rhs.0 as i64) as i32)
        }
    }
}

impl std::ops::Mul<f64> for Fixed {
    type Output = Self;
    fn mul(self, rhs: f64) -> Self { Fixed((self.0 as f64 * rhs) as i32) }
}

impl std::ops::Div<f64> for Fixed {
    type Output = Self;
    fn div(self, rhs: f64) -> Self { Fixed((self.0 as f64 / rhs) as i32) }
}
