use serde::{Deserialize, Serialize};

use super::operator::LimitsPolicy;
use crate::{Diagnostic, Severity};

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
    /// Horizontal space requested by a spacing command.
    ///
    /// `width` is measured in em of the current math size and is scaled by the
    /// layout engine, exactly like the `row_spacing` / `col_spacing` of
    /// [`SemanticNode::Matrix`].  TeX sets `\,` to 3mu, `\:` to 4mu and `\;` to
    /// 5mu of the current em (`1mu` is `em / 18`), `\ ` to a third of an em,
    /// `\quad` to 1em and `\qquad` to 2em.
    Space {
        width: Fixed,
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
    /// One AMS math environment block.
    ///
    /// The environment keeps its own kind, alignment anchors and per-row
    /// numbering intent instead of being lowered into a flagged
    /// [`SemanticNode::Matrix`]: `aligned`, `alignedat{n}`, `split`, `align`,
    /// `equation`, `gather`, `gathered` and `multline` differ in how many `&`
    /// anchors a row has, whether the rows are one equation or several, and
    /// whether the block is display level.
    Environment(MathEnvironment),
    /// Imperative: switch the enclosing layout scope to a different [`MathStyle`].
    ///
    /// Emitted by `\displaystyle`, `\textstyle`, `\scriptstyle`, `\scriptscriptstyle`.
    /// Consumed by the layout engine as a side-effect on subsequent atoms; the
    /// node itself produces no boxes.
    SetMathStyle(MathStyle),
    /// Imperative: override the limits placement of the next large operator.
    ///
    /// [`LimitsPolicy::Limits`] is `\limits` (scripts above/below the
    /// operator) and [`LimitsPolicy::NoLimits`] is `\nolimits` (scripts beside
    /// it).  The layout engine pops the policy when it reaches the next large
    /// operator so it does not bleed past one operator.
    LimitsSpec(LimitsPolicy),
}

/// One AMS math environment, with the structure its name implies.
///
/// The environment is not a matrix with an alignment flag.  Its
/// [`kind`](Self::kind) decides how many `&` anchors a row carries, whether the
/// rows form one equation or several independent ones, and whether the block
/// pins display style.  Rows keep their cells split at the anchors so layout,
/// diagnostics and structured backends can all read the authored structure.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MathEnvironment {
    /// Which AMS environment this block is.
    pub kind: EnvironmentKind,
    /// True for the starred form (`align*`, `gather*`, …), which carries no
    /// automatic numbering.
    pub starred: bool,
    /// Rows in source order.
    pub rows: Vec<EnvironmentRow>,
    /// Vertical gap between adjacent rows, in em of the current math size.
    pub row_spacing: Fixed,
    /// Horizontal gap between adjacent alignment columns, in em.
    pub col_spacing: Fixed,
}

/// One row of a [`MathEnvironment`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentRow {
    /// Cells in source order — one per `&`-separated segment.
    pub cells: Vec<SemanticNode>,
    /// Numbering intent authored on this row.
    pub numbering: Numbering,
}

/// Numbering intent an author wrote on one row.
///
/// VuTeX keeps the intent as data and assigns no numbers of its own: equation
/// numbering belongs to the document that embeds the formula, so a `\tag`
/// travels verbatim instead of being resolved here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Numbering {
    /// No `\tag` / `\notag`: the row takes part in whatever numbering the
    /// document applies to the environment.
    Default,
    /// `\notag` / `\nonumber`: the row is not numbered even when its
    /// environment normally numbers every row.
    Suppressed,
    /// `\tag{…}`: the author's label for this row, verbatim.
    Tag(String),
}

/// The AMS environments the math IR distinguishes.
///
/// The variants differ by structure, not by presentation options: they carry
/// different numbers of alignment anchors, different row relationships and
/// different levels, which is why they are modelled as distinct kinds.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvironmentKind {
    /// `aligned`: `&`-separated column pairs, width fitted to the block.
    Aligned,
    /// `alignedat{n}`: the `aligned` grid pinned to exactly `n` column pairs.
    AlignedAt {
        /// Number of right/left column pairs the environment declares.
        pairs: u32,
    },
    /// `split`: one equation broken over its rows.  A split may have any
    /// number of rows.
    Split,
    /// `align`: display-level rows, each set as `&`-separated column pairs.
    Align,
    /// `equation`: one display-level equation with no alignment anchors.
    Equation,
    /// `gather`: independent display-level rows, centred over each other.
    Gather,
    /// `gathered`: `gather`'s inner form — independent rows centred over each
    /// other, following the style of the surrounding list instead of pinning
    /// display style the way `gather` does.
    Gathered,
    /// `multline`: one display-level equation whose first row is flush left,
    /// last row flush right and middle rows centred.
    Multline,
}

/// Where the `&` alignment anchors of an environment's rows sit.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum AlignmentAnchors {
    /// Every `&` closes a right-aligned cell and opens a left-aligned one, so a
    /// row of `2n` cells is `n` column pairs.  `aligned`, `alignedat` and
    /// `align` use this form.
    ColumnPairs,
    /// No anchors: every row is one centred cell (`gather`, `equation`).
    Centered,
    /// `split`: the rows are fragments of one equation and meet at a single
    /// alignment point rather than being independent column grids.
    SingleEquation,
    /// `multline`: no anchors, but the first row is flush left, the last row
    /// flush right and the rows between are centred.
    Spread,
}

impl EnvironmentKind {
    /// Where this environment's `&` anchors sit.
    pub fn anchors(self) -> AlignmentAnchors {
        match self {
            EnvironmentKind::Aligned | EnvironmentKind::AlignedAt { .. } | EnvironmentKind::Align => {
                AlignmentAnchors::ColumnPairs
            }
            EnvironmentKind::Split => AlignmentAnchors::SingleEquation,
            EnvironmentKind::Equation | EnvironmentKind::Gather | EnvironmentKind::Gathered => AlignmentAnchors::Centered,
            EnvironmentKind::Multline => AlignmentAnchors::Spread,
        }
    }

    /// Number of column pairs the environment pins, when it pins one.
    ///
    /// Only `alignedat{n}` declares a fixed count.  `aligned` and `align` take
    /// however many pairs their widest row types.
    pub fn fixed_pairs(self) -> Option<u32> {
        match self {
            EnvironmentKind::AlignedAt { pairs } => Some(pairs),
            _ => None,
        }
    }

    /// True when the environment is a display-level block of its own.
    ///
    /// `align`, `equation`, `gather` and `multline` pin display style, so an
    /// enclosing `\scriptstyle` cannot demote their rows.  `aligned`,
    /// `alignedat`, `split` and `gathered` are inline fragments and follow the
    /// surrounding style.
    pub fn is_display_level(self) -> bool {
        matches!(self, EnvironmentKind::Align | EnvironmentKind::Equation | EnvironmentKind::Gather | EnvironmentKind::Multline)
    }

    /// The environment name as authors write it, starred forms included.
    pub fn name(self, starred: bool) -> String {
        let base = match self {
            EnvironmentKind::Aligned => "aligned",
            EnvironmentKind::AlignedAt { .. } => "alignedat",
            EnvironmentKind::Split => "split",
            EnvironmentKind::Align => "align",
            EnvironmentKind::Equation => "equation",
            EnvironmentKind::Gather => "gather",
            EnvironmentKind::Gathered => "gathered",
            EnvironmentKind::Multline => "multline",
        };
        if starred { format!("{base}*") } else { base.to_owned() }
    }
}

/// Where an environment was found, which decides what the IR can report
/// without guessing the author's intent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum EnvironmentContext {
    /// A display block of its own, e.g. `align` at the top level of a formula.
    Display,
    /// The body of an `equation` / `equation*`, where `split` belongs.
    EquationBody,
    /// Anywhere else: inside a group, a fraction numerator, a matrix cell.
    Inline,
}

impl MathEnvironment {
    /// Columns the environment presents.
    ///
    /// A fixed-pair environment presents exactly the declared pairs.  Every
    /// other kind takes the widest row, because a shorter row simply leaves the
    /// trailing columns empty.
    pub fn columns(&self) -> usize {
        let typed = self.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
        match self.kind.fixed_pairs() {
            Some(pairs) => (pairs as usize) * 2,
            None => typed,
        }
    }

    /// Per-column alignment this environment's rows use.
    ///
    /// Derived from [`EnvironmentKind::anchors`], never from the author's row
    /// contents.  `multline`'s per-row flush-left / flush-right placement is a
    /// row rule rather than a column rule, so it reports a centred grid here
    /// until layout consumes [`AlignmentAnchors::Spread`] directly.
    pub fn column_alignment(&self) -> Vec<Alignment> {
        let columns = self.columns();
        match self.kind.anchors() {
            // `split` rows meet at one alignment point, so the relation that
            // opens a row sits in the right column and its tail in the left one,
            // the same right/left alternation `aligned` and `align` use.
            AlignmentAnchors::ColumnPairs | AlignmentAnchors::SingleEquation => {
                (0..columns).map(|column| if column % 2 == 0 { Alignment::Right } else { Alignment::Left }).collect()
            }
            AlignmentAnchors::Centered | AlignmentAnchors::Spread => vec![Alignment::Center; columns],
        }
    }

    /// Diagnostics the IR can raise from the environment alone.
    ///
    /// `context` says where the environment sits, because `split` is only
    /// meaningful inside an equation.  A misplaced `split` is reported once and
    /// left where the author put it: VuTeX does not guess that it belongs to
    /// some other equation.
    pub fn diagnostics(&self, context: EnvironmentContext) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        if let EnvironmentKind::AlignedAt { pairs } = self.kind {
            let declared = (pairs as usize) * 2;
            let typed = self.rows.iter().map(|row| row.cells.len()).max().unwrap_or(0);
            if declared != typed {
                diagnostics.push(Diagnostic {
                    code: "vutex.layout.math.alignedat_columns".to_owned(),
                    severity: Severity::Warning,
                    message: format!(
                        "`\\begin{{alignedat}}` declares {pairs} column pairs ({declared} columns) but its body has {typed} columns, so the grid keeps {declared} columns"
                    ),
                });
            }
        }

        if self.kind == EnvironmentKind::Split && context != EnvironmentContext::EquationBody {
            diagnostics.push(Diagnostic {
                code: "vutex.layout.math.split_outside_equation".to_owned(),
                severity: Severity::Warning,
                message: "`split` breaks one equation over its rows and belongs inside `equation`".to_owned(),
            });
        }

        diagnostics
    }
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
#[serde(rename_all = "kebab-case")]
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MathStyle {
    Display,
    Text,
    Script,
    ScriptScript,
}

/// Immutable structured math semantics consumed by structured backends.
///
/// This is the backend-facing shape of a formula, not the parser's source
/// tree: every [`MathSemNode`] maps one-to-one onto a Presentation MathML
/// element (`mfrac`, `msqrt` / `mroot`, `msub` / `msup` / `msubsup`, `mtable`,
/// `mover` / `munder`, `mrow`, `mi` / `mn` / `mo`, `mtext`, `mspace`), and no
/// layout geometry is involved.  Structured backends render this tree directly
/// instead of reconstructing structure from painted glyph runs.
///
/// The imperative nodes of [`SemanticNode`] (`SetMathStyle`, `LimitsSpec`)
/// carry no structure and are dropped during construction.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MathSemantics {
    /// Top-level siblings of the formula, in source order.
    pub nodes: Vec<MathSemNode>,
}

impl MathSemantics {
    /// Build from a parsed [`MathSemanticTree`].
    ///
    /// A sequence root is unwrapped so the result is a flat list of siblings
    /// rather than a redundant nested row.
    pub fn from_semantic_tree(tree: &MathSemanticTree) -> Self {
        match &tree.root {
            SemanticNode::Sequence(items) => Self::from_nodes(items),
            other => Self { nodes: MathSemNode::from_semantic(other).into_iter().collect() },
        }
    }

    /// Build from one semantic node sequence.
    pub fn from_nodes(nodes: &[SemanticNode]) -> Self {
        Self { nodes: nodes.iter().filter_map(MathSemNode::from_semantic).collect() }
    }
}

/// One node of the structured math semantics tree.
///
/// Each variant corresponds to exactly one Presentation MathML element; the
/// nested boxes of the source formula (numerator, radicand, scripts, cells)
/// stay nested here instead of being flattened into a painted glyph stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum MathSemNode {
    /// Group of siblings — `mrow`.
    Row(Vec<MathSemNode>),
    /// Alphabetic atom — `mi`.
    Identifier(String),
    /// Numeric atom — `mn`.
    Number(String),
    /// Operator or fence atom — `mo`.
    Operator(String),
    /// Authored text atom — `mtext`.
    Text(String),
    /// `mfrac`; `line` is kept so a line-less fraction (a binomial) can be
    /// emitted with `linethickness="0"`.
    Fraction { numerator: Box<MathSemNode>, denominator: Box<MathSemNode>, line: LineStyle },
    /// `msqrt` when `degree` is `None`, `mroot` otherwise.
    Radical { radicand: Box<MathSemNode>, degree: Option<Box<MathSemNode>> },
    /// `msub`.
    Sub { base: Box<MathSemNode>, sub: Box<MathSemNode> },
    /// `msup`.
    Sup { base: Box<MathSemNode>, sup: Box<MathSemNode> },
    /// `msubsup`.
    SubSup { base: Box<MathSemNode>, sub: Box<MathSemNode>, sup: Box<MathSemNode> },
    /// `mtable` — rows of cells, each cell rendered inside its own `mtd`.
    Table { rows: Vec<Vec<MathSemNode>> },
    /// `mover` / `munder` depending on [`AccentPosition`].
    Accent { base: Box<MathSemNode>, accent: String, position: AccentPosition },
    /// Fence pair around a body — `mrow` wrapping `mo` fences.
    ///
    /// `None` marks a missing delimiter (`.`, as in `\left(…\right.`).
    Delimited { left: Option<String>, right: Option<String>, body: Box<MathSemNode> },
    /// Explicit math space — `mspace`, width in em of the current math size.
    Space { width_em: f64 },
    /// Placeholder for a glyph that carries neither a character nor structure.
    Empty,
}

/// Whether an accent is drawn above or below its base.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AccentPosition {
    /// Accent above the base — MathML `mover`.
    Above,
    /// Accent below the base — MathML `munder`.
    Below,
}

impl MathSemNode {
    /// Lower one [`SemanticNode`] into its structured form.
    ///
    /// Returns `None` for the imperative nodes that carry no structure.
    fn from_semantic(node: &SemanticNode) -> Option<Self> {
        let child = |node: &SemanticNode| Box::new(Self::from_semantic(node).unwrap_or(MathSemNode::Empty));

        Some(match node {
            SemanticNode::Sequence(items) => MathSemNode::Row(MathSemantics::from_nodes(items).nodes).into_row(),
            SemanticNode::HorizontalBox { content, .. }
            | SemanticNode::Paragraph { content, .. }
            | SemanticNode::VerticalBox { content, .. } => {
                MathSemNode::Row(MathSemantics::from_nodes(content).nodes).into_row()
            }
            SemanticNode::Fraction { num, den, line } => {
                MathSemNode::Fraction { numerator: child(num), denominator: child(den), line: *line }
            }
            SemanticNode::Radical { degree, radicand } => {
                MathSemNode::Radical { radicand: child(radicand), degree: degree.as_ref().map(|degree| child(degree)) }
            }
            SemanticNode::Symbol { glyph_key, .. } => match glyph_key.char {
                Some(c) if c.is_numeric() => MathSemNode::Number(c.to_string()),
                Some(c) if c.is_alphabetic() => MathSemNode::Identifier(c.to_string()),
                Some(c) => MathSemNode::Operator(c.to_string()),
                None => MathSemNode::Empty,
            },
            SemanticNode::Text { text, .. } => MathSemNode::Text(text.clone()),
            SemanticNode::Space { width } => MathSemNode::Space { width_em: width.to_f64() },
            SemanticNode::Subscript { base, sub } => MathSemNode::Sub { base: child(base), sub: child(sub) },
            SemanticNode::Superscript { base, sup } => MathSemNode::Sup { base: child(base), sup: child(sup) },
            SemanticNode::SubSuperscript { base, sub, sup } => {
                MathSemNode::SubSup { base: child(base), sub: child(sub), sup: child(sup) }
            }
            SemanticNode::Matrix { rows, .. } => MathSemNode::Table {
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|cell| Self::from_semantic(cell).unwrap_or(MathSemNode::Empty)).collect())
                    .collect(),
            },
            // Presentation MathML has exactly one `mtable` element and no field
            // for an environment name, so the environment lowers to one table.
            // Its kind, anchors and row numbering stay on the IR node
            // ([`MathEnvironment`]), which is what layout and diagnostics read.
            SemanticNode::Environment(environment) => MathSemNode::Table {
                rows: environment
                    .rows
                    .iter()
                    .map(|row| row.cells.iter().map(|cell| Self::from_semantic(cell).unwrap_or(MathSemNode::Empty)).collect())
                    .collect(),
            },
            SemanticNode::Delimited { left, right, content } => MathSemNode::Delimited {
                left: left.as_ref().and_then(delimiter_text),
                right: right.as_ref().and_then(delimiter_text),
                body: child(content),
            },
            // The parser only lowers over-accents (`\hat`, `\bar`, …), so every
            // authored accent sits above its base.
            SemanticNode::Accent { base, accent } => MathSemNode::Accent {
                base: child(base),
                accent: accent.char.map(|c| c.to_string()).unwrap_or_default(),
                position: AccentPosition::Above,
            },
            SemanticNode::SetMathStyle(_) | SemanticNode::LimitsSpec(_) => return None,
        })
    }

    /// Wrap a node list into a [`MathSemNode::Row`], keeping a lone node bare.
    fn into_row(self) -> Self {
        match self {
            MathSemNode::Row(items) if items.len() == 1 => items.into_iter().next().unwrap_or(MathSemNode::Empty),
            _ => self,
        }
    }
}

/// Character text of a delimiter glyph, when it has one.
fn delimiter_text(glyph_key: &GlyphKey) -> Option<String> {
    glyph_key.char.map(|c| c.to_string())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(c: char, role: SymbolRole) -> SemanticNode {
        SemanticNode::Symbol { glyph_key: GlyphKey::from_char(c, None, FontStyle::Math), role }
    }

    fn lower(node: &SemanticNode) -> MathSemNode {
        MathSemNode::from_semantic(node).expect("structural node")
    }

    #[test]
    fn fraction_keeps_numerator_and_denominator_nested() {
        let frac = SemanticNode::Fraction {
            num: Box::new(sym('a', SymbolRole::Ordinary)),
            den: Box::new(sym('b', SymbolRole::Ordinary)),
            line: LineStyle::Solid,
        };
        match lower(&frac) {
            MathSemNode::Fraction { numerator, denominator, line } => {
                assert_eq!(*numerator, MathSemNode::Identifier("a".into()));
                assert_eq!(*denominator, MathSemNode::Identifier("b".into()));
                assert_eq!(line, LineStyle::Solid);
            }
            other => panic!("expected Fraction, got {other:?}"),
        }
    }

    #[test]
    fn radical_degree_stays_nested() {
        let rad = SemanticNode::Radical {
            degree: Some(Box::new(sym('3', SymbolRole::Ordinary))),
            radicand: Box::new(sym('x', SymbolRole::Ordinary)),
        };
        match lower(&rad) {
            MathSemNode::Radical { radicand, degree } => {
                assert_eq!(*radicand, MathSemNode::Identifier("x".into()));
                assert_eq!(degree.as_deref(), Some(&MathSemNode::Number("3".into())));
            }
            other => panic!("expected Radical, got {other:?}"),
        }

        let plain = SemanticNode::Radical { degree: None, radicand: Box::new(sym('x', SymbolRole::Ordinary)) };
        match lower(&plain) {
            MathSemNode::Radical { degree, .. } => assert!(degree.is_none()),
            other => panic!("expected Radical, got {other:?}"),
        }
    }

    #[test]
    fn scripts_keep_their_nesting() {
        // `x^{a_b}`: a superscript whose body holds its own subscript.
        let sup = SemanticNode::Superscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sup: Box::new(SemanticNode::Subscript {
                base: Box::new(sym('a', SymbolRole::Ordinary)),
                sub: Box::new(sym('b', SymbolRole::Ordinary)),
            }),
        };
        match lower(&sup) {
            MathSemNode::Sup { base, sup } => {
                assert_eq!(*base, MathSemNode::Identifier("x".into()));
                match *sup {
                    MathSemNode::Sub { base, sub } => {
                        assert_eq!(*base, MathSemNode::Identifier("a".into()));
                        assert_eq!(*sub, MathSemNode::Identifier("b".into()));
                    }
                    other => panic!("expected nested Sub, got {other:?}"),
                }
            }
            other => panic!("expected Sup, got {other:?}"),
        }
    }

    #[test]
    fn sub_superscript_lowers_to_a_single_sub_sup_node() {
        let node = SemanticNode::SubSuperscript {
            base: Box::new(sym('x', SymbolRole::Ordinary)),
            sub: Box::new(sym('a', SymbolRole::Ordinary)),
            sup: Box::new(sym('b', SymbolRole::Ordinary)),
        };
        match lower(&node) {
            MathSemNode::SubSup { base, sub, sup } => {
                assert_eq!(*base, MathSemNode::Identifier("x".into()));
                assert_eq!(*sub, MathSemNode::Identifier("a".into()));
                assert_eq!(*sup, MathSemNode::Identifier("b".into()));
            }
            other => panic!("expected SubSup, got {other:?}"),
        }
    }

    #[test]
    fn matrix_lowers_to_table_rows() {
        let matrix = SemanticNode::Matrix {
            rows: vec![
                vec![sym('a', SymbolRole::Ordinary), sym('b', SymbolRole::Ordinary)],
                vec![sym('c', SymbolRole::Ordinary), sym('d', SymbolRole::Ordinary)],
            ],
            row_spacing: Fixed::ZERO,
            col_spacing: Fixed::ZERO,
            display_style: true,
            column_alignment: vec![Alignment::Center; 2],
        };
        match lower(&matrix) {
            MathSemNode::Table { rows } => {
                assert_eq!(rows.len(), 2);
                assert_eq!(rows[0][0], MathSemNode::Identifier("a".into()));
                assert_eq!(rows[0][1], MathSemNode::Identifier("b".into()));
                assert_eq!(rows[1][0], MathSemNode::Identifier("c".into()));
                assert_eq!(rows[1][1], MathSemNode::Identifier("d".into()));
            }
            other => panic!("expected Table, got {other:?}"),
        }
    }

    #[test]
    fn accent_keeps_base_and_accent_glyph() {
        let accent = SemanticNode::Accent {
            base: Box::new(sym('f', SymbolRole::Ordinary)),
            accent: GlyphKey::from_char('ˆ', None, FontStyle::Normal),
        };
        match lower(&accent) {
            MathSemNode::Accent { base, accent, position } => {
                assert_eq!(*base, MathSemNode::Identifier("f".into()));
                assert_eq!(accent, "ˆ");
                assert_eq!(position, AccentPosition::Above);
            }
            other => panic!("expected Accent, got {other:?}"),
        }
    }

    #[test]
    fn delimited_wraps_its_body_with_both_fences() {
        let delim = SemanticNode::Delimited {
            left: Some(GlyphKey::from_char('(', None, FontStyle::Normal)),
            right: Some(GlyphKey::from_char(')', None, FontStyle::Normal)),
            content: Box::new(sym('x', SymbolRole::Ordinary)),
        };
        match lower(&delim) {
            MathSemNode::Delimited { left, right, body } => {
                assert_eq!(left.as_deref(), Some("("));
                assert_eq!(right.as_deref(), Some(")"));
                assert_eq!(*body, MathSemNode::Identifier("x".into()));
            }
            other => panic!("expected Delimited, got {other:?}"),
        }
    }

    #[test]
    fn atoms_split_by_character_class() {
        assert_eq!(lower(&sym('x', SymbolRole::Ordinary)), MathSemNode::Identifier("x".into()));
        assert_eq!(lower(&sym('7', SymbolRole::Ordinary)), MathSemNode::Number("7".into()));
        assert_eq!(lower(&sym('+', SymbolRole::Binary)), MathSemNode::Operator("+".into()));
        assert_eq!(
            lower(&SemanticNode::Text { text: "sin".into(), font_style: FontStyle::Normal }),
            MathSemNode::Text("sin".into())
        );
    }

    #[test]
    fn imperative_nodes_are_dropped() {
        let nodes = vec![
            SemanticNode::SetMathStyle(MathStyle::Script),
            sym('x', SymbolRole::Ordinary),
            SemanticNode::LimitsSpec(LimitsPolicy::Limits),
        ];
        let semantics = MathSemantics::from_nodes(&nodes);
        assert_eq!(semantics.nodes, vec![MathSemNode::Identifier("x".into())]);
    }

    #[test]
    fn sequence_root_is_unwrapped_and_single_children_stay_bare() {
        let tree = MathSemanticTree {
            root: SemanticNode::Sequence(vec![
                sym('x', SymbolRole::Ordinary),
                SemanticNode::Sequence(vec![sym('+', SymbolRole::Binary)]),
            ]),
        };
        let semantics = MathSemantics::from_semantic_tree(&tree);
        assert_eq!(semantics.nodes, vec![MathSemNode::Identifier("x".into()), MathSemNode::Operator("+".into())]);
    }

    // ── AMS environments ──────────────────────────────────────────────

    fn env(kind: EnvironmentKind, rows: Vec<Vec<SemanticNode>>) -> MathEnvironment {
        MathEnvironment {
            kind,
            starred: false,
            rows: rows.into_iter().map(|cells| EnvironmentRow { cells, numbering: Numbering::Default }).collect(),
            row_spacing: Fixed::from_f64(0.35),
            col_spacing: Fixed::ZERO,
        }
    }

    #[test]
    fn environment_kinds_keep_their_own_anchors_and_level() {
        // Each kind answers the anchor and level questions from its name, not
        // from a matrix flag: `alignedat` pins a pair count, `split` is one
        // equation over rows, `gather`/`gathered`/`equation` have no anchors,
        // and only `align`/`equation`/`gather`/`multline` pin display style.
        assert_eq!(EnvironmentKind::Aligned.anchors(), AlignmentAnchors::ColumnPairs);
        assert_eq!(EnvironmentKind::AlignedAt { pairs: 2 }.anchors(), AlignmentAnchors::ColumnPairs);
        assert_eq!(EnvironmentKind::Align.anchors(), AlignmentAnchors::ColumnPairs);
        assert_eq!(EnvironmentKind::Split.anchors(), AlignmentAnchors::SingleEquation);
        assert_eq!(EnvironmentKind::Equation.anchors(), AlignmentAnchors::Centered);
        assert_eq!(EnvironmentKind::Gather.anchors(), AlignmentAnchors::Centered);
        assert_eq!(EnvironmentKind::Gathered.anchors(), AlignmentAnchors::Centered);
        assert_eq!(EnvironmentKind::Multline.anchors(), AlignmentAnchors::Spread);

        assert_eq!(EnvironmentKind::AlignedAt { pairs: 3 }.fixed_pairs(), Some(3));
        assert_eq!(EnvironmentKind::Aligned.fixed_pairs(), None);
        assert_eq!(EnvironmentKind::Gathered.fixed_pairs(), None);

        for kind in [EnvironmentKind::Align, EnvironmentKind::Equation, EnvironmentKind::Gather, EnvironmentKind::Multline] {
            assert!(kind.is_display_level(), "{kind:?} is a display-level block");
        }
        for kind in [
            EnvironmentKind::Aligned,
            EnvironmentKind::AlignedAt { pairs: 1 },
            EnvironmentKind::Split,
            EnvironmentKind::Gathered,
        ] {
            assert!(!kind.is_display_level(), "{kind:?} follows the surrounding style");
        }
        assert_eq!(EnvironmentKind::Gathered.name(false), "gathered");
    }

    #[test]
    fn environment_flattens_to_one_table_and_the_ir_keeps_its_kind() {
        // `\begin{split} … \tag{1} … \notag`: a split has no row limit.  The
        // Presentation MathML shape has one `mtable` element and nowhere to put
        // an environment name, so the structured projection lowers the block to
        // one table.  The kind, anchors and per-row numbering survive on the IR
        // node itself, which is what layout and diagnostics consume.
        let mut environment =
            env(EnvironmentKind::Split, vec![vec![sym('x', SymbolRole::Ordinary)], vec![sym('y', SymbolRole::Ordinary)]]);
        environment.rows[0].numbering = Numbering::Tag("1".into());
        environment.rows[1].numbering = Numbering::Suppressed;
        let node = SemanticNode::Environment(environment);

        match lower(&node) {
            MathSemNode::Table { rows } => {
                assert_eq!(rows.len(), 2, "a split has no two-row limit");
                assert_eq!(rows[0], vec![MathSemNode::Identifier("x".into())]);
                assert_eq!(rows[1], vec![MathSemNode::Identifier("y".into())]);
            }
            other => panic!("expected the block to lower to one table, got {other:?}"),
        }

        let SemanticNode::Environment(environment) = node
        else {
            panic!("expected an environment node")
        };
        assert_eq!(environment.kind, EnvironmentKind::Split);
        assert_eq!(environment.rows[0].numbering, Numbering::Tag("1".into()));
        assert_eq!(environment.rows[1].numbering, Numbering::Suppressed);
    }

    #[test]
    fn alignedat_column_mismatch_is_reported() {
        // `\begin{alignedat}{2}` declares two column pairs, so its rows carry
        // four columns.  A body with three columns is reported, not silently
        // widened or narrowed.
        let matching = env(
            EnvironmentKind::AlignedAt { pairs: 2 },
            vec![vec![
                sym('a', SymbolRole::Ordinary),
                sym('=', SymbolRole::Relation),
                sym('b', SymbolRole::Ordinary),
                sym('c', SymbolRole::Ordinary),
            ]],
        );
        assert!(matching.diagnostics(EnvironmentContext::Display).is_empty());
        assert_eq!(matching.columns(), 4);

        let short = env(
            EnvironmentKind::AlignedAt { pairs: 2 },
            vec![vec![sym('a', SymbolRole::Ordinary), sym('=', SymbolRole::Relation), sym('b', SymbolRole::Ordinary)]],
        );
        let diagnostics = short.diagnostics(EnvironmentContext::Display);
        assert_eq!(diagnostics.len(), 1, "one mismatched declaration, one diagnostic: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, "vutex.layout.math.alignedat_columns");
        assert_eq!(diagnostics[0].severity, Severity::Warning);
        assert_eq!(short.columns(), 4, "the declared grid is what the environment presents");
    }

    #[test]
    fn split_outside_an_equation_is_reported_but_stays_a_split() {
        // `split` breaks one equation over its rows, so it only belongs in an
        // equation body.  Anywhere else it is reported and left where the author
        // put it — VuTeX never re-homes the rows into an equation of its own.
        let split = env(EnvironmentKind::Split, vec![vec![sym('x', SymbolRole::Ordinary)]]);

        assert!(split.diagnostics(EnvironmentContext::EquationBody).is_empty());
        for context in [EnvironmentContext::Display, EnvironmentContext::Inline] {
            let diagnostics = split.diagnostics(context);
            assert_eq!(diagnostics.len(), 1, "{context:?}: {diagnostics:?}");
            assert_eq!(diagnostics[0].code, "vutex.layout.math.split_outside_equation");
        }
        assert_eq!(split.kind, EnvironmentKind::Split, "the diagnostic does not guess the environment's home");
    }
}
